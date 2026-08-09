# kamiroh Phase 2 — after the lettered plan

Slices A–J are done. This is a fresh pass, not a continuation: the letters came
from one plan written up front, and picking K, L, M off a leftovers list is not
the same activity. What follows is organised by **outcome**, because what is
left is no longer "fill in the architecture" but "make the thing do its job".

Written against the code as it stands, with the claims below checked rather
than remembered.

---

## 1. Where kamiroh actually is

**Real, and load-bearing:**

| | |
|---|---|
| Identity | ed25519 key custody on disk, stable across restarts |
| Allowlist | a file, deny-by-default, fatal on malformed |
| Transport | Iroh, length-prefixed codec, timeouts on both ends |
| Front | inbound accept loop, authenticated `Origin`, no enumeration oracle |
| Controller | one Kameo actor per agent, real lifecycle, `Busy` reachable |
| Console | one pane, one agent, local or remote, all four control verbs |
| Reporting | agent state pushed into Herdr's pane list |

That is a complete, careful control plane. 151 tests, warning-free, with the
security properties written down in ARCHITECTURE.md §6a–§6d.

**Stand-ins and gaps:**

| | state |
|---|---|
| The agent itself | `EchoAgent` returns its prompt. Nothing else. |
| Number of agents | Exactly one, hardcoded `ActorName::new("agent")` in `main.rs` |
| Reachability | `Endpoint::builder(Minimal)` — no relays, no discovery |
| Peer addressing | `peer_address` requires an explicit `SocketAddr` |
| Observability | `println!`/`eprintln!` in the binary; no tracing anywhere |
| Serving-node reporting | A node driven by a *peer* shows nothing in its pane |
| Allowlist reload | `reload()` exists, is tested, has no caller |

---

## 2. The two gaps that stop kamiroh doing its own job

Everything else is polish next to these.

### 2.1 You cannot reach your home node

The README's headline case is "home ↔ cafe, behind NAT". That does not work.
`bind_endpoint` uses Iroh's `Minimal` preset — no relays, no discovery — and
`KAMIROH_PEER` must carry a routable `host:port`. From a cafe, behind NAT, there
is no such address to give it. Every demo so far has used `127.0.0.1`.

This was a correct decision when it was made: F1 deliberately took the smallest
Iroh surface, and relays were "a later, additive decision" (the comment says so).
This is that later.

### 2.2 There is no agent

`EchoAgent` returns its prompt. Every layer beneath it is real, which makes this
the single most conspicuous thing in the system: a careful control plane with
nothing behind it.

**The runtime is already installed.** Herdr's socket API — the same one J2
reports to — exposes exactly what `Agent::run` needs:

```
agent.start  {name, kind, pane_id}        start a coding agent in a pane
agent.prompt {target, text, wait?}        send it a prompt
agent.wait   {target, until[], timeout_ms}  block until done or blocked
agent.read   {target, source, lines, ...}   read what it produced
```

So the answer to "what is a kamiroh agent" is available and specific: **an agent
Herdr is managing.** kamiroh's job becomes what the README always said — giving
allowlisted, peer-to-peer, remote access to agents that are running locally.
That is a much better story than kamiroh growing a process manager of its own,
and it is the reason `Agent` was put in the adapter rather than in the ports
crate.

---

## 3. Milestones

Ordered by what unblocks the most, with the riskiest design done first.

### M1 — It does something real

*Prompt a real coding agent through kamiroh, on one machine.*

`HerdrAgent` implementing `Agent`: `run(prompt)` → `agent.prompt` with a wait →
`agent.read` → return the output as a `Payload`. Replaces `EchoAgent` in the
composition root.

**Done when** a prompt sent through kamiroh reaches a coding agent in a Herdr
pane and its answer comes back — first locally, then from a second node over
Iroh on the same LAN.

**Decisions to settle first:**

- **What "done" means.** `agent.wait` takes `until[]` over Herdr's states.
  `done` is obvious; `blocked` is the interesting one — the agent is waiting for
  a human. kamiroh has no `ControlReply` for "it needs you", and inventing one is
  a domain change, which no slice has needed since B. Options: return the partial
  output with the status, map it onto `ControllerError::Rejected`, or add a
  reply variant. **This is the decision that decides whether Phase 2 touches the
  domain.**
