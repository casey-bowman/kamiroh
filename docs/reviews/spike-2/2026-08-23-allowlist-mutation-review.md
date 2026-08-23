# Review: cowork/allowlist-mutation (ac52e5ca)

*From Ander, 2026-08-23. Roadmap 3 / decision 28, reviewed against the
2026-08-23 mapping board and my pre-flight hazards note of the same day.
Branch verified as 3 commits on master a4184e80, `master` a clean ancestor.
Nothing pushed; the worktree is back at ac52e5ca with an empty `git status`.*

## Verdict

**No structural findings — nothing to stop for.** Every hazard from the
pre-flight note is closed, and closed the way the note asked rather than
worked around. Two of the closures I verified by mutation testing rather
than by reading, and one of them turned up the only finding here worth
acting on before merge.

Findings: **one coverage gap I'd fix (F1)**, three small ones (F2–F4), and
four notes that need no action. Nothing blocks the merge; F1 is cheap and
would be much more annoying to add later.

## 1. The gate, on 1.97 against crates.io

Reproduces the sandbox gate exactly.

| | Sandbox (1.95, offline) | Here (1.97, crates.io) |
|---|---|---|
| Tests | 68 passed / 0 failed | **68 passed / 0 failed** |
| Clippy | zero | **zero** (`--workspace --all-targets -D warnings`) |
| Fmt | clean | **clean** |

No warnings of any kind, including from the newer toolchain — the 1.95→1.97
gap produced nothing, as it hasn't all spike. The iroh tests ran for real
(4 passed over actual QUIC), so this is the full suite, not a subset.

One process note on my own method, since it nearly bit me: my first run
piped `cargo test` through `tail`, which truncated the per-binary results
and made 68 look like 12. The number above is from a full capture. Worth
remembering the next time a gate is run through a pipe — same class of trap
as the `PIPESTATUS` one from the bayworks errand.

## 2. Hazard-by-hazard verification

### 2a — roster lock across an await: **closed, verified exhaustively**

I checked every `lock()` site in the Kameo adapter, not just the new ones:
lines 142, 159, 199, 207, 255, 275, 327. The only new one is 275, inside
`fn actor_ref(&self, name)` — a **sync** function that clones the `ActorRef`
out and returns it, so the guard is dead before any caller can `.await`.
`admit`/`revoke` call it, get an owned handle, and only then await the ask.
This is exactly the `watch_deaths` pattern I pointed at.

Worth saying explicitly: this is not proven by the compiler. The `Send`
check only bites on a spawned future, and these are awaited directly in
tests — so the guarantee is structural (the guard cannot outlive the sync
fn), which is the stronger form anyway.

### 2b — non-reentrancy sites: **all three updated, one improved**

- `lib.rs:137` (`set_observer` doc) — lists `admit`/`revoke`, and names the
  auto-ban-on-`DeliveryDenied` case specifically.
- `lib.rs:151` (`emit` comment) — updated.
- `docs/EMBEDDING.md:80-84` — updated, and **better than I asked for**: the
  prose previously said only "must not call back into the runtime", and now
  enumerates the five methods and calls out the auto-ban trap by name.

### hazard 3 — the epoch bump: **applied at the seam, as recommended**

`fail_exchange` now bumps `waits.epoch` alongside `settle_all()`, with a
comment recording that the protection was emergent and is now local.
Correctly scoped inside the `if let Some(waits)` — no conversation, nothing
to protect. See N3 for the one thing to know about it.

### hazard 4 — the live-state pre-filter: **closed, and genuinely pinned**

Both sweeps filter on `AwaitingTheirTurn | OweThem` before failing anything.
I didn't take the tests' word for it — I deleted the filter from both
runtimes and re-ran:

- `allowlist_mutation.rs::a_second_revocation_emits_nothing` → **FAILED**
- `kameo_allowlist.rs::a_hosted_actors_guest_list_changes_while_it_runs` →
  **FAILED** (`left: 1, right: 0` at line 156)

So the event-count assertions really do pin the mechanism on both runtimes,
which is what I most wanted to confirm. Restored and re-verified green.

### hazard 1 — the Phone gap: **closed**

`Phone::on_revoked` mirrors `on_peer_death` line for line, including the
`!self.state.fail()` guard that makes it self-idempotent.
`a_phone_learns_of_revocation` pins all three cases: the live exchange
fails with `Revoked`, a different endpoint returns `None`, and a second call
on the same endpoint returns `None`. The eviction recipe names the app-side
list explicitly.

