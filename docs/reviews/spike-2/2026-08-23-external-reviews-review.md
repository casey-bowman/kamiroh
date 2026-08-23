# Review: cowork/external-reviews (9a172b54)

*From Ander, 2026-08-23. Fixes from the three commissioned external reviews,
against Mez's disposition brief (`docs/briefs/2026-08-23-external-reviews-brief.md`)
and the reports in `gemini/`, `codex/`, `grok/`. One commit on master
c0e36604, a clean ancestor. Nothing pushed; the worktree is back at 9a172b54
with an empty `git status` and a green re-run.*

## Verdict

**No structural findings.** All three fixes are correct, minimal, and pinned
— and the pinning is unusually good: every mutation target Mez nominated goes
red, and the one boundary I probed from the other side goes red too.

One finding (**F1**, a number in the README) and five notes. Nothing blocks
the merge. F1 is a one-word fix and it is worth making, because it is a
factual claim in the exact sentence whose purpose was to stop being stale.

## 1. The gate, on 1.97 against crates.io

| | Sandbox (1.95, offline) | Here (1.97, crates.io) |
|---|---|---|
| Tests | 80 / 0 | **80 / 0** |
| Clippy `--all-targets -D warnings` | zero | **zero** |
| Fmt | clean | **clean** |

All three iroh binaries ran over real sockets on ALPN `kamiroh/1`.

## 2. The nominated mutation targets

All three behave as predicted, and I added a fourth to test the third fix's
boundary from the opposite side.

### (a) Parties' `on_exchange_failed` — red, **but as a hang**

Removing both overrides so the shadow machines never fail: `tests/disconnects.rs`
**never finishes**. `deadlines` (10) and `fanout` (6) stay green, so the
blast radius is exactly the intended one.

The regression *is* caught, and it is caught in the right binary. But it
presents as a hung test rather than a failing assertion, because the party
swallows the surviving `Open` as `MustAnswerFirst`, the reply never comes, and
`block_on` parks forever. See N1 — this is the third time this shape has come
up, and it is now worth a standing note rather than a per-review remark.

### (b) The `?` on the handover ack — red, and **both** tests catch it

Answering Mez's question directly: **both**, and cleanly.

- `a_refused_handover_ack_does_not_wedge_the_exchange` — failed
- `a_blipped_ack_alone_interrupts_nothing` — failed

That is the right result rather than a redundant one. The two pin opposite
halves of Casey's ruling — a genuinely broken transport must still fail
loudly, and a transient ack-only blip must not interrupt anything — and a fix
that satisfied one while breaking the other would be caught. Restoring the `?`
breaks both because it wedges both paths at the same line, which is exactly
what makes the pair worth having.

### (c) The endpoint-liveness check — red, and **no overreach**

Removing it: `a_batch_to_an_absent_endpoint_is_refused` fails and **nothing
else does**. `an_absent_sibling_discloses_nothing` stays green, which is the
proof Mez wanted that the fix did not swallow the live-endpoint case.

### (d) My addition: the boundary is pinned from *both* sides

A too-loose check is only half the risk; a too-strict one silently breaks
decision 29's promise that names were never part of the receipt. So I made
the check overreach — requiring every *named* actor to exist, not just the
endpoint:

`an_absent_sibling_discloses_nothing` **failed**, and
`a_batch_to_an_absent_endpoint_is_refused` stayed green.

So the two tests fence the behaviour from opposite directions: loosen it and
one fails, tighten it and the other does. That is a well-pinned fix, and it
is worth saying because "we added a check and a test" usually is not.

## 3. Findings

### F1 — "80 tests as of decision 29" is the right number on the wrong commit

`README.md` now reads *"80 tests as of decision 29"*. But 80 is **this
branch's** count; at decision 29 (c0e36604) the tree has **76**.

I verified it three ways, and the reviewers verified it for me:

- `cargo test --workspace` at master: **76**. On this branch: 80.
- All three counting methods agree on 80 here (`--workspace`, `--list`,
  `--all-targets -- --list`), so this is not a method artifact.
