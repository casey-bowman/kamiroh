# Review: cowork/fanout (232158aa)

*From Ander, 2026-08-23. Story 4 / decision 29, reviewed against the fan-out
mapping board and my pre-flight hazards note of the same day. Branch verified
as 3 commits on master 5f05ee3f, a clean ancestor. Nothing pushed; the
worktree is back at 232158aa with an empty `git status` and a green re-run.*

## Verdict

**No structural findings.** Both hazards Casey ruled on after my note landed
exactly as recommended, and the two mutation targets I announced in advance
both hold up under attack — the relay pin and the unbound-name pin are real,
on both adapters.

Findings: **two coverage gaps proven by mutation (F1, F2)**, one missed doc
site (F3), and a short list of notes. Nothing blocks the merge. F1 is the one
I would close first, and the test double it needs is already in this branch.

## 1. The gate, on 1.97 against crates.io

| | Sandbox (1.95, offline) | Here (1.97, crates.io) |
|---|---|---|
| Tests | 75 / 0 | **75 / 0** |
| Clippy `--all-targets -D warnings` | zero | **zero** |
| Fmt | clean | **clean** |

`iroh_fanout` ran over real sockets alongside `iroh_conversation` and
`iroh_disconnects`, so the wire half is genuinely exercised, not compiled and
skipped.

## 2. The two announced mutation targets

I said before the build that these were the two places a test could look like
coverage without being coverage. Both survived, which is the answer I wanted
and not the one I expected.

### The N-phone relay: **real, on both adapters**

I narrowed the relay at its only mutable point — `Phone::open_many` arming
`ack_due` for the first phone only:

- `tests/fanout.rs` — **all five** tests failed.
- `iroh_fanout` — failed on exactly the assertion the errand asked about:
  `left: 1, right: 3`, *"the one receipt settles every wait"*.

Worth being precise about why that is a genuine pin rather than a tautology.
The relay loop itself is app code — the library cannot narrow it. What the
mutation attacks is the thing the library *does* own: that `open_many` arms
every phone against the shared id. That is the real Q4=a mechanism, and it is
pinned.

The `settled == 0` half pulls its weight too — see F2, where it caught a
regression the memory suite could only hang on.

### Unbound names: **both halves pinned, both behaviours**

Two separate regressions, tested separately:

- **Erroring on an unbound name** (the roster-oracle regression, and the one
  a loop over pairwise `send` would produce by accident): I made the memory
  batch return `UnknownAddress` for an unbound name.
  `an_absent_sibling_discloses_nothing` failed at the `open_many` unwrap.
  Only that test failed, which is correct — it is the only one with a ghost.
- **Withholding the receipt when nothing is bound**: caught, but see F2 for
  *how* it is caught, which is the finding.

The all-unbound half is genuinely covered: the second `fan_out_and_settle`
call in `an_absent_sibling_discloses_nothing` names `ghost1`/`ghost2` and the
helper asserts both the mailroom-claimed receipt and that it settles both
phones. That is the ruling — *"receipt sent even all-unbound"* — pinned.

## 3. Findings

### F1 — the refused-batch path is documented but unpinned *(close first)*

`Phone::open_many`'s doc makes a specific promise: *"A refused batch send
fails loudly at once (decision 26): the error returns and NO phones are
created — nothing was armed, and a fresh call is legal immediately."*

**Nothing tests it.** I swallowed the refusal entirely — `let _ =
sender.open_many(...).await;`, so a refused batch hands back N armed phones —
and the suite passed **75/75**.

The memory transport's `open_many` cannot fail (silent drops, always `Ok`),
so the error arm is unreachable from every existing test. The irony is that
the branch *already built the tool*: `TurnRefusingTransport` gained an
`open_many` arm in this very commit (`tests/deadlines.rs:522`) that refuses
batches alongside turns — and nothing uses it. That is the ripple Mez flagged
pre-emptively, and it stopped one step short.

One test: build the refusing transport with `refuse_turns` set, call
`Phone::open_many`, assert `Err(PhoneError::Transport(_))`. Worth it because
"no phones were created" is a claim about *arming* — a future refactor that
creates phones before sending would satisfy the type signature, break the
promise, and pass.

