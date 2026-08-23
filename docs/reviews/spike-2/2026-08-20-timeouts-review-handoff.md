# Review handoff: timeouts (story 1) — for Mez

*From Ander, 2026-08-20. Companion to the review itself; this is the cover
note — what I found, what I need back, and what I deliberately did not touch.
Self-contained, but the detail all lives in the review.*

## Where the review is

**`██████████████████████████/tiers/2-workshop/2026-08-20-timeouts-review.md`** (local path redacted for publication)

Note the path: that is the *tiers* folder, one level **above** the clone —
outside `tiers/2-workshop/kamiroh`. Your capability brief describes the device
bridge as reading and writing "the local clone", so if the bridge is scoped to
the repo directory you may need to reach it as `../2026-08-20-timeouts-review.md`
from a shell in the clone, or ask Casey to move it inside. Say the word and I
will drop a copy under `docs/briefs/` on a `code/*` branch instead — I left it
where the errand specified rather than guessing.

## The short version

| Branch | Verdict |
|---|---|
| `cowork/spike-2-kickoff` (`b268a80`) | **merge-ready** |
| `cowork/mapping-archive` (`35f5902`) | **merge-ready** |
| `cowork/timeouts` (`7ee1890`) | **STOP** — one demonstrated race |

Your build claims all check out on my newer toolchain (rustc 1.97.0):
**45 passed, 0 failed**, `Cargo.lock` and every `Cargo.toml` untouched,
`cargo fetch --locked` clean against crates.io, and clippy **zero warnings** —
1.97's clippy found nothing 1.95's missed. Decisions 22–25 are sound and
implemented as ruled; I am not contesting any decision.

Two findings matter. Both are liveness holes of the same family — a wait that
should exist and doesn't, or one that fires when it shouldn't — and both have
reproducers.

**Finding 1 (the STOP), `kamiroh-adapter-kameo/src/lib.rs:477`.** You asked me
to convince myself the pending-wait re-check covers the abort/stale-tell
races. It covers **ack** deadlines and does not cover **turn** deadlines: the
ack arm carries a `RequestId` to discriminate on, the turn arm carries nothing,
and `waits.turn.is_some()` goes true again the moment a *new* turn wait is
armed. Since `abort()` cannot unsend an already-queued message, a superseded
task's report is accepted as current and fails an exchange the host has just
accepted a turn on and already answered. Reproducer plus a verified ~12-line
epoch fix are on my branch; with it, 46 passed / 0 failed, clippy still clean.

**Finding 2, `kamiroh-app/src/phone.rs:95`.** `send_turn` advances the turn
machine before the transport send but arms the waits after it, so a failed send
leaves the Phone unable to time out (`next_due()` is `None`, so
`on_deadline_elapsed()` declines) *and* unable to reopen (`NotOurMove`) —
wedged silently, which is the state decision 22 calls unrepresentable. Your
"For review" note rules this shape acceptable because the exchange "will fail
by the peer's deadlines, or ours on the next legal send attempt"; for the Phone
there is no next legal send, because it is the peer's move by then. I did
**not** fix this one — see the first ask below.

Findings 3–7 (a party's illegal reply silently un-deadlining an exchange;
timer tasks outliving stopped actors; the observer called under lock; a
`TestTimer` waker nit; one feature-file wording defect) are described in full
in the review and are small enough to take from the descriptions.

## What I need back

1. **A ruling on finding 2 — this is the one thing genuinely blocking me.**
   There are three send sites and two behaviours: `LocalRuntime:389` arms the
   waits *before* sending (so a failed send still fails loudly later), while
   `Phone:95` and the Kameo responder at `lib.rs:400` arm after. Converging on
   `LocalRuntime`'s ordering is my instinct, but the alternative — fail the
   exchange outright on a send error — is defensible and reads more like
   decision 22's "failure is a local judgment". Either way it is a design call
   under settled decisions, so it is yours and Casey's, not mine. I left the
   reproducer and no fix.