- **How much output.** `agent.read` takes `lines` and a `source`
  (`visible` / `recent` / `recent-unwrapped`). A prompt's answer is not a
  well-delimited thing in a terminal. Decide what a `Payload` contains, and
  accept that it is a heuristic.
- **Timeouts.** `agent.wait` takes `timeout_ms`. A coding agent can work for
  minutes; the Iroh front has its own timeout. These must be reconciled or a
  long prompt will fail at the transport while the agent is still working.
  **This is the one that will bite.**
- **Cancel-safety.** `Agent::run` is aborted on interrupt (ARCHITECTURE.md §6c).
  Aborting mid-`agent.wait` must not leave the pane's agent in a state kamiroh
  then lies about.

**Risk:** highest of the four milestones, which is why it is first. The output
extraction is a heuristic over a terminal, and no amount of care makes that
exact.

### M2 — It works from where I am

*Reach the home node from a different network.*

Relays and discovery: a builder preset other than `Minimal`, and `KAMIROH_PEER`
accepting a bare endpoint id with no `host:port`.

**Done when** a laptop on a different network prompts an agent on the home node,
with no IP address written down anywhere.

**Decisions to settle first:**

- **Which relays.** Number 0's defaults, or self-hosted? A relay sees traffic
  timing and size, not content. ARCHITECTURE.md §5 should state plainly what a
  relay can and cannot observe, because "no central control gateway" is a README
  promise and a relay is the nearest thing to one.
- **Which discovery.** Iroh offers DNS and pkarr. Both publish *something* about
  a node id. Say what.
- **Whether `host:port` survives** as an override. It should — every existing
  test and demo uses it, and it is the only way to test without the internet.

**Risk:** low technically, higher on the security-story side. This is the
milestone that most deserves the advisor gate.

### M3 — More than one agent

*Because the addressing model has always claimed it.*

`PeerAddress` is `EndpointId + ActorName` and the front routes by name, but the
binary registers exactly one agent called `agent`. Agents should come from
config — a name, and what to start it as.

**Done when** two agents run on one node and a peer can drive each by name.

**Decisions:** where agent config lives (next to `allow` and `node.key`), and
whether kamiroh starts agents (`agent.start`) or only attaches to ones already
running. Prefer attach-only first: starting things is where lifecycle bugs live.

### M4 — Operable and trustworthy

*The accumulated debt, which is small and known.*

- **Observability.** There is no tracing at all; the binary uses `println!`.
  For a process that runs for days and talks to peers, that is the gap that will
  hurt at 2am. `tracing` was deliberately dropped from the `kameo` dependency —
  revisit as a workspace-wide decision.
- **The review queue**, unchanged since it started accumulating in F2:
  1. the enumeration argument (rests on statement order — breaks quietly)
  2. malformed-allowlist-is-fatal (can take a fleet down on a bad edit)
  3. `Agent` as an adapter trait
  4. the bounded-mailbox reasoning in G
- **Serving-node reporting** — the known J2 gap; the `AgentController` decorator
  after all, using `pane.report_agent`'s `seq` to order two sources.
- **Allowlist reload trigger** — `reload()` has no caller. A `/reload` console
  command is cheaper than a signal handler and already inside the trust boundary.
- **The three known nits** in LOOP.md, including `KeyStoreError` and
  `AllowlistError` disagreeing about how to say "the environment is
  unconfigured".

---

## 4. What this plan deliberately does not do

- **No kamiroh-side process management.** Herdr manages agents; kamiroh routes
  and authorises. The moment kamiroh spawns processes it has become a worse
  Herdr.
- **No web UI, no HTTP, no multi-tenancy.** All three are README non-goals.
- **No new ports.** A–J needed no port change after B, which is evidence the
  boundary is right. If M1 needs one, that is a signal worth stopping for, not
  routing around.
- **No crates.io release.** Publishing invites users, and the agent is currently
  an echo.

---

## 5. The decision this plan needs

**Is the goal daily use, or something publishable?**

The order above assumes **daily use** — M1 first, because a system that does
nothing real teaches you nothing real. If the goal is publishable instead, swap
M4 forward: observability and the review queue matter more than features when
strangers run it, and the enumeration argument in particular should not go to
strangers unreviewed.

One more, worth deciding once rather than per-milestone: **M1's "blocked"
question decides whether Phase 2 touches `kamiroh-domain`.** Every slice since B
has left the domain and the ports alone. That is worth a lot, and it is worth
knowing before M1 starts whether we are prepared to spend it.
