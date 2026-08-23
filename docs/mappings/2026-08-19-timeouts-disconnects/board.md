<!-- Archived per docs/mappings/README.md: the final state of this
meeting's board, rulings inline as they were made (2026-08-19/20).
Historical record — later changes of direction belong in the decision
log, not here. -->

# Example-mapping board: timeouts & disconnects

*Prepared by Mez, 2026-08-19, for workshop-2's first design conversation —
per the spike-1 session's recommendation that the two hardest roadmap items
be mapped together, with the outputs being both the design decisions and the
first Gherkin feature files. This board is the **seed**, not the session:
green and blue cards below are candidates grounded in the current code, to
be accepted, amended, or torn up together. Red cards are Casey's to
arbitrate. Glossary words are used strictly (ARCHITECTURE.md).*

## Why these two map as one conversation

Both ask the same question — **what does an exchange do when the other side
goes quiet?** — with different evidence. A timeout is *absence* of evidence
(a deadline passed in silence); a disconnect is *positive* evidence (a send
failed, a connection closed). Designed separately they produce two
half-answers that meet awkwardly in the middle, because every disconnect
eventually presents as a timeout to anyone not listening for it.

## What the code says today (facts, not cards)

- **No time exists anywhere in the library.** Domain and app layers have no
  clock, no deadline, no timer; the famous 15-second deadline lives in the
  `harness_ping` example binary only.
- **`TurnState` has no failure state.** `Idle` / `AwaitingTheirTurn` /
  `OweThem` — a hung exchange is just `AwaitingTheirTurn` forever. The
  machine cannot distinguish *slow* from *dead*, and nothing downstream can
  either.
- **There are two distinct waits**, because Ack ≠ Response (decision 4):
  waiting for the **ack** of a turn's request half (delivery receipt — fast,
  transport-scale), and waiting for the peer's **next turn** (party thinking
  time — slow, potentially agent-scale). One deadline cannot serve both.
- **`Party` has no failure channel.** `on_turn` is the trait's only method;
  a responder party whose exchange dies can never find out.
- **`Phone` is send-only.** It validates incoming turns when the app pump
  feeds them, but owns no receive path, so it has nowhere to *wait* — and
  nowhere to time out.
- **The memory adapter has no concept of connection or failure injection.**
  Killing an endpoint can only be simulated by dropping an inbox, which the
  *sender* notices (`UnknownAddress`) but a silently *waiting* peer never
  does. Deterministic disconnect scenarios need a fault surface it lacks.
- **The iroh adapter treats connections as cache entries** (one retry on
  stale). QUIC surfaces close/idle events; nobody listens to them.
- **The glossary already promises**: a conversation *spans* connections —
  "if the wire drops and returns, it is the same conversation." Any
  disconnect design that kills conversations on wire drop breaks the
  glossary.

## Story 1 (yellow): A hung exchange fails loudly

> As an application embedding kamiroh in unattended, container-based tests,
> I need an exchange whose peer has gone silent to become a loud, prompt,
> assertable failure — never a hang.

**Rule candidates (blue):**

- R1. Every exchange runs under a **deadline**; expiry fails the exchange —
  loudly, on the side that was waiting.
- R2. **Two deadline kinds**, matching the two waits: an *ack deadline*
  (short; delivery-scale) and a *turn deadline* (long; party-scale),
  independently configurable.
- R2a. *(ruled in session, 2026-08-20)* **Either deadline's expiry fails the
  exchange, and failure is a local judgment.** An elapsed ack deadline is a
  full exchange failure, not a warning — an unconfirmed delivery is
  operationally indistinguishable from a failed one, and the turn deadline
  is the wrong (party-scale) tripwire for a delivery-scale problem. No
  failure message crosses the wire in v0: each side reaches the terminal
  state on its own evidence, its own deadline. Accepted cost: a lost ack
  with a live peer kills the exchange, and the peer's genuine answer is
  later refused as a late turn — tune the ack deadline, don't soften the
  semantics.
  - *Considered and declined (session, 2026-08-20): a **double-ack** (a
    secondary ack for each primary ack), proposed to spare a peer from
    thinking on behalf of an exchange the sender has already failed. This
    is the Two Generals ladder: the secondary ack is lost on the same
    channel, killing healthy exchanges over a lost receipt-of-a-receipt,
    and no finite rung count reaches common knowledge. The wasted-work
    concern is met instead by (a) R6 — the common loss mode is a dead
    connection, which positive evidence surfaces fast on both sides; (b) an
    adapter-level ack retry, if the rare lost-ack-on-live-connection case
    ever shows up in practice; and (c) explicit cancellation ("stop") in
    the workshop-3 agent-control vocabulary — intent, not inference, is
    the right tool for calling off work. Consistent with decision 4's
    deliberately thin Ack (we do not even ack `Close` turns in v0).*
