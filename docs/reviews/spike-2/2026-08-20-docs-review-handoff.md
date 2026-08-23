# Review handoff: spike-2 docs — for Mez

*From Ander, 2026-08-20. Cover note for `2026-08-20-docs-review.md`. Three
text edits with suggested wording included; no design calls, so this needs no
round trip. A fourth finding closed itself — your cold run settled it.*

## Verdict

**Merge-after-fixes.** Four accuracy findings, of which **three remain** — all
small text edits. Finding 4 is closed: your cold-run `cargo fmt --all --check`
on 1.95 came back clean, so the sentence stands as written and needs no edit.
Nothing structural.

Gate on rustc/cargo/clippy/rustfmt **1.97.0**: build clean, **58 passed, 0
failed**, clippy **zero warnings**, `cargo fmt --all --check` **zero diffs**.
Genuinely docs-only — three files, no code touched.

## The three text edits

**1. `ARCHITECTURE.md:83` (glossary) and `docs/EMBEDDING.md:33` — the ack
deadline lost a qualifier.** Both now say it bounds waiting for the receipt of
*"something you sent"*. The code arms an ack wait only for a turn carrying a
**request half**: `Turn::Close` returns `None` from `request()`, so a closing
turn arms no ack deadline at all. Decision 22's body still says it right, and
the pre-pass glossary did too — the simplification dropped it.

This is the one I would not skip. The glossary is where this document says the
words *are* the architecture, and `EMBEDDING.md` now contradicts itself sixty
lines apart: the Party example at line 98 comments `turn.request()?` with
`// Close → None: done`, which is exactly the distinction the new sentence
erases.

> for the delivery receipt of a **request you sent** (the *ack deadline*)

**2. `ARCHITECTURE.md:359` and `docs/EMBEDDING.md:31` — one construction site
missing.** Both list `Phone::converse` and `install`/`install_party`. There is
a third: `LocalRuntime::new` and `KameoRuntime::new` both take a mandatory
`spawn_deadlines`, as decision 22's own body says. `EMBEDDING.md` never shows
a runtime being constructed in its 167 lines, so an embedder meets this as a
compile error instead of as documentation — soft landing, but the section
exists to prevent the surprise, and the list reads as complete.

**3. `docs/EMBEDDING.md:157` — the third-runtime list omits decision 26.** You
asked me to confirm completeness against what `LocalRuntime` actually does. It
calls `fail_exchange` with four causes; two are absent from the list:

| Cause | `runtime.rs` | Listed? |
|---|---|---|
| `DeadlineElapsed` | 381 | yes (22–24) |
| `Disconnected` | 291 | yes (27) |
| `IllegalReply` | 514 | **no** |
| `SendFailed` | 527 | **no** |

A third runtime built from this list would not fail an exchange on a refused
send, nor on an illegal reply from its own party — reintroducing exactly the
silent wedge and the un-deadlined hang that findings 2 and 3 of the timeouts
review closed. Those two may be the *most* worth stating, because both are
invisible until something goes wrong.

> …the dual failure surface (party notification + observer events, decision
> 25), **positive evidence failing an exchange at once — a refused send or an
> illegal reply from your own party (decision 26)** — and the death fan-out
> (decision 27); the toy `LocalRuntime` remains the readable reference for all
> **four**.

## 4. The two-toolchain fmt claim — **RESOLVED by your cold run, no edit needed**

> **Closed.** Your fresh-clone run reported `cargo fmt --all --check` clean
> (exit 0) under rustc/cargo 1.95.0 with rustfmt 1.9.0-stable. That is the
> first genuinely-1.95 check this workspace has had, and it confirms the
> claim rather than contradicting it: a tree formatted by 1.97's rustfmt is
> also already-formatted by 1.95's. **WORKFLOW.md needs no change.**
>
> Scope worth keeping in view: verified agreement is **on this tree**, not in
> general — future code could still hit a construct where the two diverge.
> The fallback already in that section (a bump that breaks agreement defers
> to the pinned cloud version) is what covers that, and should stay.
>
> The original concern follows, kept for the record.

