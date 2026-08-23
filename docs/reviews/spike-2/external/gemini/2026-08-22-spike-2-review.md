# Architectural and Code Review: kamiroh (Spike 2 / Workshop-2)

**Date:** August 22, 2026  
**Reviewer:** Gemini Spark (Autonomous Reviewer)  
**Target:** `kamiroh` repository (`tiers/2-workshop/kamiroh`)  
**Status:** Mid-Spike Review (Spike 2 — Workshop-2: Hardening for Unattended Operation)  
**Commit Inspected:** `f997e206` (and `57433294` vendor-snapshot)

---

## 1. Executive Summary

`kamiroh` is an architectural experiment building peer actors over [Iroh](https://www.iroh.computer/) and [Kameo](https://crates.io/crates/kameo) to facilitate peer-to-peer conversations across the internet with zero server infrastructure in between.

Following the success of Spike 1 (`workshop-1`), which validated the core protocol stack across real networks with direct QUIC hole-punching, **Spike 2 (`workshop-2`)** focuses on answering a crucial engineering question:

> *Can kamiroh be embedded in a host application and tested with no human watching?*

The repo is currently **in the middle of Spike 2**. The two most complex and fundamental hardening tracks — **Story 1 (Timeouts)** and **Story 2 (Disconnects)** — along with **Deny Observability** have been successfully designed via example mapping, implemented, hardened through adversarial review rounds, and thoroughly documented in `ARCHITECTURE.md` (Decisions 22–27).

### Key Highlights of Work Completed in Spike 2
- **Mandatory, Finite Deadlines (Decisions 22, 24):** Introduced the `Timer` port and `Deadlines { ack, turn }` data types. Unbounded waits are unrepresentable at all construction sites (`Phone::converse`, actor installation, and runtime initialization).
- **Domain Turn Machine Aware of Death (Decision 23):** `TurnState::Failed` turns dead exchanges into unrepresentable states while guaranteeing conversation survival across exchanges.
- **Dual Local Failure Reporting & Observability (Decision 25):** Application semantics (`Party::on_exchange_failed`) are cleanly decoupled from operator/test assertions (`Observer` receiving `Event::ExchangeFailed`, `Event::DeliveryDenied`, and `Event::IllegalTurnDropped`). Zero failure information crosses the wire, preserving deny-by-default invariants.
- **Immediate Failure on Positive Evidence (Decisions 26, 27):** Refused sends, illegal party replies, and peer death events (`DeathWatch` via QUIC `closed()` or simulated `kill_endpoint`) fail in-flight exchanges immediately without waiting for timeout backstops.
- **Executable Specifications & Mapping:** Gherkin feature files (`features/hung-exchanges-fail-loudly.feature` and `features/vanished-peers-fail-loudly.feature`) drafted and formally mapped to unit/integration test suites.

---

## 2. Architectural Analysis & Spike 2 Design Decisions

The additions in Spike 2 preserve the strict ports-and-adapters (hexagonal) structure established in Spike 1, while introducing critical operational semantics.

```
                    ┌─────────────────────────────────────────────────────────┐
                    │                      kamiroh-app                        │
                    │   ┌─────────────────────────────────────────────────┐   │
                    │   │                 kamiroh-domain                  │   │
                    │   │  • Address / Endpoint / Secret / ActorName      │   │
                    │   │  • Allowlist (Deny-by-default per delivery)     │   │
                    │   │  • TurnState (Strict alternation + Failed)      │   │
                    │   │  • Deadlines (ack, turn) & FailureCause         │   │
                    │   └─────────────────────────────────────────────────┘   │
                    │                                                         │
                    │   • Phone<T, W> (Wait tracking & alternation check)     │
                    │   • LocalRuntime (Toy reference runtime)                │
                    │   • wait (next_or_deadline_or_death determinism)        │
                    │   • inbound (process, admit, handover ack)              │
                    └───────────▲─────────────────────────────────┬───────────┘
                                │                                 │
                   implements   │                                 │ defines
                 Driving Ports  │                                 │ Driven Ports
                                │                                 ▼
┌───────────────────────────────┴──────────┐   ┌───────────────────────────────────────┐
│          Driving Edge / Adapters         │   │             kamiroh-ports             │
│                                          │   │                                       │
│ • Embedding App (Party / DynParty)       │   │ • Transport (send with Send futures)  │
│ • kamiroh-adapter-kameo                  │   │ • Inbox (next delivery)               │
│   - KameoRuntime / Host actor            │   │ • Registry (bind address -> Inbox)    │
│   - Real TokioTimer                      │   │ • Party / DynParty (on_turn, failed)  │
│   - PeerWaits with epochs                │   │ • Timer (sleep_until, now)            │
│   - Observer lock protection             │   │ • DeathWatch (next_death)             │
│                                          │   │ • Observer / Event                    │
└──────────────────────────────────────────┘   └───────────────────▲───────────────────┘
                                                                   │
                                                implements Driven  │
                                                Ports              │
                                               ┌───────────────────┴───────────────────┐
                                               │         Driven Edge / Adapters        │
                                               │                                       │
                                               │ • kamiroh-adapter-memory              │
                                               │   - MemoryNet, MemoryTransport        │
                                               │   - Fault levers: kill_endpoint,      │
                                               │     partition, heal                   │
                                               │   - TestTimer (virtual clock)         │
                                               │                                       │
                                               │ • kamiroh-adapter-iroh                │
                                               │   - IrohNet / IrohTransport           │
                                               │   - QUIC connection cache & closed()  │
                                               │   - Generation-guarded DeathWatch     │
                                               └───────────────────────────────────────┘
```

### 1. Separation of Time and Domain Logic (Decisions 22, 24)
- **Design:** The domain layer (`kamiroh-domain`) remains pure and clock-free. `Deadlines` are pure data structures (`Duration`).
- **Timer Port:** `Timer` in `kamiroh-ports` provides an asynchronous `sleep_until` contract. The application layer (`wait.rs`, `Phone`, `LocalRuntime`) coordinates timeout logic identically across environments.
- **Testing Advantage:** In tests, `TestTimer` advances time virtually by hand (`advance(Duration)`), allowing a 60-second turn deadline to trigger deterministically in microseconds with zero flakiness.

### 2. Failure Semantics & Wire Invariants (Decisions 22, 23, 25)
- **Local Judgment:** Neither timeouts nor failure causes are transmitted over the wire. Each endpoint arrives at its verdict independently based on local evidence. This avoids the classical Two Generals dilemma (e.g., rejecting secondary acknowledgments).
- **Silent Denials:** Unadmitted packets continue to be dropped silently on the wire (preventing probing/enumeration attacks), while being routed locally to the `Observer` as `Event::DeliveryDenied`.
- **Exchange Terminal vs. Conversation Survival:** When an exchange fails, `TurnState` transitions to `Failed`, refusing subsequent stale replies (`TurnError::ExchangeFailed`). However, the conversation remains open, allowing an immediate subsequent `Turn::Open`.

### 3. Positive Evidence & Accelerated Failure (Decisions 26, 27)
- **Immediate Disconnect Surface:** While timeouts serve as a silent backstop, positive evidence (a failed socket write `SendFailed`, an illegal reply `IllegalReply`, or a transport disconnect `Disconnected`) fails the exchange instantly.
- **DeathWatch Port & QUIC Integration:** The `DeathWatch` port feeds peer disconnects into runtimes. `kamiroh-adapter-iroh` maps QUIC `closed()` events to peer deaths, with explicit suppression for `LocallyClosed` events (routine cache management is not peer death).
- **Reconnect Race Guard:** `IrohTransport` holds the connection mutex across death checking to ensure that if a newer connection to the same peer has already been established, stale death reports from older connections are discarded.

---

## 3. Codebase & Implementation Assessment

### Domain Core (`crates/kamiroh-domain`)
- **Purity & Dependencies:** Zero dependencies (optional `serde`).
- **Value Objects:** Strong typing with `EndpointId`, `ActorName`, `Address`, `Hex`, `Secret`, `RequestId`.
- **State Machine:** `TurnState` accurately reflects strict alternation and fail-fast invariants.

### Application Layer (`crates/kamiroh-app`)
- **Choreography:** `Phone` cleanly manages caller-side state, tracking `ack_due` and `turn_due` deadlines.
- **Inbound Processing:** `process()` in `inbound.rs` serves as the centralized gate for admission and message conversion.
- **Race Determinism:** `next_or_deadline_or_death` in `wait.rs` establishes strict priority polling:
  $$	ext{Peer Deaths} \succ 	ext{Deadlines} \succ 	ext{Inbound Deliveries}$$
  This ensures that known deaths or expired deadlines deterministically preempt queued deliveries.

### Adapters (`crates/kamiroh-adapter-*`)
- **Memory Adapter (`kamiroh-adapter-memory`):** High utility for test suites. Features full network fault simulation (`kill_endpoint`, `partition`, `heal`) and virtual time management (`TestTimer`).
- **Kameo Adapter (`kamiroh-adapter-kameo`):** Animates actors with real `tokio` concurrency. 
  - *Hardening:* Incorporates arming epochs (`PeerWaits.epoch`) to discard stale timeout messages resulting from aborted timer tasks.
  - *Observer Safety:* Invokes observers under a mutex to guarantee zero event loss under concurrent emits.
- **Iroh Adapter (`kamiroh-adapter-iroh`):** 
  - Authenticated origin extraction from connection keys (not wire frame content).
  - Single-frame uni-streams (`ALPN = b"kamiroh/0"`).
  - Integration with Iroh 1.0 endpoint primitives.

---

## 4. Test Suite & Quality Verification

The test suite exhibits rigorous coverage across domain rules, state transitions, fault injection, and real network interaction:

| Test Scope | Primary Files | Key Invariants Pinned |
|---|---|---|
| **Timeouts & Deadlines** | `tests/deadlines.rs` | Missing ack failure, missing turn failure, late turn refusal, timely turn success, observer/party notification, illegal reply failure, send refusal failure. |
| **Disconnects & Faults** | `tests/disconnects.rs` | Killed peer immediate failure, conversation survival across death, wire blip partition & heal, silent death backstop, initiator-side death watch. |
| **Kameo Concurrency** | `crates/kamiroh-adapter-kameo/tests/` | Observer event serialization under concurrency (`observer_concurrency.rs`), stale deadline task suppression (`stale_turn_deadline.rs`), multi-actor conversation flow (`kameo_conversation.rs`), actor disconnects (`kameo_disconnects.rs`). |
| **Real Iroh QUIC Sockets** | `crates/kamiroh-adapter-iroh/tests/` | Real QUIC loopback conversations (`iroh_conversation.rs`), peer shutdown reporting, and conversations spanning connection drops (`iroh_disconnects.rs`). |
| **Cross-Crate Protocols** | `tests/` | Alternating turns (`turns_conversation.rs`), harness lifecycle commands (`harness_conversation.rs`), closed death watch safety (`closed_death_watch.rs`). |

---

## 5. Mid-Spike Status: Accomplishments vs. Remaining Scope

According to `docs/ROADMAP.md` (Workshop-2 Candidate Implementation Items), the progress for Spike 2 stands as follows:

```
[x] 1. Timeouts (Decisions 22-26) -------------------- COMPLETE
[x] 2. Disconnects mid-exchange (Decision 27) --------- COMPLETE
[ ] 3. Dynamic allowlist mutation --------------------- PENDING
[x] 4. Deny observability (Decision 25) -------------- COMPLETE
[ ] 5. Fan-out v1: same-endpoint only ---------------- PENDING / DEFERRED
[/] 6. BDD layer: Cucumber + Gherkin ----------------- PARTIAL (Features written & mapped; runner binding pending)
```

### Remaining Tasks for Spike 2 Completion:
1. **Dynamic Allowlist Mutation (Roadmap Item 3):**
   - The domain supports `Allowlist::admit` and `Allowlist::revoke`, but neither `LocalRuntime` nor `KameoRuntime` provides an API to dynamically mutate the allowlist of an already-installed actor.
2. **Cucumber Runner Binding (Roadmap Item 6 / Ruling Q7):**
   - Feature files (`features/*.feature`) are documented and manually mapped in `features/README.md`.
   - The formal binding to `cucumber-rs` step definitions requires adding the dependency and updating `vendor-snapshot`.
3. **Vendor Snapshot Alignment:**
   - As noted in `docs/briefs/2026-08-19-cloud-capability-check.md`, a dedicated `vendor-snapshot` branch on the `workshop-2` fork should be pushed when dependencies are next updated.

---

## 6. Technical Debts, Risks & Architecture Review Notes

### 1. Mailbox Occupancy vs. Arrival Time in Kameo Runtime (Documented in Decision 26)
- **Observation:** `KameoRuntime` starts timer deadlines when messages are handled in `handle(Deliver)`. If an actor's mailbox is occupied by a long-running handler, queued turns may be judged late even if they arrived on time over the network.
- **Assessment:** Acceptable for Spike 2 because actors in this spike execute short turn handlers. 
- **Recommendation:** Revisit in Spike 3 when streaming is introduced, which will offload long-running thinking from `on_turn`.

### 2. Kameo Death Report Fan-out Latency (Documented in Decision 27)
- **Observation:** In `KameoRuntime`, `watch_deaths` broadcasts `PeerDied` via mailbox messages (`tell()`). A rapid kill-and-reconnect cycle occurring within mailbox queue latency could theoretically deliver a death message to a freshly reopened exchange.
- **Assessment:** Rare edge case in practical scenarios; clean reconnection is proven over Iroh sockets. Scheduled for holistic revisit alongside arrival timestamps in the post-streaming spike.

### 3. Observer Non-Reentrancy Contract (Decision 26)
- **Observation:** `KameoRuntime::emit` calls the observer with its internal lock held to ensure no events are dropped during concurrent execution.
- **Risk:** If an observer implementation attempts to invoke runtime mutation methods (`install`, `stop`, `set_observer`) from within `on_event`, it will deadlock.
- **Assessment:** This contract is clearly documented in `ARCHITECTURE.md` and `EMBEDDING.md` ("Observe, record, return").

---

## 7. Conclusion & Next Steps

`kamiroh` in Spike 2 is in an exceptionally clean state. The ports-and-adapters architecture is faithfully preserved, the test coverage is deterministic and thorough, and the edge cases surrounding async network failure have been systematically resolved.

### Recommended Next Actions:
1. **Implement Dynamic Allowlist Mutation** on running actors (`runtime.admit(actor, endpoint)`, `runtime.revoke(actor, endpoint)`).
2. **Bind `cucumber-rs` to `features/`** to complete the BDD executable specification milestone.
3. **Graduate Spike 2 to Staging** via a clean snapshot commit per the `TIERS.md` workflow before kicking off Spike 3 (Agent-Driving Conversations).
