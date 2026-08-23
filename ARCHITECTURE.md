# kamiroh — Architecture (Spikes 1–2)

**Status:** Accepted (spike scope)
**Date:** August 12, 2026 (spike 1); spike 2 opened August 19, 2026
**Deciders:** Casey Bowman

This document records the architecture of kamiroh across architectural spikes 1
and 2. Spike 1 (the `kamiroh-workshop-1` fork) was designed from scratch,
independent of spike 0; everything below through decision 21 is its record.

## Spike 2 (workshop-2)

This fork (`kamiroh-workshop-2`) hosts the third architectural spike —
spike 2, counting from spike 0, which preceded the workshop forks. Unlike
spike 1, it does **not** start from scratch: per
[docs/ROADMAP.md](docs/ROADMAP.md), spike 2 is hardening around a sound core,
so it inherits spike 1's architecture wholesale and this document simply
continues — same glossary, same decision log, numbering unbroken. Decisions 22
onward are spike-2 decisions.

**The spike question: can kamiroh be embedded in a host application and
tested with no human watching?** The pressure comes from automated,
container-based integration testing of embedding applications — tests that
kill containers, run unattended, and must fail fast and loudly. Candidate
implementation spikes, in priority order: timeouts, disconnect mid-exchange,
dynamic allowlist mutation, deny observability, same-endpoint fan-out, and a
BDD layer. The roadmap holds the reasoning; decisions land here as each is
actually taken.

## Intent

kamiroh combines actors at each end of an internet conversation. Actors are implemented
with [Kameo]; conversations travel over [Iroh]. Where an AI agent participates, one actor
is dedicated to that agent as its communications proxy — everything the agent says or
hears in this system flows through its actor. A conversation may run agent↔agent,
agent↔app, or app↔app; it may be a single request or a long-lived exchange; and either
end may be an application embedding part of kamiroh as a library.

[Kameo]: https://crates.io/crates/kameo
[Iroh]: https://crates.io/crates/iroh

## Shape

A **modular monolith**: one deployable unit, organized as a Cargo workspace whose crate
boundaries enforce the **ports-and-adapters** (hexagonal) structure. Dependencies point
inward only; the domain compiles with no knowledge of Kameo, Iroh, or serialization
formats.

## Glossary — the layering of terms

From the wire up. Fixing these words early is deliberate: in a
ports-and-adapters design the ubiquitous language is the architecture.

- **Connection** — infrastructure, endpoint↔endpoint. The Iroh QUIC pipe (or
  nothing at all, in the memory adapter). Owned entirely by the transport
  adapter: reconnects, multiplexing, lifetimes. The domain never says this
  word.
- **Conversation** — domain, actor↔actor. The ongoing relationship between two
  Addresses, long- or short-lived. It *spans* connections: if the wire drops
  and returns, it is the same conversation. Admission guards it delivery by
  delivery. A conversation begins implicitly with its first admitted delivery —
  there is no opening handshake in v0.
- **Protocol** — the rulebook, not an instance: a named legal sequence of
  vocabulary messages (request-ack, harness), including what opens an exchange
  and what completes one. Reusable across any conversation.
- **Exchange** — one complete run of a protocol within a conversation, from its
  opening message to the protocol's terminal state — however many round trips
  the protocol defines. Request-ack is the degenerate two-message case. A long
  conversation is a series of exchanges, one protocol after another. In v0 a
  conversation runs **one exchange at a time**.
- **Turn** — one unit of party-level messaging in the `turns` protocol: "here
  is my answer to what you asked; here is what I now ask." An exchange of
  turns alternates strictly: opened by a request-only turn, continued by
  answer+request turns, concluded by an answer-only turn. One incoming turn =
  one atomic party state change = at most one outgoing turn, emitted only
  after the state settles.
- **Party** — the app-implemented brain behind an actor: the trait an
  embedding application implements to receive turns (pushed by kamiroh).
- **Phone** — the live handle an app holds on one conversation: opening it is
  purely local; it sends turns and enforces alternation on both directions.
- **Vocabulary** — the words themselves: the closed set of message kinds from
  which protocols are built.
- **Deadline** — your own time limit on a wait: for the delivery receipt of
  a request you sent (the *ack deadline* — a closing turn poses no request
  and arms none), or for the peer's next turn (the *turn deadline*). Pure data, finite and mandatory, chosen where the
  surface is constructed; each side's deadlines bound its own waiting only
  and never cross the wire (decision 22). An elapsed deadline fails the
  exchange — never the conversation.
