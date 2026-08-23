# Incident record: the 2026-08-20 arrayref supply-chain attack

**Status: closed 2026-08-23 — kamiroh verified unaffected, all
participating hosts checked clean.** Kept in-repo (graduated from the
project's session notes) because WORKFLOW.md's dependency-bump
hardenings defer to this record, and a procedure that defers to a record
wants the record next to it.

## What happened upstream

On 2026-08-20 (~07:10 UTC), malicious versions of several crates were
published to crates.io and removed within roughly an hour:
`arrayref 0.3.10`, `internment 0.8.7`, `append-only-vec 0.1.9`, and
typosquats including `proc-macro1`, `proc-macro-en`, `aovine`,
`arone*`, and `tinymember`. The payload was a `build.rs` phoning
`23.254.165.112`. `arrayref` mattered here because it is a transitive
dependency of iroh (via blake3). Advisory:
blog.rust-lang.org/2026/08/20/supply-chain-attack-on-arrayref/

## Why kamiroh was unaffected — verified, not assumed

- The committed `Cargo.lock`, frozen all spike, pins `arrayref 0.3.9`
  — the pre-compromise version.
- Its lockfile checksum matched the vendored package's checksum
  exactly; the vendored 0.3.9 has no build script.
- None of the other named crates appear anywhere in the tree.
- Every build of the spike used the committed lock (`--locked`
  verified in review), no dependency bump occurred in the exposure
  window, and the cloud sandbox cannot reach crates.io at all.

## Host checks

- Casey's machine: cargo cache and network checked 2026-08-20 — clean.
- Ander's host: checked 2026-08-23 as the gate on the cucumber-rs bump
  (step 0 of that errand) — clean; only the pre-compromise
  `arrayref-0.3.9` cached, no contact with the C2 address or its /24.

## What it changed

The dependency-bump hardenings in WORKFLOW.md are this incident's
legacy: cache-checked hosts, exact pins with a freshness window,
`cargo audit`, crate-by-crate review of new lockfile entries with every
`build.rs` read, and a vendor-snapshot recut confirmed by an
independent offline build. The first bump under the full procedure
(cucumber-rs, 2026-08-23) surfaced one unfamiliar crate name (`zmij`),
which both sessions investigated independently and cleared — Ander at
his lockfile review, Mez at the cold offline verification, each reading
its `build.rs` before the other's conclusion was known — the
procedure behaving as designed.