### F2 — a withheld receipt hangs the memory suite instead of failing it

I made the memory batch withhold the receipt unless at least one name was
bound. The regression is caught — but `cargo test --test fanout` **ran for 90
seconds and never finished**, because `block_on` parks forever
(`adapter-memory/src/testing.rs:41`) when the receipt that would wake it
never arrives.

The contrast with the wire side is instructive: the same class of regression
on iroh (receipt never sent) failed **cleanly in 0.29s**, at
`iroh_fanout.rs:101` — the `settled == 0` assertion caught it, because
without a receipt the first pairwise ack settles a phone that should already
have been settled. That is the `settled == 0` half earning its place.

So the memory suite detects this only as a hang: a CI timeout with no
diagnostic, and locally a test that looks stuck rather than broken. Not
severe — it *is* detected, and a hanging `cargo test` is not silent — but
worth recording, because the memory suite is the one that runs everywhere and
the fast one is the one that needs sockets. Closing it properly needs a
non-blocking peek on `MemoryInbox` that does not exist today, so I would
record it rather than build surface for it.

### F3 — one of the four ack-promise sites was missed

`crates/kamiroh-domain/src/vocabulary.rs:22` — the `Ack` type's own doc
comment — still reads *"Delivery acknowledgment from the remote **actor**:
'the request reached the dedicated actor and was handed over'"* with no
qualification. **No file in `kamiroh-domain` was touched by this branch.**

Decision 4 (`ARCHITECTURE.md:256`) and the glossary Ack entry
(`ARCHITECTURE.md:144`) both gained the qualification, correctly and by name.
But my hazards note listed four sites and named this one by line, and it is
arguably the most-read of the four: it is what shows on hover and in
rustdoc, and it now contradicts the two prose sites.

One sentence, mirroring the glossary's. (The fourth site,
`deadline.rs:40`'s `DeadlineKind::Ack`, I would leave — "waiting for the
delivery ack of a sent request half" stays true of a fanned phone; only what
the ack *promises* changed, not what the deadline bounds.)

## 4. The wire change (Frame struct → enum)

**Encoding.** Postcard prefixes an enum with a varint discriminant, so
`Frame::Single` is now one byte larger than the old struct and `OpenMany`
replaces N frames with one. Net saving for any batch of two or more; one byte
for everything else. No size concern.

**Compatibility — worth recording, not fixing.** The change is
wire-incompatible, and **the ALPN was not bumped** (still `kamiroh/0`). An
old peer's struct-frame decodes on a new peer as a discriminant taken from
the first byte of `from_name`'s length prefix: a one-character name reads as
`OpenMany` and misparses, anything longer fails to decode and is dropped by
the reader's `continue; // malformed frame`. Either way the failure is
*silent*, and the sender learns nothing until a deadline elapses.

Decision 5 already says vocabulary v0 is closed and cross-build evolution is
deferred, so this breaks no promise. But bumping to `kamiroh/1` would convert
a silent misparse into a refused connection at ALPN negotiation, which is the
difference between "my messages vanish" and "these two builds don't speak".
That is a one-line change and a decision-5 conversation, not a defect —
recording it so the choice is deliberate rather than defaulted.

## 5. Hazards 3–5 dispositions, checked against what was built

- **Hazard 3 (shared id — I claimed no responder cost).** Confirmed. The
  domain crate is untouched, so nothing gained an id-keyed structure, and the
  responder path is unchanged: siblings receive ordinary `Turn::Open`
  deliveries and each host keys its conversations by peer address as before.
  `iroh_fanout` is the strongest evidence — a Kameo runtime hosting both
  fanned workers with, as its comment says, *"deliberately NOTHING
  batch-aware anywhere on this side"*, and it passes over real sockets. Q4=a
  cost the responder nothing, as predicted.
- **Hazard 4 (the relay recipe).** Documented in EMBEDDING's *"Opening with
  many at once"* with all three wrinkles I listed, and the generalization I
  most wanted is there in the right form: *"all four are endpoint-scoped
  facts, and all four existing methods are per-phone. It is a loop; write it
  once."* The `Inbound::AckReceived`-drops-the-origin wrinkle was handled by
  adopting the clone-first idiom in the tests rather than by changing the
  enum — the lighter of the two options I offered, and the right one.
