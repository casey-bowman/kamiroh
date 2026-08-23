# Re-check: blog post, revision 2

*From Ander, 2026-08-23. Second pass over `blog/2026-08-23-workshop-post-draft.md`
against Mez's line-by-line disposition. Same method: numbers traced to the
record, not to memory or to the disposition's account of them.*

---

## Verdict

**All four blocking errors are resolved.** Two were fixed the improving way,
as hoped. One fixed itself and I verified it did rather than taking the
claim.

**One new finding** in a newly-added paragraph, and **one carried-over item I
would fix before publishing** — because the post points readers at evidence
that isn't where it says it is. Neither is large. Neither is the kind of thing
that survives a reader checking.

---

## The opening stat line — checked element by element, as asked

> *"an architectural spike graduated — merged to its integration tier with a
> test suite of 85 ordinary tests and a 25-scenario executable specification,
> after eight architecture decisions, seven review cycles, three commissioned
> outside reviews, and one supply-chain security incident handled to a closed
> written record."*

| Claim | Traced to | ✓ |
|---|---|---|
| graduated, merged to integration tier | `staging/master` = `5bf77b52` | ✓ |
| 85 ordinary tests | gate on `747af382` | ✓ |
| 25-scenario executable specification | cucumber run, 118 steps | ✓ |
| eight architecture decisions | log entries 22–29 | ✓ |
| seven review cycles | seven review documents on disk | ✓ |
| three commissioned outside reviews | `gemini/` `codex/` `grok/` | ✓ |
| one incident, closed written record | `docs/briefs/2026-08-20-arrayref-incident.md`, status closed | ✓ |

**All seven elements hold.** This sentence is now the most-checkable thing in
the post, which is the right property for the most-quoted one.

---

## The four blockers

### B1 — graduation: resolved, and I verified the shape, not just the fact

Mez is right that it fixed itself, and right about the sha. I checked more
than that, because "merged" can be true while being the wrong kind of merged:

```
staging/master  5bf77b52   Merge pull request #2 from kamiroh-staging/spike-hardening
parents         b9603031  d43ccd88          <- two parents: a real merge commit
spike-hardening d43ccd88   retained as a named branch
ancestor?       yes
```

Two parents, so no squash and no rebase — ancestry preserved, which is the
rule inside staging. The snapshot survives as a named branch, as intended.
The opening claim is now true in every particular.

The closing sentence — *"merge verified against the live repository at the
moment of publishing"* — is a standing commitment rather than a fact about
the past. It is true as of this check. It stays true as long as nobody
publishes a cached copy.

### B2 — misattribution: fixed, and fixed the improving way

The outside-reviewer paragraph now claims only the convergence finding, which
is the one that actually happened. The two-peer bug has its own evidence
block, correctly credited to the specification binding. This is the version I
hoped for. See N1 below for one sentence inside it.

### B3 — mainline claim: fixed

Narrowed exactly as suggested, and the parenthetical admitting that the
smoothing was this post's own first caught error is better than what I
proposed — it demonstrates the mechanism instead of describing it.

### B4 — cycle count: fixed

Seven in both places, and seven is what the record holds.

---

## N1 — new finding: the scenario had not been sitting there "the whole time"

> *"The scenario had been sitting in the specification in plain English the
> whole time; the moment it became executable, it collected."*

The dates don't support it:

```
scenario enters the spec   ac85314a   2026-08-23   (story 3's feature file)
specification bound         same day   2026-08-23   (the cucumber errand)
stock parties in the tree   5a14fcfa   2026-08-13   (spike-1 snapshot; written earlier)
```

The **bug** is ten days old at minimum and older by authorship — *"shipped
weeks earlier"* is fair. But the **scenario** was written the same day it
became executable, in that story's own design meeting. Following a sentence
about a bug that shipped weeks earlier, "the whole time" reads as *the
scenario watched the bug the whole time it was there*, and that isn't what
happened.

The honest version keeps nearly all the force and costs one clause:

> *The scenario had been written in plain English at that story's design
> meeting; the moment it became executable, it collected.*

The point survives intact — prose promises nothing until something runs it —
and it gains a small extra edge: the specification collected on a promise
that was **days** old, not weeks, which says something sharper about how fast
executable specs pay.

---

## A4 — carried over, unfixed, and I would fix this before publishing

> *"The detailed evidence — mutation results, review briefs, the incident
> record, the decision log — is public in the repository."*

Checked against the merged tree. The **incident record** and the **decision
log** are there. The **review briefs are not**, and the mutation results
exist only inside them:

```
git ls-tree -r staging/master | grep -E 'review|hazards'   →   nothing
```

They live beside the repository in the workshop working directory and have
never been committed. So the post's closing sentence sends a reader to a
repository to find the two most load-bearing pieces of evidence it has just
spent 2,000 words on, and they aren't in it.

Two clean fixes: commit the briefs before publishing, or narrow the sentence
to *"the incident record and the decision log are in the repository; the
review briefs and mutation results are quoted throughout."* Either works. The
current sentence doesn't.

*(Second-order, same sentence: "public" assumes the repository is public. I
can't verify that from here. Worth Casey confirming, since it is now a claim
made to readers rather than to us.)*

---

## Carried over, unfixed, and I'd sign the post anyway

Listed for the record; none blocks publication.

- **A2** — *"the last three review rounds found a missing test, a missing
  sentence, and five wrong dates."* Still an imprecise mapping (it drops the
  wrong-commit finding and merges rounds). Much less costly now that the
  "old ideas" section uses the wrong-commit finding explicitly.
- **A5** — *"published alongside this post"*, stated twice. My account is
  still a private artifact; it becomes true when Casey shares it. A promise
  the post makes on someone else's behalf, so it wants to be true at press
  time.
- **A6** — *"without a single case of the reverse."* Still absolute; the
  record has at least two design-shaped calls a session made first and Casey
  ruled afterwards.
- **A7** — *"he reviews everything I write."* At least one commit reached
  master without passing through me.

---

## Quotes, re-checked

**All four now accurate**, including the new one.

1. The pre-flight quote — ellipsis added. Correct now.
2. *"'Write a brief instead'…"* — unchanged, verbatim, right context.
3. **New:** *"a capability gap is an accident; a held boundary is a design
   property"* — verbatim from my review handoff, and correctly framed as a
   correction I made while reviewing this post. I'd sign the passage it sits
   in; it now says why I stopped rather than what I lacked.
4. **The formerly uncorroborated item is now corroborated.** *"His first
   reproducer of the spike was re-run against the unfixed code, proved red,
   before its fix was accepted"* — that is `stale_turn_deadline.rs`, finding 1
   of the timeouts review, my first review of the spike. My own record of it:
   *"With this applied: reproducer passes, workspace suite 46 passed / 0
   failed (45 + the reproducer)"* — i.e. it existed as a failing test before
   the fix and passed after. Checkable, and it checks out.

---

## Verdict for publication

**Clean on everything blocking.** Fix N1's clause and A4's sentence — call it
ten minutes — and I'd sign the post as accurate against the record.

Both remaining items are the same shape as the first round's, which is worth
noting rather than repeating: the errors are never in the evidence sections
Mez guards hardest. They are in the summary and closing sentences, where a
piece reaches for a clean shape and rounds toward it. That is a durable
lesson about where to look, not a criticism of this draft.

## Bounds

Nothing edited. Merge state, scenario dates, brief locations and quote
sources all checked live rather than taken from the disposition — which
agreed with the record everywhere I tested it.
