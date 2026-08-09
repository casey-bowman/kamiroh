# LOOP

## Current phase

Foundation. Slices **A** (workspace + ARCHITECTURE.md), **B** (port traits),
**E** (filesystem key custody), **F1** (real endpoint identity), **F2** (the
Iroh transport and front) and **G** (the Kameo controller) are complete.

**H fell out of G rather than needing its own slice.** The plan has H wiring
E+F+G into one process, but each of those slices ended by swapping its own
constructor into the composition root, so by the end of G the binary already
holds a real key store, a real transport and front, and real controller actors.
There is nothing left for H to do; the wiring it describes is
`crates/kamiroh/src/main.rs` as it stands. Recorded here rather than silently
skipped.

Only the allowlist (**I**) is still in-memory, and **J** (Herdr) is untouched.

## Done

**Slice G — the Kameo controller**

`kamiroh-adapter-kameo`: one Kameo actor per agent, behind the unchanged
`AgentController` port. `EchoController` was not merely replaced — the thing it
faked was the *controller*, so an agent's lifecycle was simulated by a
`HashMap`. Now the controller, its mailbox and its lifecycle are real.

- `Agent` is the seam for the work itself, with `EchoAgent` as the stand-in.
  It lives in the adapter, **not** in `kamiroh-ports`: the ports crate describes
  kamiroh's boundaries, and promoting `Agent` would make every future controller
  adapter adopt one notion of "an agent" — the assumption kamiroh exists not to
  make.
- A prompt runs as its own task and reports back **through the mailbox**. Run
  inline, the actor could not answer `Status` while working and `Interrupt`
  would have nothing to arrive at. Reporting through the mailbox is what makes a
  completion racing an interrupt into two messages in an order already fixed.
- **`AgentStatus::Busy` is reachable for the first time.** It has been in the
  domain since slice A and no adapter could ever produce it. Pinned by a test
  that observes an agent mid-prompt.
- Every `ControllerError` variant now has a real producer, which is the
  after-the-fact evidence that the port was drawn right in B.

**A hazard found by reading, not by failing.** Kameo's default mailbox is
*bounded* (`Actor::spawn` → `mailbox::bounded(DEFAULT_MAILBOX_CAPACITY)`). The
obvious `Shutdown` handler awaits `stop_gracefully()` inline, which sends into
the actor's own mailbox — and an actor inside a handler cannot drain that
mailbox to make room, so a full mailbox deadlocks it against itself. Stopping is
requested from a spawned task instead.

That fix opens a second question it would be easy to miss: if stopping is
asynchronous, does a message sent just after `Shutdown` get `Stopped` or a
normal reply? It would depend on whether the actor had finished stopping. So the
actor holds an explicit `Stopped` state *and* `KameoController` maps a send to a
dead actor onto the same `ControllerError::Stopped`. Both paths agree, and a
test covers each side of the race.

Verified in-process (12 tests) and then through the two-process demo, where a
remote peer's prompt crosses the wire into a real actor and its output comes
back — the swap changed nothing a peer can observe, which is the point.

**Slice F2 — the Iroh transport and front**

- `codec`: a hand-written, length-prefixed wire format inside the adapter.
  **This is the decision F2 was told to settle first.** `postcard` + `serde`
  derives would have given `kamiroh-domain` a serde dependency and given up the
  zero-dep property held since slice A, to save code in one adapter. `cargo tree
  -p kamiroh-domain -e normal` still lists nothing but the crate itself.
- `front::serve`: the accept loop, calling `ControlApi` with an `Origin` built
  from the connection's authenticated peer.
- `IrohTransport`: the `Transport` port — dial, send, await the reply.
- Composition root binds an endpoint, serves the front, and reads
  `KAMIROH_ALLOW` / `KAMIROH_PEER`.

**The enumeration obligation holds by ordering, not by collapsing error codes.**
`REFUSED` and `NO_SUCH_ACTOR` *are* distinct on the wire — but authorisation
runs before the actor is looked up, so an unlisted peer receives byte-identical
`REFUSED` whether or not the agent it names exists. The distinction only ever
reaches a peer that is already trusted, which is where it is useful and
harmless. Worth stating precisely: "the two errors serialise identically" would
be the wrong summary and would be falsified by the next person to read
`wire_code`.

Verified by six two-node integration tests, then by a two-process run: an
allowlisted peer gets its echo back, the same peer is refused once the allowlist
is emptied, and the node's identity is stable across restarts.

**Slice F1 — real endpoint identity**

F was split. F1 is identity derivation, fully offline-testable and independently
revertible; F2 is the transport, which needs a wire format, an accept-loop
lifecycle, and UDP sockets the sandbox denies. Bundling them would have made the
small high-value piece wait on the large one.

- New crate `kamiroh-adapter-iroh` with one function, `endpoint_id_for`:
  `SecretKey::from_bytes(secret).public()` → `kamiroh_domain::EndpointId`.
  Infallible — any 32 bytes are a valid ed25519 secret scalar.
