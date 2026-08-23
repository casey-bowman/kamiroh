# Review handoff: fan-out (story 4) — for Mez

*From Ander, 2026-08-23. Cover note for `2026-08-23-fanout-review.md`, which
holds the detail and the mutation evidence. Two small asks, two records, and
one question that is Casey's rather than yours.*

## Verdict

**Merge after two small things** — one test and one sentence. No structural
findings, nothing to unpick.

Your numbers reproduce here on rustc 1.97 against crates.io: **75 passed / 0
failed**, clippy zero, fmt clean, with `iroh_fanout` running over real
sockets rather than compiled and skipped. No manifest touched, so the shelf
stays current.

**Both hazards Casey ruled on after my note landed exactly as recommended**,
and — the part I care about more — **both mutation targets I announced in
advance held up under attack.** I said before you built that the N-phone
relay and the unbound-name behaviour were the two places a test could look
like coverage without being coverage. Neither was. That is the first time in
four stories that the pre-flight prediction and the post-build evidence
agreed with nothing left over.

## The two asks

**1. The refused-batch path is documented but unpinned — and you already
built the tool.**

`Phone::open_many`'s doc promises: *"the error returns and NO phones are
created — nothing was armed."* I swallowed the refusal so a refused batch
hands back N armed phones, and the suite passed **75/75**.

Memory's `open_many` cannot fail (silent drops, always `Ok`), so the error
arm is unreachable from every existing test. But `TurnRefusingTransport`
gained a batch-refusing arm *in this very commit* — the ripple you flagged
pre-emptively — and nothing uses it. The double exists; it stopped one step
short of the test it enables.

One test: refusing transport with `refuse_turns` set, call
`Phone::open_many`, assert `Err(PhoneError::Transport(_))`. Worth it because
"no phones were created" is a claim about *arming order*, and a future
refactor that builds phones before sending would satisfy the signature,
break the promise, and pass.

**2. One of the four ack-promise sites was missed.**

No file in `kamiroh-domain` was touched, so `vocabulary.rs:22` — the `Ack`
type's own doc comment — still reads *"from the remote **actor**: the request
reached the dedicated actor and was handed over"*, unqualified. Decision 4
and the glossary both got the qualification, correctly and by name; this is
the third of the four sites my note listed, and arguably the most-read one
since it is what shows on hover and in rustdoc. It now contradicts the two
prose sites.

One sentence, mirroring the glossary's. I would leave the fourth site
(`deadline.rs`'s `DeadlineKind::Ack`) alone — what the ack *promises*
changed, not what the deadline *bounds*.

## Two records, not requests

**A withheld receipt hangs the memory suite instead of failing it.** I made
memory withhold the receipt unless a name was bound: `cargo test` ran 90
seconds and never finished, because `block_on` parks forever with nothing to
wake it. The same regression on iroh failed **cleanly in 0.29s** — the
`settled == 0` assertion caught it, which is that half of your test earning
its keep. Recording rather than asking, because closing it properly needs a
non-blocking peek on `MemoryInbox` that does not exist, and building surface
for a test ergonomic is a bad trade. The regression *is* detected; it just
presents as a stuck test rather than a failing one.

**The wire change is incompatible and the ALPN was not bumped.** `Frame`
struct → enum costs one postcard byte and saves N frames, so no size
concern. But an old peer's frame now either misparses as `OpenMany` or is
dropped by the reader's malformed-frame arm — silently, either way, until a
deadline elapses. Decision 5 ("vocabulary v0 is closed") means this breaks no
promise, so it is not a defect. **This one is Casey's call rather than
yours**: bumping to `kamiroh/1` converts a silent misparse into a refused
connection at negotiation, which is the difference between "my messages
vanish" and "these two builds don't speak". One line, and a decision-5
conversation.

## Hazards 3–5 came back clean

- **Shared id (3).** Confirmed as predicted: the domain crate is untouched,
  nothing gained an id-keyed structure, and `iroh_fanout` proves it rather
  than asserting it — a Kameo runtime hosting both fanned workers with,
  as the comment says, *"deliberately NOTHING batch-aware anywhere on this
  side"*, passing over real QUIC. Q4=a cost the responder nothing.
- **The relay recipe (4).** In EMBEDDING with all three wrinkles, and the
  generalization in the form I most wanted: *"all four are endpoint-scoped
  facts, and all four existing methods are per-phone. It is a loop; write it
  once."* You took the lighter of the two options I offered for the
  `AckReceived`-drops-the-origin wrinkle — clone-first in the pump rather
  than changing the enum — which is the right one.
- **The deadline-scale sentence (5).** In both places it belongs, and framed
  as compensation rather than warning. Two tests advance the clock past the
  turn deadline to pin the residue.

## Better than I asked for, in three places

Worth naming, because these are the kind of thing that goes unremarked:

- **The memory partition case.** I suggested moving the receipt after the
  hold check so a partitioned batch is acked by nobody. You put the receipt
  *into* `held` alongside the batch, so it flows on `heal()` in order — more
  faithful to decision 27's partition semantics than what I proposed.
- **The third-runtime list is still five**, and EMBEDDING says *why* in prose
  ("fan-out changes nothing for a third runtime — it is a transport affair").
  I flagged this as the reflex to resist after two stories that grew it.
- **`BATCH_RECEIPT_NAME`'s doc argues its own safety** — names are claims, an
  admitted endpoint can already forge an `Ack`, so the convention adds no
  capability. That is the hazard-1 reasoning recorded exactly where an
  implementer will meet it, rather than left in a note nobody re-reads.

Two smaller notes in the review, neither needing action: the iroh reader now
awaits a write inside its accept loop (throughput only — and the lock
discipline is right, the await completes before `router.lock()`), and
`ROADMAP.md:36` is 99 characters where the file wraps at 74.

## Bounds kept

No redesign, no dependencies, no commits, nothing pushed, `vendor/` and
`vendor-snapshot` untouched. I applied five temporary mutations across three
source files to produce the evidence above (relay narrowing, per-name error,
receipt withholding on both adapters, swallowed refusal); every one was
restored from backup, and the worktree sits on `cowork/fanout` at 232158aa
with an empty `git status` and a green 75/75 re-run.
