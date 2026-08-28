# Vendored dependencies: how and why

*Plain-language guide for everyone touching this repo — Casey, Claude Code
sessions, and Mez (the cloud Cowork session). Adopted August 13, 2026;
decision 20 in `ARCHITECTURE.md`. Two steps corrected August 27, 2026 after an
outside project executed this document rather than reading it — the findings
are published verbatim at `docs/external/2026-08-27-vendoring-findings.md`,
and both suggested fixes were adopted as written.*

## The problem this solves

Mez's cloud sandbox cannot reach crates.io, so it needs dependency sources on
disk to build and test (`cargo vendor` output). But committing `vendor/` to
`master` turned out to be expensive in a way that deleting it later cannot
fix: **git history carries every committed blob forever**, and merges carry
history. After the iroh bump, `vendor/` weighed ~559 MB — and any
ancestry-preserving merge toward staging or main would have dragged all of it
along, even if a later commit deleted the folder. Tree weight and history
weight are different things.

## The scheme

- **`master` never contains `vendor/` or `.cargo/`** — both are gitignored.
  Normal builds (Casey, Claude Code, anyone online) just use crates.io;
  nothing special to do.
- **The `vendor-snapshot` branch is an artifact shelf, not history.** It is an
  orphan branch containing only `vendor/` and `.cargo/config.toml`, matching
  the current `Cargo.lock`. It is force-pushed whenever dependencies change,
  merged into nothing, and ancestor of nothing. Its weight stays its own.
- **The cloud session lays the snapshot down as untracked files**:

  ```
  git fetch origin vendor-snapshot:refs/remotes/origin/vendor-snapshot
  git restore --source=origin/vendor-snapshot -- vendor/ .cargo/
  cargo test --workspace --offline   # hermetic, as before
  ```

  The explicit refspec is not decoration. `git fetch origin vendor-snapshot`
  updates `FETCH_HEAD` and nothing else unless the clone's own refspec happens
  to cover that branch — so on a `--single-branch` clone no
  `refs/remotes/origin/vendor-snapshot` is ever created, and the restore dies
  with *"could not resolve origin/vendor-snapshot"*. A full clone hides this,
  which makes the failure depend on how the reader cloned. Naming the
  destination works either way.

  **Checking a shelf you already have.** The shelf is keyed to `Cargo.lock`
  *content*, not to the commit it was cut from, so it stays valid until a
  dependency actually moves:

  ```
  git show origin/vendor-snapshot:Cargo.lock | shasum -a 256
  shasum -a 256 Cargo.lock
  ```

  Equal hashes mean the shelf matches the tree and an offline build will
  resolve. This needs the same tracking ref the restore does, so fetch with
  the explicit refspec first.

## When dependencies change (Claude Code)

After a dep bump builds green, refresh the shelf:

```
# from the branch where Cargo.lock is current:
cargo vendor vendor
git checkout --orphan vendor-snapshot   # or: git checkout vendor-snapshot
git rm -rfq --cached .
git add -f vendor .cargo/config.toml Cargo.lock
git commit -m "vendor snapshot for Cargo.lock @ <short-sha of source branch>"
git push -f origin vendor-snapshot

# back to where you were — see "getting back" below; do NOT use checkout here
git read-tree <your working branch>
git symbolic-ref HEAD refs/heads/<your working branch>
git status --porcelain     # expect empty
```

**Getting back, and why it isn't `git checkout`.** `git rm -rfq --cached .`
above empties the index without touching the working tree, so on the orphan
branch every source file is *untracked* — and `git checkout <branch>` refuses
to overwrite untracked files, failing with a list of the entire repository.
The obvious next move, `git checkout -f`, does succeed, and that is the trap:
it overwrites those files from the target tree, so **any edit you made to a
source file during this procedure is silently discarded** and the result looks
exactly like a clean return. (An unrelated untracked file survives; it is the
edits that go.)

The three-line sequence restores the index and moves `HEAD` without touching a
single file. The `git status` line is the check, not decoration: it asks
whether any working file differs from the branch you returned to. Empty means
clean; anything else means stop and look — a modified source file appears as
` M`, which is precisely the case where `checkout -f` would have destroyed
something without saying so.

The source finding places this check *before* `symbolic-ref`; kamiroh runs it
after, where the expected output is simply empty rather than a pattern to read
inside four lines of index noise. `symbolic-ref` moves no files, so checking
afterward costs nothing and reads at a glance. The finding's placement is
equally safe.

**Post-condition:** you are back on your branch with every file as you left
it, and `vendor/` and `.cargo/` **remain on disk**, gitignored and therefore
invisible to `git status`. That is deliberate — the common case is a session
that wants the shelf it just built. To remove it, one explicit optional step:

```
git clean -fdX -- vendor/ .cargo/     # post-condition: shelf gone, nothing else touched
```

Capital `X` removes *only ignored* files under those paths. It cannot touch a
tracked file and cannot touch a genuinely untracked one. That narrowness is the
point — it is the opposite of `checkout -f`.

**The standing rule this incident bought:** *no step of this recipe deletes
anything implicitly; every change to disk state appears in a stated
post-condition.* The old scheme's convenience and its data-eating hazard were
the same behavior wearing two faces — you cannot keep the tidy half of a silent
deletion.

Force-pushing here is fine and expected: the branch is a single-writer
artifact with no downstream ancestry. (If `.cargo/config.toml` doesn't exist
because you're on the de-vendored master, create it first:)

```
[source.crates-io]
replace-with = "vendored-sources"

[source.vendored-sources]
directory = "vendor"
```

## Graduation: the history boundary

The ~590 MB of vendor blobs already in this fork's history stay its private
cost — they must never ride to staging or main. Publication
(workshop → staging) is therefore a **content boundary, not an ancestry
boundary**: the spike graduates as a fresh, vendor-free snapshot branch (its
final tree as one or a few curated commits), pushed as `spike-<name>` per
TIERS.md. Within staging, and from staging to main, the plain-merge-commit
rule applies with full force. See TIERS.md ("Cross-tier merges preserve
ancestry") for the rule and its carve-out.
