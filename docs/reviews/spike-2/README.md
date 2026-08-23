# Spike 2 — the process record

The working papers of spike 2 (*hardening*: decisions 22–29), published
deliberately. The decision log says *what* was decided and
[`docs/mappings/`](../../mappings/) says *how the questions were framed*;
this directory is the third thing — what was checked, what was found, and
what was argued about on the way.

It is kept because a paper trail whose findings are invisible is
indistinguishable from one with no findings. Everything here is verbatim as
written at the time, including the parts that turned out to be wrong.

**One deliberate edit, recorded here so the record never fixes itself
quietly.** In
[`2026-08-20-timeouts-review-handoff.md`](2026-08-20-timeouts-review-handoff.md)
a local filesystem path was redacted for publication — the personal prefix
replaced with block characters, the tail kept because the paragraph beneath
it discusses that tail. It is the only edit of any kind in this directory,
and the only path or machine name found in it: every file here and in
[`external/`](external/) was scanned independently by both sessions.

## How to read it, if you only read one

- **The argument** — [`2026-08-23-verification-pressure.md`](2026-08-23-verification-pressure.md).
  A first-person account of why the process worked, with the numbers traced
  to the documents below.
- **A worked example of the whole loop** — the fan-out story: a hazards note
  written before the build, a review that broke the code on purpose, a
  one-commit re-check confirming the fix, and a handoff summarising each.

## The shape of the loop

Every story ran the same cycle, and produced one document per step:

| Step | Document type | Written by |
|---|---|---|
| Design meeting | mapping board — see [`docs/mappings/`](../../mappings/) | design session |
| Before the build | **hazards note** — what will probably bite this | build session |
| After the build | **review** — findings, with evidence | build session |
| Cover note | **handoff** — the review's asks, condensed | build session |
| After the fixes | **confirmation** — one-commit re-check | build session |

## Story 1 — timeouts (decisions 22–25)

| Document | What it is |
|---|---|
| [`2026-08-20-story-2-hazards.md`](2026-08-20-story-2-hazards.md) | Hazards carried forward from the timeouts review into story 2 |
| [`2026-08-20-timeouts-review.md`](2026-08-20-timeouts-review.md) | The full review — seven findings, two with reproducers |
| [`2026-08-20-timeouts-review-handoff.md`](2026-08-20-timeouts-review-handoff.md) | Cover note |
| [`2026-08-20-timeouts-fixes-review.md`](2026-08-20-timeouts-fixes-review.md) | Re-review after the fixes |
| [`2026-08-20-observer-fix-check.md`](2026-08-20-observer-fix-check.md) | One-commit check of the observer fix |

## Story 2 — disconnects (decision 27)

| Document | What it is |
|---|---|
| [`2026-08-20-disconnects-review.md`](2026-08-20-disconnects-review.md) | The review — one defect, four-line fix |
| [`2026-08-20-disconnects-review-handoff.md`](2026-08-20-disconnects-review-handoff.md) | Cover note |

## Story 3 — live allowlist mutation (decision 28)

| Document | What it is |
|---|---|
| [`2026-08-23-allowlist-mutation-hazards.md`](2026-08-23-allowlist-mutation-hazards.md) | Hazards note, written before the build |
| [`2026-08-23-allowlist-mutation-review.md`](2026-08-23-allowlist-mutation-review.md) | The review — a coverage gap found by mutation testing |
| [`2026-08-23-allowlist-mutation-review-handoff.md`](2026-08-23-allowlist-mutation-review-handoff.md) | Cover note |
| [`2026-08-23-allowlist-mutation-r2-confirmation.md`](2026-08-23-allowlist-mutation-r2-confirmation.md) | One-commit check confirming the new test catches the break |

## Story 4 — same-endpoint fan-out (decision 29)

| Document | What it is |
|---|---|
| [`2026-08-23-fanout-hazards.md`](2026-08-23-fanout-hazards.md) | Hazards note, written before the build |
| [`2026-08-23-fanout-hazards-handoff.md`](2026-08-23-fanout-hazards-handoff.md) | Cover note for the hazards |
| [`2026-08-23-fanout-review.md`](2026-08-23-fanout-review.md) | The review — two coverage gaps, one missed documentation site |
| [`2026-08-23-fanout-review-handoff.md`](2026-08-23-fanout-review-handoff.md) | Cover note |
| [`2026-08-23-fanout-r2-confirmation.md`](2026-08-23-fanout-r2-confirmation.md) | One-commit check |