2. **A ruling, or at least a sentence in the decision log, on
   processing-time vs arrival-time.** The Kameo deadline clock measures when a
   turn is *processed*, not when it *arrives*: a turn that lands well inside its
   deadline but queues behind a slow party is judged late. Because one party
   object is shared across all of an actor's conversations, one slow peer can
   time out every other peer on that actor. My epoch fix removes the incorrect
   kill; it does not change this, and I do not think it should be changed
   silently either way. It may be perfectly acceptable for the spike — but an
   embedder running unattended container tests will meet it, which is precisely
   the audience decision 22 was ruled for.

3. **Who lands the epoch fix.** I cannot commit to `cowork/*` and did not.
   Either cherry-pick it onto `cowork/timeouts` yourself, or tell Casey to merge
   my branch — your call. It is one commit and the diff is quoted in full in the
   review, so you can also just retype it.

4. **Whether you want findings 3–7 from me.** All are unfixed. Say so and I
   will do them on a `code/*` branch; otherwise they are yours, and two of them
   fold naturally into errands you have already named (below).

## Reaching my work — and one obstacle

Branch **`code/timeouts-review-fixes`**, one commit `c4596cf` on top of
`cowork/timeouts`, in Casey's clone at `tiers/2-workshop/kamiroh`. It holds:

- `crates/kamiroh-adapter-kameo/tests/stale_turn_deadline.rs` — reproducer, finding 1
- `crates/kamiroh-adapter-kameo/src/lib.rs` — the epoch fix, finding 1
- `tests/phone_send_failure.rs` — reproducer, finding 2 (no fix)

**It is local only. I did not push, and nothing was committed to `cowork/*` or
`master`.** Pushing stays Casey's act.

The obstacle: **workshop-2's fork still has no `vendor-snapshot` branch** — I
confirmed `origin` carries only `master` — so you cannot do an offline cloud
build of my branch to check the fix yourself. That is the same gap your
follow-up errand 1 already names. Until it is filled, my "46 passed, clippy
clean" is a claim you are taking on my word from a 1.97.0 macOS host; if you
would rather not, hold the fix until the vendor-snapshot errand lands and
verify it then.

## Touching errands you already named

- **Errand 1 (cucumber-rs).** Finding 7 is a wording defect in
  `hung-exchanges-fail-loudly.feature`: the "A peer that never answers" scenario
  claims *"the conversation is idle and may open a new exchange"*, but after a
  timeout the state is `Failed`, not `Idle` — that is the point of decision 23.
  The second half is right. Worth fixing *before* these scenarios become step
  definitions, since a step would have to falsify it. Suggest simply dropping
  "is idle and".
- **Errand 3 (language pass).** Findings 6 and 7 are both wording, and would
  ride along naturally.
- **Errand 4 (EMBEDDING.md).** Add one line beyond the deadlines/observer gap
  you already listed: because `Timer::sleep_until` returns `impl Future` in
  trait position, `Timer` is **not object-safe** — `Box<dyn Timer>` is
  impossible and embedders must stay generic over `W: Timer`. That is the
  correct trade for decision 15's `Send` requirement, but it will surprise
  someone.

## Bounds I kept

No redesign, no dependencies, `vendor/` and `vendor-snapshot` untouched, no
commits to `cowork/*` or `master`, nothing pushed anywhere. I reviewed the
implementation against decisions 22–25 and did not relitigate the decisions.

Two things I verified rather than assumed, since both looked like problems and
turned out not to be: your third-party `Party` default method compiles clean on
1.97 (I wrote an outside implementor that relies on the defaulted
`on_exchange_failed` and boxed it as `DynParty`), and `cowork/timeouts` and
`cowork/mapping-archive` — which are siblings, both stacked on
`cowork/spike-2-kickoff`, both editing the same README paragraph — merge
cleanly; I checked with a read-only `git merge-tree` rather than reporting a
collision I hadn't confirmed.

I did not re-verify your cloud-side measurements (the crates.io 403s, the
offline 39-test run, the ~4-minute cold build). They are observations from an
environment I cannot reach, and nothing I can see contradicts them.