### hazard 5 — do-not-reopen: **closed in all three places it belongs**

The `FailureCause::Revoked` doc, `Party::on_exchange_failed`'s doc (as an
explicit exception to its own "perhaps reopen"), and EMBEDDING's failure
section, which spells out the livelock — one deadline burned per round.

### hazard 6 — the prose table, row by row

All eleven, checked individually:

| Site | State |
|---|---|
| `deadline.rs` enum | ✅ `Revoked` added, doc contrasts it with `Disconnected` |
| `deadline.rs` `Display` | ✅ the one compiler-caught site |
| `protocol.rs:86` | ✅ |
| `protocol.rs:102` | ✅ |
| `protocol.rs:150` | ✅ |
| `kamiroh-ports/src/lib.rs:80` | ✅ + the do-not-reopen exception |
| `EMBEDDING.md:69` | ✅ four-item list → five |
| `EMBEDDING.md` "all four" | ✅ → **"all five"**, with the new obligation spelled out including the revoke-twice-emits-once property |
| `ARCHITECTURE.md:387` | ✅ |
| `features/README.md` | ✅ new table, 8 rows + 2 extra pins in prose |
| `ROADMAP.md` item 3 | ✅ |

And the two I said to leave alone were left alone: **decision 26's "Two
paths" wording is untouched** (zero diff hits), and the `Event` enum gained
nothing. Both correct per Q3=b and the decision-27 precedent.

### hazard 7 — the eviction recipe: **all three sharp edges present**

Revoke-everywhere-first with the reason (redial into unrevoked actors),
`close_connection` flagged as `IrohNet`-only and wire-visible, and the
app-side allowlist named. Both honest limits are there too — the in-flight
handler still speaking, and no fleet-wide atomicity.

## 3. Findings

### F1 — the multi-conversation sweep is unpinned *(fix before merge; cheap)*

One endpoint can host several actor names, so one revoke may have to fail
several conversations. Both implementations do this correctly — they
`collect()` every match and loop.

**But no test would notice if they stopped.** Every test in the suite has at
most one conversation per endpoint, so `affected.len()` is only ever 0 or 1.
I proved it: I weakened both sweeps to `affected.iter().take(1)` with the
count clamped to `min(1)`, and the whole suite still passed **68/68**.

This is worth closing rather than noting, for two reasons. There is a
first-match-only shape *already in the file next door* — `sweep_dead`
narrows to a single endpoint and could easily be pattern-matched by a future
editor. And the count is now a public return value that callers may act on,
so a silent regression to 1 is a wrong answer, not just a missed failure.

One test: two app names at the same endpoint, both mid-exchange with the
same actor, revoke once, assert the count is 2 and that two `ExchangeFailed`
events land. It fits the existing `stage`/`open_live_exchange` helpers
almost unchanged.

### F2 — "restores silence" is tested against one endpoint, not "anyone"

The scenario says *"Then the actor receives nothing from anyone."* The test
(`revoking_the_last_guest_restores_deny_by_default`) only sends from the
endpoint it just revoked, so what it actually pins is "nothing from the
revoked endpoint" — R3's *deny-by-default* half is unexercised. One extra
send from a never-admitted endpoint, asserting a second `DeliveryDenied`,
makes the test match its scenario.

### F3 — one scenario is stronger than Kameo's guarantee (a cucumber trap)

*"Revocation bites on the very next delivery"* is literally true of
`LocalRuntime`, where a queued delivery is judged against the list at the
next `step`. It is **not** literally true of `KameoRuntime`, where a
delivery already in the host's mailbox ahead of the `Revoke` is processed
under the old admission — the honest limit the `revoke` doc-comment
correctly records.

