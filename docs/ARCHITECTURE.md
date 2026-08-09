# kamiroh Architecture

Cargo workspace modular monolith, ports and adapters (hexagonal).

This document describes what is **in the repo today**, and marks planned work as
planned. Where it and the code disagree, the code is right and this file is a
bug.

---

## 1. The dependency rule

Dependencies point inward. Adapters know about the application; the application
knows about ports; ports know about the domain; the domain knows about nothing.

```text
      ┌─────────────────────────────────────────────┐
      │  kamiroh (bin)  — composition root          │
      │  the only crate that names adapters         │
      └───────────────────┬─────────────────────────┘
                          │ wires
      ┌───────────────────▼─────────────────────────┐
      │  adapters   fs · iroh · kameo · herdr       │
      │             memory (test doubles)           │
      └───────────────────┬─────────────────────────┘
                          │ implement / call
      ┌───────────────────▼─────────────────────────┐
      │  kamiroh-ports    trait boundary            │
      └───────────────────┬─────────────────────────┘
                          │ used by
      ┌───────────────────▼─────────────────────────┐
      │  kamiroh-app      use cases                 │
      └───────────────────┬─────────────────────────┘
                          │ speaks
      ┌───────────────────▼─────────────────────────┐
      │  kamiroh-domain   names, addresses, keys,   │
      │                   control vocabulary        │
      └─────────────────────────────────────────────┘
```

**Non-negotiable:** `kamiroh-domain` and `kamiroh-app` contain no Iroh, Kameo, or
Herdr types — not in signatures, not in `Cargo.toml`.

This is verified rather than asserted:

```bash
cargo tree -p kamiroh-domain -e normal   # must list kamiroh-domain and nothing else
cargo tree -p kamiroh-ports  -e normal   # kamiroh-domain + async-trait + thiserror only
cargo check --workspace --all-targets    # must be warning-free
```

---

## 2. Crate graph as built

| Crate | Role | Depends on |
|---|---|---|
| `kamiroh-domain` | Names, addresses, key material, control vocabulary | *nothing* (std only) |
| `kamiroh-ports` | Port traits + per-port error types | domain, `async-trait`, `thiserror` |
| `kamiroh-app` | Use cases against ports | domain, ports, `async-trait` |
| `kamiroh-adapter-memory` | In-memory implementation of every driven port, incl. `EchoAgent` | domain, ports, `async-trait` |
| `kamiroh-adapter-fs` | Node key custody, the allowlist, and the agents file | domain, ports, `async-trait`, `getrandom`, `thiserror` |
| `kamiroh-adapter-iroh` | Endpoint identity (F1); peer transport and inbound front (F2) | domain, ports, `iroh-base`, `iroh` |
| `kamiroh-adapter-kameo` | One controller actor per agent | domain, ports, `async-trait`, `tokio`, `kameo` |
| `kamiroh-adapter-herdr` | The pane console, reporting, and the Herdr-managed agent | domain, ports, `async-trait`, `thiserror`, `tokio`, `serde_json` |
| `kamiroh` | Composition root (binary) | all of the above, `tokio` |

`kamiroh-adapter-iroh` depends on `iroh-base` with `default-features = false,
features = ["key"]` rather than on `iroh`: identity derivation needs the key
types, not the QUIC stack, which is ~155 crates in the tree instead of ~375.
Deriving with `ed25519-dalek` directly would be lighter still, but going through
the type Iroh itself uses makes agreement structural rather than a matter of our
reading the spec. F2 adds `iroh` proper.

### Deferred crates — a stated decision, not an omission

The build plan's tree also lists `kamiroh-adapter-iroh`, `-kameo`, and `-herdr`.
None was created up front. An empty crate that exists only to be `cargo check`ed
proves nothing and gets rewritten when the real adapter lands, so each arrives
with its slice — `-fs` in E, `-iroh` in F, `-kameo` in G, `-herdr` in J. All
four now exist, each arriving with real behaviour rather than as a stub.
`kamiroh-adapter-memory` covered the first deliverable's "no-op or in-memory
adapters so the bin compiles" in one crate, and remains the test double set.

