# Re-check handoff: revision 2 — for Mez

*From Ander, 2026-08-23. Cover note for `2026-08-23-blog-post-recheck.md`.
Second pass, same method: everything traced to the record rather than to the
disposition's account of it. The disposition agreed with the record
everywhere I tested it, which is worth saying before anything else.*

## Verdict

**Clean on everything blocking.** Two small fixes — call it ten minutes — and
I'd sign the post as accurate against the record.

Both of the improving fixes landed the way we hoped, and one of them came
back better than what I proposed: the parenthetical admitting the mainline
smoothing was this post's own first caught error *demonstrates* the mechanism
where I'd only suggested describing it.

## The opening stat line

You asked for this above all, so I traced every element to its source rather
than sampling:

| Claim | Source | |
|---|---|---|
| graduated, merged to integration tier | `staging/master` = `5bf77b52` | ✓ |
| 85 ordinary tests | gate on `747af382` | ✓ |
| 25-scenario executable specification | cucumber run, 118 steps | ✓ |
| eight architecture decisions | log entries 22–29 | ✓ |
| seven review cycles | seven review documents on disk | ✓ |
| three commissioned outside reviews | `gemini/` `codex/` `grok/` | ✓ |
| one incident, closed record | `docs/briefs/2026-08-20-arrayref-incident.md` | ✓ |

**All seven hold.** It is now the most checkable sentence in the post, which
is the right property for the one that will be quoted most.

## On the graduation: I checked the shape, not just the fact

You were right that it fixed itself and right about the sha. I went further
than confirming it, because *merged* can be true while being the wrong kind of
merged — a squash would have satisfied the word and broken the rule:

```
5bf77b52   Merge pull request #2 from kamiroh-staging/spike-hardening
parents    b9603031  d43ccd88     <- two parents: a real merge commit
spike-hardening retained as a named branch; snapshot now an ancestor
```

Two parents, so no squash and no rebase. Ancestry preserved, branch retained.
The claim is true in every particular, not just in outline.

## The two fixes

**One new, in the paragraph that replaced the misattribution.**

> *"The scenario had been sitting in the specification in plain English the
> whole time."*

The dates don't carry it. The scenario entered the spec on 2026-08-23, in
that story's own design meeting, and was bound the same day. The **bug** is
ten days old at minimum and older by authorship — *"shipped weeks earlier"*
is fair and stays. But following that sentence, "the whole time" reads as
*the scenario watched the bug all along*, and it didn't.

The fix costs one clause and I think it sharpens the point:

> *The scenario had been written in plain English at that story's design
> meeting; the moment it became executable, it collected.*

Prose promises nothing until something runs it — and the promise it collected
on was **days** old, not weeks, which says something faster about executable
specs than the current version does.

**One carried over, and this is the one I'd hold publication for.**

> *"The detailed evidence — mutation results, review briefs, the incident
> record, the decision log — is public in the repository."*

Checked against the merged tree: the incident record and the decision log are
there; **the review briefs are not**, and the mutation results exist only
inside them. They have never been committed — they live beside the repository
in the workshop directory.

So the post spends two thousand words on evidence and then sends readers to a
repository that doesn't contain the two most load-bearing pieces of it. Either
commit the briefs before publishing, or narrow the sentence to what's actually
there. Either is fine; the current sentence isn't.

*(One second-order point in the same sentence: "public" assumes the repository
is public. I can't verify that from here, and it is now a claim made to
readers rather than between us — worth Casey confirming.)*

## Carried over, unfixed, and I'd publish over all of them

For the record, not for action: the "last three review rounds" mapping is
still imprecise (much less costly now that the wrong-commit finding appears
explicitly in the old-ideas section); *"published alongside this post"* is
still conditional on Casey sharing my account; *"without a single case of the
reverse"* is still absolute where the record has two; and *"reviews everything
I write"* is a shade beyond what happened.

## Quotes: all four accurate, including the one about me

The ellipsis is in. The boundary correction is verbatim and, more to the
point, correctly framed — the passage now says *why* I stopped rather than
what I lacked, which is the version that supports your argument instead of
undercutting it.

And the formerly uncorroborated item is now corroborated. *"His first
reproducer of the spike was re-run against the unfixed code, proved red,
before its fix was accepted"* — that is `stale_turn_deadline.rs`, finding 1 of
the timeouts review. My contemporaneous note: *"With this applied: reproducer
passes, workspace suite 46 passed / 0 failed (45 + the reproducer)"* — red
before, green after. Checkable, and it checks.

## One pattern, across both rounds

Worth naming because it held twice and will hold again: **the errors are never
in the evidence sections you guard hardest.** Both rounds, every finding sat
in a summary or closing sentence — the places a piece reaches for a clean
shape and rounds toward it.

That is a lesson about where to look, not a criticism of the drafting. The
edges-intact section was right the first time and is right now; what needed
correcting was the prose that summarised it.

## Bounds

Nothing edited, either round. Merge state, scenario dates, brief locations and
quote sources all checked live.

Fix those two and it's yours to publish. Fitting that the post about a loop
where nothing stands on anyone's word ends by not standing on mine either —
every number in it is traceable, including the ones about me.
