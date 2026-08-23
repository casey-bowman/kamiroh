# Architectural and Code Review: kamiroh (Spike 2 — Near-Completion Review)

**Date:** August 23, 2026  
**Reviewer:** Gemini Spark (Autonomous Reviewer)  
**Target:** `kamiroh` repository (`tiers/2-workshop/kamiroh`)  
**Status:** Spike 2 (Workshop-2: Hardening for Unattended Operation) — Feature Implementation Complete  
**Commit Inspected:** `c0e36604`  
**Previous Review:** [2026-08-22-spike-2-review.md](2026-08-22-spike-2-review.md)

---

## 1. Executive Summary & Progress Since August 22

Since the previous review on August 22, 2026, **Spike 2 (`workshop-2`) has advanced from mid-spike progress to feature completeness across all functional roadmap tracks.**

In the last 24 hours, two major capabilities were designed via example mapping, implemented, subjected to adversarial review rounds with mutation/concurrency testing, and merged to `master`:
1. **Story 3: Allowlist Mutation on Running Actors (Decision 28)** — Dynamic `admit` and `revoke` controls on running actors across both `LocalRuntime` and `KameoRuntime`, accompanied by immediate exchange failure (`FailureCause::Revoked`) and initiator-side `Phone::on_revoked` hooks.
2. **Story 4: Same-Endpoint Fan-Out v1 (Decision 29)** — Single-send multi-conversation opening (`Transport::open_many` and `Phone::open_many`), the mailroom batch receipt convention (`BATCH_RECEIPT_NAME`), and wire protocol evolution (ALPN bumped to `kamiroh/1`).

### Current Project Vital Signs
- **Test Suite:** **76 passed, 0 failed** (expanded from 45 tests on Aug 22 and 39 at Spike 2 kickoff).
- **Lints & Style:** `cargo clippy --workspace --tests --examples` zero warnings, `cargo fmt --all --check` clean.
- **Specification:** Four complete Gherkin feature files in `features/` corresponding to Stories 1–4, fully mapped to pinning tests in `features/README.md`.
- **Wire Protocol:** ALPN transitioned from `kamiroh/0` to `kamiroh/1` following the frame format upgrade.

---

## 2. Detailed Technical Evaluation of Newly Landed Capabilities

```
                       ┌─────────────────────────────────────────────────────────┐
                       │                       kamiroh-app                       │
                       │   • Phone::open_many(from, endpoint, names, request)    │
                       │   • Phone::on_revoked(endpoint) -> Option<FailureCause> │
                       │   • LocalRuntime::admit(actor, endpoint)                │
                       │   • LocalRuntime::revoke(actor, endpoint) -> usize      │
                       └───────────▲─────────────────────────────────┬───────────┘
                                   │                                 │
                      implements   │                                 │ defines
                    Driving Ports  │                                 │ Driven Ports
                                   │                                 ▼
┌──────────────────────────────────┴───────┐   ┌─────────────────────────────────────────┐
│         Driving Edge / Adapters          │   │              kamiroh-ports              │
│                                          │   │                                         │
│ • kamiroh-adapter-kameo                  │   │ • Transport::open_many(...)             │
│   - KameoRuntime::admit / revoke (ask)   │   │ • BATCH_RECEIPT_NAME = "mailroom"       │
│   - Host::admit_here / revoke_here       │   │ • batch_receipt_sender(endpoint)        │
│   - Sweep & live exchange failure        │   │ • FailureCause::Revoked                 │
└──────────────────────────────────────────┘   └───────────────────▲─────────────────────┘
                                                                   │
                                                   implements Driven│
                                                   Ports           │
                                               ┌───────────────────┴─────────────────────┐
                                               │         Driven Edge / Adapters          │
                                               │                                         │
                                               │ • kamiroh-adapter-memory                │
                                               │   - MemoryTransport::open_many          │
                                               │   - Mailroom batch receipt generation   │
                                               │                                         │
                                               │ • kamiroh-adapter-iroh                  │
                                               │   - ALPN = b"kamiroh/1"                 │
                                               │   - Frame::OpenMany postcard codec      │
                                               │   - Fan-in delivery before admission    │
                                               └─────────────────────────────────────────┘
```

### A. Dynamic Allowlist Mutation (Decision 28)

#### 1. Core Problem Resolved
The domain core previously supported `Allowlist::admit` and `Allowlist::revoke` with per-delivery enforcement, but runtimes captured the allowlist immutably at actor initialization. Running actors could not be updated without stopping and rebinding them.

