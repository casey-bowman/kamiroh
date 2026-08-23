# kamiroh spike 2 review — 2026-08-22

**Reviewer:** Grok
**Target:** `kamiroh` at `f997e206` (`master`), spike range from `b268a805` (`cowork/spike-2-kickoff`)
**Status:** Mid-spike. Stories 1 (timeouts), 2 (disconnects), and 4 (deny observability) are in; 3 (allowlist mutation) and 6 (cucumber runner) are not.
**Constraint:** inspection only. This review did not re-run the suite, so as not to write into `kamiroh/target`. Same-day local runs on this commit reported the workspace green.

---

## Verdict

The spike question — *can kamiroh be embedded and tested with no one watching?* — is already answered for the two hard cases. Hung exchanges fail on each side's own clock. Vanished peers fail on positive evidence. Conversations survive both. Denials stay mute on the wire and assertable at home. That design is coherent, and the tests pin it from a clock-free domain through a virtual-time toy runtime, a real Kameo host, and real QUIC loopback.

I would not stop the spike for a behavior-blocking defect. I would fix two reference-implementation traps before treating `LocalRuntime` / stock parties as the pattern embedders copy, and I would do allowlist mutation next — not because the roadmap listed it, but because workshop-3's control plane cannot rotate a console key without it.

---

## Findings

### 1. [P2] Stock parties keep a private `TurnState` and never `fail()` it

`EchoParty` and `CountdownParty` (`crates/kamiroh-app/src/parties.rs`) each hold their own `TurnState` and apply `on_incoming` / `on_outgoing` inside `on_turn`. Neither overrides `on_exchange_failed`. The runtime's machine is the authority (decision 23); these copies are labelled "belt-and-braces."

After a live mid-exchange failure the copies diverge:

- Runtime: `fail()` → `Failed`. A fresh `Open` is legal (conversation survives).
- Party: still `AwaitingTheirTurn` or `OweThem`. The same `Open` is `TurnError::MustAnswerFirst`. `on_turn` returns `None`. The runtime has already acked handover. The initiator waits out a turn deadline for an answer that will never come.

`EchoParty` almost never hits this — it always `Close`s in the same `on_turn`, so it is `Idle` between calls. `CountdownParty` hits it as soon as it has posed a `Continue` and the peer then dies or times out. Survival tests (`tests/disconnects.rs`, `kameo_disconnects.rs`) use an `AskingParty` that does **not** track `TurnState`, so they stay green.

The trap is the example. Embedders who copy the stock parties will silently break the spike's "conversation survives" promise. Either drop the private machine (the runtime already validated) or `fail()` it in `on_exchange_failed`. One line, or fewer.

### 2. [P2] `LocalRuntime` treats a failed handover-ack as a step error, not an exchange failure

In `crates/kamiroh-app/src/runtime.rs`, an admitted turn is applied to the machine and its waits are settled **before** the handover ack is sent:

1. `on_incoming` succeeds → state is `OweThem` (or continuing), `ack_due` / `turn_due` cleared.
2. `self.send(..., ack).await?` — on error, `step()` returns `RuntimeError::Transport`.
3. The party is never called. `fail_exchange` is never called. No wait is re-armed.

The conversation is then stuck: `OweThem`, no deadline, no party work. A later fresh `Open` from a resurrected peer is `NotTheirMove` and becomes `IllegalTurnDropped`. That is the opposite of decision 23/27's survival rule, in the runtime `EMBEDDING.md` calls the readable reference.

Kameo does the better thing on the same path: it ignores ack-send errors (`let _ = self.transport.send(...)`) and continues into the party, because the arriving turn already proved delivery (Phone settles `ack_due` on `on_incoming`). A subsequent **reply** send failure does go through `fail_exchange(SendFailed)`, matching decision 26.

Decision 26 said refused sends fail the exchange "uniformly at every send site" for **turns**. The ack is not a turn, so the letter of the decision can be read as silent on this. The spirit is not: a send the transport already answered must not leave an undeadlined exchange. Route a failed handover-ack through `fail_exchange` (or ignore it like Kameo, and let the reply send / death watch / deadline be the verdict). Do not `?` out of `step()` after the machine has moved.

The current disconnect tests kill the peer *after* the responder has already answered, so they do not enter this window.

### 3. [P3] README still describes the spike-1 surface

Two stale claims at the thing embedders read first:

