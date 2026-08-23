# Review handoff: disconnects (story 2) — for Mez

*From Ander, 2026-08-20. Cover note for `2026-08-20-disconnects-review.md`,
which holds the detail. Short, because there is only one ask.*

## Verdict

**Merge-after-fixes** — one defect, four-line fix, already written and
verified. Take the fix and the branch is merge-ready.

Build numbers are exactly as your commit message claims: **57 passed, 0
failed**, clippy clean on rustc 1.97.0, no dependency added, `Cargo.lock` and
every `Cargo.toml` untouched. Both iroh tests pass against real sockets, and I
checked their assertions rather than trusting the green.

This is the cleanest of the three story branches I have reviewed.

## The one ask

**Take the fix, or tell me to.** `crates/kamiroh-app/src/wait.rs:68`.

The `DeathWatch` port documents the closing case — *"or `None` when the watch
is closed"* — and `next_or_deadline_or_death` does not honour it. On
`Ready(None)` control falls through to the other two arms, the completed death
future stays in the race, and the next wakeup polls it again. That panics:
`async fn` resumed after completion.

Nothing in-tree reaches it: both shipped watches are documented "never closes"
and both are. But `DeathWatch` ships with a `DynDeathWatch` object-safe twin
precisely so third-party transports can implement it, and a watch that ends
when its endpoint shuts down is the obvious way to write one — it would take
the pump down.

Fix is a `watching` flag that fuses the watch out of the race once it closes;
the diff is quoted in full in the review. Reproducer and fix are on
**`code/disconnects-review`** (`328b9c4`, one commit on top of yours, local
only). Cherry-pick it, or retype four lines — whichever suits.

The alternative — declare in the port that watches never close and drop the
`Option` — I would not take. A watch that ends is reasonable for a real
transport, and the doc change is bigger than the fix.

## The hazard note worked, and that is worth recording

All three pre-flight hazards came back correctly dispositioned, and I checked
each rather than taking the claim:

1. **The reconnect race** — guarded in `watch_connection` by comparing the
   dying connection's `stable_id()` against the cache. Right mechanism, right
   layer; the transport is genuinely the only place connection generations are
   visible. Excluding `LocallyClosed` is right too, and the asymmetry deserved
   the sentence it got: we hang up, *they* see a death.
2. **No new concurrent emitters** — traced every path to `Observer::on_event`.
   The fan-out task only `tell`s; `report_death` is not an emitter at all.
   Decision 26's non-reentrancy contract is not stressed by anything new.
3. **Zero choreography changes** — I diffed every line touching `ack_due`,
   `turn_due`, `waits.ack`, `waits.turn`, `epoch`, `settle_all` and
   `settle_ack`. The complete set of changes is **two lines**, both inside the
   new `Phone::on_peer_death`. No existing arming or settling code was
   modified at all.

That third one is a real result rather than a formality. It says
`fail_exchange` was the right seam, and it says so against a criterion set
*before* the work instead of rationalised after. Worth contrasting with story
1: finding 1 there cost a demonstration, a fix and a round trip, and the same
class of bug here was designed against from the start and left nothing to
find. If there is a next story, I would happily do the same pre-flight pass —
it is much cheaper than the review that would otherwise catch it.

I did not relitigate the accepted Kameo fan-out residue; Casey ruled it. For
the record I agree with the shape of the argument — that residue and decision
26's processing-time property are the same bug in different clothes, and
judging them together once streaming has shrunk the mailbox latency they both
live in is the right call.

## Two nits, neither blocking

- **The iroh suppression check and the report are not atomic**
  (`kamiroh-adapter-iroh/src/lib.rs:141-161`). The connections lock is released
  before `report_death` runs, so a reconnect completing in that window is not
  seen and a stale death goes out anyway. This is theoretical in a way the
  Kameo residue is not — a few instructions against a full QUIC handshake — but
  unlike that one it closes for free by holding the guard across the report
  (`report_death` takes only the watcher locks and never awaits). Worth doing
  just so the accepted residue stays a single, well-named thing.
- **`cargo fmt --check` is not clean** (36 hunks, 15 files). I checked before
  raising it: `master` already has 29, so formatting is not in this project's
  gate and your commit broke nothing — and some of the pre-existing drift is in
  my own review test files, which I should have caught earlier. Raising it only
  in case a `fmt` gate is wanted before the cucumber-rs errand adds more files.

## Still open from earlier briefs

Unchanged, and repeated so they do not get lost:

1. **Workshop-2's fork still has no `vendor-snapshot`.** Every green result
   across all four reviews is from my 1.97.0 macOS host; none of it has been
   confirmed by an offline cloud build. Your follow-up errand 1.
2. **EMBEDDING.md** now needs `DeathWatch` as well as deadlines,
   `on_exchange_failed`, the observer, and the `Timer` object-safety note. An
   embedder following it today would miss most of spike 2. Your follow-up
   errand 4 — and it has grown twice while waiting.

## Bounds kept

No redesign, no dependencies, `vendor/` and `vendor-snapshot` untouched, no
commits to `cowork/*` or `master`, nothing pushed. Working tree clean.
