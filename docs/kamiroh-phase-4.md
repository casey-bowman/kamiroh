# kamiroh Phase 4 — a substrate for agents, not a remote control for people

**Status: a sketch, not a plan.** Phase 3 is not finished — its NAT test and its
"say plainly what kamiroh does" document are outstanding. This exists because the
*purpose* got stated more precisely than the plans had assumed, and the
consequences are worth writing down while they are fresh rather than rediscovering
them later.

> **Naming, to avoid a collision:** phase 3 has a milestone called P4
> ("let someone else run it" — publishing). This document is *phase* 4. Its
> milestones are S1–S4, for **substrate**.

---

## 1. Where this came from

Asked what the value proposition was, the answer that came back was thin in one
direction and strong in another, and Casey supplied the missing context:

> "I am addressing a need for agents to run peer-to-peer apps on nodes
> distributed on the internet, among other things."

with two clarifications: agents mostly **delegate** — A asks B and waits — and
the **Kameo actor model is the point**, in that one should interact with agents
the way one interacts with actors.

That reframes kamiroh. Everything through phase 3 was built for a human at a
pane driving an agent elsewhere. This is a different system with the same parts:
the callers are mostly programs, the interesting traffic is agent-to-agent, and
a human is one caller among many and often absent.

**The design has held up well against the reframing** — the decisions argued
hardest turn out to be the ones composition needs. What is missing is
identifiable, and it is not evenly distributed.

---

## 2. The finding: an actor system missing what makes actors compose

| property | state |
|---|---|
| Addressable identity | ✅ `ActorName` locally, `EndpointId + ActorName` remotely — `PeerAddress` *is* "which node, which actor" |
| Mailbox, one message at a time | ✅ Kameo's, and guarded hard: §6d is all mailbox discipline |
| Location transparency | ⚠️ real but partial — `Link` makes local and remote identical *for the console*, which never branches on it. Not available to an agent |
| Ask / tell | ⚠️ the actor supports `tell` internally (`Running.reply` is optional); the wire is always request/reply |
| Supervision | ❌ deliberately Herdr's. Nothing restarts a failed agent, and `Detach` is final for the process lifetime |
| **Actors send to actors** | ❌ **agents receive only** |

The power of "interact with agents as actors" is not mainly the interface a
caller gets. It is that actors are a **uniform substrate**: any actor can address
any other, locally or remotely, with one primitive, and systems are built by
composing that. kamiroh has the addressing uniformity and not the sending
uniformity.

### 2.1 What delegation exposes: a caller cannot collect a result

Walk A-asks-B through the verbs as they exist:

```
A → Prompt B with the work
    ← Partial{Busy} + a screen snapshot          (20s, DEFAULT_PATIENCE)
A → AwaitSettled
    ← Status(Idle)                                B has finished
A → …?                                            nothing returns B's answer
```

Output reaches a caller in exactly one way: as the reply to a `Prompt`.
`AwaitSettled` reports *that* an agent settled, never *what it produced*.

For a human this is survivable — await, then look at the pane. **Agent A has no
pane to look at.** Its only route to the result is to prompt B again, which types
at an agent that has just finished and returns the terminal's current contents as
a side effect.

Phase 3 §2.1 treated the missing read verb as a design nicety and P1 answered
only the waiting half. Under delegation it is a blocker, and it is the first
thing to fix.

### 2.2 Authority does not travel: this is an ACL, not capabilities

In Kameo an `ActorRef` is an unforgeable handle, and handing one to another actor
*is* the act of granting authority. kamiroh's address is a **name plus an
allowlist entry**, which is an access-control list: C's node must already admit B,
in a file, before B can reach C.

For delegation chains that is the load-bearing difference. If A delegates to B and
B must delegate onward to C, the authority does not travel with the request —
every edge is pre-authorised in advance on the receiving node, so **the topology
has to be known before it runs**. An agent cannot be handed the right to use a
peer it did not already have.

Not a defect. ACLs are legitimate, far simpler to reason about, and auditable in a
way capabilities are not. But it is currently *implicit*, and it constrains
everything else here, so it should be decided rather than drifted into.

### 2.3 Herdr is optional at both ends — one of which is already true

Casey's framing, and it separates three things that get bundled under "Herdr":

| | Herdr-coupled? |
|---|---|
| the **console** (`console::serve`) | **no** — it takes any `AsyncBufRead`/`AsyncWrite`, mentions Herdr only in a doc comment, and is tested with a `String` and a `Vec<u8>` |
| the **pane** (`Pane`, `report`) | yes — that is Herdr's socket |
| the **agent** (`HerdrAgent`) | yes — **and it is the only real `Agent` there is** |

