# Merge-ready: allowlist mutation, round 2 (5f05ee3f) — for Mez

*From Ander, 2026-08-23. One-commit check on
`cowork/allowlist-mutation-r2`, closing the review in
`2026-08-23-allowlist-mutation-review.md` and its handoff. Short, because
there is nothing left to ask for.*

## Verdict

**Merge it.** All three review items closed, nothing else moved, gate green.

| | Round 1 (ac52e5ca) | **Round 2 (5f05ee3f)** |
|---|---|---|
| Tests, rustc 1.97 / crates.io | 68 / 0 | **69 / 0** |
| Clippy `--all-targets -D warnings` | zero | **zero** |
| Fmt | clean | **clean** |

One commit, `ac52e5ca` a clean ancestor, so the ref carries the whole story.
No manifest touched in either round — the `vendor-snapshot` shelf's
`Cargo.lock` blob is still identical to the branch's, so your offline 1.95
gate needs no refresh and remains genuine independent confirmation.

## The three confirmations

**1. The new test really does catch the regression.** I re-applied the same
`take(1)` weakening to both sweeps that had passed 68/68 before:
`one_revocation_fails_every_conversation_at_that_endpoint` now fails at
`assert_eq!(failed, 2)` — `left: 1, right: 2`. So the gap is genuinely
closed, not merely described.

It also asserts both party notifications carry `Revoked` and that exactly
two `ExchangeFailed` events land, so a partial regression cannot slip past
on the count alone. That is more than I asked for and it is the right more.

**2. The stranger send is genuine deny-by-default.** The actor's list only
ever held `aa`, which the test then revokes; the stranger sits at `ee`,
never admitted anywhere in the test. So it exercises R3's *other* half
rather than re-testing the revoked path under a new name — which was the
whole point of the finding. The exact ordered two-element slice assertion
also pins that nothing extra was emitted.

**3. The diff contains nothing beyond the asks.** Four files. The two source
files are **reflow-only**: I compared the prose with comment markers
stripped and it is byte-identical, so no wording changed under cover of
rewrapping — the one thing worth checking when a cosmetic commit touches
`crates/`. No runtime logic file is touched at all. The feature file gains
only the binding note, and it states the Kameo mailbox-ordering caveat
correctly. The test file gains only the extended
`revoking_the_last_guest_restores_deny_by_default` and the new test.

## Closing the loop on the pre-flight experiment

Story 3 is now the second story where a hazards note went in before the
build and the review came back with no structural findings — and this time
the only finding at all was a coverage gap, not a defect. Both of the
items I had carried across every previous review (workshop-2's missing
shelf, EMBEDDING's spike-2 backlog) are also closed as of this branch.

Worth naming what actually did the work here, since it is repeatable: the
findings that mattered came from *mutation* rather than reading — weakening
the code to see whether the suite noticed. Reading confirmed the filter was
present; only deleting it proved the tests would catch its absence. If
there is a story 4, I would keep the same three-step shape — hazards note
before the build, review after, and mutation on anything the suite claims
to pin.

## Bounds kept

No redesign, no dependencies, no commits, nothing pushed, `vendor/` and
`vendor-snapshot` untouched. I temporarily re-applied the `take(1)`
mutation to two source files to prove confirmation 1; both were restored
from backup, and the worktree sits on `cowork/allowlist-mutation-r2` at
5f05ee3f with an empty `git status` and a green 69/69 re-run.
