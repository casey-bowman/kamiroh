# Heads-up for story 2 (disconnects): three hazards carried over

*From Ander, 2026-08-20. Not a review — story 2 isn't written yet. These are
the three places the timeouts review cycle says disconnects will bite, written
down so they can be checked against work already in flight rather than found
afterwards. Ignore freely if they're already handled.*

## 1. A death report is a stale-report race waiting to happen

Finding 1 of the timeouts review was a superseded timer task killing a live
exchange: its report carried nothing that let the host tell it was out of
date, and `abort()` doesn't unsend an already-queued message. The fix was an
arming epoch carried in the message.

**A transport death report has exactly this shape**, and Q5's reconnect
promise makes it reachable *by design* rather than by accident. Because a
conversation spans connections, "the wire died" and "this exchange is dead"
are no longer the same claim. A `closed()` notification that queues behind a
reconnect — and is then delivered after the conversation has resumed — will
kill an exchange that is demonstrably alive, in precisely the way the epoch
fix stopped the turn deadline doing.

Worth deciding explicitly: what makes a death report *current*? If the answer
is "the connection generation it refers to", that wants carrying in the
message, the way `epoch` is. If the answer is "a death is a fact and never
goes stale", that's a defensible ruling — but it should be a ruling, because
the reconnect case is the counterexample.

## 2. The new emit sites are genuinely concurrent

An iroh `closed()` subscription emits from a background task, not from a
mailbox handler. That makes it a concurrent emitter — the case that broke
last round and produced the observer regression.

The lock-held `emit` on `cowork/observer-fix` handles concurrency correctly,
so nothing needs redesigning. But the non-reentrancy contract that just landed
(`Observer` in `kamiroh-ports`: observe, record, return) now binds code being
written today. A subscription task that calls back into the runtime from
inside `on_event` — to stop an actor on disconnect, say, which is a very
natural thing to reach for — deadlocks.

## 3. `fail_exchange` should be the whole seam

Both runtimes now have one home for the failure path, and decision 23 already
reserved "peer vanished" as a verdict the outside world feeds in. Adding a
`FailureCause` variant and routing it through `fail_exchange` should need no
changes to the arming/settling choreography at all.

**If it turns out to need them, that's worth pausing on rather than pushing
through** — it would mean transport death isn't actually shaped like the other
verdicts, which is a design finding and cheaper to surface early than to
unpick later.

---

Happy to review whenever it's ready. If it would help, I can also look at a
partial branch mid-flight rather than waiting for a finished commit — finding 1
was expensive precisely because it was found after it was built.
