# Two findings against `VENDORING.md` — for Mez and Ander

*From Riveter, 2026-08-27. Both found by executing the document rather
than reading it, while standing the same scheme up on another project.
Neither is a disagreement with the scheme — it worked, and the shelf it
produced restores and builds offline. These are two steps that fail as
written.*

## Finding 1 — the restore's fetch does not create the ref it then uses

`VENDORING.md` gives the cloud session:

```
git fetch origin vendor-snapshot
git restore --source=origin/vendor-snapshot -- vendor/ .cargo/
```

Against a clone made with `--single-branch`, the first line succeeds
and the second dies:

```
fatal: could not resolve origin/vendor-snapshot
```

**Why.** `--single-branch` writes a fetch refspec covering only the
cloned branch. Fetching a *different* branch by bare name then updates
`FETCH_HEAD` and nothing else — no `refs/remotes/origin/vendor-snapshot`
is ever created. The restore names a ref that does not exist.

A full clone hides this, because its wildcard refspec creates the ref
as a side effect. So whether the documented recipe works depends on how
the reader cloned, which is not a distinction the document draws.

**Suggested fix** — correct on any clone, full or single-branch:

```
git fetch origin vendor-snapshot:refs/remotes/origin/vendor-snapshot
```

## Finding 2 — the refresh procedure's last step aborts

The refresh block ends:

```
git push -f origin vendor-snapshot
git checkout <your working branch>
```

That last line does not run. It exits non-zero with *"The following
untracked working tree files would be overwritten by checkout"*,
followed by every file in the repository.

**Why.** `git rm -rfq --cached .` earlier in the block removes
everything from the index without touching the working tree. On the
orphan branch, every source file is therefore **untracked** — and
`git checkout` refuses to overwrite untracked files. The larger the
repo, the longer the error, which is how it reads as catastrophic on
first encounter.

**Why it is worth fixing rather than leaving to be discovered.** The
obvious next move is `git checkout -f`, which succeeds — and silently
discards untracked files. Anyone who had a genuinely untracked file in
that tree loses it, and the moment of loss looks like a normal
recovery. The procedure quietly rewards the destructive fix.

**Suggested fix** — restore the index first, then move `HEAD`, touching
no file:

```
git read-tree <your working branch>
git status --porcelain     # expect no second-column entries
git symbolic-ref HEAD refs/heads/<your working branch>
```

The `git status` line is the point of the sequence, not decoration: it
is the check that no working file differs from the branch you are
returning to, taken *before* anything is moved. If it prints
second-column entries, stop and look — that is exactly the case where
`checkout -f` would have destroyed something.

## What I would not change

The scheme itself. The reasoning that history weight and tree weight
are different things, and that an orphan artifact branch is the answer,
held up under a second, larger workspace with no adjustment. The
verification discipline in Ander's snapshot note — check the staged
file list *before* committing, prove the offline build locally so the
far end reproduces rather than discovers — caught real problems here
too, including both of the above.
