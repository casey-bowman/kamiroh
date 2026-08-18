# Tiers: how kamiroh development is organized

*The guide to the repo-of-repos structure this project lives in — what
the tiers are, how work flows between them, and the rules that keep
their shared history clean. Written after the process carried its first
spike (spike-1) from empty workshop to canonical main, so everything
here is practiced, not aspirational. For how work happens day-to-day
inside a workshop (the human/AI ensemble, branch namespaces,
verification habits), see [WORKFLOW.md](WORKFLOW.md).*

## The shape

Development proceeds in **spikes** — bounded experiments with a question
to answer — organized across a set of GitHub repos called **tiers**.
Every tier is a fork of the main repo. GitHub allows one fork of a repo
per organization, so each tier lives in its own org, all owned by the
same person:

| Tier | Repo | Role |
|------|------|------|
| main | `casey-bowman/kamiroh` | The canonical repo. What the project *is*. |
| staging | `kamiroh-staging/kamiroh` | Integration tier — collects graduated spikes before they flow to main. |
| workshop-0 | `kamiroh-workshop/kamiroh` | First architectural spike (archival). |
| workshop-1 | `kamiroh-workshop-1/kamiroh` | Second architectural spike: actors over iroh (archival since 2026-08-13). |
| workshop-2, … | `kamiroh-workshop-2/…` | Future architectural spikes, one fresh org and fork each. |

Two kinds of spike:

- **Architectural spikes** are grand-scale: the workshop fork *is* the
  spike. It gets its own org, its own life, and — crucially — its own
  death: once its spike graduates, the fork becomes **archival**,
  preserved forever, never synced again.
- **Implementation spikes** are small-scale: just branches inside
  whichever workshop fork they belong to. No new orgs.

## The flow

```
workshop-N fork ──► snapshot branch in staging ──► staging mainline ──► main
 (the spike,          (spike-<name>, one            (same-repo PR,       (plain merge
  full history)        vendor-free commit)           merge commit)        or ff)
```

**1. Work** happens in the workshop fork: commits to its `master`,
implementation spikes as branches, vendored dependencies if the
environment demands them (see [VENDORING.md](VENDORING.md)).

**2. Publish to staging — the content boundary.** A workshop's history
may carry weight that must never be exported: workshop-1's history
contains ~590 MB of vendored dependency sources, which committed blobs
would drag through every ancestry-preserving merge forever. So a spike
graduates as a **fresh, vendor-free snapshot**: a single curated commit
whose *tree* is the workshop master's final tree, **parented on
staging's mainline head** (not on workshop history). Concretely:

```sh
# in a clone that has both remotes; STAGING_HEAD = staging's master tip
SNAP=$(git commit-tree 'workshop-master^{tree}' -p $STAGING_HEAD \
       -m "spike-N: <name> — graduated snapshot ...")
git push staging $SNAP:refs/heads/spike-<descriptive-name>
```

Parenting the snapshot on staging's head is what makes the next step an
ordinary same-repo PR. Verify before pushing:
`git diff workshop-master $SNAP` must be **empty** — the tree travels
exactly, only the history stays home.

**3. Integrate inside staging.** Open a *same-repo* PR in the staging
repo, from `spike-<name>` into staging's `master`, and merge it with a
**merge commit**. This keeps a review/discussion record while preserving
the branch-per-spike rule: every graduated spike remains visible as a
named branch in staging forever.

**4. Graduate to main.** Staging's mainline flows to main by plain merge
or fast-forward. (Spike-1's graduation needed a one-time *reconciling*
merge — main carried two squash-merged twin commits from before the
rules below existed; the merge adopted staging's tree wholesale with
both histories as parents, paying that debt once. Future graduations
should be clean.)