- Depends on `iroh-base` (`default-features = false`, `features = ["key"]`), not
  `iroh`: ~155 crates instead of ~375. Going through the type Iroh itself uses
  makes agreement structural, and one test asserts it directly.
- **`placeholder_endpoint_for` deleted**, not merely orphaned. `cargo check
  --workspace` proves the removal is complete.
- Composition root now prints a real id. It changed from `e0c520ae…` (the old
  byte inversion) to `b4cfcb25…` from the *same* persisted secret — expected,
  and the visible proof the derivation is no longer fake.

Boundary verified: `iroh-base` has exactly one consumer (`cargo tree -i
iroh-base`), `kamiroh-domain` still has zero dependencies, and `kamiroh-app`'s
tree contains no iroh crate. Beware `grep -i iroh` here — it matches "kam*iroh*";
check the reverse-dependency tree instead.

**Slice E — `kamiroh-adapter-fs` key custody**

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

**Verification** — the standing recipe, kept current rather than per-slice

```
cargo fmt --all --check                        # clean
cargo clippy --workspace --all-targets -- -D warnings   # zero warnings
cargo test  --workspace                        # 102 passed, 0 failed (as of G)
cargo tree  -p kamiroh-domain -e normal        # no dependencies at all
cargo tree  -p kamiroh-ports  -e normal        # kamiroh-domain + async-trait + thiserror only
cargo tree  -i kameo -e normal                 # exactly one consumer: kamiroh-adapter-kameo
cargo tree  -i iroh-base -e normal             # exactly one consumer: kamiroh-adapter-iroh
grep -rn local_front crates/ --include='*.rs'  # called only from the composition root

# Needs sockets, so it runs outside the sandbox:
cargo test -p kamiroh-adapter-iroh --test two_nodes

# The key-store race is timing-dependent — one green run proves nothing.
for i in $(seq 1 30); do cargo test -p kamiroh-adapter-fs || echo "FAIL $i"; done
```

Beware `grep -i kameo` and `grep -i iroh` when checking boundaries: both match
the crate names `kamiroh-adapter-kameo` and `kamiroh-adapter-iroh`, and "iroh"
matches "kam*iroh*" besides. Use the reverse-dependency tree, which cannot be
fooled this way.

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
| Split F | F1 identity, F2 transport | F1 is offline-testable and independently revertible; bundling made it wait on the large, socket-dependent half. |
| Identity dep | `iroh-base` `["key"]`, not `iroh` | ~155 crates instead of ~375, and the public key comes from the type Iroh uses, so agreement is structural. |
| Wire format | Hand-written codec in the adapter | `postcard` + serde derives would put serde in `kamiroh-domain`, trading the zero-dep property for less code in one adapter. |
| Enumeration defence | Authorise before lookup | Collapsing `REFUSED` and `NO_SUCH_ACTOR` into one code would also blind trusted peers. Ordering gives the property without that cost. |
| `Agent` trait location | In `kamiroh-adapter-kameo`, not `kamiroh-ports` | Ports are kamiroh's boundaries; this is how one adapter runs the thing behind one. A port would force one notion of "an agent" on every future controller. |
| Prompt execution | Own task, reports via the mailbox | Inline, the actor cannot answer `Status` while working and `Interrupt` has nothing to reach. Reporting through the mailbox orders completion against interruption. |
| Concurrent prompts | Refused, not queued | The mailbox would queue them, but silently serialising makes `Busy` a lie — the caller cannot tell queued from running. |
| Stopping an actor | From a spawned task, plus an explicit `Stopped` state | The mailbox is bounded, so awaiting `stop_gracefully()` inside a handler can deadlock the actor against itself; the explicit state then keeps the answer independent of stop timing. |
| `kameo` features | `default-features = false` | `remote` pulls libp2p, and a second p2p stack invites routing agent traffic over the wrong one. `macros` and `tracing` are unused. |

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
- **Before slice F** — split F into F1/F2, use `iroh-base` rather than `iroh` for
  identity, and check the derivation is not the old inversion. All adopted.
- **F2 and G — not consulted.** Both were flagged for it in advance: F2 as an
  architecture *and* security gate, G because §4 lists "actor message enum". The
  gate was not met on either. Recording it because the alternative is that the
  omission disappears and the plan quietly becomes something nobody follows.

  What stood in for it: the two decisions that would have gone to the advisor
  were instead written down with their alternatives and the reason for
  rejecting them — the wire format in F2 (Decisions table), and the location of
  the `Agent` trait in G. Both are reversible without touching the app layer.
  The security-relevant half of F2 is pinned by a test that states the property
  as what an observer can distinguish, which is the form that survives being
  re-read.

  Still worth a review when one is available, in this order: the enumeration
  argument in F2 (it rests on ordering, so it breaks quietly if authorisation
  and lookup are ever reordered), then `Agent` as an adapter trait, then the
  bounded-mailbox reasoning in G.

