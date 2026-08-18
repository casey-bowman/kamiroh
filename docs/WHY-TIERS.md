# Why the tiers hold: verification as the point

*A companion to [TIERS.md](TIERS.md). That one says what the tiers are and
how work moves between them; this one says why the shape earns its
ceremony — written from the seat that has to check the claims rather than
make them. Written 2026-08-17, the day the tier guide landed on main and
was corrected by its own rule within the hour.*

## TL;DR

- The tiers exist so that claims about this project's history can be
  **checked instead of trusted**: every important "did that happen
  cleanly?" reduces to a hash comparison a stranger can run years from
  now, without believing anyone's account of anything.
- Each rule buys a specific check. No squash or rebase between tiers keeps
  ancestry comparisons truthful; the vendor content boundary is the one
  exception, and it ships with its own acceptance test (an empty diff);
  branches are never deleted, so the evidence stays re-runnable; small
  records may skip the ceremony precisely so the ceremony stays credible
  where it matters.
- The structure makes checks cheap; running them is still a choice — and a
  confident wrong finding costs more than a missed one, so check your own
  state before reporting on anyone else's.
- Almost none of it was foresight: each rule is the smallest thing that
  would have prevented one specific bad afternoon, and the list should grow
  only the same way. (One rule is the exception, and it is marked below.)

## Claims that can be checked instead of trusted

The tiers' real product is not organization. It is that at every boundary,
a *social* claim becomes a *mechanical* one.

"Spike-1 graduated cleanly" is a social claim. It relies on someone's care,
someone's memory, and the absence of a bad afternoon. It decays. Nobody can
re-run it in a year.

Its mechanical form is this:

```sh
git diff <workshop-1-master> 7fe985c    # empty
```

Both trees are `49c70da`. That is content-addressed truth — a stranger can
confirm it in five years without knowing who Casey or Mez is, without
trusting anyone's account of August 2026, and without the repo's authors
being alive to ask. The graduation either happened cleanly or it did not,
and the answer is a hash comparison.

Nearly every rule in TIERS.md exists to keep some claim in that second,
checkable form.

## What each rule is actually buying

- **Ancestry-preserving merges** keep `merge-base` meaningful. Because no
  tier ever squashes or rebases, "is X an ancestor of Y" has a real answer,
  and ahead/behind counts describe reality. Squash once and every later
  comparison across every tier silently starts lying.
- **The workshop→staging content boundary** is the one deliberate exception,
  and it pays for itself in the same currency: the graduating snapshot is
  accepted only if `git diff <workshop-master> <snapshot>` is empty — for
  spike-1, both trees are `49c70da`. An exception with a mechanical
  acceptance test is a bounded exception. The reasoning it rests on is
  walked through in the next section.
- **Branch-per-spike, and never deleting a branch**, keep the evidence alive
  to be re-checked. A check you can only run once is a check you have to
  remember the result of.
- **The direct lane for records** keeps the ceremony proportionate. This is
  subtler than it looks: a process heavy enough to feel absurd for a
  one-paragraph doc is a process people route around, and a rule that is
  routinely skipped protects nothing. Drawing the line explicitly is what
  keeps the spike flow credible where it matters.
- **A human gate before master moves** puts the checkpoint where it is still
  cheap. Everything upstream of that gate is reversible.

## The exception, walked through: why content crosses but history cannot

The content boundary deserves the full explanation, because it is the one
deliberate breach of this project's own iron rule, and the reasoning is
invisible in the rule's statement.

During spike-1 the cloud build environment could not reach crates.io, so
the complete source of the workspace's dependency tree was committed into
the workshop repo — about half a gigabyte of it, re-vendored four separate
times as dependencies moved. (At its largest snapshot: 19,823 files
totalling 514 MB of content. The ~590 MB quoted in TIERS.md and
VENDORING.md is the same tree by a different measure.) It was later moved
off master's tip, but git does not
forget: those blobs live permanently in workshop-1's commit *history*.

Now the chain that forces the boundary. Git merges carry history, so
merging the workshop's master into staging would make every workshop
commit — vendor blobs included — part of staging's ancestry. And clones
fetch all reachable history, not just the current tree. From that moment
on, everyone cloning staging, and then main, until the end of the project,
would carry a vendored dependency archive that is not in the tree they
checked out and never will be. Git compresses it in transit, so the cost
is not the full half-gigabyte on the wire; it is permanent, unavoidable,
and paid by every future clone. One convenient merge, billed forever.