`kamiroh-adapter-herdr` is split so that only half of it knows Herdr exists.
A pane is a terminal, so `console` takes an `AsyncBufRead` and an `AsyncWrite`
and is tested with a string and a `Vec<u8>`. `client` and `report` speak
Herdr's local socket API — newline JSON on `$HERDR_SOCKET_PATH`, method
`pane.report_agent` — and are tested against a fake socket. Outside a pane the
reporting half does nothing at all.

`serde_json` is adapter-local and does not weaken F2's decision to hand-write
the Iroh codec. That was about keeping `kamiroh-domain` dependency-free for a
protocol kamiroh defines; this is someone else's JSON, whose response shape is
theirs to change, and one field — the pane id — arrives from the environment
unvalidated and must be escaped by something that knows the rules.

`kamiroh-adapter-kameo` takes `kameo` with `default-features = false`. Its
`remote` feature pulls libp2p, and a second peer-to-peer stack in the tree would
be a standing invitation to route agent traffic over the wrong one; kamiroh's
networking is Iroh. The `macros` and `tracing` defaults are dropped as unused.

---

## 3. Ports

Two directions, which is the distinction that makes "same actor, multiple
fronts" work.

### Driving port — the outside calls in

| Port | Contract |
|---|---|
| `ControlApi` | `deliver(origin, agent, message) -> ControlReply` |

Implemented by `kamiroh-app`; called by every front. The Iroh front and the
Herdr pane console hold the same `Arc<dyn ControlApi>`, which is how both reach
one controller actor. Since J1 that is demonstrated rather than promised: the
composition root clones one handle into both.

`Origin` carries the trust decision as a type. It is **opaque**: its two cases
are reachable only through named constructors, and the app layer reads it back
with `remote_endpoint() -> Option<EndpointId>`.

- `Origin::remote(endpoint)` — checked against the allowlist. Adapters must pass
  the endpoint the transport **authenticated**, never one read out of message
  content.
- `Origin::local_front()` — a front on this node (a Herdr pane); already inside
  the trust boundary, so the allowlist is skipped.

Opacity is the point. A public `Origin::Local` variant would be constructible by
every crate depending on `kamiroh-ports`, putting the Iroh adapter one typo away
from silently disabling the allowlist for all remote traffic. As a constructor,
claiming local trust is a deliberate act and `grep -r 'local_front'` lists every
place in the tree that does — the same audit affordance as
`NodeSecret::expose_bytes`.

**The audit rule changed in J1.** It used to be "no adapter may appear in that
grep", which held while every adapter was a transport. `LocalLink` is the case
the constructor was written for, so the rule is now:

> Only `kamiroh-adapter-herdr`'s `LocalLink` and the composition root may call
> `Origin::local_front()`. Any *transport* adapter appearing in that grep is
> still a bug.

The justification is that a pane is a process on this machine, started by
whoever owns the node, so it is already inside the boundary the allowlist
defends. Note that `RemoteLink`, in the same file, does **not** call it — the
console does not get to vouch for the far end.

### 3a. Fronts and consoles are not the same thing

This document said for a long time that the Herdr adapter would be "a second
front calling the same `ControlApi`". That is true of half of it, and the half
it misses is the one users actually want.

- A **front** is inbound. Something arrives, and the adapter calls `ControlApi`
  to serve *this node's* agents. The Iroh accept loop is a front.
- A **console** is outbound. A person acts, and the adapter calls `Transport` to
  drive *another node's* agents. `IrohTransport` is the machinery; the pane is
  the human end of it.

`kamiroh-adapter-herdr` is both, and the console half is the point: a pane on a
laptop, an agent that has been running at home for a week. The README always
said so — "locally or across the network—to drive agents" — but the slice table
said "front", and a reader following the plan would have built the wrong thing.

The distinction matters beyond naming, because the trust rules differ. A front
must decide whether to trust what arrived: `Origin::remote(..)` is checked
against the allowlist, `Origin::local_front()` is not. A console makes no trust
decision at all — it is the *far end's* allowlist that judges it, which is why
`RemoteLink` never constructs an `Origin` and why a refusal comes back as a
`TransportError`, not a `ControlApiError`.

### Driven ports — the inside calls out