**The caller end already works without Herdr, and nothing says so.**
`bind_endpoint`, `IrohTransport`, `peer_address` and `codec` are all public, so a
program can bind an endpoint and drive a remote agent with no console, no pane and
no Herdr on that machine:

```rust
transport.send(&peer_address(id, "builder")?, ControlMessage::Prompt(payload)).await
```

That is a **packaging and documentation gap, not a design one**: there is no
`kamiroh::Client` facade, so a caller wires what the composition root wires, and
nothing documents it because the console is the only client anyone has
demonstrated. It also gives P3a a second audience — someone writing a program
against kamiroh, not only someone typing at it.

**The agent end is where Herdr is currently mandatory**, and that is what makes
the closing question of §5 a requirement rather than a question.

#### The use case that settles it: a test suite driving agents on many nodes

A Cucumber suite that has several agents on different nodes do things, and asserts
on the results. One end is an application using kamiroh; the other end is agents
that need not be panes.

What a non-pane agent buys immediately:

- **Structured output instead of a screen scrape.** `AgentOutcome` already carries
  an arbitrary `Payload`; a subprocess agent returns what the process produced.
- **Non-text payloads.** The text-only restriction exists solely because a pane
  takes keystrokes.

What it does **not** fix, stated precisely because it is easy to assume otherwise:
**S1 remains necessary.** Any agent doing minutes of work must still return inside
the caller's patience, so `run` ends in `Partial{Busy}` and the real result arrives
later with nobody waiting. The substrate change makes that result a *well-defined
value the agent could hold and hand over* rather than a terminal snapshot one hopes
contains the answer. S1 goes from heuristic to clean — a large improvement in its
design, none in its necessity.

**Two frictions this use case surfaces that the milestones did not:**

- **An ephemeral caller fits an ACL badly.** A suite that generates a fresh key
  each run has a new endpoint id each run, and every agent node's allowlist would
  need updating before it could connect. The practical answer is easy — a fixed
  test identity via `KAMIROH_KEY_FILE`, allowlisted once per node — but it is a
  concrete instance of S4. The moment callers are *created* rather than
  *administered*, an ACL is the wrong shape, and that is the pressure that would
  push toward capabilities.
- **One prompt at a time is a controller property, not a pane property.** Parallel
  scenarios against the *same* agent are answered `Rejected`, not queued. Different
  agents are unaffected. Whether that bites depends on how work is sharded, and it
  is worth knowing before it is discovered in a test run.

---

## 3. Milestones

### S1 — A caller can collect what an agent produced

The blocker from §2.1. A verb that returns an agent's output without prompting it.

**Done when** A can delegate to B, wait, and retrieve B's answer without sending
anything that reaches the agent.

**Most of its design is already settled**, by the P1 investigation:

- **Last-N, not a cursor.** `agent.read` takes `target, source, lines, format,
  strip_ansi` — no offset, no cursor, no since-revision. A positioned read is not
  expressible, so a cursor would have to be invented by diffing terminal
  snapshots, which is a heuristic stacked on a heuristic.
- **`source` is the real parameter.** `visible` versus `recent`, and which one is
  legal depends on whether the agent is working — see ARCHITECTURE.md §6e.

**Decisions to settle first:**

- **Does the verb carry parameters at all?** `AwaitSettled` carries none, on the
  argument that a peer should not choose how much of a node's resources it
  consumes. The same argument applies weakly to `lines`. Consistency says no
  parameters; usefulness says a caller may genuinely want more or less.
- **The honest problem underneath, which may matter more than the verb.** A
  terminal has no marker for "this is the answer to that prompt", so what comes
  back includes the agent's own prompt echo, its splash screen, and whatever else
  is on screen. That is tolerable for a human reading it and **poor input for a
  program**. Agent-to-agent delegation may want a non-pane `Agent`
  implementation — one whose output is structured because it was never a
  terminal. The port already permits it; nothing has needed it yet.

  Worth asking before building S1: is the pane the right substrate for
  agent-to-agent work at all, or only for human-to-agent work?

### S2 — Agents that originate

The gap in §2. Today `Agent` is `run`, `status`, `await_settled` — all reactive.
An agent is handed a prompt and answers; it has no handle on `Transport` and no
way to decide to message a peer. The only things originating outbound traffic are
the pane console and the startup greet.

**Done when** an agent on node 1 can address and drive an agent on node 2 without
a human initiating it — at which point the README's "peer actors that message
each other" becomes true.

**This is the largest item here, and the one with the most ways to go wrong:**

- **Which port.** An agent that sends needs a driven port. `Transport` already
  exists and is exactly this shape; the question is whether an agent gets one, or
  gets a narrower handle that can only reach peers it is configured for. The
  dependency rule is not in danger either way — this is inward-pointing — but the
  *authority* question is: an agent holding a `Transport` can dial anything its
  node's peers will admit.
