# Merge-ready: fan-out, round 2 (c0e36604) — for Mez

*From Ander, 2026-08-23. One-commit check on `cowork/fanout-r2`, closing the
review in `2026-08-23-fanout-review.md` and its handoff. Short, because
nothing is left open.*

## Verdict

**Merge it.** Both asks closed, Casey's ALPN ruling carried everywhere it is
named, nothing else moved.

| | Round 1 (232158aa) | **Round 2 (c0e36604)** |
|---|---|---|
| Tests, rustc 1.97 / crates.io | 75 / 0 | **76 / 0** |
| Clippy `--all-targets -D warnings` | zero | **zero** |
| Fmt | clean | **clean** |
| iroh over real sockets | 4 tests | **4 tests, on the new ALPN** |

One commit, `232158aa` a clean ancestor, no manifest touched in either round
— the `vendor-snapshot` shelf stays current, so the offline 1.95 gate remains
genuine independent confirmation rather than my host's word.

## The four confirmations

**1. The new test catches the regression.** I re-applied the same
swallowed-refusal weakening that passed 75/75 last round —
`let _ = sender.open_many(...)` — and `a_refused_batch_creates_no_phones`
fails at `tests/deadlines.rs:569`. It is the only failure, which is the right
scope.

Worth noting the claim ended up doubly protected: on `Err` the
`Result<Vec<Self>, PhoneError>` carries no phones at all, so "no phones
created" is structural in the signature. What the test pins is the half that
*isn't* structural — that the send is attempted and its error propagates
rather than being swallowed. That is exactly the refactor the doc comment
warns about, and now it cannot sneak past.

**2. The three ack-promise statements agree, each in register for its site.**
Decision 4 takes a parenthetical, the glossary a qualified-exception clause,
and `vocabulary.rs` a full paragraph placed before the decision-4 reference
so a reader meets the exception and then the pointer. Same claim, three
lengths, no copy-paste.

`deadline.rs` left alone, as preferred — what the ack *promises* changed, not
what the deadline *bounds*, so `DeadlineKind::Ack` was accurate before and
still is.

**3. The ALPN bump is complete, and each site carries the reason.** Three
places — the const, the module doc, ARCHITECTURE decision 19 — and **zero
`kamiroh/0` remaining anywhere** in code or docs. The historical record in
`advisories/`, `briefs/` and `mappings/` correctly keeps the old value, which
is what an archive is for.

All four iroh tests pass over real sockets on the new ALPN, including
`a_conversation_spans_connections` and `a_shutdown_peer_is_reported_dead` —
so the reconnect and death paths were re-proven on the bumped protocol, not
just the fan-out one.

The three sites each say *why* rather than only what: an incompatible wire
change bumps the number so mismatched builds refuse each other at the
handshake instead of silently dropping frames. That was the whole point of
raising it, and it survived into the code rather than staying in a review.

**4. Nothing beyond the asks.** Five files. The *only* non-comment,
non-test line changed anywhere in `crates/` is the ALPN constant itself —
everything else is doc prose or the new test. The ROADMAP wrap is fixed
(99 → 70 characters); its continuation line is now short at 31, which is
cosmetic-of-a-cosmetic and not worth another round.

## Closing story 4

This one is worth recording, because it is the first time the loop ran with
nothing left over. Both mutation targets I named *before* the build held up
under attack afterwards; both hazards Casey ruled on after the note landed as
recommended; and the two findings that did surface were a missing test and a
missing sentence, not a defect.

Three of the four things I would have called wins were yours unprompted: the
memory partition case (receipt into `held`, better than the fix I proposed),
the third-runtime list held at five with the reasoning in prose, and
`BATCH_RECEIPT_NAME`'s doc arguing its own safety where an implementer will
meet it.

The pattern that keeps earning its place: hazards note before the build,
review after, and mutation on anything the suite claims to pin. Four stories
in, the reviews are getting shorter because the notes are getting used.

## Bounds kept

No redesign, no dependencies, no commits, nothing pushed, `vendor/` and
`vendor-snapshot` untouched. I re-applied one temporary mutation to
`phone.rs` to produce confirmation 1; it was restored from backup, and the
worktree sits on `cowork/fanout-r2` at c0e36604 with an empty `git status`
and a green 76/76 re-run.