So this one crossing works differently from every other. The spike's result
crosses as a fresh commit whose *tree* is the workshop's final tree exactly,
but whose only parent is staging's own head — zero workshop commits in its
ancestry. The content transfers; the baggage stays home. Two things keep
the breach from being a hole in the ancestry rule: it ships with a
mechanical acceptance test (the diff between workshop master and the
snapshot must be empty — for spike-1 both trees are `49c70da`), and the
workshop goes archival at that moment, so the severed chain never needs
mending.

This is also the entire explanation of a graduated workshop's odd counters:
the permanent ahead count is the history that stayed home, and the growing
behind count is main moving on without it.

## The instruction carries its own test

The smallest piece of this machinery is the easiest to overlook. Errands
arrive as a git bundle plus a line like:

```sh
git log --oneline -2 cowork/tiers-guide   # expect 3ea3d48 on 4805a49
```

That comment is a falsifiable prediction. It converts "apply this merge"
into "apply this merge *if the world is as I believe it to be*," and it can
fail loudly, before anything lands, instead of quietly afterward. An
instruction that states its own expected ancestry is the difference between
a courier and a reviewer.

## Verification cuts both ways

On 2026-08-17 the same discipline produced two findings, opposite in kind.

**A real error.** The freshly written tier guide claimed a graduated
workshop would read "29 ahead, 3 behind" main *forever*. But the guide had
itself legislated a direct lane for records onto main, and every record
that takes that lane increments the behind count. The brief that landed
that morning had already made it 4; landing the guide made it 5. Verified
by arithmetic against the repos, not by reading carefully.

**A false alarm.** Minutes earlier, the guide's central tree-identity claim
appeared to be false: the workshop's tree and main's did not match. The
cause was not the guide. It was a local clone two commits stale. After a
fetch, the trees matched exactly. Had that been reported before refreshing,
it would have been a confident, evidenced, *wrong* finding.

The false alarm is the more instructive half. A missed error costs one
error. A false one spends the reviewer's credibility — and in an ensemble
where a human relays findings between sessions that cannot talk directly,
that credibility is the scarce resource. Recon is not only how you catch
the other side's mistakes; it is mostly how you avoid manufacturing your
own.

## The structure makes the check cheap; it does not make it happen

Nothing here is automatic. Every check above was run because someone chose
to run it. What the tiers contribute is making that choice easy — turning
"did this graduate cleanly?" into a two-command question with an
unambiguous answer. A structure that makes verification expensive gets
verification theater instead.

And the habits themselves were bought with mistakes. The rule that caught
the stale clone — refresh local state before reporting a discrepancy — was
written down after an earlier failure, the relay merge divergence of
2026-08-13 (see `advisories/`). That is the actual engine of this project:
mistake, written habit, cheaper check next time. Not foresight.

## Every rule is a scar

For a newcomer finding the flow heavy, it helps to know that almost none of
it was designed in the abstract:

- The ancestry rule exists because a squash-merged PR into main created
  divergent twin histories that took a reconciling merge to repair.
- The content boundary exists because half a gigabyte of vendored
  dependency sources would otherwise ride every cross-tier merge forever.
- The direct lane is the exception that proves the habit: it was born from
  a question asked before the bad afternoon rather than after one. Someone
  noticed that a process heavy enough to feel absurd for a one-paragraph
  doc would simply be routed around, and drew the line while it was still
  hypothetical.
- The "dated snapshot, not an invariant" framing exists because a guide
  printed a number that its own publication changed.

With that one exception, each rule is the smallest thing that would have
prevented a specific bad afternoon. That is why the list is short, and why
it should be added to only the same way — foresight is welcome, but it has
a worse track record here than hindsight does.

## The day this note describes

The tier guide landed on main at `3ea3d48`. Its first external review
falsified one of its claims within the hour, and the falsifying evidence
was the review's own merge commit — the guide's behind count had already
moved. The repair landed at `42bee2c`, which moved the number again, to 6,
and said so in the text.

A process whose description can be corrected *by the process, while
running*, is one worth trusting with code. Documentation that describes a
live system is subject to that system while being written — and the tiers
are shaped so that when this happens, it shows up as a diff rather than as
a misunderstanding.

---

*Written by Ander (the local Claude Code build/verify session). The tier
design is Casey's and Mez's; this note is the reviewer's view of why it
works.*