| Port | Contract | Notes |
|---|---|---|
| `Transport` | `local_endpoint_id()`, `send(to, message) -> ControlReply` | Outbound only; inbound arrives via `ControlApi` |
| `Allowlist` | `is_allowed(&EndpointId) -> bool` | Sync, infallible, deny-by-default |
| `KeyStore` | `load_or_create() -> NodeSecret` | Returns domain-typed key material |
| `AgentController` | `dispatch(agent, message) -> ControlReply` | The message path to the actor, not the actor |
| `Agent` | `run(prompt) -> Result<AgentOutcome, AgentError>` | The work itself. Added in M1 — see below |

**`Agent` was an adapter trait until M1.** It lived in `kamiroh-adapter-kameo`
from slice G, with a note arguing it should stay there: promoting it would make
every future controller adapter adopt one notion of "an agent". That held while
one crate both defined and implemented it, and stopped holding when
`kamiroh-adapter-herdr` arrived to implement it. A trait that one adapter drives
and another satisfies is a boundary, and the alternative was an adapter
depending on an adapter. The agnosticism concern survives the move intact:
nothing in the port says what an agent *does*.

Two consequences worth stating, because both were easy to get wrong:

- **`run` is fallible.** An agent runtime that cannot be reached must be an
  `AgentError`, never an empty `AgentOutcome` — otherwise an infrastructure
  failure arrives at the caller looking like something the agent said.
- **Returning is not a claim of completion.** `AgentOutcome` carries a status,
  and the controller turns anything short of `Idle` into `ControlReply::Partial`
  rather than `Output`.

### Three decisions worth recording

**Async via `#[async_trait]`, not AFIT.** Native `async fn` in traits is not
dyn-compatible, and the composition root holds `Arc<dyn Port>` so adapters can be
swapped without changing app-layer signatures. `Allowlist` stays synchronous: it
is set membership.

**Per-port `thiserror` enums, never `anyhow`.** `anyhow` in a port trait leaks
"whatever the adapter felt like" into the application layer and makes specific
failures untestable. Each port owns its error type.

**`AgentController` is a port, not an actor.** `kamiroh-adapter-kameo` will
implement it on top of a Kameo actor. `kameo::Actor` never appears above the
adapter ring.

---

## 4. Domain types

**`EndpointId`** — an opaque `[u8; 32]`. Iroh's node id is an ed25519 public key,
but naming `iroh::NodeId` in the domain would break the dependency rule on day
one, and `String` would be wrong forever. Display and `FromStr` use lowercase hex
(64 chars); parsing accepts either case. The Iroh adapter converts at the
boundary.

**`ActorName`** — validated: non-empty, ≤64 bytes, `[A-Za-z0-9._-]` only. The
character set is restricted so names are safe in wire framing, logs, and paths
without escaping; `../etc` and whitespace are rejected at construction.

**`NodeSecret`** — opaque 32 bytes with a redacted `Debug`, zeroing on drop, and
a conspicuously named `expose_bytes()` so every place key material leaves custody
is greppable.

Its constructors exist to stop plaintext copies escaping the type.
`from_bytes` takes its argument by value, so the caller keeps an unprotected
array; `from_fill` instead hands the closure a buffer *inside* the secret, and
`from_hex` / `write_hex_into` parse and render in place. The fill closure is also
what keeps the domain RNG-free — `kamiroh-adapter-fs` supplies `getrandom`.
`ParseNodeSecretError` carries no fragment of its input, not even the offending
character, because that input is key material and errors get logged.

**`Payload` and the agent-agnostic boundary.** kamiroh fixes the control *verbs*
(`Prompt`, `Status`, `Interrupt`, `Shutdown`) and leaves the *content* opaque: a
`Payload` is bytes plus a content type, interpreted only by the agent behind the
controller. `Payload::text` is a convenience for the common case, **not** a claim
that agents are text-in/text-out. This is the deliberate reading of
"agent-agnostic": kamiroh routes and authorises, and never parses agent content.

---

## 5. The security boundary

Everything inbound funnels through one place — `ControlService::deliver` in
`kamiroh-app`.

1. Authorisation happens **first**. Nothing is looked up and nothing is
   dispatched before the origin is admitted.
2. **Deny by default.** An empty allowlist rejects every endpoint. There is no
   "empty means allow all" mode and no constructor that permits everything.
3. **No enumeration.** `Allowlist` exposes no method returning the set of allowed
   endpoints, so no caller can fetch the list and apply its own weaker filter.
