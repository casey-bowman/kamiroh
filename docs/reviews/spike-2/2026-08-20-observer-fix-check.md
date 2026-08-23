# One-commit check: `cowork/observer-fix` (`0c4a956`) — for Mez

*From Ander, 2026-08-20. Third and final pass of the timeouts review cycle:
the observer regression and the two nits from
`2026-08-20-timeouts-fixes-review.md`. Short, because it is clean.*

## Verdict

**Merge-ready. No findings.**

## Build and test

Toolchain **rustc 1.97.0 / cargo 1.97.0 / clippy 0.1.97**, macOS, on
`cowork/observer-fix` at `0c4a956`:

```
cargo test  --workspace                        # 50 passed, 0 failed
cargo clippy --workspace --tests --examples    # clean — ZERO warnings
```

**50 passed, 0 failed** — as expected. No dependency added; `Cargo.lock` and
every `Cargo.toml` untouched. My `concurrent_emits_are_not_lost` reproducer
passes: two denials raised on independent hosts, **two observed**. The event
loss is gone.

## What I checked

- **The regression is fixed, by remedy #1 as ruled.** `emit` calls the
  observer under the lock again, so concurrent emits from independent hosts
  serialize and nothing is dropped.
- **The non-reentrancy contract is documented in all three places it needs to
  be**: inline at `emit`, on `set_observer`, and — the one that matters most —
  on the `Observer` trait itself in `kamiroh-ports`. That last one is where a
  third-party observer author will actually read it, and it is phrased as a
  contract ("observe, record, return") rather than a footnote. This is better
  placement than I suggested; I only named the runtime.
- **Both nits are now described accurately** rather than glossed. The refused-
  `Close` case is genuinely subtle — the machine concludes before the send is
  attempted, so the state says `Idle` while the event says failed — and both
  the `Phone` and `LocalRuntime` comments now say so plainly, including that
  the party is still told honestly that the peer never received its final
  answer. A reader meeting that path cold will not think it is a bug.
- **Decision 26's addendum records the rejected alternative and why.** Worth
  calling out: writing down that releasing the lock *was tried* and
  demonstrably dropped concurrent events is what stops this being
  re-litigated in six months by someone who notices the non-reentrancy rule
  and thinks it looks gratuitous. The trade is stated in the right terms —
  a loud deadlock on a documented rule beats load-dependent silent loss.

Nothing to fix, and nothing I would ask you to change.

## Closing the loop on the review cycle

All seven original findings are now resolved across three commits:

| Finding | Resolution |
|---|---|
| 1 — stale turn-deadline race (STOP) | epoch fix, `c4596cf` |
| 2 — Phone send-failure wedge | decision 26, `bb7bb6e` |
| 3 — illegal party reply un-deadlines | decision 26, `bb7bb6e` |
| 4 — detached timer tasks | `on_stop`, `bb7bb6e` |
| 5 — observer under lock | `bb7bb6e`, corrected in `0c4a956` |
| 6 — `TestTimer` waker accumulation | documented, `bb7bb6e` |
| 7 — feature-file wording | `bb7bb6e` |

Two things from my earlier briefs are still open and are **not** in this
branch, so they do not get lost:

1. **Workshop-2's fork still has no `vendor-snapshot`**, so none of this has
   been verified by an offline cloud build — every green result across all
   three passes is from my 1.97.0 macOS host. Your follow-up errand 1 covers
   it.
2. **EMBEDDING.md** still needs the deadlines / `on_exchange_failed` /
   observer additions, plus the line that `Timer` is not object-safe. Your
   follow-up errand 4.

## Bounds kept

No redesign, no dependencies, `vendor/` and `vendor-snapshot` untouched, no
commits to `cowork/*` or `master`, nothing pushed. Working tree clean at
`0c4a956`.
