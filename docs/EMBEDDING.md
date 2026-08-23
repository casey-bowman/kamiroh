# Embedding kamiroh

How an application puts its own actors in conversation across the internet
using kamiroh as a library. Everything here happens on the embedder's side of
the app boundary; kamiroh itself is not modified. (See `ARCHITECTURE.md` for
the hexagon and the glossary; decisions 16–17 define the surfaces used here.)

## The two surfaces

An embedding app touches exactly two things per actor:

- **`Party`** (implement it): the brain behind an actor. kamiroh calls
  `on_turn` with each incoming turn; the method's `&mut self` is the
  atomicity contract — one turn in, one atomic state change, at most one
  turn out, sent only after the method returns.
- **`Phone`** (hold it): the handle for initiating. Constructing one opens a
  conversation locally (nothing crosses the wire until the first turn);
  `open` starts an exchange, `send_turn` continues one, and alternation is
  enforced on both directions.

Turn bodies are opaque bytes: whatever envelope or encoding the app already
speaks rides through kamiroh untouched.

## Patience is not optional

Every conversation surface asks you, at construction, how long you are
willing to wait — and the compiler will not let you skip the question
(decisions 22–24). In plain terms:

- **You choose your own time limits.** A `Deadlines { ack, turn }` value is
  a required argument at all three construction sites: `Phone::converse`
  (per conversation), `install`/`install_party` (per actor, covering every
  conversation it responds in), and the runtime constructors
  (`LocalRuntime::new`/`KameoRuntime::new` take `spawn_deadlines`, given to
  everything their harness spawns). The *ack* deadline bounds waiting for
  the delivery receipt of a *request* you sent — transport-scale, short; a
  closing turn poses no request and arms no ack wait. The *turn* deadline
  bounds waiting for the peer's next answer — thinking-scale, generous.
  There is no default and no "wait forever": pick numbers that fit your
  deployment.
- **The other side never sees your limits.** Deadlines are yours alone;
  the peer has its own, and neither is told the other's. A consequence
  worth knowing: while *your* party is composing its answer, it is being
  judged against the *peer's* clock, which you cannot see — so be
  conservative about your own slowness.
- **When a wait runs out, the exchange fails — loudly, never silently.**
  The same is true when the transport refuses a send or reports the peer's
  endpoint gone: failure arrives at once on positive evidence, and the
  deadline is only the backstop for pure silence (decisions 26–27). The
  *conversation* always survives a failed exchange; a fresh `open` is
  legal immediately.
