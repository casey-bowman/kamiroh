# Review handoff: the external-review round — for Mez

*From Ander, 2026-08-23. Cover note for
`2026-08-23-external-reviews-review.md`, which holds the detail and the
mutation evidence. One ask, and it is one word.*

## Verdict

**Merge after one word.** No structural findings. All three fixes are
correct, minimal, and pinned — and the pinning is the best of the four
rounds: every mutation target nominated goes red, plus one I added.

Your numbers reproduce here on rustc 1.97 against crates.io: **80 passed / 0
failed**, clippy zero, fmt clean, all three iroh binaries over real sockets
on `kamiroh/1`. No manifest touched, so the shelf stays current.

## The one ask

**`README.md`: "80 tests as of decision 29" — 80 is right, "as of decision
29" is not.** At c0e36604 the tree has **76**; 80 is this branch's count,
after the four tests you just added.

I checked it three ways before raising it, and one of the reviewers checked
it for me: `cargo test --workspace` at master gives 76; all three counting
methods agree on 80 here, so it is not a method artifact; and **Gemini
independently counted 76** at that commit — *"76 passed, 0 failed (expanded
from 45 tests on Aug 22 and 39 at Spike 2 kickoff)"*.

Drop the qualifier, or re-anchor it to the external-review round. Worth the
keystroke because this sentence exists to close a P3 that two reviewers
raised twice; shipping it with the number pinned to the wrong commit
re-opens the same finding in a smaller form.

One caution while the file is open: **Codex's report says 73 at that
commit, and that one is a miscount.** My measurement and Gemini's agree on
76. Do not correct toward 73.

## Your three mutation questions, answered

**(a) Remove the parties' overrides — does the countdown test go red?** Yes,
and only `tests/disconnects.rs` is affected — `deadlines` (10) and `fanout`
(6) stay green, so the blast radius is exactly the intended one.

But it goes red *as a hang*, not as a failing assertion: the party swallows
the surviving `Open`, the reply never comes, and `block_on` parks forever.
See the carry-forward below — this is now a pattern rather than an incident.

**(b) Restore the `?` — do both deadline tests catch it, or only one?**
**Both**, and cleanly.

That is the right answer rather than a redundant one. The pair fences
opposite halves of Casey's ruling — a broken transport must still fail
loudly, a transient ack-only blip must not interrupt anything — and a fix
satisfying one while breaking the other would be caught. Restoring the `?`
breaks both because it wedges both paths at the same line, which is what
makes the pair worth keeping rather than merging into one test.

**(c) Remove the liveness check — does the new test catch it while the
absent-sibling test stays green?** Yes to both, exactly as hoped.

**(d), which I added: is the boundary pinned from the *other* side too?**
Yes. A too-loose check is only half the risk — a too-strict one silently
breaks decision 29's promise that the names were never part of the receipt.
So I made the check overreach, requiring every *named* actor to exist:
`an_absent_sibling_discloses_nothing` failed and the new test stayed green.

Loosen it and one test fails; tighten it and the other does. That is a
well-fenced fix, and worth saying out loud because "we added a check and a
test" usually does not earn that description.

## The P3 dispatch holds up

Checked against what the reviewers wrote, not against the brief's summary.
Grok's README list — 39 tests, ports = four, Status naming only 22–27, no
mutation or fan-out, no `kamiroh/1`, stale adapter rustdoc — is dispatched
item by item, and "seven ports" is correct (nine `pub trait`s minus the two
blanket-implemented object-safe twins). Only the test count slipped.

On the rustdoc notes I checked the two things that could have made them
wrong, since a historical note that is itself inaccurate would be worse than
the stale block it replaced: the version claims match the lock (`iroh 1.0` →
1.0.3, `kameo 0.22` → 0.22.2), and the numbered *"Assumption point"* markers
the notes call archaeology **still exist** in both files. The note points at
something real.

Fix 3 follows Grok's own suggested wording almost verbatim, which is the
cleanest possible disposition of an outside finding.

## Two to carry, neither for this branch

- **The hang shape is now a pattern.** Three times across two reviews a
  regression has been caught only as a hung memory test — fan-out's withheld
  receipt, and now the party shadow. Same cause every time: `block_on` parks
  forever when a reply that should arrive never does, while the iroh
  equivalents fail in under a second because those tests wrap `timeout(...)`.
  Not worth fixing per-test. But **the cucumber-rs errand is going to touch
  the memory test harness anyway** — a bounded `block_on` there would convert
  a whole class of future regressions from "CI timed out" into "assertion
  failed". Worth putting on that errand's list.
- **`send` and `open_many` now order their checks differently.** Pairwise
  checks the partition first (held, returns `Ok`); `open_many` checks
  endpoint existence first (errors). I think that is right — "no such
  endpoint" should outrank "partitioned", and it matches `kill_endpoint`
  making death positive evidence — but nobody ruled it. The one observable
  consequence: `kill_endpoint(bb)` then a batch to `bb` now errors where it
  previously acked. Nothing in the suite does that today.

## Authorized beyond the rulings

Three, none a problem. The one worth a sentence: **all three fixes are filed
as a single addendum to decision 26.** That fits fix 2 exactly, but fix 1 is
decision 23's territory (the turn machine knows about death — so must a
shadow of it) and fix 3 is decision 29's (what the receipt promises). Filing
by review round keeps the round together and is defensible; I mention it
only because a future reader asking "why do parties fail their shadow?" will
look near 23.

## What I checked that was simply right

The source diff is *exactly* the three fixes — stripping comments and tests,
the whole `crates/` change is the liveness check, two `on_exchange_failed`
overrides, and `?` → `let _ =`. Nothing rode along. `tests/disconnects.rs`
is pure addition, so no existing assertion was weakened to let the new one
pass. And the decision-26 addendum says plainly that the documented
reference runtime was the less faithful of the two, which is the kind of
admission that is easy to soften and was not.

## Bounds kept

No redesign, no dependencies, no commits, nothing pushed, `vendor/` and
`vendor-snapshot` untouched. I applied four temporary mutations across three
source files to produce the evidence above, and briefly checked out `master`
to count its tests; everything was restored, and the worktree sits on
`cowork/external-reviews` at 9a172b54 with an empty `git status` and a green
80/80 re-run.