- **Gemini independently counted 76** at c0e36604 (*"76 passed, 0 failed
  (expanded from 45 tests on Aug 22 and 39 at Spike 2 kickoff)"*).

The fix is to drop or re-anchor the qualifier — "80 tests" alone, or "as of
the external-review round". Trivial, and worth doing precisely because this
sentence exists to answer a P3 that two reviewers raised twice; shipping it
with the number attached to the wrong commit re-opens the finding in a
smaller form.

(Codex's report says 73 at the same commit. That one is Codex's miscount —
my measurement and Gemini's agree on 76 — so nobody should "correct" the
README toward 73. Recording it so the discrepancy is explained rather than
rediscovered.)

## 4. The external P3s, dispatched accurately

Checked against what the reviewers actually wrote, not against the brief's
summary of it.

- **README staleness** (Codex ×2, Grok ×2). Grok's list was: 39 tests, ports
  = four, Status naming only 22–27, no mention of mutation or fan-out, no
  `kamiroh/1`, and stale adapter rustdoc. **Every item dispatched.** The ports
  line now names seven, which is correct: nine `pub trait`s exist, but
  `DynParty` and `DynDeathWatch` are the blanket-implemented object-safe
  twins, not separate ports. Only the test count slipped (F1).
- **Adapter rustdoc** (Codex, Grok). Both "written without compiling" blocks
  are now historical notes. I checked the two things that could have made
  them wrong: the version claims are accurate (`iroh 1.0` → lock says 1.0.3;
  `kameo 0.22` → 0.22.2), and the numbered *"Assumption point"* markers the
  notes call archaeology **still exist** in both files
  (`kameo/src/lib.rs:397`, `iroh/src/lib.rs:516,527`), so the note points at
  something real rather than at deleted comments.
- **Death-watch lifetime** (Grok). Dispatched to EMBEDDING plus
  `watch_deaths`' rustdoc, both stating that watches never close and that
  dropping the handle detaches, so `abort()` is the only off switch. See N4:
  Grok framed this as a *shape* problem and it was answered with disclosure,
  deliberately and honestly.
- **Grok's P2s 1 and 2 and the Codex/Grok convergent P2.** All three fixed,
  and finding 5's fix follows Grok's own suggested wording almost exactly —
  *"make `open_many` return `UnknownAddress` when no mailbox belongs to
  `to_endpoint`, and keep the all-unbound-live-endpoint case as the explicit
  one."* That is what shipped, and (d) above shows both halves are pinned.
- **Gemini.** The brief says the reports found nothing new; that reads
  correctly — the 08-23 report is an assessment with no numbered findings.
  Its value here turned out to be concrete rather than rhetorical: its
  independent test count is what confirms F1.

## 5. What was authorized beyond the rulings

Asked directly. Three, none of them a problem:

- **Where the addendum was filed.** All three fixes are recorded as one
  addendum to **decision 26**. That fits fix 2 exactly (positive evidence,
  loud failure), but fix 1 is really decision 23's territory (the turn
  machine knows about death — a shadow machine must too) and fix 3 is
  decision 29's (what the receipt promises). Filing by *review round* rather
  than by subject is a defensible editorial choice and keeps the round
  together; noting it because a future reader looking up "why do parties fail
  their shadow?" will look near 23, not 26.
- **The memory check's position in the method.** See N2 — it changed the
  relative order of the existence and partition checks between `send` and
  `open_many`. I think it is right, but it was not ruled.
- **README prose beyond the flagged items.** The Status block gained new
  sentences about mutation, fan-out and `kamiroh/1` that go past "fix the
  numbers". Accurate, and the reviewers asked for exactly this content.

## 6. Notes, no action needed

- **N1 — the hang shape is now a pattern, not an incident.** Three times in
  two reviews a regression has been caught only as a hung memory test
  (fan-out's withheld receipt; now the party shadow). The cause is always the
  same: `block_on` parks forever (`adapter-memory/src/testing.rs:41`) when a
  reply that should arrive never does. The wire-side equivalents fail cleanly
  in under a second because the iroh tests wrap `timeout(...)`. Not worth
  fixing per-test, but if the cucumber-rs errand is going to touch the memory
  test harness anyway, a bounded `block_on` would convert a whole class of
  future regressions from "CI timed out" into "assertion failed". Worth
  putting on that errand's list rather than this branch's.
- **N2 — `send` and `open_many` now order their checks differently.**
  Pairwise checks the partition *first* (held, returns `Ok`); `open_many`
  checks endpoint existence first (errors). So a batch to a partitioned,
  mailbox-less endpoint errors where a pairwise send would be silently held.
  I think that is correct — "no such endpoint" should outrank "partitioned",
  and it is consistent with `kill_endpoint` making an endpoint's death
  positive evidence. The one real consequence: a test that calls
  `kill_endpoint(bb)` and *then* batches to `bb` now gets a loud
  `UnknownAddress` where it previously got a receipt. Nothing in the suite
  does that today.
- **N3 — the parties fix changes shipped example code.** `EchoParty` and
  `CountdownParty` are what embedders copy, which is the whole argument for
  the fix; it also means the `Party` trait's default `on_exchange_failed`
  (a no-op) is now the thing a hand-written party will get wrong in exactly
  the way the stock parties just stopped getting wrong. The doc on the trait
  already says a party embedded for unattended operation should override it.
  Nothing to change; worth knowing the sharp edge moved rather than vanished.
- **N4 — a shape finding answered with disclosure.** Grok's death-watch P3
  says the lifetime is "the wrong shape for a long-running embedder". The
  response documents it (*"plan for it if your embedding creates watches
  dynamically"*) rather than changing it. That is a legitimate P3 disposition
  at spike scale and the prose is honest about the limit; recording it so the
  finding is understood as deferred rather than resolved.
- **N5 — Codex's socket note.** Its report mentions the first sandboxed run
  failed only where real-iroh tests bind sockets, passing on rerun with
  normal permissions. Consistent with everything I see; the iroh tests pass
  here on every run. No action, but it explains any future "external reviewer
  saw failures" confusion.

## 7. Things I checked that were simply right

- **The source diff is exactly the three fixes.** Stripping comments and
  tests, the entire `crates/` change is: the liveness check, two
  `on_exchange_failed` overrides, and `?` → `let _ =`. Nothing rode along.
- **`tests/disconnects.rs` is pure addition** — no existing assertion was
  weakened to make the new one pass, which is the thing worth checking when a
  fix lands with a test in the same commit.
- **The decision-26 addendum tells the story straight**, including that the
  documented reference runtime was the less faithful of the two — the kind of
  admission that is easy to soften and was not.
- **Both fixed parties carry a comment explaining *why*** the shadow must
  fail, with `CountdownParty`'s noting it is the one that actually hits the
  case. Future readers get the reasoning, not just the call.
- **The brief's characterization of the reports is accurate** on every point
  I sampled — the convergence claim (Codex and Grok independently on the
  memory receipt) is real, and Grok's altitude warning is quoted in the
  spirit its report intended.

---

F1 is the only thing I would change before merge, and it is one word. The
rest is recording: N1 is worth carrying to the cucumber-rs errand, and N2 and
the addendum's filing are judgement calls I agree with but that nobody ruled.