- **Time enters through the `Timer` port.** Real deployments pass
  `TokioTimer` (from `kamiroh-adapter-kameo`); deterministic tests pass
  the hand-advanced `TestTimer` (from the memory adapter's `testing`
  module), where a sixty-second deadline elapses in microseconds. One
  gotcha: `Timer` is **not object-safe** (newer rustc says "not dyn
  compatible") — `Box<dyn Timer>` will not compile, so keep your own types
  generic over `W: Timer`. That is the
  deliberate price of decision 15's `Send` guarantee.

## Hearing about failure

Failure is told twice, to two different audiences (decision 25):

- **Your `Party` is told about its own exchanges.** Override
  `on_exchange_failed(&mut self, from, cause)` — the default is a no-op,
  so existing parties compile unchanged, but a party embedded for
  unattended operation should clean up, abandon work in progress, and
  perhaps reopen. `FailureCause` says why: a deadline elapsed, a send was
  refused, your own reply was illegal, the peer disconnected, or your own
  side revoked the peer. One exception to "perhaps reopen": on `Revoked`,
  do **not** reopen unprompted — your own operator cut that peer off, its
  replies will be denied at your door, and a party that retries anyway
  livelocks against a peer you deliberately silenced, burning one deadline
  per round.
- **Your test or operator code observes everything.** Hand the runtime an
  `Observer` via `set_observer` — any `FnMut(Event) + Send` closure works —
  and it receives `ExchangeFailed`, `DeliveryDenied`, and
  `IllegalTurnDropped`. This is how an unattended test asserts positively
  that a denial happened instead of waiting to see that nothing did. One
  hard rule (decision 26): runtimes may call the observer while holding
  internal locks so that no event is ever lost (the Kameo runtime does),
  and in exchange it must **not call back into the runtime** — no
  `set_observer`, `install`, `stop`, `admit`, or `revoke`. In particular,
  an observer that revokes on `DeliveryDenied` — auto-ban on denial — is
  the natural thing to write and it deadlocks. Observe,
  record, return; mutate from your own code, outside the observer.
- **Nothing about failure ever crosses the wire.** Each side reaches its
  own verdict on its own evidence; a denied sender learns nothing at all.

## Hearing about disconnects

Transports report what they know (decision 27). Get a `DeathWatch` from
your network handle (`net.death_watch()` on both the memory net and the
iroh net) and either hand it to a runtime — `LocalRuntime::watch_deaths` /
`KameoRuntime::watch_deaths` — or race it in your own pump alongside the
inbox and deadline (`wait::next_or_deadline_or_death`, and
`Phone::on_peer_death` for the initiating side). A reported death fails
live exchanges with that endpoint immediately; silence is never evidence
(that is the deadline's job), and a wire blip that heals in time
interrupts nothing. For unattended tests, the memory net is a fault
simulator by contract: `kill_endpoint` is positive evidence,
`partition`/`heal` is buffered silence.

One lifetime note for long-running embedders: death watches never close
on their own, and `KameoRuntime::watch_deaths` returns a `JoinHandle`
whose *drop detaches* the task (tokio semantics) — `abort()` is the only
off switch. The nets also keep every watch subscription for their own
lifetime. Fine at test scale (one watch, one net, one process exit);
plan for it if your embedding creates watches dynamically.

## Changing your mind about a peer

Allowlists mutate on running actors (decision 28): `admit(name, endpoint)`
and `revoke(name, endpoint)` on both runtimes, no restart. Three things to
hold onto:

- **A revocation bites at once and fails loudly.** The next delivery from
  the revoked endpoint is denied, and the actor's *live* exchanges with it
  fail immediately with `FailureCause::Revoked` — positive evidence, no
  deadline waited out. Conversations survive; a fresh exchange is legal if
  you re-admit. Both calls are idempotent, and revoking twice emits
  nothing the second time.
- **An admission replays nothing.** Deliveries denied before you admitted
  were dropped, not queued — the peer must send again.
- **Your Phones are your own job.** The runtime's `revoke` reaches only
  the actors it hosts. If you initiate with a `Phone`, its inbound traffic
  is guarded by *your* allowlist — the one your pump passes to
  `process()` — which the runtime cannot see. Revoke there yourself, and
  tell each affected Phone with `Phone::on_revoked(&endpoint)`, exactly as
  you relay deaths with `on_peer_death`: the live exchange fails at once
  instead of waiting out its deadline.

**The eviction recipe** — for a compromised or rotated key that must stop
working *now*:

1. **Revoke everywhere first**: every hosted actor that admits the key,
   *and* your own app-side allowlist + Phones (the previous paragraph).
   Order matters — close the connection first and the evicted peer simply
   redials into whichever actors you haven't revoked yet.
2. **Then tear down the pipe**, if you want to: `close_connection(peer)`.
   This step is transport-specific — it lives on `IrohNet`, not on any
   port; the memory net has no equivalent (its levers are fault
   simulation, not eviction). It is also *wire-visible*: revocation alone
   is silent (the peer sees nothing but silence and cannot distinguish
   "revoked" from "slow"), while a closed connection is positive evidence
   in the peer's hands. With a stolen key, stealth is worthless and speed
   isn't — pay the visibility. For routine rotation, skip this step.

Two honest limits. Revocation stops what you *hear*, not what an in-flight
handler *says*: a reply your party was already composing still goes out.
And revoking across N actors is N separate calls landing at N moments —
there is no fleet-wide atomic eviction; the actor is the unit of trust and
therefore the unit of mutation.

## Opening with many at once

`Phone::open_many` opens exchanges with several actors at ONE endpoint in
a single send (decision 29) and hands back N ordinary Phones. From that
moment there is nothing special about them — which puts three small jobs
on your pump:

- **Relay everything endpoint-scoped to every phone of the batch.** The
  batch receipt arrives once, claimed-from the mailroom
  (`kamiroh_ports::BATCH_RECEIPT_NAME`): pass its `Ack` to every phone's
  `note_ack`. The same loop rule covers a reported death
  (`on_peer_death`), a revocation on your own allowlist (`on_revoked`),
  and an elapsed ack deadline — all four are endpoint-scoped facts, and
  all four existing methods are per-phone. It is a loop; write it once.
- **Route replies by the proven sender.** Each worker answers on its own
  schedule; `delivery.from` picks the phone. The workers' own pairwise
  acks arrive after the receipt and settle nothing — `note_ack` returns
  `false` and that is fine.
- **Tighten the turn deadline.** The receipt promises arrival at the
  endpoint, not that each sibling exists and admits you — so a denied,
  absent, or duplicate sibling is *silence that surfaces at the turn
  deadline*, where pairwise it would have surfaced at the short ack
  deadline. You construct the N Phones, so you set their `Deadlines`: a
  tighter turn deadline is the natural compensation for the lost
  safety net.

A refused batch send fails loudly at once (decision 26): the error
returns and no phones exist. And fan-out changes nothing for a third
runtime — it is a transport affair, so the obligations list below stays
at five.

## Pattern: wrapping an existing request/response actor

If the app already has an actor (or any async component) with an ask-shaped
surface — "take a request value, return a response value" — the Party
wrapper is mechanical:

```rust
impl Party for MyActorBridge {
    async fn on_turn(&mut self, _from: &Address, turn: Turn) -> Option<Turn> {
        let request = turn.request()?;                  // Close → None: done
        let value = decode(&request.body);              // app's own encoding
        let answer = self.inner.ask(value).await;       // app's own actor
        Some(Turn::Close {
            response: Response { id: request.id, body: encode(&answer) },
        })
    }
}
```

Single-round exchanges (`Open` in, `Close` out) reproduce the app's existing
request/response idiom exactly. Multi-round exchanges (`Continue` turns) are
available whenever a dialogue is genuinely conversational. Errors the app
would return locally encode as error-shaped response bodies — kamiroh does
not interpret bodies.

Fire-and-forget notification patterns map as an `Open` answered by a trivial
`Close`: the sender gains delivery confidence it never had locally. There is
no fan-out primitive in v0; a broadcast to N remote subscribers is N
conversations.

## Pattern: proxy-then-promote (walking an actor tree onto the network)

Many apps keep their actors behind a supervisor: children created, messaged,
and closed through one funnel. Such a tree walks onto the network in two
optional steps — the second taken per child, later, with real usage in view.

**1. Proxy.** Install *one* kamiroh actor per app instance, backed by a Party
wrapper that translates single-round exchanges into the supervisor's existing
operations (create → id, forward(id, request) → response, close(id)). The
app's own message envelope rides in turn bodies. Zero changes to the app's
module; the whole tree becomes remotely reachable at once. Two costs, worth
accepting consciously: the allowlist can only grant access to the *whole
tree* (admitting an endpoint to the proxy is a privileged grant, in the same
sense as kamiroh's own harness actor — decision 6), and all remote traffic
serializes through the proxy's mailbox (if the supervisor already awaits its
children inline, this property exists locally; the network amplifies it).

**2. Promote.** When a specific child needs direct conversation, finer trust,
or parallelism, give it its own kamiroh actor: its id becomes an `ActorName`,
its wrapper asks it directly, its allowlist grants access to *it alone*, its
mailbox unblocks the funnel, and long multi-round exchanges with it stop
threading through the supervisor. Lifecycle coupling (create/close must now
bind/unbind the kamiroh actor) follows the same shape as kamiroh's harness
spawn/stop. The proxy remains for lifecycle and the unpromoted rest.

The steps compose — both are just parties behind actors — and nothing in
step 1 forecloses or prejudges step 2.

## Pattern: a third runtime (engine replacement)

The runtimes shipped here (`LocalRuntime`, `kamiroh-adapter-kameo`) host app
behavior inside an engine. An embedder whose own actor system must *host*
kamiroh actors (supervision trees, instrumentation, lifecycle idioms) can
write a driving adapter of the same shape, given three engine capabilities:
dynamic spawn/stop, a per-actor serialized mailbox (the atomicity substrate),
and tokio-compatible `Send` handler execution (decision 15). Since spike 2 a
runtime also owes the deadline choreography (arm on send, settle on
ack/turn, fail loudly on expiry — decisions 22–24), the dual failure surface
(party notification + observer events, decision 25), positive evidence
failing an exchange at once — a refused send or an illegal reply from your
own party (decision 26) — the death fan-out (decision 27), and live
allowlist mutation — `admit`/`revoke` on running actors, with a revocation
failing live exchanges at once and only live ones, so revoking twice emits
once (decision 28); the toy
`LocalRuntime` remains the readable reference for all five.

The definition of done is already executable: the harness and turns
integration tests are the conformance suite. A third runtime is correct when
they pass against it. Absent a genuine need to host, wrapping via `Party`
(above) is smaller in every way.
