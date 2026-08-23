# kamiroh spike 2 review - 2026-08-22

## Scope

Reviewed current `kamiroh` `master` at `f997e206` against the spike-2 range beginning at `b268a805` (`cowork/spike-2-kickoff`). Focus was on the timeout/disconnect implementation and the embedding-facing docs added or updated during spike 2.

## Findings

1. **[P3] README test count is stale after spike 2.**
   `README.md:58-60` says the hermetic suite is "39 tests", but `cargo test --workspace --all-targets -- --list` currently lists 58 tests. This is small, but it makes the top-level status block understate the coverage spike 2 added and may confuse future reviewers trying to reconcile README claims with CI output.

2. **[P3] Adapter rustdoc still describes a pre-compilation assumption state.**
   `crates/kamiroh-adapter-kameo/src/lib.rs:14-29` still says it was written against `kameo = "0.17"` "without compiling", and `crates/kamiroh-adapter-iroh/src/lib.rs:22-46` says the same for `iroh = "0.35"`-era APIs. The code now builds against `kameo 0.22.2` and `iroh 1.0.3`, and the completion briefs record the API drift that was actually fixed. Leaving those notes as current rustdoc makes generated docs and source readers think the adapters are still speculative.

## Notes

- I did not find a new behavior-blocking issue in the spike-2 implementation. The deadline and disconnect surfaces are well pinned by tests across the domain/app layer, the memory adapter, the Kameo runtime, and real-Iroh loopback.
- The Kameo processing-time timer behavior and the kill/resurrect fan-out race are already called out as accepted spike-2 residue in `ARCHITECTURE.md:431-500`, so I am not re-raising them as fresh findings here.
- The initial sandboxed test run failed only in real-Iroh tests while binding sockets; rerunning with normal local socket permissions passed.

## Verification

- `git diff --check b268a805..HEAD` - passed
- `cargo fmt --check` - passed
- `cargo clippy --workspace --all-targets -- -D warnings` - passed
- `cargo test --workspace --all-targets` - passed with normal local socket permissions
- `cargo test --workspace --all-targets -- --list` - listed 58 tests