- **Batch receipt** — the one delivery receipt a fan-out gets (decision 29):
  an ordinary `Ack`, claimed-from the endpoint's mailroom convention,
  promising "the batch reached the endpoint" — less than the pairwise
  ack's reached-and-admitted. It settles every fanned conversation's ack
  wait at once; whether each sibling *exists and admits you* only ever
  shows as that conversation proceeding, or as silence.

## Domain model

The domain crate holds:

- **Endpoint** — an Iroh endpoint identity (a public key). The unit of transport-proven
  identity.
- **Hex** — hex-string value objects for keys and identifiers.
- **Secret** — secret-key material backing an endpoint, handled as a domain value with
  care taken not to leak it through `Debug`/logs.
- **ActorName** — a name unique *within* an endpoint.
- **Address** — the pair (Endpoint, ActorName). How one actor designates another.
- **Actor** — the domain concept of a named communicating party at an endpoint
  (distinct from Kameo's actor type, which implements it in the runtime adapter).
- **Allowlist** — per-actor inbound policy; see Trust model.
- **Conversation** — the ongoing actor↔actor relationship (see Glossary), with
  app-layer state tracking the current exchange.
- **Exchange** — one run of a protocol within a conversation (see Glossary).
- **Vocabulary** (module) — the constrained set of message kinds actors may exchange.
  Agnostic to the kind of agent (or non-agent) behind either end.
- **Protocol** — a named, legal sequence of vocabulary messages between two parties,
  each party opaque (agent or embedding app, one side or both).

## Trust model

The two halves of an Address carry different kinds of trust:

- An **Endpoint** is a public key. Iroh proves, cryptographically, which endpoint a
  connection comes from.
- An **ActorName** is *claimed* by the remote runtime, not proven. Names are addressing,
  not authentication.

Consequently the **allowlist holds endpoints only**: admitting an endpoint means
trusting that endpoint's runtime, including its honesty about which of its actors is
speaking. Allowlist semantics:

- **Deny by default** — an actor with an empty allowlist receives nothing.
- **Checked per delivery**, not only at conversation-open, so a long-lived connection
  cannot outlive a revocation.

## Vocabulary v0

A closed, compile-time set (Rust enums) shared by both ends. Wire encoding is an
adapter concern, not a domain one.

- **Request** — payload addressed to the party behind an actor.
- **Ack** — delivery acknowledgment from the remote *actor*: "the request reached the
  agent's dedicated actor and was handed over." Deliberately distinct from any future
  `Response` (the party's actual answer), so response semantics can arrive later
  without remodeling. One qualified exception: a fan-out's **batch receipt**
  (decision 29) is an `Ack` from the *endpoint*, claimed-from the mailroom
  convention, promising arrival at the endpoint only — sent before any
  actor or allowlist is consulted.

- **Response** — the party's actual answer to a Request, distinct from Ack.
- **Turn** — `Open { request }` / `Continue { response, request }` /
  `Close { response }`: the enum encodes that a turn is never empty.

Protocols in v0:

- **request-ack** — the first and simplest protocol: one Request, one Ack.
- **turns** — party-level conversation in strictly alternating Turns, tracked
  by the `TurnState` machine on both sides (decision 17). Runtimes ack a
  turn's request half on handover to the party — the fast receipt while the
  party thinks; a `Close` gets no ack in v0 (deferred reliability work).
- **harness** — a minimal lifecycle/test protocol: spawn a named actor, stop it,
  ping it. Its exchanges are command/reply pairs: `Spawn → Spawned`,
  `Stop → Stopped`, `Ping → Pong`, with `Failed` as the error reply to any
  command. It exists so integration tests can orchestrate both ends of a real
  Iroh conversation using the system's own machinery — and it doubles as proof
  that the protocol abstraction generalizes beyond request-ack. Admitting an
  endpoint to `harness` is a privileged grant; the full agent-control
  vocabulary is deliberately deferred.

## Hexagon

**Core (inside):**

- `kamiroh-domain` — the model above; pure, sync, dependency-light.
- `kamiroh-app` — application services: conversation lifecycle, routing inbound
  deliveries to the right actor, allowlist enforcement, protocol state.

**Ports (`kamiroh-ports`, their own crate):**

The app-facing boundary (the "1A boundary") is exactly two surfaces
(decision 16):

- **`Party`** (driven, push) — the trait the embedding app implements per
  actor; kamiroh drives it with incoming turns. Its signature is the
  atomicity contract (decision 17).
- **`Phone`** (driving handle, in `kamiroh-app`) — how an app opens
  conversations and sends turns; alternation-enforcing.

The kamiroh↔engine boundary (the "1B boundary") stays internal plumbing —
`Transport`, `Registry`, `Inbox`, and the runtimes' hosting contract — and
apps never see or name it:

- *Driven* — `Transport`: open/accept conversations to an Address, send/receive
  vocabulary messages. Defined by the core, implemented by adapters.
  `Registry`/`Inbox`: local actor binding and the pull surface the runtimes'
  pumps drain.

Putting the port traits in a dedicated crate means *driven* adapters depend on
`kamiroh-domain` + `kamiroh-ports` only — never on the application layer — so the
hexagon's dependency arrows are enforced by the compiler, not convention.

**Adapters (outside, named `kamiroh-adapter-*`):**

Adapters come in two kinds, and the dependency rule differs:

- *Driven* adapters are called **by** the core through ports and stay app-blind:
  - `kamiroh-adapter-iroh` — implements `Transport`/`Registry` on Iroh
    connections; owns endpoint setup, connection lifetimes (short- or
    long-lived), and the wire codec.
  - `kamiroh-adapter-memory` — an in-process `Transport`/`Registry` for tests:
    exercises the core with no network involved.
- *Driving* adapters call **into** the application — like a web framework
  hosting handlers — and so legitimately depend on `kamiroh-app`:
  - `kamiroh-adapter-kameo` — animates domain Actors as Kameo actors:
    mailboxes, supervision, the dedicated-actor-per-agent pattern, hosting the
    app layer's inbound processing and harness execution.
- Agents themselves live **outside** the hexagon, on the driving side, behind their
  dedicated actors.

## Workspace layout

```
kamiroh/                      # workspace root; root crate `kamiroh` is the facade
├── Cargo.toml                # [workspace] + the facade package
├── src/                      # facade: re-exports, wiring, prelude for embedders
└── crates/
    ├── kamiroh-domain/
    ├── kamiroh-ports/
    ├── kamiroh-app/
    ├── kamiroh-adapter-iroh/
    ├── kamiroh-adapter-kameo/
    └── kamiroh-adapter-memory/
```

The root `kamiroh` crate keeps the published name and crates.io metadata, and is what
embedding applications depend on.

## Testing strategy

- Domain and application logic: unit tests, no I/O.
- Integration: two real Iroh endpoints in one test process, orchestrated over the
  `harness` protocol — spawn an echo actor on the far side, run request-ack through
  it, stop it, assert allowlist denials for unadmitted endpoints.

## Decision log

1. **Modular monolith, Cargo workspace, ports-and-adapters.** One unit to build and
   reason about at spike scale; crate boundaries make the hexagon compiler-enforced
   rather than conventional.
2. **Allowlist checks endpoints only.** Names are unauthenticated claims; a policy
   keyed on (endpoint, name) would imply a guarantee the transport cannot provide.
3. **Deny by default; enforce per delivery.** Empty list means silence; revocation
   takes effect on live connections.
4. **Ack ≠ Response.** v0 confirms delivery to the dedicated actor only. Response
   semantics (correlation, timeouts, partials, reconnect-after-answer) are expected
   to be subtly complicated and are deferred as their own design step — last.
   (Qualified by decision 29: a fan-out's batch receipt is the one `Ack` that
   confirms arrival at the *endpoint* rather than at an actor.)
5. **Vocabulary v0 is closed.** Both ends compile against the same enums; versioning
   and cross-build evolution deferred until a second consumer exists.
6. **Minimal harness protocol now; agent control later.** Just enough lifecycle
   (spawn/stop/ping) to let tests drive both ends; the general "control a remote
   agent" vocabulary waits for a real security design.
7. **Kameo and Iroh are adapters.** The domain speaks of Actors and Conversations;
   the crates that realize them are replaceable at the edges.
8. **Ports get their own crate; adapters are named `kamiroh-adapter-*`.** A layout
   convention carried over from spike 0 at Casey's direction (structure only — no
   other spike-0 design is imported). It lets adapters compile against domain +
   ports without seeing the application layer, and makes the adapter roster legible
   at a glance.
9. **"Exchange" is a first-class term.** One complete run of a protocol within a
   conversation, spanning as many round trips as the protocol defines. It names
   where protocol state lives: the app layer tracks, per conversation, the
   current exchange and its progress.
10. **One exchange at a time per conversation (v0).** Strictly sequential;
    interleaved concurrent exchanges drag in correlation machinery that belongs
    with the deferred response-semantics work.
11. **No opening handshake (v0).** Admission is checked per delivery, so a
    handshake adds no security; a conversation begins implicitly with its first
    admitted delivery. A hello/capability protocol can slot in later as just
    another protocol if wanted.
12. **Local actor binding is a port.** The runtime asks the transport to bind an
    Address and receives that actor's Inbox; dropping the Inbox unbinds. The
    memory net implements it as registration; the Iroh adapter will implement it
    as routing inside the endpoint.
13. **Driving adapters may depend on the app layer; driven adapters may not.**
    Refines decision 8, which was written with driven adapters in mind. The
    Kameo runtime is a driving adapter — its whole job is hosting application
    behavior (inbound processing, harness execution) inside real actors — so it
    depends on `kamiroh-app`, exactly as a web framework depends on the handlers
    it hosts. Transport adapters remain app-blind.
14. **Dependencies are vendored.** The cloud workspace cannot reach crates.io,
    so `cargo vendor` output and `.cargo/config.toml` are committed once heavy
    deps (kameo, tokio, iroh) land. Cost: vendored source in the fork's history.
    Benefit: hermetic offline builds everywhere, cloud included.
15. **Driven-port futures are `Send`.** `Transport::send` and `Inbox::next`
    return `impl Future + Send`, stated explicitly in the trait (RPITIT with
    a bound) rather than via `async fn`. Surfaced by the first multi-threaded
    consumer (the Kameo runtime, whose engine requires `Send` handler
    futures) — but adopted as a fact about the system, not a kameo
    accommodation: these ports exist to be crossed by threads. The former
    `#![allow(async_fn_in_trait)]` "spike scope" shortcut is retired. A
    `?Send`/single-threaded variant is deliberately not provided until a
    single-threaded embedder exists to justify it. (Full deliberation:
    `docs/advisories/2026-08-12-kameo-ports-send-*.md`.)
16. **The app boundary is two surfaces: Party and Phone.** An embedding app
    implements `Party` (one per actor — the brain behind it, driven by
    kamiroh, push not pull) and holds `Phone`s (the driving handle: open a
    conversation locally, send turns). The kamiroh↔engine boundary stays
    internal plumbing apps never see. Opening a conversation remains
    handshake-free (decision 11): constructing a Phone is a local act.
17. **Turns are the unit of party-level messaging; exchanges alternate
    strictly.** A turn couples "answer to your outstanding request" with
    "optionally, my next request"; the `Turn` enum (Open/Continue/Close)
    makes an empty turn unrepresentable. One incoming turn = one atomic
    party state change (enforced by `Party::on_turn(&mut self, …)` and
    per-actor mailbox serialization) = at most one outgoing turn, sent only
    after the handler returns. Strict alternation (the `TurnState` machine,
    held by both sides and enforced by runtimes and Phones) collapses
    response correlation: exactly one request is outstanding per exchange, so
    `RequestId` is audit/timeout material, not disambiguation. The delivery
    `Ack` stays distinct (decision 4): runtimes ack a turn's request half on
    handover, before the party thinks. Deferred: acks for `Close` turns,
    timeouts, disconnect mid-exchange, streaming/partial responses.
18. **Wire encoding is postcard over a feature-gated serde.** The domain
    stays dependency-free by default; the `serde` feature adds derives to the
    vocabulary (with `ActorName` deserializing through its validating
    constructor). Wire adapters enable the feature and choose the format —
    the Iroh adapter uses postcard (compact, serde-native). Format choice
    stays adapter-local; nothing outside an adapter may depend on it.
19. **Iroh adapter v0: static peer book, one frame per uni-stream, origin
    from the connection.** Endpoint-id→address resolution is an explicit
    peer book (`add_peer`), per the deferred-discovery decision. Each message
    travels as one length-delimited postcard frame `{from_name, to_name,
    message}` on a fresh uni-stream over a cached per-peer connection (one
    retry on stale connections); the ALPN is `kamiroh/1` (bumped from
    `/0` when decision 29 changed the frame layout — an incompatible
    wire change bumps the version, so mismatched builds refuse each
    other at the handshake rather than silently dropping frames; ruled
    by Casey at the fan-out review). The receiving
    adapter constructs `Delivery::from` with the endpoint taken from the
    connection's authenticated remote key — never from frame content — and
    only the name halves ride in the frame. Relays and discovery are
    disabled in tests (loopback direct addresses); production relay policy
    is deferred.
20. **Vendored sources live on an artifact branch; publication is a history
    boundary.** Refines decision 14 after the iroh tree took `vendor/` to
    ~559 MB: committed blobs ride ancestry-preserving merges forever, so
    `master` now gitignores `vendor/` and `.cargo/`; the orphan
    `vendor-snapshot` branch (force-pushed, merged into nothing) carries
    them for the cloud session's hermetic builds. The workshop's existing
    vendor history stays its private cost: graduation to staging publishes
    a fresh vendor-free snapshot branch — a deliberate content-not-ancestry
    boundary, carved out from the cross-tier merge-commit rule, justified
    because a workshop fork is archival once its spike graduates. Within
    staging and staging→main, ancestry-preserving merges remain mandatory.
    (Full guide: `docs/VENDORING.md`.)

21. **Internet-facing operation uses n0's public infrastructure; hermetic
    stays the default.** `NetProfile::Hermetic` (relay-less, lookup-less,
    static peer book) remains what tests and closed deployments get, and is
    the default. `NetProfile::N0` (`presets::N0`) turns on n0's relay fleet
    and address publishing/lookup: peers dial by endpoint id alone, and NAT
    traversal — hole-punching with relay fallback — is Iroh's job, exactly
    as designed. Consequences owned: an N0 endpoint publishes a signed
    address record to n0's public lookup service, and internet operation
    depends on n0's infrastructure (self-hosted relays deferred until
    wanted). The relay-less apparatus (fixed-port binding, leased
    port-forwards via scripts/internet-check-serve.sh) remains in-tree as
    the documented fallback for single-NAT setups an operator controls —
    and as the boundary marker of what relay-less operation cannot reach:
    multi-NAT and CGNAT'd hosts, which need N0.