4. **No information leak on refusal.** `ControlApiError::NotAllowed` carries only
   the endpoint. A rejected caller cannot learn which agents exist here, because
   the refusal happens before any lookup that could tell it apart from "unknown
   agent". This holds at the `ControlApi` boundary; §7 records the matching
   obligation for the Iroh adapter's wire replies.
5. **Local trust is not ambient.** `Origin` is opaque, so only code that calls
   `Origin::local_front()` can skip the allowlist, and every such call site is
   greppable. See §3.

Pinned by tests in `crates/kamiroh-app/src/control_service.rs` (empty allowlist
denies everything; an unlisted endpoint reaches no agent; local origin still
works) and `crates/kamiroh-adapter-memory/src/allowlist.rs`.

### 5a. Reachability is not admission — and it is a separate disclosure

M2 added `Reach`, and it changes what a node *reveals*, not what it admits. The
allowlist is untouched by it. But the README promises "no central control
gateway", and a relay is the nearest thing to one, so what each option actually
exposes is worth stating rather than implying.

| | `Reach::Direct` (default) | `Reach::Anywhere` |
|---|---|---|
| Relays | none | n0's, when no direct path exists |
| Address lookup | none | n0's DNS/pkarr |
| What leaves this machine | nothing, except to peers it dials | a **signed record of this node's addresses**, published under its endpoint id |
| To dial a peer you need | its `host:port` | its endpoint id |

**What a relay can observe.** QUIC is end-to-end encrypted between endpoints, so
a relay carries ciphertext. It can see which two endpoint ids are talking, when,
and how much — traffic metadata, not content. It cannot read a `ControlMessage`,
cannot forge one, and cannot admit itself: it is not on anyone's allowlist and
the front authenticates the peer from the connection, not from the path.

**What discovery publishes.** With `Anywhere`, this node signs a record listing
its relay URL and direct IP addresses and publishes it to n0's service, keyed by
its endpoint id. The consequence worth being explicit about: *anyone who knows
the endpoint id can learn where the node is*, whether or not the allowlist would
admit them. Reachable is not admitted — but "unlisted peers cannot even find me"
stops being true, and that is a genuine change to a node's exposure.

**Which is why it is opt-in.** `KAMIROH_REACH` defaults to `direct`. A node does
not start announcing where it lives because someone failed to set a variable,
and the startup line always says which mode is in force. Tests and demos are
`Direct` for the same reason: publishing to a public service is not a decision a
test suite gets to make on its own.

**Not yet decided:** self-hosted relays and a self-hosted pkarr server. Both are
supported by Iroh and both would remove the third party without changing
kamiroh's model. Until then, `Anywhere` means trusting n0 with metadata, and
that is the honest summary.

---

## 6. What the binary does today

`cargo run -p kamiroh` resolves each port and runs a smoke path:

```text
key file:    ~/.config/kamiroh/node.key
endpoint id: <64 hex chars — this node's ed25519 public key>
agent:       agent
prompt -> "hello"
unlisted peer eeee…eeee -> refused
```

The loopback transport delivers via `Origin::remote(local endpoint)`, so the
smoke path exercises the real allowlist rather than bypassing it. The binary
self-allows for that reason; a real node's allowlist is built from configured
peers and does not contain itself.

**Real as of slice E:** key custody. The secret comes from OS entropy and lives
in `$XDG_CONFIG_HOME/kamiroh/node.key` (or `$HOME/.config/...`), overridable
with `KAMIROH_KEY_FILE`.

**Real as of slice F1:** the endpoint id. It is the ed25519 public key derived
from that secret by `kamiroh_adapter_iroh::endpoint_id_for` — the same id a peer
will see, since it comes from the very key type Iroh uses. Together these mean a
node's identity is genuine and stable across restarts.

`placeholder_endpoint_for`, which faked an id by inverting the secret's bytes,
was **deleted** in F1 rather than left unused: a fake key derivation sitting in a
test-double crate is available to be called by mistake. `InMemoryKeyStore` still
exists as a test double but no longer backs the binary.

**Nothing is in-memory any more.** Key custody became real in E, the transport
and front in F2, the controller in G, and the allowlist in I. Every driven port
now resolves to an adapter that touches the world.