- R3. **Deadlines are data; expiry is an event.** The domain stays pure —
  it learns "the deadline elapsed" as an injected fact and answers with a
  transition, but never reads a clock. Clocks live in runtimes/adapters.
- R4. A failed exchange **tells both local surfaces**: the initiating app
  (via its Phone) and a responding party (via a new, default-implemented
  notification on `Party`) — no one learns of the death by hanging.
- R5. **Failure is a terminal state of the exchange, not the conversation.**
  The conversation survives and may open a fresh exchange. (Late replies to
  a failed exchange are dropped as protocol violations — the alternation
  machine already refuses them once state has moved on.)

**Example candidates (green — future Gherkin, glossary-worded):**

```gherkin
Scenario: A peer that never answers
  Given an exchange between two parties with a turn deadline
  And the exchange is awaiting the peer's turn
  When the deadline elapses with no turn arriving
  Then the exchange fails with a timeout
  And the waiting party is told the exchange failed
  And the conversation is idle and may open a new exchange

Scenario: A slow but timely answer
  Given an exchange with a turn deadline
  When the peer's turn arrives before the deadline elapses
  Then the exchange continues as if no deadline existed

Scenario: An ack that never comes
  Given a sent turn awaiting its delivery ack, with an ack deadline
  When the ack deadline elapses first
  Then the exchange fails with a timeout
  And the sender's party is told the exchange failed

Scenario: The two sides converge on failure separately
  Given an exchange whose sender has failed it on an elapsed ack deadline
  And a peer that believes the exchange is alive
  When the peer's own turn deadline elapses with no further turn arriving
  Then the peer's side of the exchange fails with a timeout
  And no failure message has crossed the wire

Scenario: A late turn after a failed exchange
  Given an exchange that already failed by timeout
  When the peer's answer finally arrives
  Then it is refused as no part of any exchange
```

**Question cards (red — Casey arbitrates):**