WORKFLOW.md's new "The gate" section records as standing fact that *"the two
toolchains' rustfmt outputs were verified to agree (1.95 and 1.97), so no
canonical formatter version is needed."*

Casey has since confirmed he ran the sweep himself from the local repo — so on
**1.97**. The sweep is therefore not evidence of agreement: it is one
toolchain's output, and **1.95's rustfmt has never formatted or checked this
tree**. My clean 1.97 result is that same toolchain a second time, so it adds
nothing either.

I had written that the 1.95 half rested on a run of yours. Nothing I saw
supported that — I should have asked whose run it was rather than assuming
there had been one. Apologies for putting words in your gate.

Why this is worth doing before the others: the same commit makes `cargo fmt
--all --check` a **standing gate item**. If the two rustfmts differ anywhere,
the gate fails for whichever of you did not last format the tree — and
reformatting to fix it flips the failure to the other side. That is a
1.97-versus-1.95 ping-pong that would present as an inexplicable
formatting-only diff, not as a version disagreement, and it would land on
whoever is least expecting it.

**One command settles it:** `cargo fmt --all --check` on your 1.95 sandbox
against the merged tree. Clean means the sentence was right all along and is
now genuinely verified. Any diff means the workspace *does* need a canonical
formatter version — which is precisely what the sentence says it does not.

Until then the honest wording is that the tree is formatted with 1.97 and 1.95
is unchecked. The fallback already in the section — a future bump that breaks
agreement defers to the pinned cloud version — is the right shape and should
stay regardless.

## Nits

- **`EMBEDDING.md:53`** — "`Timer` is not object-safe" is true; I checked
  rather than trusting my own earlier claim, and `Box<dyn Timer>` does fail.
  But on 1.97 the error reads *"the trait `Timer` is not dyn compatible"* —
  rustc renamed the concept. A parenthetical would save a search.
- **`EMBEDDING.md:72`** — "called under the runtime's lock" is true of
  `KameoRuntime`; `LocalRuntime` has no lock (its `emit` takes `&mut self`,
  and the borrow checker forbids re-entry anyway). The `Observer` trait doc
  hedges correctly with "runtimes *may*"; EMBEDDING states it flatly. The
  operative rule is right for both, so this is presentation only.
- **`EMBEDDING.md:79`** — `death_watch()` lives on the net, not the
  `Transport` handle; "from your network" would match the call shown.

## The rest held up

The Q1 pass is otherwise simplified-but-unchanged; I checked it
phrase-by-phrase against the decision bodies rather than reading for tone.
*"no way to say 'wait forever'"* maps exactly onto "no `Default`, no
`Option`, and no unbounded variant", and *"the other side never sees your
limits"* onto "bound its own waiting only and never cross the wire".

One change I want to call out as a genuine improvement rather than neutral:
the glossary now says an elapsed deadline fails the exchange *"never the
conversation"* where it used to say *"not the conversation"*. Decision 23's
own wording is "terminal for the *exchange*, never the conversation" — so the
language pass moved the glossary **closer** to the decision while simplifying
it. That is the thing going right.

The spike-numbering wobble from the kickoff review is settled: naming both
numbers and the reason they differ, in one clause, is exactly enough for a
cold reader.

And the section I would keep as-is under any edit: *"while your party is
composing its answer, it is being judged against the peer's clock, which you
cannot see"*. That is the most useful new sentence in the document for anyone
about to write a slow party, and it is not derivable from the API.

## Standing items

**EMBEDDING.md is no longer a gap.** With these three edits, follow-up errand
4 is closed — deadlines, `on_exchange_failed`, the observer, `DeathWatch` and
the `Timer` note are all there and all checkable. That leaves one:

**Workshop-2's fork still has no `vendor-snapshot`.** Every green result
across all four reviews — including this gate run — is from my 1.97.0 macOS
host, and none has been confirmed by an offline cloud build. Follow-up errand
1, and now the only one left standing.

## Bounds kept

No redesign, no dependencies, `vendor/` and `vendor-snapshot` untouched, no
commits to `cowork/*` or `master`, nothing pushed. Working tree clean; no
`code/*` branch this time, since all three findings are text.
