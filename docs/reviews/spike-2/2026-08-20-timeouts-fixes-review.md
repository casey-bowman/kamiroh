# Re-review: `cowork/timeouts-fixes` (`bb7bb6e`) — for Mez

*From Ander, 2026-08-20. Second pass over the review round implementing
Casey's rulings on findings 2–7 (decision 26). Companion to
`2026-08-20-timeouts-review.md`, which holds the original findings and the
numbering used throughout. Self-contained.*

## Verdict

**Merge-after-one-fix.** One regression, in the finding 5 remedy: observer
events are now silently dropped under concurrency. Everything else in the
commit is correct and lands cleanly.

The regression is my fault as much as anyone's — my finding 5 wording was
ambiguous, and the reading you took is the one that breaks. Detail below.

## Build, test, clippy

Toolchain **rustc 1.97.0 / cargo 1.97.0 / clippy 0.1.97**, macOS. On
`cowork/timeouts-fixes` at `bb7bb6e`:

```
cargo build --workspace                        # clean
cargo test  --workspace                        # 49 passed, 0 failed
cargo clippy --workspace --tests --examples    # clean — ZERO warnings
```

**49 passed, 0 failed** — exactly as your commit message claims. `Cargo.lock`
and every `Cargo.toml` untouched across `c4596cf..bb7bb6e`; no dependency
added. Clippy on the newer toolchain still finds nothing.

## What landed correctly

Findings **2, 3, 4, 6 and 7** are all implemented as ruled, and I could not
fault any of them:

- **Finding 2 (the Phone wedge).** Closed. A refused send now fails the
  exchange at once at all three send sites, uniformly.
- **Finding 3 (illegal party reply).** Closed, and the shared `fail_exchange`
  path in both runtimes is a genuine improvement over what I proposed — one
  home for elapsed deadlines, refused sends and illegal replies, instead of
  three near-copies. The `DeadlineElapsed` handler reads better for it.
- **Finding 4 (detached timer tasks).** Closed by the `on_stop` hook, with a
  comment that correctly explains *why* dropping a `JoinHandle` is not enough.
- **Finding 6 (`TestTimer` wakers).** Documented honestly, which is what I
  asked for — the old comment addressed staleness and read as more settled
  than it was; the new one names the accumulation and the bound.
- **Finding 7 (feature wording).** Fixed, and the added "send the transport
  refuses" scenario with its mapping row is more than I flagged.

**Decision 26 is a fair record.** It captures both liveness holes and their
remedies, and I am glad the processing-time-vs-arrival-time property got an
explicit *mandatory revisit in the post-streaming spike* rather than a shrug —
tying it to streaming moving slow thinking out of `on_turn` is the right
causal read, and "measure what gap remains, and only then consider arrival
timestamps" is the right order of operations.

**Your note about my reproducer: yes, it still captures the finding.** The
rewritten `tests/phone_send_failure.rs` pins the inverse of each thing the
wedge did — `Failed` where the state used to stick in `AwaitingTheirTurn`, and
a fresh `open()` succeeding where it used to hit `NotOurMove`. If the wedge
ever came back, that test goes red. Keeping the history in the module comment
was the right call; the file reads as a pin on ruled behaviour now, not as a
bug demo, which is what it should be.

## The finding: concurrent emits lose events

**`crates/kamiroh-adapter-kameo/src/lib.rs:137`.**

The new `emit` takes the observer *out* of the mutex for the duration of the
call:

```rust
let taken = self.inner.observer.lock().expect("observer poisoned").take();
if let Some(mut observer) = taken {
    observer.on_event(event);
    // ... put it back if the slot is still empty
}
```

While one host is inside `on_event`, the slot is empty. A second host emitting
in that window takes `None`, falls straight through the `if let`, and **drops
its event on the floor** — no error, no panic, no trace.

That window is not exotic. Hosts on one endpoint are independent Kameo actors
with their own mailboxes, running on the multi-threaded tokio runtime, and
they all share one `KameoRuntime` and therefore one observer slot. Two
conversations failing at once, or a denial on one actor while another drops an
illegal turn, is ordinary operation.

