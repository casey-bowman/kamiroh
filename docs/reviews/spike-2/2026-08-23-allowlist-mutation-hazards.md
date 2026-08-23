# Pre-flight hazards: allowlist mutation on running actors (roadmap 3)

*From Ander, 2026-08-23. Not a review — the story isn't written yet. Read
against `master` a4184e80 and the mapping board's rulings
(`2026-08-23-mapping-board-allowlist-mutation.md`). No code was touched.
Ignore freely where something is already handled.*

---

## 0. The rulings stand — no STOP

I checked each ruling for safety, since that was the stop condition. **None
is unsafe as ruled**, and two are better than they look:

- **Q2=a's new variant earns its keep.** Giving revocation its own cause
  rather than reusing `Disconnected` is what lets a party tell "the peer
  vanished, retry later" from "my own operator cut this peer off, do *not*
  retry". Hazard 5 below is exactly the bug you get without that distinction.
- **R4 + the no-auto-close addendum are self-consistent** with the iroh
  adapter as built. `close_connection` sees `LocallyClosed` on this side,
  which `watch_connection` deliberately excludes from death reports
  (`crates/kamiroh-adapter-iroh/src/lib.rs:162`). So the eviction recipe's
  two steps cannot double-fail the same exchange locally. That is a real
  property of the code, not an assumption.

One ruling is **under-specified rather than unsafe** — Q2=a's "the actor's
live exchanges" does not reach app-held `Phone`s. That is hazard 1, and it
wants a decision before the build, not a stop.

---

## 1. "The actor's live exchanges" does not include app-held Phones

This is the one I would settle before writing any code.

`revoke` can only fail exchanges the runtime knows about: the
`conversations` map inside `LocalActor` / `Host`. That map is
**responder-side only**. An embedder that initiates holds a `Phone`
(`crates/kamiroh-app/src/phone.rs`), which the runtime has never heard of —
and which guards its own inbound traffic with its **own separate
`Allowlist`**, the `app_list` value every app-side pump in the test suite
passes to `process()` (`tests/turns_conversation.rs:99`,
`tests/deadlines.rs:121`, `tests/disconnects.rs:136`, and five more).

Two consequences, both security-relevant for the console-key story:

1. **`runtime.revoke(actor, E)` does not stop E talking to your Phone.** The
   Phone's pump checks `app_list`, which the runtime cannot reach. An
   operator who "revoked everywhere" via the runtime API has not revoked
   everywhere.
2. **A live Phone exchange with E will not fail at once.** It sits until its
   own ack/turn deadline elapses — precisely the "wait out the deadline"
   outcome Q2=a exists to prevent.

Decision 27 hit this same shape and solved it explicitly: the Phone got
`on_peer_death` (`phone.rs:185`) so an app pump can feed it the verdict.
**The symmetric move is `Phone::on_revoked(&mut self, endpoint) ->
Option<FailureCause>`**, modelled on `on_peer_death` line for line — same
guard, same clearing of `ack_due`/`turn_due`, returning
`FailureCause::Revoked`. It is about eight lines and it keeps the two
surfaces telling the same story.

Whether that is in scope is Casey's call. If it is deferred, the gap should
be written into EMBEDDING.md's eviction recipe rather than left implicit,
because "revoke at every actor that admits the key" reads as complete and
isn't.

Note `on_peer_death` also models the *right* idempotence pattern — it tests
`!self.state.fail()` and returns `None` when nothing was live. See hazard 4.

---

## 2. KameoRuntime's revoke path: three deadlocks and an honesty limit

The errand asked about lock discipline around the lock-held `emit`. There are
three distinct ways to deadlock here and they want different fixes.

### 2a. Do not hold the roster lock across the await

`revoke` must find the host's `ActorRef` in `inner.roster`
(`std::sync::Mutex`) and then message it. If the guard is still alive at the
await point, two things go wrong: the future stops being `Send` (a
`MutexGuard` held across `.await`), and — worse, because it is not always a
compile error — any roster-taking work the host does while you wait
deadlocks. `Host::execute` calls `runtime.install_party` and `runtime.stop`
(`crates/kamiroh-adapter-kameo/src/lib.rs:682`), both of which lock the
roster.

**`watch_deaths` already has the correct pattern** (`lib.rs:243`): collect
the `ActorRef`s into a `Vec`, let the guard drop, *then* message. Copy it.

### 2b. Observer re-entrancy is newly reachable