- Q1. **Where are deadlines configured?** Per-Phone / per-actor install /
  per-exchange override / per-protocol defaults? And is there a *default*
  deadline at all, or explicit-only — is an unbounded wait ever a
  legitimate thing to embed, in a spike whose question is unattended
  operation?
  - *Half ruled (session, 2026-08-20): deadlines are **finite and
    mandatory** — no unbounded waits, no opt-out.*
  - *Siting ruled (session, 2026-08-20, as recommended): per-conversation, fixed
    at each side's existing construction site, required in the signature —
    `Phone::converse` gains a mandatory `Deadlines { ack, turn }` (no
    `Default`, no `Option`; the compiler enforces the ruling), and actor
    install gains the same, applying to all conversations the actor
    responds in. The granularity asymmetry falls out of decision 11 (a
    responder has no earlier moment than install), and install is already
    where the allowlist lives — policy travels with policy, each side's
    deadlines its own local judgment (R2a). Rejected: protocol-level
    defaults (the rulebook cannot know the deployment — the same protocol
    wants milliseconds on loopback and minutes before an agent).
    Deferred per the house rule: per-exchange overrides at `open()` (a
    backward-compatible widening when an afternoon demands it) and any
    library-shipped default constants (the harness example's 15s becomes
    that example's own explicit choice). `Deadlines` is a domain value
    (R3); the transport idle-timeout stays adapter configuration.
    Understanding pinned in session: two construction sites for two roles —
    install sets responder patience per-actor, `Phone::converse` sets
    initiator patience per-conversation; an actor doing both uses both.
    Each side's deadlines bound only its own waiting; nothing crosses the
    wire; asymmetric patience between peers is legal. Corollary: a party's
    thinking time is judged by the *other* side's unseen clock — embedders
    should be conservative about their own slowness until workshop-3's
    streaming gives slow producers a way to never be silent.*
- Q2. **What is "loud" for a responder?** A new `Party` method (default
  no-op?), or a shared observable-event surface — which would also be the
  natural home for deny observability (roadmap item 4), two birds?
  - *Ruled (session, 2026-08-20): both surfaces, with distinct jobs — and
    one shared invariant: **never on the wire, always local.***
    - *A single local **event surface** (observer hook / event stream the
      runtime emits into) carries the three events the library knows but
      currently swallows: `ExchangeFailed`, `DeliveryDenied`,
      `IllegalTurnDropped`. It serves observers — unattended tests assert
      positively instead of waiting out "nothing happened" windows;
      operators get visibility. One mechanism covers roadmap items 1, 2,
      and 4.*
    - *Exchange failure is additionally told to the affected **party**
      via a new default-no-op `Party` method — the party must act on it
      (clean up, abandon work, maybe reopen). Application semantics, not
      observability.*
    - *Denials and dropped illegal turns reach the event surface **only**,
      never a party: denied traffic is pre-admission by definition, and
      routing it into application code invites the behavior changes the
      roadmap forbids. The wire stays exactly as mute as deny-by-default
      demands — no nack, no error, no hint to the sender.*
- Q3. **Does `TurnState` itself gain failure transitions** (domain-level
  terminal state), or does failure live in app-layer conversation state
  *above* the pure machine? R3 permits either; they test differently.
  - *Ruled (session, 2026-08-20): the machine learns about death.*
    `TurnState` gains a `Failed` state, entered when the outside world
    feeds in a verdict (deadline elapsed, peer gone — R3: time stays
    outside, only verdicts come in). From `Failed` the machine refuses
    every turn of the dead exchange by itself — one home for the rule, no
    note to forget to check, pure unit-testable. Precedent: decision 17's
    make-illegal-states-unrepresentable style. Implementation notes
    settled in passing: a fresh `Open` is legal from `Failed` (R5 — the
    conversation survives; the new exchange's state then judges any
    stragglers, which fall to id mismatch); the failure *cause* need not
    split the state — the machine refuses identically either way, and
    cause detail travels on the Q2 event surface.*
- Q4. **Clock mechanics:** tokio time in the real runtimes plus manual
  expiry injection in the memory world, or a first-class `Clock` port?
  - *Ruled (session, 2026-08-20): a first-class **timer port** — time is
    I/O, and goes through a port like everything else. Shaped as a timer,
    not a clock-reader: the app layer needs "wake me at this deadline" (a
    `sleep_until`-style future, decision-15 `Send` bound), not a pollable
    `now()`. Lives in `kamiroh-ports`, consumed by `kamiroh-app`; the
    Kameo/iroh runtimes supply a tokio-backed implementation, tests a
    hand-advanced one (`advance(61s)` fires a 60s deadline instantly — no
    real sleeping, no flaky windows). The deciding argument over bare
    verdict injection: the timer **choreography** (arm on send, cancel on
    ack/turn arrival, re-arm per round, tear down on close/failure) is the
    bug-prone part, and the port pulls it into shared app-layer code
    tested deterministically — the same code in tests and production —
    instead of being duplicated untested per adapter. R3 intact: domain
    stays clock-free, `Deadlines` are data, `TurnState` eats only
    verdicts. Bonus: the Gherkin step "when the deadline elapses" binds to
    one line of test-clock advancement.*

## Story 2 (yellow): A vanished peer fails loudly

> As the same embedding application, when my peer's container is killed
> mid-exchange, I need the failure surfaced in seconds (positive evidence),
> not after a long deadline expires — while an ordinary wire blip must not
> kill anything the glossary promised would survive it.

**Rule candidates (blue):**

- R6. **Positive evidence accelerates; the deadline is the backstop.** A
  failed send or a closed-connection event fails the current exchange
  immediately; silence is failed only by R1's deadline. Nothing waits out a
  long deadline when the transport already knows.
- R7. **Disconnect kills exchanges, never conversations.** The wire
  dropping and returning is the same conversation (glossary); losing the
  *exchange* on positive death evidence is the pragmatic v0 line, and
  resuming an in-flight exchange across a reconnect is explicitly out of
  scope for workshop-2.
- R8. **Transports must report what they know.** The `Transport` port grows
  a way for connection-level death to reach the app layer as an event —
  the iroh adapter maps QUIC close/idle onto it; the memory adapter gains a
  fault-injection surface (kill endpoint / partition) so unattended tests
  can *cause* what containers cause.
  - *Verified against vendored iroh + our adapter (session, 2026-08-20):
    nature gives three tiers — a graceful `CONNECTION_CLOSE` is active and
    immediate (iroh exposes an awaitable `Connection::closed()`); a silent
    death is detected, not notified — keep-alive heartbeats every 5s and a
    default 30s idle timeout bound it (a timeout at a lower layer: a dead
    peer emits nothing, so probe-and-bound is the only physics available);
    and a send onto a dead connection fails promptly. There is no
    cross-notification: each surviving side learns locally, on its own
    timer — the transport layer independently enforces R2a's
    local-judgment principle. Today our adapter discards all three tiers:
    `is_closed()` is checked only lazily before a send, the read loop
    exits silently on close, and `closed()` resolves unobserved — a purely
    waiting party learns nothing, ever. R8's concrete job: watch
    `closed()` per cached connection and fail that connection's in-flight
    exchanges (R6), conversation surviving (R7). The idle-timeout knob
    joins Q1's configuration story.*

**Example candidates (green):**

```gherkin
Scenario: The peer endpoint dies mid-exchange
  Given an exchange awaiting the peer's turn
  When the peer's endpoint is killed
  And the transport reports the conversation's connection dead
  Then the exchange fails at once, well before any deadline
  And the waiting party is told the exchange failed

Scenario: A wire blip is not a death
  Given an exchange awaiting the peer's turn
  When the connection drops and is re-established
  And the peer's turn arrives within the deadline
  Then the exchange continues in the same conversation

Scenario: A silent death is caught by the backstop
  Given an exchange awaiting the peer's turn
  And the transport observes nothing unusual
  When the peer's process is frozen rather than killed
  Then the turn deadline elapses and the exchange fails with a timeout
```

**Question cards (red):**

- Q5. **How does the wire-blip scenario square with the iroh adapter's
  one-frame-per-stream design?** (Likely cleanly — streams are per-message —
  but the reconnect path is untested and the "same conversation" promise is
  currently pinned by prose, not by a test.)
  - *Ruled (session, 2026-08-20): **pin it by test in this spike.** Q5 is a
    commission, not a debate: nothing about a conversation lives inside a
    connection (identity is endpoint keys; turn state lives in the app
    layer), so the promise should hold cleanly — but nobody has run the
    experiment. Workshop-2 gets a test that kills the connection
    mid-conversation, lets it re-establish, and proves the exchange
    continues — plus the receiving-side twin: a fresh inbound connection
    from the same peer routes deliveries to the same actors (today the
    adapter's read loop just ends silently on close). Uses the fault
    surface whose home Q6 decides.*
- Q6. **Where does the memory adapter's fault surface live** — the adapter
  proper (part of its contract) or its `testing` module (an affordance)?
  This decides whether fault semantics are part of the `Transport` port's
  meaning or a test-double convenience.
  - *Ruled (session, 2026-08-20): **the adapter proper** — the front
    panel, not the debug menu. Two reasons: (1) once R8 makes reporting
    connection death part of what a transport *means*, a transport that
    cannot fail at all no longer faithfully models the port — fault
    injection is the memory adapter keeping up, not a bolt-on; (2) the
    lever's users are not just this repo's tests: embedders running their
    own unattended tests of their own parties ("does my party clean up
    when its exchange dies?") need a stable, documented surface — which
    is workshop-2's own spike question, applied to embedders. The memory
    adapter is thereby declared a small honest simulator of a network,
    faults included.*
- Q7. **Do we take a dependency for BDD now** (cucumber-rs, needs a dep bump
  + vendor refresh through Ander) or land the first feature files as
  documentation-plus-matching-tests and wire the runner as its own errand?
  - *Ruled (session, 2026-08-20): **feature files first, cucumber-rs
    later** — with a standing obligation, recorded in three places so it
    cannot be missed: when the runner errand happens, every scenario in
    `features/` must be bound to real step definitions (the existing
    ordinary tests become the steps' plumbing), and the interim
    scenario→test mapping is retired. The obligation lives in (1) this
    card, (2) `features/README.md`, which maps each scenario to the
    ordinary test pinning it today and names the runner errand as owing
    the binding, and (3) the timeouts branch's completion brief, where
    "wire cucumber-rs and bind features/" is the named follow-up errand
    (a dep bump, so it goes through Ander per VENDORING.md).*

## Suggested session mechanics

One story at a time, red cards first-class: we walk the rules, Casey
accepts/amends each, we work the examples until they expose a missing rule
or a new red card, and anything still red at the end becomes an advisory or
a deferred-list entry. Outputs: numbered decisions into ARCHITECTURE.md
(22+), feature files into the repo (location per Q7), implementation as the
following errand — with Ander reviewing both the decisions and the diff.
