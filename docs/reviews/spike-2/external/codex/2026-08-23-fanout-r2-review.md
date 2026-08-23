# kamiroh fan-out r2 review - 2026-08-23

## Scope

Reviewed current `kamiroh` `master` at `c0e36604` against the range since the
last Codex review (`f997e206..HEAD`). Focus was on the newly delivered
allowlist-mutation and same-endpoint fan-out paths, especially the `Phone`,
runtime, memory, Kameo, and Iroh adapter behavior that changed after spike 2.

## Findings

1. **[P2] Memory fan-out can acknowledge a batch that never reached any endpoint.**
   `crates/kamiroh-adapter-memory/src/lib.rs:242-300` always synthesizes and
   enqueues the batch receipt to the sender before checking whether the target
   endpoint has any mailbox at all. Unbound names should indeed disclose
   nothing and still receive one endpoint-level receipt when a live endpoint
   receives the frame, but the memory adapter has no separate endpoint-liveness
   concept; a batch to an entirely absent `bb` is indistinguishable from a live
   endpoint with no matching actors. As a result, `Phone::open_many` returns
   phones and the caller can settle their ack waits from a `mailroom` receipt
   even though no receiving endpoint accepted the batch. Pairwise memory sends
   still report `UnknownAddress` for this kind of no-route case, and Iroh
   fan-out cannot synthesize a receipt without first connecting to the peer.
   This can let memory-backed tests prove "endpoint arrival" in a scenario that
   would be a refused send or pure silence in the real transport. Consider
   adding endpoint presence to `MemoryNet`, or making `open_many` return
   `UnknownAddress` when no mailbox belongs to `to_endpoint` while keeping the
   all-unbound-live-endpoint case explicit in tests with a bound sentinel actor.

2. **[P3] README test count is still stale, and now farther off.**
   `README.md:59-61` still says the hermetic suite has 39 tests. The current
   workspace lists 73 tests with `cargo test --workspace --all-targets -- --list`.
   This was already stale at the prior review; fan-out and allowlist mutation
   have widened the gap.

3. **[P3] Adapter rustdoc still says the adapters were not compiled against old APIs.**
   `crates/kamiroh-adapter-kameo/src/lib.rs:14-29` still says the adapter was
   written against `kameo = "0.17"` without compiling, and
   `crates/kamiroh-adapter-iroh/src/lib.rs:22-46` still says the same for
   `iroh = "0.35"`-era APIs. The code now compiles and tests against current
   dependencies, and the Iroh adapter has even been bumped to ALPN
   `kamiroh/1`. Leaving the old assumption block as live crate docs makes the
   generated docs read more speculative than the code actually is.

## Notes

- I did not find a new behavior-blocking issue in the allowlist mutation
  implementation. The revoke/admit paths are covered in the local runtime and
  Kameo adapter, including idempotent revocation and the multi-conversation
  same-endpoint sweep.
- The fan-out protocol shape is otherwise well pinned: same-endpoint typing,
  one request id across N phones, all-unbound/denied sibling silence, refused
  transport send creating no phones, and real-Iroh frame fan-in are all covered.
- The first sandboxed full test run failed only where real-Iroh tests tried to
  bind sockets. Rerunning with normal local socket permissions passed.

## Verification

- `git diff --check f997e206..HEAD` - passed
- `cargo fmt --check` - passed
- `cargo clippy --workspace --all-targets -- -D warnings` - passed
- `cargo test --workspace --all-targets` - passed with normal local socket permissions
- `cargo test --workspace --all-targets -- --list` - listed 73 tests