## The outside reviews

Three other AI systems — Gemini, Codex and Grok — were commissioned to
review the spike cold, late in the work. Each reported twice: once mid-spike
and once near completion. **Their reports are published here verbatim**, in
[`external/`](external/), alongside the disposition and the verification of
what they found.

All three surviving findings were in code that had already reached the
mainline, which is what the outsiders were for. Two of the three converged
independently on the same one.

Reading them next to the disposition is the point: the reports are the raw
input, and the review below is what happened when each finding was checked
against the code rather than accepted on authority.

| The reports | |
|---|---|
| [`external/gemini/`](external/gemini/) | Project assessment (08-22), plus spike-2 reviews on 08-22 and 08-23 |
| [`external/codex/`](external/codex/) | Overall sense and spike-2 review (08-22), fan-out review (08-23) |
| [`external/grok/`](external/grok/) | Overall sense and spike-2 review (08-22), spike-2 review (08-23) |

| The disposition | |
|---|---|
| [`2026-08-23-external-reviews-review.md`](2026-08-23-external-reviews-review.md) | Review of the fixes, including whether each outside finding was dispatched accurately |
| [`2026-08-23-external-reviews-review-handoff.md`](2026-08-23-external-reviews-review-handoff.md) | Cover note |
| [`2026-08-23-spike-2-close-handoff.md`](2026-08-23-spike-2-close-handoff.md) | Close-out check: per-peer parties, hardenings recorded, conventions written |

Grok's two-plane brief and response are **not** here: they are workshop-3
design inputs rather than spike-2 reviews.

## The executable specification, and graduation

| Document | What it is |
|---|---|
| [`2026-08-23-cucumber-completion.md`](2026-08-23-cucumber-completion.md) | Binding all four feature files to running steps — and the supply-chain gate that preceded the dependency bump |
| [`2026-08-23-cucumber-handoff.md`](2026-08-23-cucumber-handoff.md) | Cover note |
| [`2026-08-23-graduation-brief.md`](2026-08-23-graduation-brief.md) | The snapshot to this tier: what was verified, and the step that blocked |

## Infrastructure and capability notes

| Document | What it is |
|---|---|
| [`2026-08-19-workshop-2-cloud-capability-check.md`](2026-08-19-workshop-2-cloud-capability-check.md) | What the sandboxed design session can and cannot do |
| [`2026-08-20-vendor-snapshot-up.md`](2026-08-20-vendor-snapshot-up.md) | The vendored-dependency shelf, and how offline builds are verified |
| [`2026-08-20-docs-review.md`](2026-08-20-docs-review.md) | Review of the spike's documentation pass |
| [`2026-08-20-docs-review-handoff.md`](2026-08-20-docs-review-handoff.md) | Cover note |

## Reviewing the write-up itself

The blog post about this spike went through the same loop, against the same
record. Included because the post claims it was reviewed, and a claim like
that should be checkable.

| Document | What it is |
|---|---|
| [`2026-08-23-blog-post-review.md`](2026-08-23-blog-post-review.md) | First pass — four factual errors, all of one kind |
| [`2026-08-23-blog-post-review-handoff.md`](2026-08-23-blog-post-review-handoff.md) | Cover note |
| [`2026-08-23-blog-post-recheck.md`](2026-08-23-blog-post-recheck.md) | Second pass on the revision |
| [`2026-08-23-blog-post-recheck-handoff.md`](2026-08-23-blog-post-recheck-handoff.md) | Cover note |

## Two notes on reading these

**They are verbatim, including the mistakes.** Several documents contain
claims corrected later — a test count anchored to the wrong commit, five
dates a day ahead, a finding credited to the wrong party. The corrections
are in the later documents rather than edited into the earlier ones. That is
deliberate: a record that quietly fixes itself can't be used to check
anything.

**The handoffs repeat the reviews on purpose.** Each review has a cover note
condensing it for the session that had to act on it. If you want the
findings, read the review; if you want what was actually asked for, read the
handoff.
