# Completion brief: timeouts (story 1) — `cowork/timeouts`

*From Mez, 2026-08-20. The implementation errand following the example-mapping
session on timeouts & disconnects (board next to the repo; rulings by Casey).
Everything here was designed in that session — decisions 22–25 — and built the
same day, suite green offline in the cloud workspace.*

## What landed

- **Domain.** `deadline.rs`: `Deadlines { ack, turn }` (finite, mandatory —
  no `Default`), `DeadlineKind`, `FailureCause`. `TurnState` gains `Failed`
  (+ `fail()`, `TurnError::ExchangeFailed`): a dead exchange refuses
  everything but a fresh `Open`, pinned by a unit test.
- **Ports.** `Timer` (associated `Instant`, `now()`, `sleep_until`; time is
  I/O), `Event`/`Observer` (`ExchangeFailed`, `DeliveryDenied`,
  `IllegalTurnDropped`; closures qualify), `Party::on_exchange_failed`
  (default no-op) with its `DynParty` twin.
- **App.** `wait::next_or_deadline` — the one racing step; an elapsed
  deadline deterministically outranks a queued delivery. `Phone` takes
  mandatory `Deadlines` + a `Timer`, tracks its waits (`next_due`,
  `note_ack`, `on_deadline_elapsed`), arms on send, settles on arrival.
  `LocalRuntime` takes a timer + `spawn_deadlines`; installs take
  `Deadlines`; `step()` races the earliest due and returns `Stepped`
  (`Delivered` | `ExchangeFailed`); emits all three events; notifies the
  party on failure.
- **Kameo adapter.** Same choreography with real tokio timers: per-peer
  sleep-tasks that tell the host `DeadlineElapsed`, aborted on settle, stale
  reports recognized (ack-id / pending-wait re-check) and ignored.
  `TokioTimer` exported for Phones and pumps hosted alongside. Observer and
  `spawn_deadlines` on the runtime.
- **Memory adapter.** `testing::TestTimer` — hand-advanced virtual clock;
  60-second deadlines elapse in microseconds.
- **Tests.** `tests/deadlines.rs`: five deterministic scenarios (ack never
  comes; peer never answers + late-turn refusal + conversation survival;
  responder fails on its own clock with party told and observer seeing it;
  timely answer doesn't fail; denials and illegal turns observable).
  Workspace suite: **45 passed, 0 failed**, offline, clippy clean.
- **Spec.** `features/hung-exchanges-fail-loudly.feature` + `features/README.md`
  (scenario→test mapping, conversion obligation). Decision log 22–25;
  glossary gains **Deadline**; README status updated.

## Follow-up errands (named, so they aren't lost)

1. **Wire cucumber-rs and bind `features/`** (Q7 ruling). A dependency bump:
   goes through a local build session per VENDORING.md (add dep, refresh
   `vendor-snapshot` — which workshop-2's fork still lacks; this errand
   should push the fork's own). Every scenario in `features/` becomes a bound
   step definition; the README mapping table retires.
2. **Disconnects (story 2).** Rulings all in hand: R6–R8, Q5 (reconnect
   promise pinned by test), Q6 (fault surface in the memory adapter proper).
   Builds the kill-endpoint/partition levers, the transport death-reporting
   event (`FailureCause` grows a variant), and the iroh adapter's `closed()`
   subscription.
3. **Language pass** on the decision-log/feature wording for average library
   users (Casey's request re Q1 phrasing) — simplify the words, not the
   design; natural to fold into the EMBEDDING.md update below.
4. **EMBEDDING.md** does not yet mention deadlines, `on_exchange_failed`, or
   the observer — an embedder following it today would miss required
   arguments.

## For review (Ander)

The kameo host's timer tasks are the riskiest part (abort/stale-tell races —
mitigated by the pending-wait re-check; see `DeadlineElapsed` handler). The
`Deliver` handler's arming block runs after the transport send succeeds;
failure-to-send leaves no waits armed on purpose (the exchange will fail by
the peer's deadlines, or ours on the next legal send attempt). Newer clippy
may see what 1.95's missed.
