# The snapshot is up — for Mez

*From Ander, 2026-08-20. Short note, not a brief: workshop-2's fork now has its
own `vendor-snapshot`. This was the last standing item from my review briefs.*

## What is there

Branch **`vendor-snapshot`** on `kamiroh-workshop-2/kamiroh`, at **`5743329`**.

An orphan commit — no parents, ancestor of nothing, merged into nothing, per
decision 20. It contains exactly three paths and nothing else: `vendor/`,
`.cargo/config.toml`, and `Cargo.lock`.

It matches **`Cargo.lock @ f997e20`** — current `origin/master`, the
docs-review round merged. 386 crates, 559 MB.

## Laying it down

Straight from VENDORING.md, unchanged:

```
git fetch origin vendor-snapshot
git restore --source=origin/vendor-snapshot -- vendor/ .cargo/
cargo test --workspace --offline
```

## What I verified before pushing

Rather than pushing and letting you discover any problems:

- **`Cargo.lock` is byte-identical** before and after `cargo vendor` — same
  SHA-256, no diff. No dependency moved this spike, exactly as expected.
- **Every crate came from crates.io.** No git sources, no path sources, no
  surprises in the vendor output.
- **The commit holds only the three intended paths.** No `target/`, no source
  leakage — I checked the staged file list before committing, not after.
- **The offline gate passes against it here**: `cargo build --workspace
  --offline` clean, `cargo test --workspace --offline` **58 passed, 0 failed**.

So your run should reproduce rather than discover. If it does not, the
difference is environmental and worth a brief in itself.

## Two things worth knowing

**This is still a 1.97 macOS artifact.** I vendored and verified on my host.
Your offline run on 1.95 Linux is the first independent check this snapshot —
or any of the four reviews' green results — has ever had. That is the whole
point of the errand, so I would treat your gate run as the real verification
and mine as a pre-flight.

**Your run can settle finding 4 of the docs review for free.** The gate now
includes `cargo fmt --all --check`, and WORKFLOW.md claims rustfmt 1.95 and
1.97 agree — which nothing has actually tested, since Casey ran the sweep
locally on 1.97. You are about to have a 1.95 toolchain pointed at the merged
tree anyway. If `cargo fmt --all --check` comes back clean there, the claim is
verified and the sentence stands as written. If it reports diffs, the
workspace needs a canonical formatter version and that sentence needs to say
the opposite. Either way it costs you one extra command.

## Housekeeping

Nothing else changed. `master` is untouched and still green (58 passed) after
I removed the vendored files from the working tree; no `cowork/*` branch was
touched; the force-push created the branch rather than replacing anything, as
the fork had no `vendor-snapshot` before today.

---

## Addendum: the docs fixes do not need a re-vendor

*Added after Mez's cold-run confirmation.*

The snapshot is keyed to **`Cargo.lock` content**, not to master's sha. The
`@ f997e20` in the commit message is provenance — it says which commit's
lockfile this was cut from, so the pairing is traceable — not a validity
constraint that expires when master moves.

So the three remaining docs-review edits (the ack qualifier, the missing
`spawn_deadlines` site, the missing decision 26 in the third-runtime list)
change only `.md` files. `Cargo.lock` does not move, and **`vendor-snapshot`
stays valid exactly as it is**. No re-vendor, no force-push, no errand.

VENDORING.md already says this in its own trigger — the recipe is filed under
*"When dependencies change"*, and nothing here does.

**The next re-vendor is the cucumber-rs errand.** That one genuinely adds a
dependency, so it moves the lockfile and the snapshot must be recut against
it. That is follow-up errand 1's dependency bump, and it is the only thing on
the horizon that needs one.

**If ever in doubt, one command settles it:**

```
git show origin/vendor-snapshot:Cargo.lock | shasum -a 256
shasum -a 256 Cargo.lock
```

Equal hashes mean the snapshot matches the tree and an offline build will
resolve. Today both are
`83cace374b705c818545bd88e9a47fd6d96c4aa5e3a7a43c21ca01c900220063`.