`kamiroh-adapter-memory` is therefore a test-double crate and nothing else.
`EchoController` survives for front tests, which want a controller that answers
immediately and needs no runtime; note what it cannot do — holding agent state
in a map, it can never report `AgentStatus::Busy`, so a test that needs an agent
genuinely at work belongs with the Kameo adapter. `InMemoryAllowlist` has one
remaining production caller: the composition root uses it for the
`KAMIROH_ALLOW` override, where the set comes from an env var and there is no
file to re-read.

## 6a. Key custody rules

Enforced by `kamiroh-adapter-fs` and pinned by its tests:

- **Publish with `hard_link`, never `rename` and never in place.** A node
  identity needs its file to be both non-clobbering and atomically published.
  `rename` clobbers, destroying an existing identity. `O_CREAT | O_EXCL` on the
  final path looks safe but publishes the *name* before the contents, so a
  process starting concurrently reads a zero-length file — an observed race, not
  a theoretical one. Writing a temp file, fsyncing, then linking gives both.
- **Check permissions before reading.** A key readable beyond its owner is
  already compromised; reading it first and erroring afterwards would pull it
  into memory anyway. The check covers the file (no group/other access at all)
  *and* its directory (no group/other **write**, which would let another account
  swap the key file out even though it could never read it).
- **Both paths are checked.** A key created by an older version, restored from a
  backup, or copied by hand never went through the create path — that case is
  exactly what the check exists for.
- **A corrupt key file is reported, never replaced.** It may be a recoverable
  identity, and overwriting it destroys the node.
- **No plaintext copy outlives its function.** Buffers holding key material on
  the way to or from disk zero themselves on drop, matching `NodeSecret`.

---

## 6b. Allowlist custody rules

Enforced by `kamiroh-adapter-fs` and pinned by its tests. Set against §6a on
purpose — the two files sit in the same directory and their rules are **not**
the same, because what they protect is not the same:

| | `node.key` | `allow` |
|---|---|---|
| Contents | A secret | Public keys |
| Readable by others | ✗ refused | ✓ fine |
| Writable by others | ✗ refused | ✗ refused |
| Corrupt file | Reported, never replaced | Reported, node refuses to start |
| Absent file | Created | Means "admit nobody" |

- **Secrecy is not the property; integrity is.** Demanding `0600` on an
  allowlist would be theatre: the contents are public keys, and it would only
  make the file harder to inspect. Group- or other-*writable* is refused on both
  the file and its directory, because an account that can append a line — or
  swap the file — can admit itself to this node.
- **Permissions are checked before contents.** A file anyone can rewrite is not
  evidence of anything, so there is no point parsing it first. Pinned by
  `permissions_are_checked_before_contents`.
- **A malformed file is fatal, and a partial list is never used.** One bad line
  rejects the whole file. The allowlist is the trust boundary, so a file that
  cannot be fully understood means the operator's intent is unknown — and both
  ways of guessing are wrong. Admitting the lines that did parse enforces a
  policy nobody wrote; admitting nobody while looking healthy hides the mistake
  behind what appears to be a network problem.
- **An absent file is not a failure.** It means what an empty one means: admit
  nobody. That is the deny-by-default §3 requires, and it is the state of a
  fresh node before anyone configures it.
- **The startup line always names the source.** With `KAMIROH_ALLOW` overriding
  the file, the failure worth designing against is an operator editing a file
  the node never read.
- **A failed reload changes nothing.** The file is parsed before the write lock
  is taken, so the previously loaded set survives and the error is returned
  rather than swallowed. The adapter does not choose between the two risks —
  retaining a stale list can miss a revocation, emptying one locks out every
  peer over a typo — because only the caller knows which it is running.
- **`SIGHUP` re-reads it.** A signal rather than a console command, because the
  node that needs this is the one with nobody at its pane: a home node serving
  peers, whose allowlist is what an operator edits. The composition root keeps
  the concrete handle solely for this; everything else uses the port. A bad edit
  costs a `warn` naming the file and line, and the previous set stays in force.

## 6c. Controller actor rules

Enforced by `kamiroh-adapter-herdr` and pinned by its tests:

- **Reporting decorates `Link`, not `AgentController`.** The obvious choice is a
  decorator over the controller port, which sees every message any front
  delivers. It is wrong for the case that matters: a pane driving an agent on
  another node never touches the local controller, so it would report nothing
  precisely when there is something to report. A pane's state is the state of
  the agent *that pane is bound to*, which is what `Link` names.