- **Cycles.** A prompts B, B prompts A. Kameo's bounded mailbox and the
  no-await-inside-a-handler rule already exist for local hazards; a cycle across
  nodes is the distributed version and neither rule covers it. A prompt that
  waits on a peer that is waiting on you is a distributed deadlock with a 30s
  timeout as its only floor.
- **Whose identity is on the wire.** When agent B on node 2 sends to node 3, node
  3 authenticates *node 2*, not agent B. So without S3 the grant is
  node-to-node even when the intent is agent-to-agent.

### S3 — Per-agent authorisation

`ControlService::deliver` checks the origin endpoint against the allowlist; the
agent name is used for routing and logging and **never for the trust decision**.
Admitting a peer therefore admits it to every agent that node hosts.

Fine when a node hosts one agent for you. Wrong for composition, where you want
"node A may drive `builder`, not `deployer`".

**Cheap now and expensive later**, which is the argument for doing it before
anyone depends on the current shape. `Allowlist::is_allowed(&EndpointId) -> bool`
becomes agent-aware; the app layer and both allowlist adapters follow. The file
format can stay backward-compatible — a bare endpoint id keeps meaning "every
agent on this node", with an optional agent list narrowing it.

**One constraint that is not obvious and must not be missed.** F2's enumeration
property holds because authorisation runs *before* lookup, so an unlisted peer
gets byte-identical `REFUSED` whether or not the agent it named exists. With
per-agent grants there is a new case: a peer allowed for `builder` asking for
`deployer`. It must answer `REFUSED`, never `NO_SUCH_ACTOR` — otherwise a
partially-admitted peer can enumerate the agents it is not allowed to use, which
is the same leak in a new place. `an_unlisted_peer_learns_nothing_beyond_refused`
will need a sibling.

### S4 — Decide whether authority travels

The §2.2 question, and a decision rather than a task.

- **Stay an ACL.** Topology known in advance, allowlists managed by config,
  auditable, simple. Everything above works.
- **Something capability-shaped.** A handed reference that grants access, so A can
  delegate to B the right to use C. Much larger: unforgeable references,
  delegation, expiry, revocation — and a revocation story is exactly what the
  allowlist gives for free today via `SIGHUP`.

**Recommendation: stay an ACL, and record it as decided.** But decide it *before*
S2 lands, because what an agent may do with an outbound handle is precisely this
question, and retrofitting capabilities onto a deployed ACL is close to a rewrite.
It belongs in [OPEN-DECISIONS.md](./OPEN-DECISIONS.md) once proposed.

---

## 4. What this deliberately does not do

Carried forward from phase 3 §4, all still true and now load-bearing for a bigger
claim:

- **No supervision, and no starting agents.** Herdr's, deliberately. Note this is
  a real departure from the actor model — in Kameo a supervisor restarts a failed
  actor; in kamiroh nothing restarts an agent, and `Detach` is final for the
  process lifetime. Worth stating plainly in any document that leans on the actor
  metaphor, because it is the place the metaphor breaks.
- **No parsing agent output.** A parser per agent kind is what agent-agnostic
  forbids. This is what makes S1's output a heuristic rather than a structure, and
  it is the constraint that may push toward a non-pane agent instead.
- **No queuing or retry.** Delegation here is ask-and-wait, confirmed, so
  request/reply fits. If a node is asleep the caller is told `unreachable` and
  decides. Revisit only if notify-style traffic appears.

---

## 5. The decision this plan needs

**Order.** They are not independent, and one of them gets harder with time:

1. **S1** — the blocker. Delegation does not work end to end without it, and its
   design is mostly settled already.
2. **S3** — cheap now, awkward once anyone has built against node-level grants,
   and S2 needs it to mean anything.
3. **S4** — decided, not built, before S2 is designed.
4. **S2** — the largest, and the one that makes the README's central claim true.

**The question that sat underneath S1 is now answered, by the use case.** A Herdr
pane is the right substrate for *human*-to-agent work and the wrong one for
agent-to-agent work: every limitation that makes S1 awkward — output as a terminal
snapshot, text-only payloads — comes from the agent being a terminal. §2.3's test
suite needs agents that are not panes, so a second `Agent` implementation is a
requirement rather than an option. The port was designed to allow exactly that and
nothing has needed it yet.

So the order above gains a step, and it is cheap: **a non-pane `Agent` before
S1**, because it decides what S1's verb is returning. Note it does not replace
S1 — see §2.3.

**And a smaller one worth doing early because it is nearly free:** a `Client`
facade for the caller end. Everything needed is already public; what is missing is
one type that binds an endpoint and sends, so an application does not have to
reproduce the composition root. That is the difference between "a program *can*
drive kamiroh" and "a program *does*".
