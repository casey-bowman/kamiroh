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
      │  adapters   (kamiroh-adapter-memory today)  │
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
| `kamiroh-adapter-memory` | In-memory implementation of every driven port | domain, ports, `async-trait` |
| `kamiroh-adapter-fs` | Node key custody on disk | domain, ports, `async-trait`, `getrandom` |
| `kamiroh-adapter-iroh` | Endpoint identity (F1); peer transport and inbound front (F2) | domain, ports, `iroh-base`, `iroh` |
| `kamiroh-adapter-kameo` | One controller actor per agent | domain, ports, `async-trait`, `tokio`, `kameo` |
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
with its slice — `-fs` in E, `-iroh` in F, `-kameo` in G. Only `-herdr` (J) is
still deferred. `kamiroh-adapter-memory` covered the first deliverable's "no-op
or in-memory adapters so the bin compiles" in one crate, and remains the test
double set.

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

Implemented by `kamiroh-app`; called by every front. The Iroh adapter and the
optional Herdr adapter will hold the same `Arc<dyn ControlApi>`, which is exactly
how both reach one controller actor.

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
`NodeSecret::expose_bytes`. Any transport adapter appearing in that grep is a
bug.

### Driven ports — the inside calls out

| Port | Contract | Notes |
|---|---|---|
| `Transport` | `local_endpoint_id()`, `send(to, message) -> ControlReply` | Outbound only; inbound arrives via `ControlApi` |
| `Allowlist` | `is_allowed(&EndpointId) -> bool` | Sync, infallible, deny-by-default |
| `KeyStore` | `load_or_create() -> NodeSecret` | Returns domain-typed key material |
| `AgentController` | `dispatch(agent, message) -> ControlReply` | The message path to the actor, not the actor |

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

**Still in-memory:** the allowlist, which gains a real config source in I. The
transport and front became real in F2, the controller in G.

`EchoController` survives as a test double for front tests, which want a
controller that answers immediately and needs no runtime. Note what it cannot
do: holding agent state in a map, it can never report `AgentStatus::Busy`, so a
test that needs an agent genuinely at work belongs with the Kameo adapter.

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

## 6b. Controller actor rules

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
- **A shut-down agent answers the same way whether or not its actor has finished
  stopping.** The actor holds an explicit `Stopped` state *and* a send to a dead
  actor maps to `ControllerError::Stopped`, so the answer never depends on
  timing. Stopping is requested from another task: the mailbox is bounded, and an
  actor awaiting a send into its own mailbox from inside a handler cannot drain
  it to make room.
- **Nobody waiting on a prompt is left hanging.** Interrupt, shutdown, and the
  actor's own `on_stop` each answer an outstanding prompt before dropping it.

---

## 7. Where the next slices attach

| Slice | Crate | Attaches at |
|---|---|---|
| ~~E~~ | `kamiroh-adapter-fs` | ✅ done — `KeyStore`, replacing `InMemoryKeyStore` in the binary |
| ~~F1~~ | `kamiroh-adapter-iroh` | ✅ done — real `EndpointId` derivation; `placeholder_endpoint_for` deleted |
| ~~F2~~ | `kamiroh-adapter-iroh` | ✅ done — `Transport` + an inbound front calling `ControlApi` with an authenticated peer |
| | | Both obligations held. The enumeration one holds **by ordering, not by collapsing codes**: `REFUSED` and `NO_SUCH_ACTOR` are distinct on the wire, but authorisation runs before the actor is looked up, so an unlisted peer gets byte-identical `REFUSED` whether or not the agent it names exists. The distinction only ever reaches a peer already trusted. Pinned by `an_unlisted_peer_learns_nothing_beyond_refused`. |
| | | `Origin` is built from the connection's authenticated peer; `grep -rn local_front crates/` still shows the composition root as the only caller. |
| ~~G~~ | `kamiroh-adapter-kameo` | ✅ done — `AgentController`, replacing `EchoController` in the binary |
| I | allowlist config source | `Allowlist` — replaces the in-memory list |
| J | `kamiroh-adapter-herdr` | A second front calling the same `ControlApi` |

Each is a constructor swap in `crates/kamiroh/src/main.rs`. No slice above should
require an app-layer or domain change; if one does, that is the signal to revisit
the port rather than to leak the adapter's type inward.