**5. Afterward.** The workshop fork is archival: all branches, briefs,
and advisories preserved in place; its `vendor-snapshot` branch is
**never deleted** (it is what makes the workshop's builds reproducible
offline). Staging picks main's later commits up by fast-forward
(GitHub's "Sync fork" button does exactly this) whenever convenient.

## What may land on main directly

The spike flow exists to contain the hazards of *experimental* work:
unsettled code, exploratory history, heavyweight vendored dependencies.
Not everything has those hazards, and forcing everything through three
tiers would only manufacture ceremony — while quietly tempting people to
skip the flow entirely when it feels heavy. So the line is drawn
explicitly:

- **Anything that changes behavior rides a spike.** Code, tests, build
  configuration, dependencies — an implementation-spike branch in the
  active workshop, or a fresh workshop org for architectural work. No
  exceptions for "small" changes; small is how spike discipline erodes.
- **Records and documentation of graduated work may land on main
  directly**: validation briefs, process guides, README wording. These
  document what main already contains rather than changing what it does.
  They still arrive with the usual discipline — a short-lived, named,
  reviewed branch, merged by fast-forward — just without the tier hops,
  which would add nothing since there is no experiment to contain.

When in doubt, it rides a spike. The direct lane is for ink, not
machinery.

## The two iron rules

**Cross-tier merges preserve ancestry.** The tiers are permanent,
coexisting branches of one lineage, continuously compared by ancestry.
Traffic between tiers uses plain **merge commits or fast-forwards** —
never squash or rebase merges, which re-apply commits under new hashes,
orphan the shared history, and poison every later sync and ahead/behind
count. (This rule was learned the hard way: a squash-merged PR into main
created divergent twin histories that took a reconciling merge to
repair.) Linearize only history that is private or about to die —
unpublished branches rebase freely; tier mainlines live forever.

**Workshop → staging is a content boundary, not an ancestry boundary.**
The one carved-out exception to ancestry preservation, for the vendored-
blob reason above. The snapshot exports the *result*; the workshop keeps
the *history*; and because the workshop goes archival at that moment,
the divergence never needs re-syncing. Direct workshop → main PRs are
retired for the same reason.

## Reading the ahead/behind numbers

Because the tiers are permanent forks, GitHub's ahead/behind counters
need interpretation, not alarm. The one that most needs explaining is
the graduated workshop, so here is the real case.

**Worked example: workshop-1 shows "29 ahead, 3 behind" main — forever —
and that is the process working, not drift.** The two numbers are the
two halves of the content boundary:

- The **29 ahead** are the spike's actual development history — the
  scaffold, the adapters, the vendoring era, the docs, the merges. This
  history *never* flows to main, on purpose: it carries the vendored-
  dependency blobs (~590 MB) that the content boundary exists to keep
  out of main. Main received the spike's *final tree* via the snapshot;
  it will never receive these commits.
- The **3 behind** are the graduation commits themselves — the snapshot,
  staging's PR merge, and the graduation merge into main. These *never*
  flow back, on purpose: the workshop went archival at graduation, and
  nothing syncs into an archive.

The same story as a picture — one lineage, told twice:

```
 (shared seed 380bc98)
      │
      ├── a ── b ── c ── … ── z            workshop-1 master
      │      the 29: real history,           (archival: frozen forever,
      │      vendor era included,             tree IDENTICAL to the
      │      never exported                   snapshot's tree)
      │
      └────── S ───── M₁ ───── M₂          main master
           snapshot   staging   graduation
           (tree of    PR merge  merge
            z exactly)
              └──────── the 3: never flow back ────────┘
```

The commits differ; the *code* does not. The meaningful check is the
tree, not the counter: `git diff workshop-1-master main-master` was
empty at graduation and stays empty forever, because neither side's
master moves again. A dev seeing "29 ahead, 3 behind" on workshop-1
should read it as a tombstone inscription: *here lies spike-1's full
history; its result lives on in main.*

The other counters read conventionally:

- **Staging *N* behind, 0 ahead of main** — healthy. Staging is a strict
  ancestor; "Sync fork" (a fast-forward) zeroes it whenever you care to.
- **Anything ahead on staging that isn't a spike branch, or new commits
  on an archival workshop's master** — *that* is worth investigating:
  archival means archival.

A tip that spares every future reader this section: put the explanation
in the workshop repo's GitHub **About/description** field (browser edit,
no commit, tree stays pristine) — e.g. *"Archival: spike-1 (actors over
iroh), graduated to main 2026-08-13. The permanent ahead/behind split vs
main is by design — see docs/TIERS.md in main."* It's the first thing a
visitor sees, right next to the counter that prompted their question.

## Practical notes from the first graduation

- **GitHub's compare banner on a fork defaults the PR base to the
  upstream repo.** Publishing a spike branch in staging and clicking
  "Compare & pull request" will offer a cross-fork PR into *main* —
  wrong tier, and it may report "Can't automatically merge." Change
  the **base repository** dropdown to the staging repo itself (or use
  the direct URL: `…/kamiroh-staging/kamiroh/compare/master...spike-<name>`).
- **Check the merge button's dropdown** says "Create a merge commit" —
  GitHub remembers the last-used mode per repo, and squash is a common
  default. (CLI equivalent: `gh pr merge --merge`.)
- Every push needs credentials scoped to the *target* repo; public
  forks are world-readable but never world-writable. In this project's
  practice, pushes are made by a human (or a locally-running agent)
  from local clones over SSH; cloud-sandboxed agents hand commits over
  as git bundles instead (see WORKFLOW.md).
- New organizations cannot be created via API — browser only.

## Starting the next architectural spike

Create the new org and fork (browser), clone it as `tiers/N-workshop/…`
alongside the others, and begin. The fork starts from main's current
state — which, thanks to the rules above, contains every previous
spike's result with clean ancestry back to the seed.
