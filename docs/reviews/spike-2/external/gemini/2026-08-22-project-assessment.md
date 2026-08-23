# kamiroh — Overall Project Assessment & Strategic Perspective

**Date:** August 22, 2026  
**Author:** Gemini Spark (Autonomous Reviewer)  
**Location:** `gemini/2026-08-22-project-assessment.md`  
**Context:** Mid-Spike 2 (Workshop-2) Evaluation & Trajectory Analysis  
**Companion Document:** [2026-08-22-spike-2-review.md](2026-08-22-spike-2-review.md)

---

## 1. Executive Perspective: Substrate First, Agents Second

Many agent-communication frameworks begin at the top of the stack: prompt protocols, high-level task planners, tool brokers, and LLM-centric orchestration frameworks, leaving transport, connection lifetimes, and network edge cases as ad-hoc secondary concerns.

**`kamiroh` takes the inverted approach: it builds the foundational communications substrate from the wire up.**

Before introducing AI agent semantics or complex agent-control vocabularies, the project has established:
1. Cryptographic transport identities via Iroh QUIC public keys.
2. Deny-by-default perimeter security, verified on every delivery.
3. Strict alternation turn-taking state machines (`TurnState`) that make out-of-order protocol violations unrepresentable.
4. Deterministic timeout choreography and immediate peer disconnect propagation.
5. An in-process fault simulator and virtual clock for hermetic, instantaneous testing.

This bottom-up progression ensures that when agent-driving conversations are introduced in Spike 3, they will execute over a hardened, predictable foundation capable of unattended operation.

---

## 2. Core Architectural Pillars

### Strict Hexagonal Discipline (Ports and Adapters)
The dependency hierarchy is compiler-enforced via dedicated Cargo workspace crates rather than convention:
- **`kamiroh-domain`**: Pure, synchronous, zero-dependency domain logic. It possesses no awareness of I/O, async runtimes, or wire serialization.
- **`kamiroh-ports`**: Trait definitions (`Transport`, `Inbox`, `Registry`, `Party`, `Timer`, `DeathWatch`, `Observer`) with explicit `Send` futures (`RPITIT`). Driven adapters compile against `domain` + `ports` only, remaining entirely blind to the application layer.
- **`kamiroh-app`**: Reusable application choreography (`Phone`, `LocalRuntime`, `Inbound`, `wait`).
- **`kamiroh-adapter-*`**: Pluggable implementations (`memory`, `kameo`, `iroh`) that can be swapped without altering the core.

### Time as I/O & Deterministic Simulation
By treating time as an external device via the `Timer` port and modeling deadlines as pure domain data (`Deadlines`), the project avoids coupling the application layer to real clocks. 
- In production, `TokioTimer` utilizes real `tokio` time.
- In tests, `TestTimer` advances time virtually, enabling multi-minute turn deadlines to trigger deterministically in microseconds with zero flakiness.

### Trust Asymmetry and Wire Silence
- **Trust Model:** An `Endpoint` (public key) is cryptographically proven by the QUIC connection; an `ActorName` is merely claimed by the remote runtime. All admission policy evaluates only the proven endpoint identity.
- **Wire Invariants:** Security denials and local failure verdicts produce zero error chatter on the wire. Unadmitted packets are dropped silently to prevent port-scanning and enumeration attacks, while remaining fully observable at home through the `Observer` port.

---

## 3. Engineering Process and Methodology

The project maintains an exceptional degree of development discipline:

1. **The "House Rule" of Disciplined Deferral:**
   Advanced features (e.g., cross-endpoint fan-out, dynamic peer discovery, complex response correlation, wire protocol negotiation) are deliberately deferred until an actual operational requirement demands them. This protects the core from premature complexity.
2. **Ubiquitous Language as Architecture:**
   The glossary in `ARCHITECTURE.md` (*Conversation, Exchange, Turn, Party, Phone, Deadline, Connection*) provides unambiguous, load-bearing concepts that are reflected consistently across code, tests, and documentation.
3. **Traceable Decision History:**
   The repository preserves a complete, numbered architectural decision log (Decisions 1–27), archived example-mapping boards (`docs/mappings/`), and formal advisories (`docs/advisories/`) documenting trade-offs and rationale.
4. **Tiered Repository Architecture:**
   The multi-tier repo-of-repos structure (`main` $\rightarrow$ `staging` $\rightarrow$ `workshop-N`) with snapshot boundaries cleanly separates exploratory, vendoring-heavy spike history from the canonical repository mainline.

---

## 4. Key Challenges & Strategic Inflection Points Ahead

As `kamiroh` completes Spike 2 and prepares for **Spike 3 (Conversations driving agents)**, the primary architectural challenges center on the shift from fast programmatic actors to slow, generative agent workloads:

### 1. Slow Producers vs. Strict Alternation: The Streaming Imperative
In Spikes 1 and 2, `Party::on_turn` handlers execute quickly (echo, test harnesses). AI agents, however, are slow producers (multi-second LLM inference, long-running tool loops).
- **The Pressure:** Because an actor's mailbox is serialized during `on_turn`, a slow agent occupying the handler will delay queued turns, potentially triggering turn timeouts in other concurrent conversations.
- **The Solution:** Spike 3 must introduce **streaming partials and progress updates**, offloading long-running cognition from synchronous `on_turn` execution.

### 2. Allowlist Lifecycle and Capability Delegation
- **Dynamic Mutation:** Real agent deployments require mutating allowlists on running actors (`admit`/`revoke`), particularly when spawning short-lived worker actors.
- **Scoped Delegation:** An agent spawning sub-agents must be able to grant each sub-agent a strictly narrower allowlist than its own.

### 3. Decentralized Discovery & Identity Topologies
- Dialing currently relies on static peer books or pre-shared endpoint IDs. As multi-agent networks scale, establishing peer discovery without reintroducing centralized broker servers will become an important design area.

---

## 5. Summary Verdict

`kamiroh` represents a well-conceived, highly disciplined approach to peer-to-peer actor networking. By methodically resolving difficult distributed systems challenges—NAT hole-punching, transport death detection, local failure verdicts, and timeout determinism—prior to layering on agent intelligence, the project has built a robust foundation for autonomous agent communication.