#### 2. Architecture & Design Implementation
- **Local Control Only:** `admit(actor, endpoint)` and `revoke(actor, endpoint)` are exposed as local methods on `LocalRuntime` and `KameoRuntime`. Remote/wire mutation was deliberately deferred to Spike 3 alongside the operator port to prevent compromised keys from hijacking their own revocation channel.
- **Revocation as Positive Evidence:** Revoking an endpoint immediately fails all active in-flight exchanges with peers at that endpoint with `FailureCause::Revoked`. The conversation itself survives, allowing subsequent exchanges if the endpoint is re-admitted.
- **Initiator-Side Parity:** `Phone::on_revoked(endpoint)` was added to enable the application driving handle to fail initiator-side exchanges when an application-level allowlist revokes a peer.
- **Connection Isolation (Rule R4):** Revoking an actor's guest does **not** close underlying QUIC transport connections, ensuring policy changes never leak as positive wire signals to the revoked peer. Eviction of compromised peers is documented as a two-step recipe in `docs/EMBEDDING.md`: `runtime.revoke()` followed by `net.close_connection()`.
- **Concurrency & Idempotence Hardening:** In `KameoRuntime`, allowlist mutations use dedicated `ask` messages (`AdmitHere` / `RevokeHere`) rather than external lock coupling, and pre-filter on live turn states to ensure a second revocation of the same endpoint is a quiet no-op at the `Observer`.

---

### B. Same-Endpoint Fan-Out v1 (Decision 29)

#### 1. Core Problem Resolved
Initiating work across multiple colocated actors on a single remote endpoint (such as worker actors spawned by a test harness or supervisor) previously required $N$ independent wire sends, $N$ connections/streams, and $N$ round-trips.

#### 2. Architecture & Design Implementation
- **Type-Enforced Constraint (Rule R1):** `Transport::open_many(from, to_endpoint, to_names, request)` takes a single `EndpointId` and a slice of `ActorName`s. Mixed-endpoint batches are unrepresentable by type definition.
- **Wire Codec & ALPN Upgrade (Decision 19 & 29):**
  - `Frame` in `kamiroh-adapter-iroh` was refactored from a struct into an enum (`Frame::Single` and `Frame::OpenMany`).
  - To prevent silent frame drops and deadlocks between incompatible builds, the wire protocol ALPN was bumped to `b"kamiroh/1"`. Mismatched versions now fail immediately during QUIC ALPN negotiation.
- **Batch Receipt Semantics (The "Mailroom" Convention):**
  - The endpoint acknowledges the entire batch with a single `Message::Ack` carrying the shared `request.id`.
  - The sender address is marked with `BATCH_RECEIPT_NAME` (`"mailroom"`).
  - The batch receipt promises arrival at the endpoint **prior to admission evaluation**. Consequently, if an individual sibling actor denies or does not exist, silence surfaces at the **turn deadline** rather than the ack deadline.
- **Zero-Leak Roster Protection (Rule R4):** If a batch names actors that are not currently bound at the receiving router, the transport silently drops those deliveries without error. This prevents external peers from using batch sends to probe or enumerate local actor rosters.
- **Runtime Independence (Rule R5):** The batch frame is fanned into individual `Delivery` items at the transport adapter layer before hitting the actors' inboxes. As a result, neither `LocalRuntime` nor `KameoRuntime` required any changes to support fan-out.
- **Caller Ergonomics:** `Phone::open_many` executes one wire send and returns `Vec<Phone>`, establishing $N$ independent, ordinary pairwise conversations sharing only their initial request ID.

---

## 3. Roadmap Execution Status (Spike 2 / Workshop-2)

With the landing of Decisions 28 and 29, the implementation status of Workshop-2 candidate items is as follows:

| Roadmap Item | Status | Governing Decisions | Verification & Pinning |
|---|---|---|---|
| **1. Timeouts** | **Delivered** | Decisions 22, 23, 24, 26 | `tests/deadlines.rs` (deterministic `TestTimer`) |
| **2. Disconnect mid-exchange** | **Delivered** | Decision 27 | `tests/disconnects.rs`, `iroh_disconnects.rs` |
| **3. Dynamic allowlist mutation** | **Delivered** | Decision 28 | `tests/allowlist_mutation.rs`, `kameo_allowlist.rs` |
| **4. Deny observability** | **Delivered** | Decision 25 | `Event::DeliveryDenied` via `Observer` port |
| **5. Fan-out v1 (same-endpoint)** | **Delivered** | Decision 29 | `tests/fanout.rs`, `iroh_fanout.rs` (QUIC batch) |
| **6. BDD Layer (Cucumber/Gherkin)** | **Features Mapped** | Q7 ruling | 4 `.feature` files in `features/` mapped to unit tests |