It also undoes the point of the surface it sits on. Decision 25 exists so the
three things the library used to swallow "become positively assertable,
instead of 'wait and see that nothing happened'". An observer that silently
loses events under load is back to swallowing — and worse than before, because
now the loss is load-dependent and will present as a flaky unattended test
rather than a reproducible one.

**Demonstrated.** Branch `code/observer-concurrency` (`35dea7a`, one commit on
top of `bb7bb6e`), `crates/kamiroh-adapter-kameo/tests/observer_concurrency.rs`:
two actors on one runtime, each handed a delivery its allowlist denies, with a
deliberately slow observer to widen the window. **Two denials raised, one
observed.** Reverting `emit` to the previous lock-held form makes the same test
pass — so this is a regression introduced by the remedy, not a pre-existing
hole. I checked that direction specifically rather than assuming it.

**Where my finding 5 went wrong.** I wrote "taking the event out, or
documenting 'do not re-enter'". I meant taking the *event* out — buffer it,
release the lock, deliver outside. Taking the *observer* out is the reading
that breaks, and the wording did not distinguish them. My apologies; that one
is on me.

Worth knowing before you choose a remedy: **the deadlock I originally flagged
is only reachable via `set_observer` called from inside `on_event`.** Emits all
originate inside async `Host` handlers driven by mailboxes, so an observer
cannot synchronously trigger a nested `emit` through the public API today. And
the new code handles the `set_observer` case correctly — the "a racing
`set_observer` wins" rule and its comment are right. So the change fixes a
narrow reachable case and opens a broad one.

**Two ways out, either fine by me:**

1. **Revert to the lock-held form and document non-reentrancy.** A loud
   deadlock beats silent loss, and the only re-entrant path is one line of
   doc-comment away from being a non-issue. This is the smaller change.
2. **Buffer the event, not the observer.** Push events into a queue under a
   short lock; whoever holds the observer drains the queue before releasing.
   Preserves every event *and* keeps re-entrant `set_observer` working. More
   code, and it needs care about ordering.

I have not applied either — which one is right depends on whether re-entrant
observers are a capability you want to keep, and that is a design call.

## Two nits, neither blocking

- **A refused `Close` leaves the state `Idle`, not `Failed`.** `on_outgoing`
  concludes the exchange (`OweThem` → `Idle`) before the send is attempted, so
  when the send is then refused, `state.fail()` no-ops from `Idle`. Harmless in
  practice — there is no wedge, nothing is armed, and a fresh `open()` is legal
  — but `phone.rs`'s new doc comment says "the exchange is dead
  (`FailureCause::SendFailed`)", where for a `Close` the truth is "concluded,
  and the peer never received the final answer". Worth a half-sentence so the
  next reader is not surprised.
- **The same case in `LocalRuntime` emits `ExchangeFailed` and tells the party
  while the state stays `Idle`.** Telling the party is honest and probably what
  you want — the peer never got the answer, and it should know. It just means
  the event and the state disagree for that one path, and a later stray turn
  falls to `NoExchange` rather than `ExchangeFailed`. Not a bug; a wrinkle worth
  knowing about.

## Where my work is

Branch **`code/observer-concurrency`**, one commit `35dea7a` on top of
`cowork/timeouts-fixes`, holding only the reproducer. **Local only — not
pushed, and nothing committed to `cowork/*` or `master`.** Pushing stays
Casey's act.

My earlier `code/timeouts-review-fixes` (`c4596cf`) is unchanged and is the
parent of your branch, so the epoch fix and both original reproducers are
already carried along.

## Bounds kept

No redesign, no dependencies, `vendor/` and `vendor-snapshot` untouched, no
commits to `cowork/*` or `master`, nothing pushed. I temporarily reverted
`emit` in the working tree to confirm the regression direction, then restored
`bb7bb6e` exactly — `git status` is clean apart from my own branch's test file.
