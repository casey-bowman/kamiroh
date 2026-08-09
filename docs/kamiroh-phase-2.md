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

### ~~M1~~ — It does something real ✅ done

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

**Outcome.** All four decisions landed as written, and the timeout one was
indeed the one that bit: 20s patience under the front's 30s, pinned by a test.
Two things the plan missed — `Agent::run` had to become *fallible* (an
unreachable runtime must not arrive as agent speech), and `Agent` had to move to
`kamiroh-ports` (a second adapter implementing it makes it a boundary). The
output heuristic is as inexact as predicted and is documented as such in
ARCHITECTURE.md §6e.

**The live run against a real `claude` agent then found three bugs no unit test
could have** — a finished agent returns to `idle` rather than `done`, Herdr
reports an expired wait as an error rather than a state, and `local_smoke` was
prompting the real agent at every startup. All three are fixed and pinned. The
lesson is worth carrying into M3: a fake built from a schema agrees with
whatever you assumed the schema meant.

**Only `claude` is exercised.** Each Herdr agent kind has its own detection
manifest, and "which state means finished" is exactly the sort of thing that
differs between them. M3 should exercise at least `codex` or `gemini` before
kamiroh claims to drive agents generally.

### ~~M2~~ — It works from where I am ✅ done (one caveat)

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

**Outcome so far.** Implemented as `Reach::{Direct, Anywhere}` behind
`KAMIROH_REACH`, defaulting to `Direct`. All three sub-decisions went as the
plan asked: n0's defaults for now with self-hosting recorded as the way to
remove the third party; both relay and lookup disclosures written up in
ARCHITECTURE.md §5a; and `host:port` kept, since it is the only thing that works
on a LAN or in a test.

The security-story risk the plan predicted is where the work actually went. The
sharpest sentence to keep: **reachable is not admitted, but "unlisted peers
cannot even find me" stops being true.** That is why it is opt-in.

**Verified live** against n0's real service, with a negative control: the same
bare endpoint id fails under `direct` with an error naming the fix, and succeeds
under `anywhere`. One variable changed, so discovery is demonstrably what did
the work.

**The caveat, stated plainly:** both nodes were on one machine, so this proves
identity-only addressing and *not* NAT traversal. A relay may never have been
used. Closing that needs a second machine on a different network — worth doing
the first time you actually sit in a cafe, rather than simulating it.

This remains the milestone that most deserves the advisor gate: the reasoning in
ARCHITECTURE.md §5a has been read by nobody but its author.

### ~~M3~~ — More than one agent ✅ done

*Because the addressing model has always claimed it.*

`PeerAddress` is `EndpointId + ActorName` and the front routes by name, but the
binary registers exactly one agent called `agent`. Agents should come from
config — a name, and what to start it as.

**Done when** two agents run on one node and a peer can drive each by name.

**Decisions:** where agent config lives (next to `allow` and `node.key`), and
whether kamiroh starts agents (`agent.start`) or only attaches to ones already
running. Prefer attach-only first: starting things is where lifecycle bugs live.

**Outcome.** Agents come from a file; `KAMIROH_AGENT_TARGET` still overrides it,
which kept every demo working. Two agents on one node, driven by name from a
peer, with an unknown name correctly refused. Attach-only as recommended —
kamiroh does not call `agent.start`.

**The second kind found a real per-kind difference, as feared.** A startup
confirmation prompt is `blocked` for `claude` and `idle` for `codex`. kamiroh
reports what Herdr detects and does not second-guess it, so this is a documented
limitation (§6e) rather than a bug kamiroh can fix — inferring state from
terminal output would mean a parser per kind, which agent-agnostic forbids.

**Answered: a remote operator *can* unblock an agent.** Tested against a
workspace-trust dialog in an unapproved directory — `/status` reported
`blocked`, sending `1` cleared the dialog, and the agent then did real work. The
consent still happens; it travels. That is the difference between kamiroh
telling you an agent is stuck and letting you unstick it, and it is most of the
value.

Note what should **not** be done to make this easier: pre-approving trust from a
script. The prompt guards an agent's access to a directory, and automating past
it in a test is one short step from automating past it in the product.

### ~~M4~~ — Operable and trustworthy ✅ done

*The accumulated debt, which is small and known.*

- ~~**Observability.**~~ ✅ Done. `tracing` across app and adapters, subscriber
  in the binary, diagnostics on stderr because stdout is the pane console, and a
  test pinning that agent content never reaches a log. `kameo`'s `tracing`
  feature was revisited and stays off. See ARCHITECTURE.md §6f.
- ~~**The review queue**~~ ✅ Worked through: one mutation-tested, one
  demonstrated, one real bug found and fixed, one struck as resolved by M1. Only
  malformed-allowlist-is-fatal remains, and it is a human decision rather than
  an unexamined argument.