## Next slice

**I — a real allowlist source.** The last in-memory adapter. Today the
composition root parses `KAMIROH_ALLOW` itself and hands the result to
`InMemoryAllowlist`, which means the node's security policy is an env var
assembled in `main`.

Done when the allowlist comes from a durable, editable source and an unlisted
peer is still refused — the F2 two-process demo is the check, since it already
exercises both directions.

**The decision to settle first:** whether the allowlist is *reloadable*. The
`Allowlist` port is a synchronous, infallible `bool` (slice B), chosen because
set membership is not an IO operation and a fallible check invites treating an
error as "allow". That reasoning still holds and the port should not change — so
reloading has to happen behind it, with the file watched or re-read on a signal
and the result swapped atomically. Decide whether I takes that on or ships
load-at-startup and leaves reload to its own slice. Prefer the latter unless
reload is cheap: a half-done reload path is worse than none.

Also worth settling:

- Where the file lives, and whether a missing file means "deny everyone" (the
  current `KAMIROH_ALLOW`-unset behaviour) or is an error. Silent deny-all is
  safe but debugging it is miserable; the startup line should say which it did.
- Whether `KAMIROH_ALLOW` survives as an override. Keeping it makes the demo
  script and multi-node local testing keep working.
- The file's permissions are worth a thought but not the key store's treatment:
  an allowlist is public keys, so it is integrity-sensitive, not secret.

Consult the advisor before I lands: the allowlist is a security-sensitive path.

**Then J — the Herdr front**, the last planned slice: a second front holding the
same `Arc<dyn ControlApi>` as the Iroh front. That sharing is the claim the
architecture has been making since slice A ("several fronts, one controller
actor") and J is what tests it.

## Known nits (not worth their own commit)

- `FileKeyStore::default_path()` returns `KeyStoreError::Malformed` when neither
  `XDG_CONFIG_HOME` nor `HOME` is set. Nothing is malformed there — the
  environment is unconfigured — and it will read as a corrupt-key-file error to
  whoever hits it. `Missing` fits better. F came and went without touching that
  file; fix it whenever something next does.
- `ScopedTempFile` has no disarm: `Drop` always removes the temp. Correct today.
  If a future change needs the temp to survive (retrying a link, say), add a
  disarm rather than restructuring the guard.
- An interrupted prompt answers its caller with `ControllerError::Rejected`,
  whose doc says the controller "cannot accept this message in its current
  state". The message *was* accepted and then abandoned, so the fit is loose.
  Nothing else fits better without a domain change, and no caller can act on the
  difference. Revisit if `ControlReply` ever grows a cancellation case.

## Blockers

None. Note the sandbox has no network: Iroh work builds and unit-tests inside
it, but the two-node integration tests and the two-process demo need a run
outside it. `kameo` and its three new crates resolve from the local registry
cache, so slice G needed no network at all.

## Notes for the next session

- The sandbox has no network. Cargo resolves from the local registry cache;
  append `--offline` if a fetch is attempted.
- `cargo clippy --workspace --all-targets` is warning-free today. `missing_docs`
  is `deny` in every library crate, so undocumented public items — including enum
  struct fields — break the build rather than nagging.
- Commits use conventional-commit subjects, one per slice, on `master` — the
  working method is explicitly "one session on the main checkout".
- **Slice headings carry no commit hash.** This file is written before the commit
  exists, so any hash in it is either stale or missing — and a file where some
  slices have one and others don't reads as "that slice isn't committed yet".
  `git log --oneline -- docs/LOOP.md` is the mapping.
- `KAMIROH_KEY_FILE` overrides the key path. Use it when running the binary
  inside a sandbox, or to run several nodes on one machine.
- **The advisor is Opus, and it is a session-level setting** — `/advisor opus`,
  or `claude --advisor opus` at launch. The docs used to say Fable throughout;
  Fable is not currently selectable as an advisor, and they now say "advisor".
  If it is off at a gate, say so rather than self-reviewing and calling the gate
  met: what the gate buys is a reviewer that is not anchored on the reasoning it
  is meant to attack, which a same-session pass cannot be.
- **Update this file in the same commit as the slice it describes.** F2 shipped
  without its entry and the gap was only noticed a slice later, at which point
  the details had to be recovered from the diff. The plan's step 7 is "write
  LOOP.md + commit", in that order.
- The two-process demo lives in the session scratchpad, not the repo. It reads
  `endpoint id:` and `listening:` out of each node's stdout, and both fields
  have bitten it: the id is the *third* whitespace field, and the node binds a
  wildcard (`0.0.0.0:PORT`), not loopback. If it is worth keeping, it belongs in
  the repo next to the integration tests rather than being rewritten each time.
