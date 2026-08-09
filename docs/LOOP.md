# LOOP

## Current phase

Foundation. Slices **A** (workspace + ARCHITECTURE.md), **B** (port traits) and
**E** (filesystem key custody) are complete. A node now has a real, persistent
identity; transport, allowlist and controller are still in-memory.

## Done

**Slice E — `kamiroh-adapter-fs` key custody** (commit pending)

- `FileKeyStore`: OS entropy via `getrandom`, stored as hex at
  `$XDG_CONFIG_HOME/kamiroh/node.key`, overridable with `KAMIROH_KEY_FILE`.
- Publish via temp file + `hard_link` — see the race below.
- Permission checks before any read, on both the file (`0600`) and its parent
  directory (not group/other writable), on the create *and* load paths.
- Domain gained `NodeSecret::{from_fill, from_hex, write_hex_into}` and
  `ParseNodeSecretError`, plus a private `hex` module shared with `EndpointId`.
  The `KeyStore` port and `kamiroh-app` were **not** touched — the port held.
- Composition root swapped `InMemoryKeyStore` → `FileKeyStore`; that swap was a
  one-line constructor change, which is the evidence the boundary is right.

**A real race, found and closed.** The first implementation created the key with
`O_CREAT | O_EXCL` at its final path, then wrote. That is non-clobbering but not
atomically published: the name exists before the contents do, so a concurrently
starting process reads a **zero-length** file and reports a spurious `Malformed`.
Reproduced as `got 0` — 4 failures in 5 full-suite runs. Replaced with temp file
→ fsync → `hard_link`, which is non-clobbering *and* publishes atomically.
Verified with the same loop harness that caught it: **30/30 clean**.

Two follow-on details from that fix: temp paths need a per-attempt counter as
well as the pid, or threads in one process stage onto a single path and delete
each other's candidate; and the temp file is removed by a scope guard on every
exit path, since a stranded one is a live secret loose in the key directory.

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
cargo test  --workspace                 # 60 passed, 0 failed
cargo run   -p kamiroh                  # same endpoint id on a second run; unlisted peer refused
cargo tree  -p kamiroh-domain -e normal # no dependencies at all
cargo tree  -p kamiroh-ports  -e normal # kamiroh-domain + async-trait + thiserror only

# The concurrency race is timing-dependent — one green run proves nothing.
for i in $(seq 1 30); do cargo test -p kamiroh-adapter-fs || echo "FAIL $i"; done
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
| Key publish | temp + `hard_link` | The only option that is both non-clobbering and atomically published; `rename` clobbers, `O_EXCL`-in-place exposes an empty file. |
| Key format | Hex + newline | Inspectable, never mistaken for corrupt binary, reads like an `EndpointId`. |
| Key entropy | `getrandom` only | A node secret is generated once; a seeded PRNG layer would add surface for nothing. |

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
- **Before slice E** — endorsed the design (hex storage, permission check before
  read, `0o022` mask on the parent directory rather than requiring `0o700`,
  wiped intermediate buffers, `NodeSecret::from_fill` so no plaintext copy
  exists outside the type) and set the scope boundary: E is custody only, the
  ed25519 endpoint id belongs to F.
- **On the race, mid-slice** — the pre-slice advice had been to prefer `O_EXCL`
  on the final path and *not* to use `hard_link`. The reproduction overrode it:
  that framing weighed clobbering against non-clobbering and missed the
  atomic-publish axis. Reconciled, then switched. Worth remembering as a pattern
  — a reproduced failure outranks a design preference.

## Next slice

**F — `kamiroh-adapter-iroh`.** It is the last piece holding a placeholder:
`placeholder_endpoint_for()` inverts the secret's bytes, where the real endpoint
id is the ed25519 public key derived from it. F also brings the first genuine
front — an inbound path calling `ControlApi` with an *authenticated* peer.

Done when a node reports its real Iroh endpoint id, derived from the secret slice
E persists, and an inbound message from an allowlisted peer reaches an agent.

Two things to settle in F, both already recorded:

- The wire reply to an unauthorised peer must not distinguish "refused" from
  "no such actor" (ARCHITECTURE.md §7). `TransportError` separates them for the
  local caller, which is right; serialising that difference back to a rejected
  peer hands it an enumeration oracle.
- Iroh's node id converts to `EndpointId` at the adapter boundary. If the domain
  needs to learn an Iroh type for this, the conversion is in the wrong place.

Consult the advisor before F lands: it is both an architecture boundary and a
security-sensitive path.

## Blockers

None. Note the sandbox has no network, so slice F's Iroh work can be built and
unit-tested but a real two-node connection will need a run outside it.

## Notes for the next session

- The sandbox has no network. Cargo resolves from the local registry cache;
  append `--offline` if a fetch is attempted.
- `cargo clippy --workspace --all-targets` is warning-free today. `missing_docs`
  is `deny` in every library crate, so undocumented public items — including enum
  struct fields — break the build rather than nagging.
- Commits use conventional-commit subjects, one per slice, on `master` — the
  working method is explicitly "one session on the main checkout".
- `KAMIROH_KEY_FILE` overrides the key path. Use it when running the binary
  inside a sandbox, or to run several nodes on one machine.