- **Reporting never delays or fails a control message.** States go to a
  background task through a bounded channel, and a full channel drops the
  update. A sidebar label is not worth making someone's prompt slower, and
  Herdr being down is not a reason for kamiroh to stop working.
- **Absence is normal, not degraded.** No `HERDR_PANE_ID` means `attach` returns
  the link untouched and says so once. kamiroh runs outside Herdr as a matter of
  course.
- **Asking does not change the answer.** Only a prompt reports `working` before
  delegating; `/status` must not briefly claim the agent is busy just by being
  asked. A `Status` reply is reported as whatever the agent said.
- **A failure reports `unknown`, never `idle`.** An unreachable peer says
  nothing about the agent behind it, and `idle` would be a guess presented as a
  fact.
- **One request per connection.** Herdr answers and then closes; three `ping`s
  on one connection produce one response. Holding a connection open succeeds
  once and then fails forever with a broken pipe. Established by experiment
  against `herdr 0.8.0`, not from its documentation.
- **Nothing on the startup path may block on a peer.** `greet` is spawned rather
  than awaited: an unreachable peer costs the full dial timeout, and a pane that
  cannot be typed at for 16 seconds is worse than one that reports its trouble.

Two mapping gaps, both decisions rather than oversights. `AgentStatus::Starting`
maps to `unknown` rather than `idle`, since a sidebar reading "idle" invites
someone to prompt an agent that is not ready — though it is unreachable today,
as `KameoController` spawns actors already idle. And nothing maps to Herdr's
`blocked`, which means "waiting on a human"; no kamiroh agent currently waits on
input, and when one does, that is where it surfaces.

**Both directions report, through one reporter.** Decorating `Link` alone left a
serving node's pane permanently idle: it has nobody at its console, and peers'
messages arrive through the Iroh front. So `AgentController` is decorated too.

The two share a single channel rather than being two reporters ordered by
Herdr's optional `seq`. Sharing removes the race instead of sequencing it — one
channel, one connection, and the order the channel already imposes. `seq`
remains available if a genuinely independent source ever appears.

**Still not covered:** one pane shows one agent. A node hosting several would
have them overwrite each other in the pane list, which is M3's problem to
solve.

## 6c2. Agent configuration rules

Enforced by `kamiroh-adapter-fs`'s `agents` module and pinned by its tests:

- **One agent per line, `<name> <target>`** — the allowlist's shape, so an
  operator editing one need not learn a second format.
- **The file says *what*, not *how*.** A target is an opaque string here;
  resolving `w1:p2` to a Herdr pane, or `echo` to the stand-in, is the
  composition root's business. Otherwise Herdr ends up in the filesystem adapter
  and a second runtime has to be added in two places.
- **A bad line rejects the whole file**, as with the allowlist: hosting *some*
  of the agents an operator asked for is its own kind of wrong.
- **A repeated name is refused.** The front routes by name, so two agents
  sharing one is ambiguity, not untidiness.
- **A target containing spaces is refused.** Without it, `my agent w1:p2` parses
  as the agent `my` with target `agent w1:p2` — accepted, wrong, and silent.
- **Absent means one agent, not none.** A node with no file hosts `agent`, which
  is what every kamiroh node did before the file existed. An empty *file* is how
  you ask for a node that hosts nothing.
- **A pane still shows one agent.** The reporter filters to the pane's own
  agent; letting every hosted agent report would have them overwrite each other
  in Herdr's list, which tells an operator less than reporting nothing.

## 6d. Herdr reporting rules

Enforced by `kamiroh-adapter-kameo` and pinned by its tests:

- **One actor per agent; every state change goes through its mailbox.** That is
  what makes the state machine safe without a lock, and it is why a prompt
  finishing cannot interleave with an interrupt — they are two messages in an
  order the mailbox already fixed.
- **A prompt runs as its own task, and reports back through the mailbox.** If it
  ran inline, the actor could not answer `Status` while working and `Interrupt`
  would have nothing to arrive at. The task never touches actor state directly.
- **`Agent` is an adapter trait, not a port.** The ports crate describes
  kamiroh's boundaries; `Agent` describes how *this* adapter runs the thing
  behind one. Promoting it would make every future controller adapter adopt one
  notion of "an agent", which is the assumption kamiroh exists not to make.
