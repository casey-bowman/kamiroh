# Review: `cowork/disconnects` (`dce3547`) — story 2, decision 27

*From Ander, 2026-08-20. Independent review of one commit on merged `master`
(`eb350db`), at `tiers/2-workshop/kamiroh`. Third review in the spike-2 cycle;
the pre-flight hazard note `2026-08-20-story-2-hazards.md` shaped this work
before it was written, so this pass checks each hazard's disposition as well
as the code. Self-contained.*

## Verdict

**Merge-after-fixes.** One real defect, latent but demonstrable, with a
verified four-line fix. Everything else is sound, and all three pre-flight
hazards are correctly dispositioned.

This is the cleanest of the three story branches I have reviewed. The defect
below is on a path nothing in-tree can reach today.

## Build, test, clippy

Toolchain **rustc 1.97.0 / cargo 1.97.0 / clippy 0.1.97**, macOS:

```
cargo build --workspace                        # clean
cargo test  --workspace                        # 57 passed, 0 failed
cargo clippy --workspace --tests --examples    # clean — ZERO warnings
```

**57 passed, 0 failed** — exactly as the commit message claims. No dependency
added; `Cargo.lock` and every `Cargo.toml` untouched. Both iroh tests pass
against real sockets, and neither is vacuous — I checked the assertions rather
than trusting the green.

## The three hazards

**Hazard 1 — a death report racing the reconnect: guarded, correctly, at the
right layer.** `watch_connection` (`kamiroh-adapter-iroh/src/lib.rs:141`)
compares the dying connection's `stable_id()` against whatever the cache now
holds for that peer, and suppresses the report when a newer connection has
already replaced it. That is exactly the "connection generations are visible
here" reasoning, and the transport is the only layer that can do it. Excluding
`LocallyClosed` is right too, and the asymmetry is worth the sentence the code
gives it: we hang up, *they* see a death.

The Kameo fan-out residue is Casey's ruling with a mandatory post-streaming
revisit, so it is not mine to relitigate — but I will record that I agree with
the *shape* of the argument. Both that residue and decision 26's
processing-time property are the same bug in the same clothes: a message about
the world, processed after the world moved on. Judging them together, once
streaming has shrunk the mailbox latency they both live in, is the right call.

One narrow nit within the guard, below.

**Hazard 2 — no new concurrent emitters: verified.** I traced every path that
can reach `Observer::on_event`. The Kameo `watch_deaths` task only `tell`s
hosts; the `PeerDied` handler emits from inside a mailbox handler, exactly
like every existing emit. `LocalRuntime::sweep_dead` emits through `&mut self`
on the single-threaded toy runtime. The iroh `report_death` is not an emitter
at all — it pushes to a per-watch queue and wakes it. The non-reentrancy
contract from decision 26 is not stressed by anything new here.

**Hazard 3 — zero choreography changes: verified, rigorously.** I diffed every
line touching `ack_due`, `turn_due`, `waits.ack`, `waits.turn`, `epoch`,
`settle_all` and `settle_ack` across all three runtimes. The complete set of
changes is two lines, and both are inside the *new* `Phone::on_peer_death` —
no existing arming or settling code was modified at all. Decision 27's claim
that the verdict shape held is not just true, it is tight. That is a real
result: it means `fail_exchange` was the right seam, and it is worth noticing
that the criterion was set before the work rather than after.

## Finding — a closed `DeathWatch` panics the race

**`crates/kamiroh-app/src/wait.rs:68`.**

The port documents the closing case explicitly
(`kamiroh-ports/src/lib.rs:172`): *"The next endpoint known dead, or `None`
when the watch is closed."* The race handler does not honour it:

```rust
if let Poll::Ready(Some(endpoint)) = death.as_mut().poll(cx) {
    return Poll::Ready(Waited::PeerDeath(endpoint));
}
// Ready(None) falls through here — and the future stays in the race
```

On `Ready(None)` control falls through to the deadline and delivery arms. If
both are pending the closure returns `Pending`, and the *next* wakeup polls
the already-completed death future again. That panics: **`async fn` resumed
after completion**.

Neither shipped watch reaches it — `MemoryDeathWatch` and `IrohDeathWatch` are
both documented "Never closes", and both are. But `DeathWatch` is a port, with
a `DynDeathWatch` object-safe twin provided precisely so third parties can
implement it, and a third-party watch that honours the documented `None`
contract takes the pump down with it. A transport whose watch ends when the
endpoint shuts down is the obvious way to write one.

**Demonstrated.** `tests/closed_death_watch.rs` on my branch: a closed watch,
an empty inbox, and a delivery that arrives after the race has parked. Without
the fix it panics; with it, the delivery is returned normally.

**Fix — verified.** Fuse the watch out of the race once it closes:

```diff
     let mut next = pin!(inbox.next());
+    // A watch that closes (`None`) drops out of the race for good: polling a
+    // future after it has completed panics, and this closure is re-entered
+    // on every wakeup.
+    let mut watching = true;
     std::future::poll_fn(move |cx| {
-        if let Poll::Ready(Some(endpoint)) = death.as_mut().poll(cx) {
-            return Poll::Ready(Waited::PeerDeath(endpoint));
-        }
+        if watching {
+            match death.as_mut().poll(cx) {
+                Poll::Ready(Some(endpoint)) => return Poll::Ready(Waited::PeerDeath(endpoint)),
+                Poll::Ready(None) => watching = false,
+                Poll::Pending => {}
+            }
+        }
```

With it applied: **58 passed, 0 failed** (57 + the reproducer), clippy clean.

The alternative is to change the port's contract to say the watch never
closes and drop the `Option`. I would not — a watch that ends is a reasonable
thing for a real transport to have, and the fix is smaller than the doc change.

## Nits, neither blocking

- **The suppression check and the report are not atomic**
  (`kamiroh-adapter-iroh/src/lib.rs:141-161`). The connections lock is released
  at the end of the `superseded` block, and `report_death` runs after it. A
  reconnect that completes in that window is not seen, so a stale death is
  reported anyway. The window is a few instructions against a full QUIC
  handshake, so this is theoretical in a way the Kameo fan-out residue is not —
  but unlike that one it is closable for free, by holding the guard across the
  report (`report_death` takes only the watcher locks and never awaits). Worth
  doing simply so the accepted residue stays a *single*, well-named thing.
- **`cargo fmt --check` is not clean** — 36 hunks across 15 files. I checked
  before reporting it: `master` already has 29, so formatting is plainly not in
  this project's gate and this commit did not break anything. Some of the
  pre-existing drift is in my own review test files, which I should have caught
  earlier. Mentioning it only in case a `fmt` gate is wanted before the
  cucumber-rs errand adds more files; if not, ignore.

## Checked and correct

- **Memory adapter fault levers.** `partition`/`heal` hold and flush in send
  order, and `heal` preserves relative order through the `drain`/`partition`
  pair. The detail I liked: a partitioned send to an address that does not
  exist returns `Ok(())` rather than `UnknownAddress` — during a partition you
  *cannot* know the peer is gone, so manufacturing evidence there would be
  wrong. That is the Q6 ruling implemented with its reasoning intact.
  `kill_endpoint` correctly gives the other shape: mailboxes vanish, senders
  get errors, watches report.
- **`LocalRuntime` three-way race and tombstones.** Poll order (deaths,
  deadline, delivery) matches the decision, and all three arms register wakers
  before any `Pending`. `sweep_dead` runs before the wait as well as after a
  death, so a tombstone set while another actor was stepping still lands.
  Termination is safe: `fail_exchange` moves the state out of the
  `AwaitingTheirTurn | OweThem` filter, so the sweep cannot loop. Tombstone
  clearing on any inbound delivery is right, and I checked the case that
  worried me — an outbound send to a tombstoned endpoint succeeding, then the
  sweep killing the fresh exchange. It is unreachable in `LocalRuntime`,
  because every outbound send follows an inbound delivery that has already
  cleared the tombstone.
- **Both watches are cancellation-safe.** `step` builds a fresh
  `next_death_boxed()` every iteration and drops it when another arm wins.
  Neither implementation pops from its queue except on `Ready`, so no death is
  lost to a dropped future — the failure mode that would have been easy to
  introduce here.
- **Both iroh tests earn their keep.** `a_conversation_spans_connections`
  pins Q5 over real QUIC: close the cached connection, then complete a second
  exchange on the *same* phone, with the receiving side routing the fresh
  inbound connection to the same actor. `a_shutdown_peer_is_reported_dead`
  pins evidence flowing from `closed()` to the watch, and registers the watch
  after the connection exists, which also (incidentally) pins that late
  registration works.
- **Feature file and mapping.** All five scenarios have pinning tests; all
  named tests exist with the spelling given. `a_shutdown_peer_is_reported_dead`
  is extra coverage beyond the table rather than a missing row.

## Where my work is

Branch **`code/disconnects-review`**, one commit `328b9c4` on top of
`cowork/disconnects`: the fix in `crates/kamiroh-app/src/wait.rs` and the
reproducer `tests/closed_death_watch.rs`. **Local only — not pushed, and
nothing committed to `cowork/*` or `master`.**

## Suggested disposition

Take the four-line fix (or cherry-pick `328b9c4`) and `cowork/disconnects` is
merge-ready. The iroh atomicity nit is worth a follow-up but should not hold
the branch.

Still open from earlier briefs, unchanged: workshop-2's fork has no
`vendor-snapshot`, so none of this has been verified by an offline cloud
build — every green result across all four reviews is from my 1.97.0 macOS
host; and EMBEDDING.md still needs deadlines, `on_exchange_failed`, the
observer, the `Timer` object-safety note, and now `DeathWatch` too.
