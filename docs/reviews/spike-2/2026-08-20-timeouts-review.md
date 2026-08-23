# Review: `cowork/timeouts`, `cowork/spike-2-kickoff`, `cowork/mapping-archive`

*From Ander, 2026-08-20. Independent review at Mez's request. Repo:
`kamiroh-workshop-2` fork at `tiers/2-workshop/kamiroh`, `master` = `b960303`.
Self-contained: readable without the codebase.*

---

## Verdicts

| Branch | Verdict |
|---|---|
| `cowork/spike-2-kickoff` (`b268a80`, docs) | **merge-ready** |
| `cowork/mapping-archive` (`35f5902`, docs) | **merge-ready** |
| `cowork/timeouts` (`7ee1890`) | **STOP** — one demonstrated race (finding 1) |

`cowork/timeouts` is close, and everything Mez claims about it is true: the
suite is green, clippy is clean, the decisions are implemented as ruled. The
STOP is one concrete defect, in exactly the place the completion brief flagged
as riskiest, plus a second liveness hole of the same family found nearby. Both
have reproducers; one has a verified fix. Neither is a design problem —
decisions 22–25 are sound and implementable as ruled. **No decision is being
questioned.**

---

## Build, test, clippy

Toolchain: **rustc 1.97.0 / cargo 1.97.0 / clippy 0.1.97** (2026-07-07),
macOS (darwin 25.6.0). Mez's cloud sandbox is 1.95.0; this is the newer
toolchain the errand asked to test against.

Run on `cowork/timeouts`, against crates.io, exactly as specified:

```
cargo fetch --locked          # OK — resolved unchanged, no network surprises
cargo build --workspace       # clean, no warnings
cargo test  --workspace       # 45 passed, 0 failed
cargo clippy --workspace --tests --examples   # clean — ZERO warnings
```

- **45 passed, 0 failed** — matches the brief exactly.
- **`Cargo.lock` and every `Cargo.toml` are untouched** across
  `master...cowork/timeouts`. No dependency was added; `--locked` confirms the
  lockfile resolves as committed.
- **Clippy found nothing 1.95's clippy missed.** Zero warnings on the newer
  toolchain, including `--tests --examples`. This is a clean result, not a
  quiet one — I checked for suppressed output.
- Test arithmetic checks out: 39 on master + 5 (`tests/deadlines.rs`) + 1
  (the new `protocol.rs` unit test) = 45.