Both current tests avoid it by sending after the revoke resolves, so nothing
is wrong today. But this feature file is destined for cucumber-rs, and a
step definition written from the scenario text alone ("send, then revoke,
then assert denial") will pass on the toy runtime and flake on Kameo. Worth
a note in the feature file now, while the reason is fresh, so the binding
errand doesn't rediscover it as a flake.

### F4 — `ask` failures all report `UnknownActor`

`admit`/`revoke` map every `SendError` to `RuntimeError::UnknownActor`, so a
host that stopped between the roster lookup and the ask reports "no such
actor in this runtime". Defensible — by the time you hear it, it's true —
and consistent with `stop`. Noting it only because the roster lookup already
returns that same error for a genuinely absent name, so the two causes are
now indistinguishable to a caller. Fine for the spike.

## 4. What was authorized beyond the board

Asked directly. Four things, none of them a problem, one worth recording:

- **`Phone::on_revoked`** — you ruled it in scope, so this is authorized, not
  self-authorized. Noting it only because decision 28 records it as folded in
  at build time, which is the right place for it.
- **ROADMAP items 1, 2 and 4 annotated as delivered.** The board ruled on
  item 3 only. I checked the other three claims rather than assuming: item 1
  → decisions 22–24, item 2 → decision 27, item 4 → decision 25's
  `Event::DeliveryDenied`. All three are accurate. Item 4 is the loosest fit
  (it asked for "a tracing hook or counter" and got an observer event), and
  the annotation says as much. Tidy-up beyond the errand, but true tidy-up.
- **The epoch bump changes a shared seam.** It was my recommendation, but it
  affects the `DeadlineElapsed`, `SendFailed`, `IllegalReply` and
  `Disconnected` paths too, not only `Revoked`. Verified benign —
  `stale_turn_deadline.rs` and the disconnect suites all still pass — but it
  is a change to spike-2 machinery that story 3 didn't strictly need.
- **Returning a count, `ask`-based methods, the `*_here` split.** All from
  my note's "smaller things", none board-ruled, all implementation shape.

## 5. Notes, no action needed

- **N1 — the 2c protection is unexercised.** `admit_here`/`revoke_here` exist
  so a future internal caller can mutate without asking its own mailbox, and
  nothing calls them yet except the message handlers. That is exactly the
  intent (scaffolding for workshop-3's wire form), so it is correct that no
  test covers it — just don't expect the split to be *validated* until the
  wire form lands.
- **N2 — the two runtimes differ, and the docs are honest about it.** Each
  `revoke` doc-comment describes its own runtime's real guarantee rather than
  a shared fiction. This is the divergence I flagged in the note's "smaller
  things", handled better than by pretending it away. See F3 for the one
  place the prose hasn't caught up.
- **N3 — the epoch bump is unpinned, unavoidably.** No test can fail on it,
  because by construction it only matters on a path that doesn't exist yet
  (a future fail that skips `settle_all`). Insurance, as intended.
- **N4 — cross-feature ordering is untested.** Death-then-revoke and
  revoke-then-death on the same endpoint have no test. Low risk: both go
  through the same live-state filter, which F1's sibling assertions now pin
  on both runtimes. Not worth a test of its own.
- **N5 — cosmetic.** Three doc comments have ragged reflow after the
  insertion — `protocol.rs:103` leaves "Terminal for" alone on a short line,
  and `protocol.rs:86` and `kamiroh-ports/src/lib.rs:81` are similar.
  `cargo fmt` doesn't reflow prose, so this needs a human pass or nothing.

## 6. Things I checked that were simply right

Recording these so they don't get re-reviewed:

- **The archived board is byte-identical to the board you ruled on**
  (`diff` clean against the copy next to the repo). No silent drift between
  what was ruled and what was filed.
- **Decision 28 matches the rulings** — Q1=c, Q2=a, Q3=b, Q4 retired, R1–R5,
  the no-auto-close addendum with both of its reasons, and the eviction
  recipe pointer. It also attributes the folded-in hazard findings and
  records the `FailureCause` breaking-change reasoning.
- **`Stepped` gained no variant**, so no existing test expectation moved —
  the count-returning shape kept that surface still.
- **The feature file's 8 scenarios map 1:1 to the README table**, with the
  Phone and Kameo pins listed separately in prose. No orphan scenario, no
  orphan test.
- **`a_readmitted_endpoint_opens_a_fresh_exchange` really does pin
  `Failed → Open`** at the runtime: the ack it asserts is only sent after
  `on_incoming` succeeds, so an illegal-turn regression would drop the ack
  and hang the loop rather than passing quietly.

---

F1 is the only one I'd hold the merge for, and only because it is a ten-minute
test that gets harder to justify writing once the branch has landed. F2 and
F3 are worth folding into the same commit while the file is open. Happy to
re-run the gate on whatever comes back.