- ~~**Serving-node reporting**~~ ✅ Done, but not as planned: both decorators
  share one reporter instead of being ordered with `seq`, which removes the race
  rather than sequencing it.
- ~~**Allowlist reload trigger**~~ ✅ Done via `SIGHUP`, not a console command:
  the node that needs reloading is the headless one with nobody at its pane.
- ~~**The three known nits**~~ ✅ One fixed (`KeyStoreError::Unconfigured`, so
  the two error types agree); two stand as decisions rather than debt.

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

### 5.1 The domain question, in full

**Will you add `AgentStatus::Blocked` to `kamiroh-domain`?** Worth deciding once,
before M1 starts, rather than mid-slice.

*The case:* an agent stops at "run this command? (y/n)". From a cafe that is the
single most useful thing to know — still working, or waiting on you. Herdr has
`blocked` as one of its five states and built its UI around it.

*Why kamiroh cannot say it:*

```rust
// kamiroh-domain
enum AgentStatus  { Starting, Idle, Busy, Stopped }     // no "waiting on a human"
enum ControlReply { Accepted, Status(AgentStatus), Output(Payload) }

// kamiroh-adapter-kameo
async fn run(&self, prompt: Payload) -> Payload;        // can only return bytes
```

Even when `HerdrAgent` sees `agent.wait` return `blocked`, that fact dies inside
the adapter. There is no return path.

*It is three decisions, and only one is expensive:*

| | change | cost |
|---|---|---|
| 1 | `Agent::run` returns more than a `Payload` | **Forced, cheap.** Adapter-local trait, two implementors, one crate. Needed whatever else is decided. |
| 2 | `AgentStatus::Blocked` | **Cheap, additive.** One byte on the wire (`Starting=1 … Stopped=4`, so `Blocked=5`). Fills the gap §6d already documents. |
| 3 | A `ControlReply` variant carrying output *and* blocked-ness | **Cheaper than it looks.** Four sites — `encode_reply`, `decode_reply`, `console::render`, `report::state_after` — in two crates, every one caught by the compiler. |

*The wire cost is smaller than it looks.* `PROTOCOL_VERSION` guards framing, and
neither change alters framing — both add a discriminant inside the existing
shape. An older node receiving an unknown byte gets `CodecError::Discriminant`,
a clean protocol error rather than a silent misparse. No version bump; mixed
versions fail legibly.

*The guard against motivated reasoning is not the streak, it is agnosticism.*
`Blocked` means "cannot proceed without a human" — it says nothing about what
agents do, so it passes. `AwaitingToolApproval` would be Claude-Code-shaped and
would fail. And the "no domain change since B" record has already delivered its
evidence that the boundary is right; preserving it further is sunk cost.
ARCHITECTURE.md §7 agrees: a needed domain change is "the signal to revisit the
port", not a prohibition.

**Recommended: take all three, with 3 in its minimal additive form** — one
variant meaning "here is what it said, and it is now waiting for you".

An earlier draft of this section recommended deferring 3, on the grounds that it
was expensive and that the gap was worth learning from use. Both were wrong, and
the correction is worth keeping because the reasoning generalises:

- **The cost was inflated** by counting files that merely *mention*
  `ControlReply` rather than the four that match on it exhaustively. One design
  decision and four compiler-caught follow-ons is not expensive.
- **"Learn from use" applies to unknown failures, not predicted ones.** This
  failure is fully specified in advance: a prompt ending blocked returns
  `Output`, `state_after` maps `Output → Idle`, and the pane reports an agent
  that is waiting for you as idle.
- **It contradicts §6d**, written two commits earlier: *"a failure reports
  `unknown`, never `idle` … `idle` would be a guess presented as a fact."*
  Deferring means committing that error deliberately, in the feature whose whole
  purpose is reporting state.
- **For a remote agent the reply is the only channel.** Status lives on the home
  node; the reporter runs on the laptop. A follow-up `/status` after each prompt
  would work, but that is a round trip per prompt to cover a missing field.

What survives from the deferral case is that the *shape* is uncertain until
`HerdrAgent` exists — which argues for the minimal variant now rather than a
rich redesign, not for waiting.

**A staging preference is still legitimate.** Splitting M1 into M1a (working
against `EchoAgent`, domain untouched) and M1b (the domain change) spends
nothing if M1 turns out badly. That is a choice about sequencing, and should be
made as one rather than justified by a cost that is not there.

**If the answer is no**, M1 returns `Output` only and blocked-ness is invisible
to kamiroh. Workable, but the "my agent is stuck" case never reaches you
remotely, which is a good part of why you would want kamiroh at a cafe at all.

---

## Done

All four milestones and the review queue. Continued in
[kamiroh-phase-3.md](./kamiroh-phase-3.md), whose finding is that the four
`ControlMessage` verbs are now the constraint — ample when an agent echoed, too
narrow once a real one is behind them.