### Remaining Scope Before Spike 2 Graduation:
1. **Cucumber Runner Binding (Roadmap Item 6):**
   - The feature specifications are complete and verified against ordinary tests.
   - The formal integration of `cucumber-rs` as a test runner is an isolated dependency addition that will proceed via the standard `VENDORING.md` workflow.
2. **Vendor Snapshot Refresh:**
   - Push the updated `vendor-snapshot` branch to `kamiroh-workshop-2` upon taking the cucumber dependency.
3. **Graduation to Staging:**
   - Execute the vendor-free snapshot commit to `kamiroh-staging/kamiroh` (`refs/heads/spike-2`) per `TIERS.md`.

---

## 4. Code Quality & Architectural Integrity Review

### Crate Structure and Boundary Discipline
1. **`kamiroh-domain`**: Remains pure Rust with zero external crates (optional `serde`). Domain entities (`TurnState`, `Deadlines`, `Allowlist`, `Address`, `Message`) enforce invariants without I/O entanglement.
2. **`kamiroh-ports`**: Port interfaces are clean, minimal, and fully decoupled from concrete engines. Traits enforce `Send` future bounds for multi-threaded executor safety.
3. **`kamiroh-app`**: Coordinates conversation lifecycles and race conditions (`wait.rs`) deterministically across both test doubles and production runtimes.
4. **`kamiroh-adapter-*`**:
   - `adapter-memory`: Serves as a full-featured network simulator supporting partitions, node kills, and virtual time advancement.
   - `adapter-kameo`: Provides robust actor animation with epoch-based timer invalidation and thread-safe event emission.
   - `adapter-iroh`: Cleanly maps QUIC streams and connection states to kamiroh vocabulary with zero trust placed on unauthenticated wire frames.

### Test Coverage Analysis
The workspace now contains 76 automated tests spanning every critical integration path:
- **Unit & Property Tests:** Strict turn alternation, allowlist set operations, hex encoding, deadline data structures.
- **Choreography & Race Tests:** Deadlines racing arrivals, deaths preempting timeouts, concurrent observer event emission without loss.
- **Fault Injection Tests:** Unannounced container termination, network partitioning and healed packet delivery, allowlist revocation mid-conversation.
- **Real QUIC Transport Tests:** Loopback QUIC conversation flows, socket-level peer shutdown detection, reconnection across dropped connections, and real multi-actor QUIC batch fan-out.

---

## 5. Architectural Readiness for Spike 3 (Workshop-3)

Spike 2 has accomplished its core mission: **kamiroh is now thoroughly hardened for unattended, automated, container-based execution.**

The system is in a strong position to enter **Spike 3 (Conversations that drive agents)**. The foundation addresses all major prerequisites for agent integration:

```
┌─────────────────────────────────────────────────────────────────────────┐
│                    Workshop-3 Readiness Assessment                      │
├────────────────────────────┬────────────────────────────────────────────┤
│ Prerequisite               │ Spike 2 Implementation                     │
├────────────────────────────┼────────────────────────────────────────────┤
│ Unattended Timeout Safety  │ Mandatory, finite Deadlines (Decisions 22) │
│ Fast Disconnect Detection  │ DeathWatch over QUIC closed() (Dec 27)     │
│ Dynamic Agent Permissions  │ Runtime admit/revoke (Decision 28)         │
│ Sub-agent Task Spawning    │ Same-endpoint fan-out (Decision 29)        │
│ Test / CI Observability    │ Observer Event stream (Decision 25)        │
│ Wire Security Invariants   │ Origin authentication + deny silence       │
└────────────────────────────┴────────────────────────────────────────────┘
```

### Primary Focus Areas for Spike 3:
1. **Streaming & Partial Progress:** Offloading long-running generative LLM reasoning from synchronous `Party::on_turn` execution into streaming turns.
2. **Agent Proxy Party:** Implementing the `Party` wrapper that bridges kamiroh turns to active agent processes.
3. **Agent-Control Vocabulary:** Establishing structured lifecycle commands (`start`, `stream_progress`, `deliver_result`, `cancel`).
4. **Remote Operator Port:** Extending allowlist management and capability delegation over the wire with cryptographic authorization.

---

## 6. Conclusion

The development of Spike 2 has progressed cleanly and methodically. All architectural decisions (22 through 29) adhere to the project's core principles of hexagonal isolation, wire silence, and local failure verdicts. The test suite is fast, deterministic, and comprehensive. 

The codebase is ready for the cucumber runner binding and subsequent graduation to `staging`.