**Portability of the ports (asked explicitly).** Verified empirically, not by
inspection: I wrote a third-party `Party` implementor in a new test crate file
that overrides only `on_turn`, relies on the defaulted `on_exchange_failed`,
and is boxed as `Box<dyn DynParty>`. It compiles clean on 1.97 with no
warnings. The `Timer` trait's `Send`/`'static` bounds are satisfied by both
shipped implementations. One note, not a defect: because `sleep_until` returns
`impl Future` in trait position, `Timer` is not object-safe — `Box<dyn Timer>`
is impossible, and embedders must stay generic over `W: Timer`. That is the
right trade for decision 15's `Send` requirement, but it is worth one line in
EMBEDDING.md when that errand happens.

---

## Findings, most severe first

### 1. STOP — a superseded turn-deadline timer kills a live exchange

**`crates/kamiroh-adapter-kameo/src/lib.rs:477`** (the `DeadlineElapsed`
handler's staleness re-check).

The completion brief asks me to convince myself the pending-wait re-check is
sufficient against abort/stale-tell races. **It is sufficient for ack
deadlines and insufficient for turn deadlines**, and the asymmetry is visible
in the code:

```rust
let still_pending = match (kind, ack_id) {
    (DeadlineKind::Ack, Some(id)) => matches!(waits.ack, Some((due, _)) if due == id),
    (DeadlineKind::Turn, _) => waits.turn.is_some(),   // <-- no discriminator
    _ => false,
};
```

An ack report carries the `RequestId` it was armed for, so a report from a
superseded task is rejected. A turn report carries nothing. `waits.turn.is_some()`
becomes true again the moment a *new* turn wait is armed — so a report from a
task that was aborted rounds ago is accepted as current.

`abort()` does not unsend an already-queued message. The window is real:

1. Host arms turn wait **T1** for peer A and moves on.
2. Host becomes busy — the party is thinking about *another* peer's turn (the
   party is one object shared across all of that actor's conversations).
3. A's next turn arrives and queues behind the busy handler.
4. T1 fires and queues its `DeadlineElapsed` behind A's delivery.
5. Host drains: it processes A's turn — `settle_all()` (T1's abort lands too
   late), the party answers, a **new** turn wait T2 is armed.
6. Host processes T1's stale report. `waits.turn.is_some()` is true — it is
   T2 — so the exchange is failed.

The exchange killed in step 6 is one the host had just accepted a turn on and
**already sent an answer for**. The peer holds that answer and believes the
exchange is live; the host has marked it dead and told its party so. Decision
22 sanctions the two sides failing *at different moments*; it does not
sanction a side failing an exchange it has just answered.

**Demonstrated, not theorised.**
`crates/kamiroh-adapter-kameo/tests/stale_turn_deadline.rs` on my branch
reproduces it deterministically. The test drives the sequence above and then
concludes the exchange immediately, so that no legitimately-armed turn wait
remains — any `ExchangeFailed` after that point can only be the stale report.
Without the fix it fails with `ExchangeFailed { cause: DeadlineElapsed(Turn) }`
for a peer whose turn was answered; with the fix it passes.

*(Worth recording: my first version of this test could not tell the spurious
kill from a legitimate later timeout, and I only trusted the finding after
rewriting it to rule that out. The distinction matters — the naive test is
red for the wrong reason.)*

**Fix — verified.** Give the waits an arming epoch and carry it in the
report, so a superseded round is recognised regardless of kind. This also
closes the same latent hole on the ack side, where safety currently rests on
`RequestId`s never repeating within a conversation — true for the shipped
parties, but not something the type system or the protocol promises.

```diff
 struct PeerWaits {
     state: TurnState,
     ack: Option<(RequestId, tokio::task::JoinHandle<()>)>,
     turn: Option<tokio::task::JoinHandle<()>>,
+    /// Bumped every time this conversation arms a round of waits. Carried in
+    /// [`DeadlineElapsed`] so a report from a *superseded* timer task — one
+    /// aborted after its message had already queued — is recognized as stale
+    /// even though a wait of the same kind is armed again.
+    epoch: u64,
 }

 struct DeadlineElapsed {
     peer: Address,
     kind: DeadlineKind,
     ack_id: Option<RequestId>,
+    /// The arming round this task belonged to (see [`PeerWaits::epoch`]).
+    epoch: u64,
 }
```

In the arming block (`lib.rs:400`, just inside `if sent.is_ok()`), bump once
and stamp both spawned tasks:

```diff
     let waits = self.conversations.entry(reply_to.clone()).or_default();