- **`Agent::run` must be cancel-safe.** Interrupt and shutdown abort the task, so
  the future is dropped wherever it was suspended.
- **One prompt at a time, refused rather than queued.** The mailbox would happily
  queue a second, but silently serialising them would make `Busy` a lie: the
  caller would wait with no way to tell queued from running.
- **Nothing is awaited inside a handler without a bound.** While an inline await
  runs, nothing else in the mailbox moves — so an agent runtime that accepts a
  connection and never answers would make `Interrupt` and `Shutdown` unreachable
  and the agent unstoppable. `Status` is the only inline await, and it is capped
  at `STATUS_TIMEOUT`; a timeout leaves the cached status alone, exactly as an
  error does. Everything slow is spawned. M1 broke this rule while citing it,
  which is why it now has a test.
- **A shut-down agent answers the same way whether or not its actor has finished
  stopping.** The actor holds an explicit `Stopped` state *and* a send to a dead
  actor maps to `ControllerError::Stopped`, so the answer never depends on
  timing. Stopping is requested from another task: the mailbox is bounded, and an
  actor awaiting a send into its own mailbox from inside a handler cannot drain
  it to make room.
- **Nobody waiting on a prompt is left hanging.** Interrupt, shutdown, and the
  actor's own `on_stop` each answer an outstanding prompt before dropping it.

---

## 6e. Agent runtime rules

Enforced by `kamiroh-adapter-herdr`'s `HerdrAgent` and pinned by its tests:

- **Patience is set by the most impatient caller.** The Iroh front gives a
  request 30s and the transport gives a reply 30s, so an agent that waits longer
  is answered by a timeout instead of by the agent. `DEFAULT_PATIENCE` is 20s,
  and a test asserts it stays under the front's timeout.
- **Running out of patience is not a failure.** It produces `AgentStatus::Busy`
  and whatever the agent had said by then, as a `Partial` reply.
- **Herdr not knowing is not kamiroh claiming completion.** An `unknown` state
  becomes `Busy`, not `Idle` — the same rule as §6d, applied to the agent rather
  than the pane.
- **Wait on the states a real agent actually reaches.** Claude Code returns to
  `idle` when it has answered, *not* to `done`, so the wait list is
  `[idle, blocked, done]`. Established by watching one; the earlier
  `[done, blocked]` meant every prompt expired instead of completing.
- **An expired wait is `Busy`, not an error.** Herdr reports it as an error
  (`code: "timeout"`), which is a statement about the wait, not about the agent.
  Mapping it to `AgentError` made a slow agent look like a broken socket.
- **Nothing on the startup path may prompt the agent.** `local_smoke` sends
  `Status`, not `Prompt`: with a real agent behind the port, a startup smoke
  that prompts spends money and puts words in the agent's mouth.
- **`Status` asks the agent; it does not report a cached value.** An agent can
  change state with kamiroh doing nothing — a permission dialog on startup
  blocks one before it has ever been prompted. `Agent::status` returns
  `Option<AgentStatus>`, where `None` means "no better answer than yours", so an
  agent whose state only moves when it runs keeps the default and costs nothing.
- **`impl Agent for Arc<dyn Agent>` must forward every method.** A defaulted
  method it does not override is answered by the *default*, silently and with no
  compile error. `status` was added with a default and this impl kept it for one
  commit; the symptom was kamiroh reporting `Idle` for an agent stopped at a
  dialog, with all unit tests green because they bypassed the wrapper.
- **A non-text prompt is refused, not typed at a terminal.** A pane takes
  keystrokes; sending arbitrary bytes and calling the result an answer is worse
  than saying no.
- **kamiroh does not start, supervise or parse the agent.** Herdr does the first
  two; nobody does the third. The output stays an opaque `Payload` all the way
  out to the peer.

**A third limit, and the one that bites hardest: detection quality differs per
agent kind.** Herdr decides an agent's state from its own per-kind manifest, and
those manifests are not equally good. Measured: a startup permission prompt is
reported as `blocked` for `claude` and as `idle` for `codex` — the same
situation, two different answers, and the second is wrong in the dangerous
direction. kamiroh reports what Herdr detects and does not second-guess it:
inferring state from terminal output would mean a parser per kind, which is
exactly what agent-agnostic forbids. So the accuracy of `Blocked` is Herdr's to
own, and kamiroh's job is not to make it worse.