`emit` (`lib.rs:143`) calls `on_event` under the observer mutex, and the
documented rule is that observers must not call back into the runtime. The
rule is currently written as an enumeration — "no `set_observer`, `install`,
or `stop`" — at `lib.rs:137`, again at `lib.rs:149`, and mirrored in prose at
`docs/EMBEDDING.md:76`. **`admit`/`revoke` must join that list, in all three
places.**

This is not pedantry. The obvious naive thing to write, once revocation
exists, is an observer that sees `Event::DeliveryDenied` and revokes the
offender — auto-ban on denial. That deadlocks, and it is the first thing
someone will try.

### 2c. `ask` from inside a handler self-deadlocks (a workshop-3 trap)

If `revoke` is implemented with `ask`, a host that revokes *itself* from
inside its own message handler waits on a mailbox it is occupying. No path
does this today. **Workshop-3's wire form is exactly this path** — a harness
handler executing an `Admit`/`Revoke` command. Worth choosing an API shape
now that doesn't foreclose it (an internal sync path that mutates the
allowlist directly, distinct from the external message-based one).

Related asymmetry worth noting deliberately: `install` and `stop` are sync,
but `admit`/`revoke` will be **async** on both runtimes (`fail_exchange`
awaits the party callback). That is a first for `KameoRuntime`'s mutation
API and it is what makes 2c bite.

### 2d. What "bites at once" can honestly mean

Deliveries already queued in the host's mailbox are processed **before** a
revoke that arrives behind them. Kameo serializes per actor, the pump
`tell`s each delivery as it drains the inbox (`lib.rs:228`), and a `Revoke`
joins the same FIFO. So:

- R2's "bites on the very next delivery" is true only for deliveries
  *processed after the revoke is handled*, not deliveries that have already
  arrived. An attacker who queued ten messages gets all ten admitted.
- Revocation latency is bounded by the current handler completing, which
  includes `party.on_turn_boxed` — unbounded in principle. Decision 26
  already records this shape ("processing time, not arrival time").

Neither is fixable in this spike and neither needs fixing; both need
*saying*, so the feature file doesn't promise something the mailbox cannot
deliver.

**For test determinism, prefer `ask` over `tell`** for the mutation methods:
a test that can `await` the revoke and then assert is deterministic; one
that `tell`s and hopes is a flake generator, especially under
`start_paused = true`.

---

## 3. The epoch question: no bump needed — and why

Asked directly: **does `Revoked` need to bump or settle `PeerWaits` the way
`DeadlineElapsed` does?** No. Reusing `fail_exchange` (`lib.rs:547`) is
sufficient, and I traced both windows:

- `fail_exchange` calls `waits.settle_all()` (`lib.rs:330`), which aborts
  both timer tasks and sets `ack`/`turn` to `None`. A stale `DeadlineElapsed`
  already queued when the revoke lands fails the `still_pending` check on
  `waits.turn.is_some()` / the ack match — **before** the epoch comparison
  matters.
- If instead the stale report arrives *after* a re-admit and a fresh arming,
  the arming path bumps `waits.epoch` (`lib.rs:489`) — the only arming site —
  so the epoch check catches it.

Both windows are covered, so this needs no new machinery.

**But the protection is emergent, not local.** It holds only because every
revoke goes through `fail_exchange` *and* every re-arm bumps the epoch. A
future bulk-revoke that fails exchanges by a shorter path would silently lose
it. I would **bump the epoch inside `fail_exchange`** — one line,
`waits.epoch = waits.epoch.wrapping_add(1);` next to `settle_all()` — so the
invariant lives at the seam rather than being reconstructed by whoever reads
it next. Cheap insurance, and it makes the property checkable in one place.

---

## 4. Double-fail: the live-state filter is what makes R5 true

Asked directly: is there a double-fail hazard when revoking an endpoint
already in `dead`? **Not if the revoke sweep copies the existing predicate —
and R5 breaks observably if it doesn't.**

The mechanism: `TurnState::fail()` (`protocol.rs:152`) is already idempotent
and returns `bool` — `Failed → false`, no state change. But **both runtimes'
`fail_exchange` ignore that bool** and unconditionally notify the party and
emit `Event::ExchangeFailed` (`runtime.rs:227`, `lib.rs:547`). So a second
fail on an already-failed exchange produces a **spurious duplicate event and
party callback** even though the state machine correctly did nothing.

