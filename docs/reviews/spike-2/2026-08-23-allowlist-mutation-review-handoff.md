# Review handoff: allowlist mutation (story 3) — for Mez

*From Ander, 2026-08-23. Cover note for
`2026-08-23-allowlist-mutation-review.md`, which holds the detail and the
evidence. Short, because there is one ask and it is small.*

## Verdict

**Merge-after-one-test.** No structural findings, no defects, nothing to
unpick. One coverage gap I would close before the branch lands — a test, not
a fix; the code it would cover is already correct.

Your numbers are exactly right: **68 passed / 0 failed, clippy zero, fmt
clean**, reproduced here on rustc 1.97.0 against crates.io. No dependency
added, no manifest touched, and the iroh tests ran against real sockets. I
checked assertions rather than trusting green.

This is the smoothest branch of the four. Every hazard in the pre-flight
note came back closed, and closed the way the note asked rather than routed
around — which is the second time that has now paid for itself.

## The one ask

**Add one test: two conversations at the same endpoint.**

One endpoint can host several actor names, so a single revoke may have to
fail several conversations. Both runtimes do this correctly — they collect
every match and loop.

**But nothing would notice if they stopped.** Every test in the suite has at
most one conversation per endpoint, so `affected.len()` is only ever 0 or 1.
I checked this by weakening both sweeps to `affected.iter().take(1)` with the
count clamped to `min(1)`: the whole suite still passed 68/68.

Two reasons I would rather not leave it:

1. **There is a first-match-only shape already in the file next door.**
   `sweep_dead` deliberately narrows to a single endpoint. A future editor
   pattern-matching on the neighbour would land exactly the regression the
   suite cannot see.
2. **The count is now a public return value.** A silent regression to 1 is a
   wrong answer to a caller, not merely a missed failure.

The test: two app names at one endpoint, both mid-exchange with the same
actor, revoke once, assert the count is 2 and that two `ExchangeFailed`
events land. It fits your existing `stage` / `open_live_exchange` helpers
almost unchanged — that harness is why this is a ten-minute job now and an
annoying one after the branch has landed.

Two smaller things worth folding into the same commit while the files are
open (both detailed in the review):

- **"Restores silence" tests one endpoint, not "anyone."** The scenario
  promises the actor receives nothing from *anyone*; the test only sends from
  the endpoint it just revoked, so R3's deny-by-default half is unexercised.
  One extra send from a never-admitted endpoint fixes it.
- **One scenario is stronger than Kameo's real guarantee.** "Revocation bites
  on the very next delivery" is literally true of `LocalRuntime` and not of
  `KameoRuntime`, where a delivery already queued ahead of the `Revoke` is
  processed under the old admission. Your `revoke` doc-comment records this
  honestly and both tests avoid it by sending after the revoke resolves — so
  nothing is wrong today. But this file is bound for cucumber-rs, and a step
  written from the scenario text alone will pass on the toy runtime and flake
  on Kameo. A note in the feature file now saves the binding errand a flake
  hunt later.

## The hazard note came back clean, and I verified rather than trusted

All seven dispositions checked individually. Two I checked by mutation
instead of by reading, because reading cannot distinguish "pinned" from
"happens to pass":

- **Hazard 4 (the live-state pre-filter) is genuinely pinned.** I deleted the
  filter from both runtimes; both revoke-twice tests failed —
  `a_second_revocation_emits_nothing` and the Kameo one at line 156. The
  event-count assertions really do hold the mechanism, on both runtimes. This
  was the disposition I most wanted to confirm, because idempotence at the
  observer is invisible until something counts events.
- **Hazard 2a (roster lock across an await) is structurally closed.** I
  checked every `lock()` site in the adapter, not just the new ones. The
  guard lives inside a sync `actor_ref()` that returns an owned handle, so it
  cannot outlive the function — stronger than relying on the `Send` check,
  which would only bite on a spawned future and these are awaited directly.

The rest: 2b updated in all three places (and EMBEDDING now *enumerates* the
five methods where it previously said only "don't call back into the
runtime" — better than I asked for); hazard 3's epoch bump applied at the
seam with the reasoning recorded; hazard 1's `Phone::on_revoked` mirroring
`on_peer_death` including the self-idempotence guard; hazard 5's
do-not-reopen in all three places it belongs; hazard 7's three sharp edges
plus both honest limits; and **all eleven prose sites** from the hazard-6
table, checked row by row.

Two negative results worth as much as the positive ones: **decision 26's
"Two paths" wording is untouched** and the **`Event` enum gained nothing** —
both correct, per Q3=b and the decision-27 precedent. And the **archived
board is byte-identical** to the one Casey ruled on, so there is no drift
between what was ruled and what was filed.

That is now two stories running where a pre-flight pass cost an hour and the
review found nothing structural. I would keep doing them.

## Notes, no action

- **The 2c split is unexercised, correctly.** `admit_here` / `revoke_here`
  exist so workshop-3's harness handler can mutate without asking its own
  mailbox. Nothing calls them yet but the message handlers, which is the
  intent — just don't expect the split to be *validated* until the wire form
  lands.
- **The epoch bump touches a shared seam.** It was my recommendation, but it
  affects `DeadlineElapsed`, `SendFailed`, `IllegalReply` and `Disconnected`
  too, not only `Revoked`. Verified benign — `stale_turn_deadline` and both
  disconnect suites still pass — and unpinnable by construction, since it
  only matters on a path that does not exist yet.
- **ROADMAP items 1, 2 and 4** were annotated delivered beyond what the board
  ruled. I checked all three claims rather than assuming: item 1 →
  decisions 22–24, item 2 → decision 27, item 4 → decision 25. All accurate;
  item 4 is the loosest fit and says so.
- **Cosmetic:** three doc comments have ragged reflow after the insertion
  (`protocol.rs:103` strands "Terminal for" on a short line, and
  `protocol.rs:86` and `kamiroh-ports/src/lib.rs:81` are similar).
  `cargo fmt` does not reflow prose, so this needs a human pass or nothing.

## Both items I carried forward are now closed

Retiring these from the running list, having checked rather than assumed:

1. **Workshop-2's `vendor-snapshot` exists and is current.** The shelf's
   `Cargo.lock` blob is *identical* to this branch's, and the branch adds no
   dependency and touches no manifest — so it needs no refresh, and your
   offline 1.95 gate is genuine confirmation rather than my host's word.
   That closes the gap I had flagged across all four previous reviews.
2. **EMBEDDING.md has caught up.** "Hearing about disconnects" landed, and
   this branch adds "Changing your mind about a peer". An embedder following
   it today gets the whole of spike 2 rather than a third of it.

## Bounds kept

No redesign, no dependencies, no commits, nothing pushed, `vendor/` and
`vendor-snapshot` untouched. I did temporarily mutate two source files to
prove the two coverage claims above; both were restored from backup and the
worktree is back at `ac52e5ca` with an empty `git status` and a green
68/68 re-run.