22. **Deadlines are finite, mandatory, per-conversation, and each side's own.**
    In plain terms: you choose your own time limits when you create a Phone,
    install an actor, or construct a runtime (whose `spawn_deadlines` cover
    everything its harness spawns); there is no way to say "wait forever";
    and the other side never sees your limits — it has its own.
    (Spike 2; example-mapping session of 2026-08-19/20, with Casey ruling.)
    Two deadline kinds match the two waits decision 4 made distinct: the *ack
    deadline* (delivery-scale) bounds waiting for the receipt of a sent
    request half; the *turn deadline* (party-scale) bounds waiting for the
    peer's next turn. `Deadlines` is a required argument — no `Default`, no
    `Option`, no unbounded variant — at each side's construction site:
    `Phone::converse` for initiators (per conversation) and actor install for
    responders (per actor; decision 11 leaves a responder no earlier moment).
    Runtimes take `spawn_deadlines` for harness-spawned actors. **Either
    deadline's expiry fails the exchange, and failure is a local judgment**:
    an unconfirmed delivery is operationally indistinguishable from a failed
    one, and no failure message crosses the wire — each side reaches the
    terminal state on its own evidence, so the two sides may fail the same
    exchange at different moments. Accepted cost: a lost ack with a live peer
    kills the exchange and the peer's genuine answer is later refused as a
    late turn — tune the ack deadline, don't soften the semantics. Considered
    and declined: a *double-ack* (acking the ack) to spare a peer wasted
    thinking — the Two Generals ladder; the secondary ack is lost on the same
    channel and no finite rung count reaches common knowledge. Deferred per
    the house rule: per-exchange deadline overrides at `open()`, and any
    library-shipped default constants (the harness example's numbers are that
    example's own explicit choice). The library grew no defaults on purpose.
23. **The turn machine knows about death.** (Spike 2.) `TurnState` gains a
    `Failed` state, entered by `fail()` when the outside world feeds in a
    verdict (deadline elapsed; later, peer vanished or revoked). From `Failed` the
    machine refuses every turn of the dead exchange by itself —
    `TurnError::ExchangeFailed` — so a dead exchange is unrepresentable to
    talk to, in decision 17's make-illegal-states-unrepresentable style; the
    rule lives in one place instead of as a note every runtime must remember
    to check. A fresh `Open`, ours or theirs, is legal from `Failed`: failure
    is terminal for the *exchange*, never the conversation, and stragglers
    from the dead exchange then fall to id mismatch. The failure *cause* does
    not split the state (the machine refuses identically either way); cause
    detail travels on decision 25's event surface. The machine still never
    reads a clock: time stays outside, only verdicts come in.
24. **Time is I/O: a first-class timer port.** (Spike 2.) `kamiroh-ports`
    gains `Timer` — an associated `Instant`, `now()`, and a
    `sleep_until`-style future (`Send`, per decision 15); deliberately a
    timer, not a pollable clock-reader. The deciding argument over bare
    verdict injection: the timer *choreography* — arm on send, cancel on
    ack/turn arrival, re-arm per round, tear down on close or failure — is
    the bug-prone part, and the port pulls it into shared app-layer code
    (`Phone` wait tracking, `wait::next_or_deadline`, the runtimes' step/task
    arming) tested deterministically, the same code in tests and production,
    instead of being duplicated untested per adapter. Implementations:
    `TestTimer` (memory adapter's `testing`; a virtual clock advanced by
    hand — sixty-second deadlines elapse in microseconds, and an elapsed
    deadline deterministically outranks a queued delivery) and `TokioTimer`
    (kameo adapter; the Kameo hosts arm real tokio sleep-tasks, aborted on
    settle, with stale reports recognized and ignored). The domain stays
    clock-free (decision 23); `Deadlines` stays data (decision 22).
25. **Failure is told twice; silence is observable — never on the wire,
    always local.** (Spike 2; delivers roadmap item 4 alongside item 1.) The
    affected party learns its exchange died through
    `Party::on_exchange_failed` (default no-op, so existing parties compile;
    application semantics — the party must clean up, abandon work, perhaps
    reopen). Observers — unattended tests, operators — get a single local
    event surface: runtimes emit `Event::ExchangeFailed`,
    `Event::DeliveryDenied`, and `Event::IllegalTurnDropped` into an
    `Observer` (a trait; any `FnMut(Event) + Send` qualifies) so the three
    things the library used to swallow become positively assertable, instead
    of "wait and see that nothing happened". Denials and dropped illegal
    turns reach the event surface *only*, never a party: denied traffic is
    pre-admission by definition, and routing it into application code would
    invite exactly the behavior changes the roadmap forbids. The wire stays
    as mute as deny-by-default demands — no nack, no error, no hint to the
    sender.

26. **Positive local evidence fails an exchange immediately; the deadline is
    only the backstop for silence.** (Spike 2; from Ander's independent
    review of the decision 22–25 implementation, remedies ruled by Casey,
    2026-08-20.) Two paths join the elapsed deadline as exchange killers,
    uniformly at every send site: a **refused send** of a turn
    (`FailureCause::SendFailed` — the transport's error proves the turn
    never left, so nothing waits out a deadline the transport already
    answered; this also closes the reviewer-demonstrated wedge where a
    Phone's failed send advanced the machine but armed no wait, leaving an
    exchange that could neither time out nor reopen), and an **illegal
    reply from the local party** (`FailureCause::IllegalReply` — the reply
    is dropped, and the exchange fails loudly rather than lingering
    un-deadlined behind a buggy party). Both are told twice per decision
    25. Recorded alongside, from the same review: the Kameo runtime's
    deadline clock judges **processing time, not arrival time** — a turn
    that arrives in time but queues behind a slow party is judged late,
    and one slow peer can time out an actor's other conversations.
    Accepted and documented for spike 2 (the cause is long handler
    occupancy under per-actor serialization, decision 17); **mandatory
    revisit in the post-streaming architectural spike**, since streaming
    moves slow thinking out of `on_turn` and should starve the problem of
    its cause — measure what gap remains, and only then consider arrival
    timestamps. (The review also hardened decision 24's Kameo timer tasks
    with an arming epoch, closing a demonstrated stale-report race.)
    Second-pass addendum, same review: **the observer is called under the
    runtime's lock, and observers must not re-enter the runtime.** The
    alternative — releasing the lock around the call — was tried and
    demonstrated to silently drop concurrent events, which betrays
    decision 25 outright; a documented non-reentrancy rule whose violation
    deadlocks loudly beats load-dependent silent loss, and re-entrant
    observers are a capability nothing needs today (the house rule
    applies). Pinned by the review's `observer_concurrency` reproducer.
    External-review addendum (2026-08-23, ruled by Casey): a refused
    **handover-ack** send is deliberately *ignored* in both runtimes —
    the ack is not a turn, the arriving turn already proved delivery to
    the peer's benefit (a turn supersedes a missing ack), a transient
    blip then recovers gracefully, and a genuinely broken transport
    still fails loudly at the reply send through this decision's own
    machinery. Previously `LocalRuntime` aborted the step after the
    machine had moved, leaving the exchange un-deadlined with the party
    never called — a silent wedge in the documented reference runtime,
    while Kameo behaved correctly; the fix makes both tell one story,
    pinned by `a_refused_handover_ack_does_not_wedge_the_exchange` and
    `a_blipped_ack_alone_interrupts_nothing`. From the same review
    round: the stock parties now `fail()` their shadow turn machines in
    `on_exchange_failed` (a party that tracks its own machine must also
    fail it, or a surviving fresh `Open` is swallowed as
    `MustAnswerFirst` — the examples embedders copy must model the
    discipline), and the memory adapter's `open_many` refuses a batch
    to an endpoint with no mailboxes at all, keeping the simulator no
    more generous than the wire on decision 29's receipt promise.
    Ruled at that fix's review (Casey): in `open_many`,
    endpoint-nonexistence outranks a partition — the batch errors
    rather than being silently held — because the wire's own equivalent
    (the peer-book lookup) fails locally before any packet moves, and a
    partition cannot hide it; it also matches `kill_endpoint` treating
    death as positive evidence.

27. **Disconnects: transports report what they know; positive evidence
    fails live exchanges at once; conversations survive.** (Spike 2; rules
    R6–R8 and rulings Q5/Q6 of the mapping session, board archived at
    `docs/mappings/2026-08-19-timeouts-disconnects/`.) A new driven port,
    `DeathWatch`, carries the transport's positive evidence that a peer
    endpoint is gone; runtimes race it alongside deadlines and deliveries
    (deaths outrank an elapsed deadline, which outranks a queued delivery),
    and a reported death fails the affected **live exchanges only**
    (`FailureCause::Disconnected`, through the same `fail_exchange` seam as
    every other verdict — no choreography changes were needed, confirming
    the verdict shape). The conversation survives, as the glossary demands.
    What counts as evidence is the transport's business: the iroh adapter
    subscribes to every connection's `closed()` — deliberately excluding
    `LocallyClosed` (hanging up is not the peer dying) and **suppressing
    reports superseded by a newer connection to the same peer** (the
    reviewer's pre-flight hazard: a death report racing the reconnect this
    same decision promises; the transport is the one layer where connection
    generations are visible, so the guard lives there). The memory adapter
    is a fault simulator per the Q6 ruling, in the adapter proper:
    `kill_endpoint` (mailboxes vanish, senders get errors, watches report —
    positive evidence), and `partition`/`heal` (deliveries *held* and
    flushed in order, modeling QUIC retransmission across a blip — silence,
    never evidence; the deadline backstop's job). The Q5 ruling is pinned
    by test over real QUIC: a conversation spans connections, and a fresh
    inbound connection routes to the same actors. Silence semantics
    unchanged: a partition that never heals is caught by decision 22's
    deadlines. **Accepted residue, mandatory revisit:** in the Kameo
    runtime a death report fans out as a mailbox message, so a
    kill-and-resurrect race inside that fan-out latency (milliseconds)
    could stale-kill a fresh exchange; the toy runtime is immune by
    construction (deaths are checked before every delivery, and hearing
    from an endpoint clears its tombstone). Closing it fully needs
    connection generations carried through the delivery path — deferred,
    and **re-judged in the post-streaming architectural spike together
    with decision 26's processing-time revisit**: both are messages about
    the world processed after the world moved on, and streaming shrinks
    exactly the mailbox latency they live in.

28. **Allowlists mutate on running actors, through a local lever only;
    revocation is positive evidence.** (Spike 2; rules R1–R5 and rulings
    Q1–Q3 of the mapping session, board archived at
    `docs/mappings/2026-08-23-allowlist-mutation/`.) The domain always
    knew how (`admit`/`revoke`, enforcement on every delivery — decision
    3); what was missing was any path to a running actor's list, so the
    per-delivery promise was kept but unreachable (the roadmap-3 gap).
    Now each runtime exposes **`admit(actor, endpoint)` /
    `revoke(actor, endpoint)`** — callable only by the process that owns
    the runtime. **No wire form in this spike** (Q1): mutation over the
    network — and with it, who may aim it at whom — is deferred to the
    next architectural spike alongside the operator port, deliberately:
    the repair channel for a stolen privileged key must not be the
    channel the stolen key controls. The unit of mutation is the actor,
    the unit of trust (R1); a revocation bites on the very next delivery
    even mid-conversation (R2); revoking the last endpoint restores
    deny-by-default silence (R3); both calls are idempotent (R5). A
    revocation is **positive local evidence** (decision 26's principle):
    the actor's live exchanges with the revoked endpoint fail at once
    (`FailureCause::Revoked`, through the same `fail_exchange` seam),
    while the conversation survives — a fresh exchange is legal on
    re-admission, mirroring decision 27's shape (Q2). No policy-change
    events (Q3): the local caller knows what it did, the effects are
    already observable twice over (`ExchangeFailed` at once,
    `DeliveryDenied` thereafter), and the `Event` enum stays uniformly
    failure-shaped until remote mutation makes audit events load-bearing.
    **Mutation never touches connections** (R4): admission is the
    actor's policy, the pipe is the node's plumbing, and auto-closing
    unadmitted connections was considered and rejected — it would leak a
    deliberately silent policy change onto the wire (decision 25's
    never-on-wire rule) and couple teardown to a transient census. The
    deliberate eviction of a compromised peer is a documented two-step
    recipe instead (revoke everywhere, then `close_connection`), in
    `docs/EMBEDDING.md`. From the pre-flight hazards note (Ander), folded
    in at build time: the runtime's lever cannot reach app-held `Phone`s,
    which guard inbound traffic with the application's own allowlist — so
    the Phone gets **`on_revoked`**, mirroring decision 27's
    `on_peer_death`, and the eviction recipe names the app side
    explicitly; the revoke sweep pre-filters on live turn-state (as the
    death sweeps do), which is what keeps idempotence true *at the
    observer* — a second revocation fails nothing and emits nothing — and
    is pinned by test on both runtimes; and `FailureCause` gaining
    `Revoked` is a breaking change for exhaustive matchers, of which the
    workspace has exactly one (its own `Display`) and downstream has none
    (no `#[non_exhaustive]`, deliberately — the compiler pointing at
    matchers is the feature).

29. **Fan-out v1: one send opens many conversations at one endpoint —
    and is over the moment it happens.** (Spike 2; rules R1–R5 and
    rulings Q1–Q4 of the mapping session, board archived at
    `docs/mappings/2026-08-23-fanout/`.) One send carries one opening
    request to N *named* actors at ONE endpoint; what comes back is N
    ordinary pairwise conversations that share nothing but their birth.
    The constraint is **same-endpoint, enforced by the call's type** (one
    endpoint parameter, a list of names — a mixed batch is
    unrepresentable, and no kinship concept like supervisor-and-children
    enters the model, because the transport could not verify one). The
    **batch receipt is a single ordinary `Ack`, sent at endpoint arrival,
    before admission** (Q1): it promises "the batch reached the
    endpoint," deliberately less than the pairwise ack's
    "reached-and-admitted" — a denied sibling therefore surfaces at the
    turn deadline rather than the ack deadline, and denial remains
    exactly what it always was on the wire: silence. (An ack listing
    admissions was rejected outright — admission decisions never cross
    the wire, decision 25.) All N conversations share the **one request
    id** (Q4) — legal, since ids are unique per conversation — which is
    what lets that single receipt settle every initiator-side ack wait as
    plain `Ack{id}` with no new vocabulary. Only **opening turns** fan
    out (Q2): a `Continue` belongs to one exchange by construction, and
    harness fan-out (ping-all, stop-all) is deferred as a definite later
    item, in Casey's words "address the rest sometime later for sure."
    The initiator-side surface is a **helper returning N ordinary
    Phones** (Q3): the thing that fanned is gone before the first reply
    arrives, so "fan-out exists only at the opening instant" (R2) is
    structural, not disciplinary; a conference abstraction, if one is
    ever wanted, builds on top without touching the protocol. Admission
    stays per-actor with denials observable at the receiving home (R3);
    no wildcard — the sender names every recipient, because
    "all actors here" would let any admitted peer enumerate the roster
    (R4); and the batch is a transport affair — adapters fan the frame
    into N ordinary `Delivery` values before admission, leaving the
    domain untouched (R5). Settled at build time, from the pre-flight
    hazards note (Ander) and two further rulings (Casey): the receipt's
    claimed sender is the **mailroom convention**
    (`kamiroh_ports::BATCH_RECEIPT_NAME`) — deliberately NOT a reserved
    namespace, with the collision accepted out loud: names are claims,
    never proof (decision 2), and an admitted endpoint could already
    forge a plain `Ack` under any name, so a convention-named receipt
    adds no capability anyone lacks. **Unbound names in a batch disclose
    nothing** — they drop silently (as the iroh adapter already did
    pairwise; a per-name error would be a roster oracle, R4's leak
    through a different door), and the receipt is sent even when every
    name is unbound: it promises arrival, and the names were never part
    of the promise. **Neither runtime changed** — fan-in happens below
    them, which is R5 doing its job, and it carries two consequences
    stated plainly: a third runtime owes nothing new for fan-out (the
    EMBEDDING obligations list stays at five), and admitted siblings
    still ack pairwise as any responder does — those later acks settle
    nothing (`note_ack` is idempotent) and are accepted residue, since
    eliminating them would need runtimes to know about batches. This
    decision **qualifies decision 4 by name**: the batch receipt is the
    one `Ack` confirming arrival at an endpoint rather than at an actor.
    Documented residue of the Q1 trade: a denied, absent, or duplicate
    sibling is silence that surfaces at the *turn* deadline — the fanned
    conversation gave up its short-deadline safety net, and a tighter
    turn deadline is the natural compensation, said where the recipe
    lives (`docs/EMBEDDING.md`).

## Deferred

- Response semantics (the subtle part, saved for last — see decision 4).
- Agent-control vocabulary beyond the test harness.
- Allowlist mutation over the wire, and who may aim it at whom (decision
  28 keeps the lever local; the wire form travels with the operator-port
  design).
- Harness fan-out (ping-all, stop-all) — a definite later item per the
  fan-out session's Q2 ruling, once decision 29's opening-turn machinery
  exists to carry it.
- Cross-endpoint fan-out, the genuinely hard version (decision 29 is
  same-endpoint only, on purpose).
- Discovery: how initiators learn Addresses (static configuration for the spike).
- Name authentication within an endpoint, if it is ever wanted.
- Vocabulary versioning across differing builds.
- Wire format selection (serde-compatible; chosen inside the transport adapter).