- **Hazard 5 (the deadline-scale sentence).** Present in both places it
  belongs: `Phone::open_many`'s doc and the EMBEDDING bullet, both framing
  the tighter turn deadline as compensation for a lost safety net rather than
  as a warning. `a_denied_sibling_is_silence_and_the_rest_proceed` and the
  ghost half of `an_absent_sibling_discloses_nothing` both pin the residue by
  advancing the clock past the turn deadline.

## 6. What was authorized beyond the rulings

Asked directly. Four, none of them a problem:

- **The ALPN decision.** Not bumping it is a real choice about wire
  compatibility that no ruling covers — see §4. The most substantive of the
  four, and the only one I would put in front of Casey.
- **`open_many` is a required trait method, not a defaulted one.** Correct as
  built: a default looping `send` would deliver the fan-out and silently omit
  the receipt, which is worse than a compile error. It does mean any
  third-party `Transport` breaks until it implements the method — which is
  precisely what happened to `TurnRefusingTransport`, in-tree, visibly. R5's
  "at most one method" is satisfied.
- **Empty and duplicate name lists.** My note predicted both would be decided
  by accident. Empty falls out correctly (receipt sent, no phones, `Ok`) and
  duplicates are documented in EMBEDDING's residue list rather than deduped —
  the "or document" half of what I suggested. Neither is tested; neither
  needs to be.
- **ROADMAP item 5 marked delivered.** In scope and accurate.

## 7. Notes, no action needed

- **N1 — the iroh reader now writes.** `spawn_reader` awaits `send_frame` for
  the receipt inside its accept loop, so the next uni stream on that
  connection waits for the receipt to be written. Throughput only, and the
  lock discipline is right: the await completes *before*
  `shared.router.lock()`, so no guard crosses it — the same 2a-class care as
  last round, applied unprompted in a new place.
- **N2 — the memory partition case is better than I asked for.** I warned
  that synthesizing the receipt before the partition check would ack traffic
  that never arrived. The implementation puts the receipt *into* `held`
  alongside the batch, so a partitioned batch is acked by nobody and the
  receipt flows on `heal()` in order. That is more faithful to decision 27's
  partition semantics than the "just move it after the check" I suggested.
- **N3 — the receipt vanishes if the sender is unregistered.** Memory's
  `open_many` pushes the receipt only `if let Some(mailbox) =
  ...get_mut(from)`. Unreachable in practice (an initiator needs an inbox to
  hear replies at all), and erroring would be worse. Noted only because it is
  a silent branch.
- **N4 — cosmetic.** `docs/ROADMAP.md:36` is 99 characters where the rest of
  the file wraps at 74; the inserted `*Delivered (decision 29).*` pushed it
  over. Same class as last round's reflow nit.

## 8. Things I checked that were simply right

- **The board is byte-identical** to the one Casey ruled on (`diff` clean).
  Third story running.
- **The third-runtime obligation list is still "all five"** — the thing I
  asked to be checked deliberately, because the reflex after two stories that
  grew it would be to add a sixth. EMBEDDING even says why in prose: *"fan-out
  changes nothing for a third runtime — it is a transport affair."*
- **Neither runtime was touched**, exactly as R5 promised, and `iroh_fanout`
  proves it rather than asserting it.
- **The signature carries the rules**: one `EndpointId` plus names makes a
  mixed batch unrepresentable (R1), and taking a `Request` rather than a
  `Message` makes non-opening fan-out unrepresentable (Q2). The feature file
  says so explicitly instead of inventing a scenario that cannot fail.
- **Five scenarios, five pinning tests, 1:1 in the README table**, with the
  all-unbound half and the iroh wire pin both called out in prose.
- **`BATCH_RECEIPT_NAME`'s doc argues its own safety** — names are claims, an
  admitted endpoint can already forge an `Ack`, so the convention adds no
  capability. That is the hazard-1 reasoning recorded where an implementer
  will actually meet it.

---

F1 is the one I would close before merge — one test, using a double this
branch already built. F3 is one sentence. F2 and §4's ALPN question are
records, not requests: F2 because the fix costs new surface, and the ALPN
because it is Casey's call whether decision 5's "closed vocabulary" covers a
silent wire break.