**Two limits, stated rather than hidden.** `agent.read` returns the last N lines
of a pane, and a terminal has no marker for "this is the answer to that prompt"
— so what counts as output is a heuristic and may include the prompt's own echo.
And there is no way to ask for the *rest* of a long answer, because
`ControlMessage` has no verb for it; a caller can prompt again and read more,
which is a workaround rather than a design. Adding that verb wants a real
long-running agent to inform it.

## 6f. Logging rules

Enforced by `kamiroh-app` and pinned by `a_prompts_content_never_reaches_the_log`:

- **Agent content is never logged — only its shape.** Log `?message`, never
  `%payload.as_text()`. `Payload`'s `Debug` reports content type and length,
  which is why `?message` is safe by construction; the domain pins that with
  `debug_reports_length_not_content`, and `kamiroh-app` pins the log itself,
  because that is where the mistake would be made. kamiroh is agent-agnostic:
  it has no business knowing what a prompt says, still less writing it down.
- **Diagnostics go to stderr; stdout belongs to the pane console.** Since J1,
  stdout is where an agent's answers appear and where a person is typing. A log
  line there lands in the middle of someone's conversation.
- **A refusal is a `warn`.** Someone kamiroh does not admit tried to drive an
  agent. Rare on a healthy node, and the first thing worth seeing on an
  unhealthy one.
- **Endpoint ids may be logged; secrets never can.** An endpoint id is a public
  key. `NodeSecret`'s `Debug` is redacted and pinned by
  `debug_output_never_contains_key_material`.
- **Dependencies are quiet by default.** `kamiroh_*` at `info`, everything else
  at `warn`, overridable per crate with `KAMIROH_LOG`. Iroh has plenty to say at
  `debug`, and a node that floods its own diagnostics is no more observable than
  one that says nothing.

`Targets` rather than `EnvFilter`: per-crate filtering without pulling regex in
for dynamic matching kamiroh does not use. `kameo`'s own `tracing` feature stays
off — it was dropped in G as an unadopted facade, and now that the facade is
adopted the reason is different but the answer is the same: actor-internal spans
would be noise next to the events above. Revisit if an actor bug ever needs it.

## 7. Where the next slices attach

| Slice | Crate | Attaches at |
|---|---|---|
| ~~E~~ | `kamiroh-adapter-fs` | ✅ done — `KeyStore`, replacing `InMemoryKeyStore` in the binary |
| ~~F1~~ | `kamiroh-adapter-iroh` | ✅ done — real `EndpointId` derivation; `placeholder_endpoint_for` deleted |
| ~~F2~~ | `kamiroh-adapter-iroh` | ✅ done — `Transport` + an inbound front calling `ControlApi` with an authenticated peer |
| | | Both obligations held. The enumeration one holds **by ordering, not by collapsing codes**: `REFUSED` and `NO_SUCH_ACTOR` are distinct on the wire, but authorisation runs before the actor is looked up, so an unlisted peer gets byte-identical `REFUSED` whether or not the agent it names exists. The distinction only ever reaches a peer already trusted. Pinned by `an_unlisted_peer_learns_nothing_beyond_refused`. |
| | | `Origin` is built from the connection's authenticated peer; `grep -rn local_front crates/` still shows the composition root as the only caller. |
| ~~G~~ | `kamiroh-adapter-kameo` | ✅ done — `AgentController`, replacing `EchoController` in the binary |
| ~~I~~ | `kamiroh-adapter-fs` | ✅ done — `Allowlist` from a file, replacing the list the composition root built from an env var |
| ~~J1~~ | `kamiroh-adapter-herdr` | ✅ done — the pane console: one pane, one agent, local **or remote** |
| | | ⚠ The row above used to read "a second front calling the same `ControlApi`", which is half the story. See §3a. |
| ~~J2~~ | `kamiroh-adapter-herdr` | ✅ done — reports the pane's agent state via `pane.report_agent`; see §6d |
| ~~M1~~ | `kamiroh-adapter-herdr` | ✅ done — `HerdrAgent`: a real coding agent behind the `Agent` port; see §6e |

Each is a constructor swap in `crates/kamiroh/src/main.rs`. No slice above should
require an app-layer or domain change; if one does, that is the signal to revisit
the port rather than to leak the adapter's type inward.