That is deliberate, not a bug: LocalRuntime's `fail_exchange` doc comment
documents the refused-`Close` case where the state is `Idle` but the party is
told anyway, honestly, that the peer never got its answer. **So
`fail_exchange` cannot simply be taught to honour the bool** — the fix has to
be upstream of it.

Which means the revoke sweep must pre-filter on live state, exactly as
`PeerDied` (`lib.rs:635`) and `sweep_dead` (`runtime.rs:270`) already do:

```
matches!(state, TurnState::AwaitingTheirTurn { .. } | TurnState::OweThem { .. })
```

With that filter, every ordering is safe — death-then-revoke, revoke-then-
death, revoke-then-revoke all find the state already `Failed` and emit
nothing further. Without it, R5's "revoking twice is a no-op" is true in the
`BTreeSet` and false at the observer, which is the level tests assert at.

Worth an explicit scenario: **revoke twice, assert exactly one
`ExchangeFailed`.**

Two smaller notes in the same area:

- Filter on `peer.endpoint == endpoint` and fail **all** matches. One
  endpoint can host several actor names, so one revoke may fail several
  conversations — `PeerDied` loops for this reason.
- `LocalRuntime`'s `dead` set needs no involvement. Note in passing that
  `self.dead.remove` runs before admission (`runtime.rs:391`), so a revoked
  endpoint that keeps sending keeps clearing its own tombstone. That is
  correct — it *is* alive — but it means "revoked" and "dead" stay
  independent, which is what R4 wants.

---

## 5. Re-admit is clear — but an auto-reopening party is a retry storm

Asked directly: does anything wedge on revoked-then-readmitted? **No.**
`Failed → Open` is legal in both directions (`protocol.rs:165` outgoing,
`protocol.rs:204` incoming), the Phone documents a fresh `open` as legal
after failure, and `fail_exchange` leaves no waits armed. The path is clean.

The hazard is one layer up. `Party::on_exchange_failed`'s own documentation
suggests the party may "perhaps reopen" (`kamiroh-ports/src/lib.rs:80`).
Consider a party that does, after a revocation:

1. Operator revokes E. The exchange fails with `Revoked`.
2. The party reopens — and **outbound still works**, because R4 makes the
   allowlist inbound-only.
3. E receives the open and replies. **We deny our own peer's reply.**
4. Our ack deadline elapses. The party reopens again.

A livelock against a peer we deliberately cut off, burning a deadline per
round. Nothing in the runtime stops it, and the operator sees a `DeliveryDenied`
storm rather than silence.

The fix is documentation, and it is the payoff for Q2=a's separate variant:
**`Revoked` is the one cause where reopening is wrong.** EMBEDDING.md's
failure section should say so where it lists the causes.

A cheaper related point for the feature file: **admitting does not replay
denied traffic.** Denied deliveries are dropped, not queued, so example 1
("warming up to a new peer") only works if the peer sends *after* the admit
lands. Scenario wording should make the peer retry rather than implying the
earlier message arrives late.

---

## 6. The `FailureCause` ripple: the complete site list

Good news first: **there is exactly one exhaustive `match` on `FailureCause`
in the whole workspace** — the `Display` impl at
`crates/kamiroh-domain/src/deadline.rs:70`. Everywhere else the type is
constructed, compared with `assert_eq!`, or carried in a struct field. So the
compiler will point at precisely one site, and adding the variant is
mechanically trivial.

The risk is entirely in **prose the compiler cannot check**. Sites that
enumerate the causes and go stale silently:

| Site | What's there |
|---|---|
| `kamiroh-domain/src/deadline.rs:51` | the enum — add `Revoked` + doc |
| `kamiroh-domain/src/deadline.rs:70` | `Display` — **the only compiler-caught site** |
| `kamiroh-domain/src/protocol.rs:86` | "a deadline elapsed, the peer vanished" |
| `kamiroh-domain/src/protocol.rs:101` | `TurnState::Failed` doc, same pair |
| `kamiroh-domain/src/protocol.rs:148` | `TurnState::fail` doc, same pair |
| `kamiroh-ports/src/lib.rs:80` | `Party::on_exchange_failed` doc, same pair |
| `docs/EMBEDDING.md:69` | the four-item prose list → five (and see hazard 5) |
| `docs/EMBEDDING.md:168` | **"all four"** third-runtime obligations → five |
| `ARCHITECTURE.md:387` | decision 23's "(deadline elapsed; later, peer vanished)" |
| `features/README.md` | new scenario → pinning-test table section |
| `docs/ROADMAP.md:27` | item 3's "currently unreachable" is no longer true |