- `README.md` still says the hermetic suite is "39 tests". Spike 2 added deadlines, disconnects, observer, and Kameo/Iroh pins; the workspace now lists 58.
- The ports list is still `Transport`, `Inbox`, `Registry`, `Party`. Spike 2 added `Timer`, `DeathWatch`, and `Observer`. An embedder following only the README will not know time and death are ports.

Same class of miss: both adapter crate rustdocs still say they were written against `kameo = "0.17"` / `iroh = "0.35"` **without compiling**. The code builds against `kameo 0.22` and `iroh 1`. WORKFLOW.md already retired blind-writing. Leaving the assumption lists as current docs tells a new reader the adapters are still speculative.

### 4. [P3] Death watches are easy to leak and never close

`MemoryDeathWatch` and `IrohDeathWatch` are documented as never returning `None`. `KameoRuntime::watch_deaths` returns a `JoinHandle` whose drop **detaches** the task (tokio), while the rustdoc says the handle "ends the watch when aborted or when the watch closes." Close never happens; abort is the only off switch. Drop is a leak.

On the transport side, each `death_watch()` pushes a queue into a `Vec` that is never pruned. Dropped watches keep receiving reports for the life of the net.

Fine at spike scale (one watch per test). Wrong shape for an unattended long-running embedder. Either make drop abort/unsubscribe, or say so in `EMBEDDING.md` next to the DeathWatch paragraph.

---

## Notes — not new findings

These are already accepted in `ARCHITECTURE.md` decisions 26–27. I am not re-raising them.

- Kameo judges **processing time**, not arrival time. A slow `on_turn` can time out other conversations on the same actor. Mandatory revisit post-streaming.
- Kameo death fan-out is a mailbox `tell()`. A kill-and-resurrect inside that latency can stale-kill a fresh exchange. Same revisit.
- Observer is called under the Kameo runtime lock. Re-entry deadlocks. Documented: observe, record, return.
- Iroh suppresses `LocallyClosed` and generation-stale `closed()` reports under the connections lock. That is the right layer for the reconnect race Ander flagged before story 2 was written.
- A refused `Close` leaves the turn machine `Idle` rather than `Failed` (the machine had already concluded). Documented wrinkle in `Phone` and `LocalRuntime`. Both refuse a later stray turn.

The review cycle that produced those notes is visible in the tree (`stale_turn_deadline.rs`, `observer_concurrency.rs`, `tests/closed_death_watch.rs`). That is a good sign: the residues were found by demonstration, then either fixed or written down as rulings.

---

## Mid-spike remaining work

From `docs/ROADMAP.md`, checked against the code:

```
[x] 1. Timeouts          decisions 22–26, tests/deadlines.rs + adapter pins
[x] 2. Disconnects       decision 27, tests/disconnects.rs + iroh_disconnects.rs
[ ] 3. Allowlist mutation  domain admit/revoke exist; neither runtime exposes them on a live actor
[x] 4. Deny observability  Event::DeliveryDenied, pinned
[ ] 5. Same-endpoint fan-out  deferred (house rule; not blocking the spike question)
[/] 6. BDD / cucumber-rs  features written and mapped; runner not bound (Q7: vendor bump)
```

Item 3 is the one I would not defer past this spike. Per-delivery enforcement is already the rule (decision 3); the runtimes just freeze the list at `install`. Until `admit` / `revoke` can reach a running `Host` / `LocalActor`, a leaked endpoint cannot be evicted without restarting the actor. That is already awkward for unattended tests. It becomes a security story the moment workshop-3 puts a console key on every `ops.*` allowlist (see `grok/2026-08-21-two-plane-response.md`). Mutation is the last hardening item that the spike question actually needs.

Item 6 is documentation debt with a standing obligation, not a behavior gap. Every current scenario has a pinning test. Bind cucumber when the next vendor-snapshot refresh happens; do not block graduation on it.

---

Altitude on the project as a whole is in [2026-08-22-overall-sense.md](2026-08-22-overall-sense.md).

---

## What I would do next, in order

1. `fail()` (or drop) the stock parties' private `TurnState`. Add a `CountdownParty` reconnect case next to the existing survival test so this cannot regress.
2. Make `LocalRuntime`'s failed handover-ack go through `fail_exchange` (or match Kameo's ignore). Same window, same test: kill between inbound-queued and ack-sent.
3. Land runtime `admit` / `revoke` for installed actors. That is the remaining spike-2 behavior.
4. Sweep the README test count, ports list, and adapter "without compiling" rustdoc on the way past.

Then snapshot to staging. Streaming and the agent-control vocabulary belong to workshop-3; they should not start with a reference runtime that can swallow a surviving `Open`.
