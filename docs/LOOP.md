# LOOP

## Current phase

Foundation. Slices **A** (workspace + ARCHITECTURE.md) and **B** (port traits)
are complete, together with an in-memory adapter set and a working composition
root — the whole of the master prompt's first deliverable.

## Done

**Slice A — workspace + crate graph + ARCHITECTURE.md**

- Root `Cargo.toml` converted from a single package to a workspace
  (`resolver = "3"`, `[workspace.package]`, `[workspace.dependencies]`). The old
  `src/lib.rs` scaffold (`add`/`it_works`) was deleted, not carried forward.
- Five crates created: `kamiroh-domain`, `kamiroh-ports`, `kamiroh-app`,
  `kamiroh-adapter-memory`, `kamiroh` (bin). crates.io metadata moved to the bin.
- `docs/ARCHITECTURE.md` written against the code as built.

**Slice B — port traits**

- Driving: `ControlApi` with an opaque `Origin` (`remote()` / `local_front()`).
- Driven: `Transport`, `Allowlist`, `KeyStore`, `AgentController`.
- Per-port `thiserror` error enums; `#[async_trait]` on the async ports for
  dyn-compatibility; `Allowlist` synchronous and infallible.

**Beyond the two slices, to make the bin real**

- `kamiroh-domain`: `EndpointId` (hex, round-tripping), `ActorName` (validated),
  `PeerAddress`, `NodeSecret` (redacted `Debug`, zeroed on drop), `Payload` /
  `ControlMessage` / `ControlReply` / `AgentStatus`.
- `kamiroh-app`: `ControlService` — authorise, then dispatch.
- `kamiroh-adapter-memory`: `InMemoryAllowlist`, `InMemoryKeyStore`,
  `EchoController`, `LoopbackTransport`.
- `crates/kamiroh/src/main.rs`: wiring only; prints the endpoint id and runs a
  smoke path in both the allowed and refused directions.

**Verification** (all green, offline)

```
cargo fmt --all -- --check              # clean
cargo clippy --workspace --all-targets  # zero warnings
cargo test  --workspace                 # 37 passed, 0 failed
cargo run   -p kamiroh                  # prints endpoint id; prompt echoes; unlisted peer refused
cargo tree  -p kamiroh-domain -e normal # no dependencies at all
cargo tree  -p kamiroh-ports  -e normal # kamiroh-domain + async-trait + thiserror only
```

## Decisions

| Decision | Choice | Why |
|---|---|---|
| Crate count | 5 now, not the plan's 8 | Empty adapter crates prove nothing and get rewritten; each arrives with its slice. Stated, not silent. |
| Async style | `#[async_trait]` | Native AFIT is not dyn-compatible; the composition root needs `Arc<dyn Port>`. |
| Errors | Per-port `thiserror` enums | `anyhow` in a port leaks adapter detail into the app and makes failures untestable. |
| `EndpointId` | Opaque `[u8; 32]`, hex display | Naming `iroh::NodeId` in the domain breaks the dependency rule; `String` is wrong forever. |
| Agent-agnostic | Fixed verbs, opaque `Payload` | kamiroh routes and authorises; it never parses agent content. |
| Allowlist | Sync `bool`, no enumeration, deny-by-default | An allowlist check is set membership; a fallible one invites treating an error as "allow". |
| `Origin` | Opaque, built via `remote()` / `local_front()` | A public `Local` variant is constructible by every adapter; local trust must be a deliberate, greppable act. |
| Lints | `deny(missing_docs)`, `forbid(unsafe_code)` | The docs promised a warning-free build; the lint level now enforces it. |

## Advisor consultations

- **Before slice A/B** — crate graph, the `EndpointId` trap, async-vs-generic,
  error types, allowlist semantics, agent-agnostic payloads, workspace mechanics,
  and what "green" must mean. All adopted; the deferred-crate decision is
  recorded above and in ARCHITECTURE.md §2.
- **Before declaring the phase done** — found one real hole and three points of
  polish, all fixed:
  1. **`Origin::Local` was constructible by any crate depending on
     `kamiroh-ports`**, putting a future Iroh adapter one typo away from silently
     disabling the allowlist. `Origin` is now opaque with named constructors. The
     tell was a runtime `panic!` in a loopback test standing in for a type-level
     guarantee; that assertion is now about the adapter, not the type.
  2. `InMemoryKeyStore` copied bytes out of `NodeSecret` into a `Debug`-deriving
     struct, stripping both the redacted `Debug` and the zero-on-drop. It stores
     the `NodeSecret` itself now, pinned by a test.
  3. ARCHITECTURE.md claimed a no-enumeration property the *wire* protocol does
     not yet owe; §7's slice-F row now records that obligation explicitly.
  4. `missing_docs` promoted from `warn` to `deny`, so the doc's promise of a
     warning-free build is enforced rather than aspirational.

## Next slice

**C — `kamiroh-domain` deepening**, or skip straight to **E** (`adapter-fs`
`KeyStore`).

Slice C as scoped in the build plan is largely already done: the domain exists
with 15 passing unit tests. The higher-value next step is **E**, which replaces
the two loud placeholders:

- `InMemoryKeyStore::insecure_dev()` — a fixed, publicly known secret
- `placeholder_endpoint_for()` — byte inversion, not a key derivation

Slice E is done when a node generates a real secret with a CSPRNG, persists it
with owner-only permissions, and keeps a stable endpoint id across restarts.
Consult the advisor before it lands: key custody is a security-sensitive path.

## Blockers

None.

## Notes for the next session

- The sandbox has no network. Cargo resolves from the local registry cache;
  append `--offline` if a fetch is attempted.
- `cargo clippy --workspace --all-targets` is warning-free today. `missing_docs`
  is `deny` in every library crate, so undocumented public items — including enum
  struct fields — break the build rather than nagging.
- Nothing is committed. The tree is clean, green, and uncommitted by design:
  `docs/prompt.txt` asks for LOOP.md updates and does not authorise commits.