`EMBEDDING.md:168` is the one I'd have missed on a fast pass, and it matters
most: **"the toy `LocalRuntime` remains the readable reference for all
four"** enumerates what a third runtime owes — deadline choreography, the
dual failure surface, positive evidence, the death fan-out. Live allowlist
mutation becomes the **fifth**, and an engine-replacement author who misses
it ships a runtime whose allowlists are unrevokable.

**Two things that do *not* need editing**, so nobody spends the effort:

- **ARCHITECTURE decision 26's "Two paths join the elapsed deadline."** That
  wording is scoped to decision 26's own two paths. Decision 27 added
  `Disconnected` without touching it; decision 28 should follow the same
  precedent.
- **The `Event` enum.** Q3=b adds no variants, and no exhaustive match on
  `Event` needs revisiting.

`FailureCause` has no `#[non_exhaustive]` (nothing in the workspace does), so
this is a breaking change for any downstream matcher. There are none. Worth
one line in the decision entry rather than an attribute.

Next decision number is **28** (`ARCHITECTURE.md:464` is 27).

---

## 7. The eviction recipe has three sharp edges

The EMBEDDING.md recipe is the deliverable most likely to be quietly wrong,
because it is the one a stressed operator follows literally.

1. **`close_connection` is not a port method.** It exists only on `IrohNet`
   (`kamiroh-adapter-iroh/src/lib.rs:341`) and is `async`. The memory adapter
   has no equivalent — it offers `kill_endpoint`, `partition`, `heal`. The
   recipe must say step 2 is transport-specific, or an embedder on another
   transport will look for a lever that isn't there.
2. **The order is load-bearing.** Revoke everywhere *first*, then close. Close
   first and the evicted peer simply redials into whichever actors haven't
   been revoked yet. The board writes the steps in the right order; the
   recipe should say the order is mandatory and why.
3. **"Every actor that admits the key" includes your own app-side
   `Allowlist`** if you run a Phone pump — see hazard 1. As worded, the
   recipe reads as complete while leaving the Phone's list untouched.

Also worth stating, because it will surprise someone: revocation stops what
you *hear*, not what an in-flight handler *says*. A handler that was mid-await
on its party when the revoke queued will still send its reply to the revoked
peer. Consistent with R4 (policy is inbound-only), but it means "revoke
returned" and "nothing more goes out" are different claims.

And the eventual-consistency point: revoking across N actors is N separate
messages landing at N different moments. There is no fleet-wide atomic
eviction in this spike, and the window is real. That is fine — R1 says the
actor is the unit — but the recipe shouldn't imply otherwise.

---

## 8. Smaller things

- **LocalRuntime's shape.** `step` holds `&mut self` across its await, so the
  borrow checker already guarantees no revoke can land mid-step: revocation
  happens strictly between steps. That means **no tombstone set is needed** —
  the `dead` set exists because deaths arrive asynchronously *during* a
  step's race, which revocations never do. Copying decision 27's deferred-
  sweep shape here would be cargo-culting. Call `fail_exchange` directly.
- **Don't return `Stepped` from `revoke`.** `fail_exchange` returns
  `Result<Stepped, _>`, but a revoke is not a step. Discard it and return
  something honest — a count of exchanges failed, mirroring
  `PeerVanished { exchanges_failed }`. `Stepped` is only ever `assert_eq!`d
  in tests, never exhaustively matched, so leaving it alone costs nothing.
- **The two runtimes will differ observably**, and the feature file has to
  survive it. A Kameo test can revoke while the actor is genuinely blocked
  waiting; a LocalRuntime test can only revoke between `step` calls. Scenarios
  worded as "while the actor is waiting" will pass on one and be untestable on
  the other. Word them around the observable effect instead.
- **`features/README.md` has a standing obligation** that a scenario without a
  pinning test is "a gap, not a decoration". The new scenarios need their
  table rows in the same commit.
- **The memory adapter needs no new fault lever** for these stories —
  revocation is a runtime API call, not a transport fault. Story 3 may be the
  first that doesn't touch `MemoryNet` at all.

---

Happy to review whenever it's ready — and as last time, a partial branch
mid-flight is fine and cheaper than a finished one. Hazards 1 and 4 are the
two I'd most want to see settled before the code sets: hazard 1 because it
changes the API surface, hazard 4 because it's invisible until a test asserts
event counts.
