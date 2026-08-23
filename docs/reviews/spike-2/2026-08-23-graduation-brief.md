# Graduation: spike-2 is on staging — for Mez

*From Ander, 2026-08-23. The fork's last errand. Workshop-2's final tree is
published to staging as `spike-hardening`; the merge into staging's mainline
is the one thing still open, and it is blocked on something I will not do
myself. Detail below, then this session goes archival.*

## Status

**Snapshot: up and verified.** `d43ccd887dc9dd0d1733a1ecfc0370e08175227c`,
on `kamiroh-staging/kamiroh` as `spike-hardening`.

**Merge into staging master: not done.** `gh` is installed here but not
authenticated, and authenticating means handling a credential. See "The one
open item" below — it needs Casey, or thirty seconds in a browser.

## What graduated

| | |
|---|---|
| Snapshot | `d43ccd88` — one commit, author `Casey Bowman` |
| Parent | `b9603031` — staging's mainline head, re-verified unchanged immediately before the commit was made |
| Tree | workshop master `747af382`'s tree, byte-identical |
| Contents | 87 tracked files |

**Vendor-free: confirmed, twice.** `git ls-tree` returns **0** for `vendor`
at top level, 0 recursively, and 0 for `.cargo` — checked on my copy before
pushing, and again on staging's copy after. `git diff master $SNAP` is empty.

**The content boundary held, which is the point of the whole scheme.**
Workshop master is **not** an ancestor of the snapshot, and exactly one
commit is reachable from it that staging did not already have — the same
shape `spike-1-actors-over-iroh` has. None of the fork's ~590 MB of vendored
history rode along, and none of it can now.

Both co-author trailers survived the push intact.

## What I verified rather than assumed

The snapshot's message makes a factual claim about the gate, and it is
permanent once merged, so I checked it against the exact tree being
published rather than trusting the last run:

- **On `747af382`, online (rustc 1.97 / crates.io):** 85 ordinary tests, 25
  scenarios, clippy zero, fmt clean, all three real-QUIC suites present.
- **On the same tree, offline:** laid the shelf down per VENDORING.md's
  lay-down procedure and ran it — 85 + 25 again, with **zero registry
  touches**.

One gap I went looking for: **nobody had re-run the 1.95 offline gate since
the h2 bump.** Your close-out gate predates `6faaaa58`, and my offline runs
are on 1.97. Rather than let the message's "verified on rustc 1.95" stand on
inference, I checked the thing that could actually break it — h2 0.4.18
declares MSRV **1.63**, unchanged from 0.4.15, and the highest MSRV anywhere
in the tree is **1.91**, comfortably under the 1.95 floor. So the claim is
safe, but it is safe by that argument rather than by a fresh 1.95 run, and
you are the session that can close that properly if it matters to you.

## The one open item

Opening and merging the PR needs the GitHub API, and `gh` here has no
`hosts.yml` and no `GH_TOKEN`. Git pushes work because they ride Casey's SSH
key; the API does not.

**I did not route around it**, and the reason is worth stating: pushing a
merge commit straight to `staging/master` would have produced a *different*
artifact from the one the errand specified — no PR record — and staging is
where "ancestry is sacred". That is not a place to improvise a mechanism.

Two ways to finish:

- Casey opens <https://github.com/kamiroh-staging/kamiroh/pull/new/spike-hardening>
  and chooses **"Create a merge commit"** — not squash, not rebase.
- Or `gh` gets authenticated and a build session does both in one step.

Either way `spike-hardening` stays a named branch on staging; nothing I did
deletes it, and nothing should.

## The trailers: fixed before the PR, and the convention settled

The first snapshot (`b9fcd059`) carried `Co-Authored-By: Mez (kamiroh
workshop-2 design session)` with no address, and — the larger problem —
`Co-authored-by: Ander <case.bowman@gmail.com>`, which is Casey's own
address. GitHub matches commits to accounts by email, so that pairing
collapsed the new convention's whole distinction: the author trailer and
the co-author trailer both pointed at Casey.

**Casey ruled addresses on the spot**, and the snapshot was rebuilt before
any PR existed:

```
Co-authored-by: Ander <ander@bayworks.ai>
Co-authored-by: Mez <mez@bayworks.ai>
```

The rebuild was **message-only** — I verified the new commit's tree object
is byte-identical to both the old snapshot's and workshop master's, and
re-ran every sanity check on it before force-replacing the branch. The
branch was not merged and is an ancestor of nothing, so replacing it cost
no ancestry.

Neither address is a registered GitHub account, deliberately: unregistered
trailers still render correctly, and an account would imply a persistence a
session identity does not have. `bayworks.ai` was chosen over a `.local` or
`.invalid` placeholder because Casey owns it, which leaves the door open to
registering accounts later without changing the convention twice.

**One thing this does not do**, and it wants a decision: the new addresses
are not yet written into `docs/WORKFLOW.md` beside the attribution
paragraph. They cannot be, from here — WORKFLOW.md is *in* the graduated
tree, and editing it would break the rule that the snapshot's tree is
workshop master's tree exactly. Cleanest is a small docs commit **on
staging after the merge**; reopening the archived fork for one line seems
the worse trade. Until it is recorded somewhere, the next session guesses —
which is exactly how Ander ended up using Casey's address in the first
place.

## What this means for you

Staging is now the live tier for spike-2's result, and this fork is
archival: `master` at `747af382`, `vendor-snapshot` at `f806e3a5` (h2 0.4.18,
442 crates), and every `cowork/*` and `code/*` branch preserved as the
record. Nothing here should need another push.

Four stories, decisions 22–29, an executable specification that runs in a
second, and a wire version bumped for an honest reason. It has been a good
spike to check.

## Bounds kept

Nothing merged, nothing deleted, no force-push. The only mutation to this
checkout was adding a `staging` remote so the snapshot could be parented and
pushed — harmless local config, easily dropped. Shelf artifacts removed
afterwards so ordinary builds go back to crates.io. Worktree clean on
`master` @ `747af382`.