+    waits.epoch = waits.epoch.wrapping_add(1);
+    let epoch = waits.epoch;
     if let Some(id) = posed {
```

and in the handler:

```diff
-        let still_pending = match (kind, ack_id) {
-            (DeadlineKind::Ack, Some(id)) => matches!(waits.ack, Some((due, _)) if due == id),
-            (DeadlineKind::Turn, _) => waits.turn.is_some(),
-            _ => false,
-        };
+        let still_pending = epoch == waits.epoch
+            && match (kind, ack_id) {
+                (DeadlineKind::Ack, Some(id)) => matches!(waits.ack, Some((due, _)) if due == id),
+                (DeadlineKind::Turn, _) => waits.turn.is_some(),
+                _ => false,
+            };
+```

With this applied: reproducer passes, **workspace suite 46 passed / 0 failed**
(45 + the reproducer), clippy still zero warnings.

**A design question this raises, for Casey/Mez — not for me to settle.** The
Kameo deadline clock measures *processing* time, not *arrival* time: a turn
that arrives inside its deadline but queues behind a slow party is judged
late. With a party that can think for minutes and one party object shared
across an actor's conversations, one slow peer can time out every other peer
on that actor. The epoch fix removes the incorrect kill; it does not change
this. It may well be acceptable for the spike — but it is a property of
decision 22's implementation worth a sentence in the decision log either way,
because an embedder will meet it.

### 2. High — a failed send wedges a Phone: it can neither time out nor reopen

**`crates/kamiroh-app/src/phone.rs:95-102`**.

`send_turn` advances the turn machine *before* the transport send, but arms
the waits *after* it:

```rust
let progress = self.state.on_outgoing(&turn)?;   // state advances
self.transport.send(...).await.map_err(...)?;    // <-- early return here
let now = self.timer.now();
self.ack_due = posed.map(|id| (id, now + self.deadlines.ack));   // never reached
```

When the send fails, the Phone is left in `AwaitingTheirTurn` with **no wait
armed at all**. The consequences compound:

- `next_due()` returns `None`, so a pump has nothing to race — it blocks on
  the inbox forever.
- `on_deadline_elapsed()` is guarded by `next_due()?`, so it returns `None`
  and refuses to fail the exchange even if called directly.
- `open()` cannot start a fresh exchange: the state is `AwaitingTheirTurn`, so
  it is refused with `NotOurMove` — the escape hatch decision 23 provides from
  `Failed` is unreachable, because the exchange never reached `Failed`.

The exchange is wedged silently and permanently. That is precisely the state
decision 22 declares unrepresentable ("no unbounded variant… so 'this exchange
can hang forever' is unrepresentable").

**Demonstrated:** `tests/phone_send_failure.rs` on my branch. It opens a Phone
against an unbound address so the memory transport refuses the send, then
asserts each step of the wedge.

The completion brief's "For review" note anticipates this shape for the Kameo
`Deliver` handler and rules it acceptable — *"failure-to-send leaves no waits
armed on purpose (the exchange will fail by the peer's deadlines, or ours on
the next legal send attempt)."* For the Phone that reasoning does not hold:
after a failed send there **is** no next legal send attempt, because it is now
the peer's move. The peer's deadlines fail the peer's side; ours never fails.

Note that **`LocalRuntime` already does this correctly** — at
`runtime.rs:389` it arms the waits *before* calling `send`, so a failed send
still leaves a deadline that will fail the exchange loudly on a later `step()`.
Three send sites, two behaviours. I'd propose `LocalRuntime`'s ordering as the
one to converge on, but which way to converge is a design call, so I have
**not** applied a fix — only the reproducer.

The same argument applies, less sharply, to the Kameo responder at
`lib.rs:400`: on a failed send its state has advanced and nothing is armed, so
that side hangs silently while the peer times out. Same family; same choice.

### 3. Medium — a party's illegal reply silently un-deadlines the exchange

**`crates/kamiroh-app/src/runtime.rs:386`** (`Err(_) => false`) and the
equivalent `.ok()` at **`lib.rs:400`**.

If a party returns a turn that fails `on_outgoing` — answering the wrong
request id, say — the reply is dropped, and because the incoming turn already
cleared both waits, the conversation is left with no deadline armed and the
state stuck in `OweThem`. A buggy third-party party turns into a silent hang
rather than a loud failure, which is the outcome this whole story exists to
prevent. The error is also swallowed entirely: it reaches neither the observer
nor the party.

Suggestion, in decision 25's spirit: emit an event on this path (it is exactly
"something the library observed and would otherwise swallow"), and consider
failing the exchange rather than leaving it un-deadlined.

### 4. Low — timer tasks outlive the actor that armed them

**`crates/kamiroh-adapter-kameo/src/lib.rs:229`** (`entry.actor_ref.kill()`).

`Host` has no `on_stop`. Killing the actor drops `conversations`, which drops
the `JoinHandle`s — and dropping a `JoinHandle` **detaches** a tokio task, it
does not abort it. Every armed timer task therefore keeps sleeping until its
deadline expires, then does a `tell` to a dead actor which fails harmlessly.

No correctness impact, and bounded — but bounded by `deadlines.turn`, which
the glossary says may be minutes for a thinking agent. In an unattended test
suite that spawns and stops many actors (which is the stated use case), that
is a real accumulation. Cheapest fix: an `on_stop` that walks `conversations`
calling the existing `settle_all()`.

This is the "JoinHandles never being awaited" question from the errand: never
awaiting them is fine — nothing needs their result, and `settle_all()` aborts
them on every normal path. The gap is only the actor-teardown path.

### 5. Low — the observer is called while a lock is held

**`crates/kamiroh-adapter-kameo/src/lib.rs:137-145`**.

`emit` holds the `observer` mutex across `observer.on_event(event)`. Since
`Observer` is blanket-implemented for closures, an observer that calls back
into the runtime (`set_observer`, or anything that emits) self-deadlocks on a
non-reentrant `std::sync::Mutex`. Unlikely, but it is a footgun on a surface
explicitly built for operators to plug into. Taking the event out, or
documenting "do not re-enter the runtime from an observer", would close it.

### 6. Nit — `TestTimer` accumulates wakers

**`crates/kamiroh-adapter-memory/src/testing.rs:127`**.

`TestSleep::poll` pushes a waker on every poll and never removes one for a
dropped future; entries are only drained when the clock advances past their
due instant. Since `next_or_deadline` builds a fresh sleep future on each
`step()`, a long-running test loop grows `sleepers` monotonically. Test-only
and small, and the code comments the re-registration deliberately — but the
comment addresses waker *staleness*, not accumulation, so it reads as more
settled than it is.

### 7. Nit — a feature scenario contradicts the state machine

**`features/hung-exchanges-fail-loudly.feature`**, scenario "A peer that never
answers": *"And the conversation is idle and may open a new exchange"*.

After a timeout the state is `Failed`, not `Idle` — that is the whole point of
decision 23, and `Phone::state()` will report `Failed` to anyone who looks.
The second half of the claim is right and is what matters. Since these
scenarios are under a standing obligation to become executable step
definitions, the wording should not assert something a step would have to
falsify. Suggest dropping "is idle and": *"And the conversation may open a new
exchange"*.

---

## What I checked and found correct

Recording these so the review's coverage is legible, and because several were
called out as risky:

- **`wait::next_or_deadline` (`wait.rs`)** — poll ordering and waker
  correctness are right. The deadline is polled first, so an elapsed deadline
  deterministically outranks a queued delivery, as documented. Both futures are
  polled (and therefore register their wakers) before any `Pending` return, so
  no wakeup can be lost. One property worth knowing rather than fixing: when
  the deadline wins, the half-polled `inbox.next()` future is dropped, so this
  design assumes `Inbox::next` is cancellation-safe. Both shipped inboxes are;
  it is an unstated contract on the port.
- **The domain layer** — `deadline.rs` and the `TurnState::Failed` work are
  clean and match decisions 22 and 23 exactly. `fail()` is correctly a no-op
  from `Idle`/`Failed`; a fresh `Open` is legal from `Failed` in *both*
  directions; stragglers correctly fall to id mismatch afterwards. The unit
  test pins all of it. The domain reads no clock anywhere — decision 24 holds.
- **Mandatory deadlines at every construction site** — decision 22's
  requirement is met with no escape hatches: `Phone::converse`, `install`,
  `install_party` all take `Deadlines` by value; both runtimes take
  `spawn_deadlines`. There is no `Default`, no `Option`, no unbounded variant
  anywhere in the tree. I looked for a way to construct a surface without
  deadlines and there isn't one.
- **`LocalRuntime` arming/settling** — matches decision 22's choreography. Waits
  clear on a legal incoming turn, are not cleared by an *illegal* one (the `?`
  on `on_incoming` correctly returns before the clearing), are never armed on a
  concluded exchange, and `earliest_due` is recomputed per `step()` so no stale
  timer can exist. This is the cleanest of the three implementations.
- **Event surface** — all three variants reach the observer from both runtimes,
  and denials and illegal turns reach the observer *only*, never a party, as
  decision 25 requires. Nothing failure-related crosses the wire; I checked the
  send paths specifically for this.
- **No dependencies added, `vendor/` and `vendor-snapshot` untouched.**
- **The two sibling branches merge cleanly.** `cowork/timeouts` and
  `cowork/mapping-archive` are both stacked on `cowork/spike-2-kickoff`
  (`b268a80`) and both edit the same README paragraph, which looked like a
  collision. `git merge-tree` (read-only) merges them without conflict. No
  action needed — noted because the branch topology isn't obvious from the
  errand's description of them as three independent branches.

---

## Docs branches

### `cowork/spike-2-kickoff` — merge-ready

Sanity-checked for factual errors as asked. Every internal link resolves
(`docs/ROADMAP.md`, `TIERS.md`, `VENDORING.md`, `WHY-TIERS.md`, and the
capability brief all exist at the paths given). The capability brief's claims
are self-consistent and the two I could verify from here both hold: `master` is
indeed `b960303`, and workshop-2's fork indeed has no `vendor-snapshot` branch
(`origin` carries only `master`). The 1.95.0 sandbox figure matches the
language-feature floor this workspace has been working to.

One phrasing wobble, not an error: ARCHITECTURE.md's spike-2 section opens
*"This fork hosts the third architectural spike"* immediately under the heading
"Spike 2". Both are true — spike 2 is the third counting spike 0 — but a reader
meeting the document cold will trip. "the third architectural spike (spike 2;
spike 0 preceded the workshop forks)" would settle it.

I did not re-verify the cloud-side measurements (crates.io 403s, the offline
39-test run, the ~4-minute cold build); they are observations from an
environment I can't reach, and nothing in them contradicts what I can see.

### `cowork/mapping-archive` — merge-ready

Docs only, as described: adds `docs/mappings/README.md` and the
2026-08-19 timeouts/disconnects board, plus a README pointer to the new folder
and nine lines in WORKFLOW.md. No code, no dependency, no factual claim I could
falsify. The board's rulings are consistent with decisions 22–25 as
implemented.

The `features/README.md` scenario→test mapping table on `cowork/timeouts` is
accurate: all five test names it cites exist in `tests/deadlines.rs` with the
spelling given, and all six scenarios in the feature file have a row. (Finding
7 above is the one wording defect in the feature file itself.)

---

## Where my work is

Branch **`code/timeouts-review-fixes`** (local only — not pushed, and nothing
was committed to `cowork/*` or `master`), one commit `c4596cf` on top of
`cowork/timeouts`:

- `crates/kamiroh-adapter-kameo/tests/stale_turn_deadline.rs` — reproducer for
  finding 1.
- `crates/kamiroh-adapter-kameo/src/lib.rs` — the epoch fix for finding 1
  (the diff above; ~12 lines).
- `tests/phone_send_failure.rs` — reproducer for finding 2. **No fix applied**
  — the remedy is a design call.

With that branch: **46 passed, 0 failed, clippy clean on 1.97.0.**

Findings 3–7 are unfixed and uncommitted; each is small enough to take from the
descriptions above.

## Suggested disposition

Land `cowork/spike-2-kickoff` and `cowork/mapping-archive` as they are.
Hold `cowork/timeouts` for finding 1 (fix ready and verified) and a ruling on
finding 2 (reproducer ready, remedy is Casey's or Mez's call). Findings 3–7 are
merge-after-fixes material at most, and 6–7 could equally ride along with the
already-scheduled language pass and cucumber-rs errands.
