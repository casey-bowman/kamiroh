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
| `kamiroh` | Composition root (binary) | all of the above, `tokio` |

### Deferred crates — a stated decision, not an omission

The build plan's tree also lists `kamiroh-adapter-iroh`, `-kameo`, `-herdr`, and
`-fs`. They are **not** created yet. An empty crate that exists only to be
`cargo check`ed proves nothing and gets rewritten when the real adapter lands, so
each arrives with its slice (E–G, J). `kamiroh-adapter-memory` covers the first
deliverable's "no-op or in-memory adapters so the bin compiles" in one crate.

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

`cargo run -p kamiroh` wires every port to an in-memory adapter, then:

```text
endpoint id: fffefdfc…e1e0
agent:       agent
prompt -> "hello"
unlisted peer eeee…eeee -> refused
```

The loopback transport delivers as `Origin::Remote(local endpoint)`, so the
smoke path exercises the real allowlist rather than bypassing it. The binary
self-allows for that reason; a real node's allowlist is built from configured
peers and does not contain itself.

**Two placeholders, both loud in the code:**

- `InMemoryKeyStore::insecure_dev()` returns a fixed, publicly known secret and
  persists nothing. Real custody (persistence, owner-only permissions, a CSPRNG)
  arrives in slice E.
- `placeholder_endpoint_for()` inverts the secret's bytes. It is **not** a key
  derivation; the real endpoint id is the ed25519 public key, which the Iroh
  adapter supplies in slice F.

---

## 7. Where the next slices attach

| Slice | Crate | Attaches at |
|---|---|---|
| E | `kamiroh-adapter-fs` | `KeyStore` — replaces `InMemoryKeyStore` |
| F | `kamiroh-adapter-iroh` | `Transport` + a front calling `ControlApi`; supplies the real `EndpointId` |
| | | ⚠ The reply sent **over the wire** to an unauthorised peer must not distinguish "refused" from "no such actor". `TransportError` separates them for the local caller, which is right; serialising that distinction back to a rejected peer would hand it the enumeration oracle §5.4 promises not to give. |
| G | `kamiroh-adapter-kameo` | `AgentController` — replaces `EchoController` |
| I | allowlist config source | `Allowlist` — replaces the in-memory list |
| J | `kamiroh-adapter-herdr` | A second front calling the same `ControlApi` |

Each is a constructor swap in `crates/kamiroh/src/main.rs`. No slice above should
require an app-layer or domain change; if one does, that is the signal to revisit
the port rather than to leak the adapter's type inward.
