# Roadmap: the next two spikes

*A dated statement of intent — written 2026-08-17, between spikes, while
spike-1's lessons are fresh. This is a plan, not a promise: real design
decisions get made (and numbered) in the working spike's own
`ARCHITECTURE.md`, and this file should be read the way the tier guide
reads its counters — as a snapshot, superseded by whatever actually
happens. Structure per [TIERS.md](TIERS.md): each architectural spike
gets a fresh workshop org.*

## Workshop-2 — embeddable and testable unattended

**The spike question: can kamiroh be embedded in a host application and
tested with no human watching?** Spike-1 proved the conversation stack on
real networks with a person at each end of the run. The next pressure
comes from automated, container-based integration testing of embedding
applications — tests that kill containers, run unattended, and must fail
fast and loudly. Candidate implementation spikes, in rough priority
order:

1. **Timeouts.** The protocol layer has none; the 15-second deadlines in
   the check binary live in the example, not the library. A hung exchange
   must become a loud failure.
2. **Disconnect mid-exchange.** Currently undefined behavior, and
   container-based tests kill endpoints as a matter of course. What a
   conversation does when its peer vanishes needs an answer.
3. **Dynamic allowlist mutation.** The domain supports `admit`/`revoke`
   and enforcement is per-delivery — revocation *would* take effect on a
   live connection — but the Kameo runtime fixes an actor's allowlist at
   install time, so nothing can actually perform a revocation on a
   running actor. The promise is kept but currently unreachable.
4. **Deny observability.** Denied deliveries vanish silently — right for
   security, awkward for a test that wants to *assert* a denial happened.
   A tracing hook or counter, not a behavior change.
5. **Fan-out, v1: same-endpoint only.** One send reaching several actors
   that all live on a single endpoint. The design observation that keeps
   this simple: colocation collapses partial failure — one connection,
   one delivery event, one ack covering the batch — and the load-bearing
   constraint is *same-endpoint*, which is mechanically checkable, not
   *supervisor-and-children*, which would import a kinship concept the
   transport cannot verify and does not need. Admission stays per-actor:
   siblings may individually deny, and that residue is deterministic
   policy, not network luck. Cross-endpoint fan-out — the genuinely hard
   version — stays deferred until an actual afternoon demands it.
6. **A BDD layer: Cucumber + Gherkin.** Executable specification sitting
   *above* the existing test suite, not replacing it: the unit and
   integration tests remain the fast day-to-day gate, while Gherkin
   feature files state the claims a reader needs to believe — the trust
   model, turn alternation, the ack-before-answer promise — in prose a
   non-implementer can read and a machine can falsify. The project is
   unusually ready for this: BDD lives or dies on a shared vocabulary,
   and `ARCHITECTURE.md`'s glossary (conversation, exchange, turn,
   party, phone, admit, deny…) already is one. Discipline that keeps it
   honest: scenarios stay declarative and glossary-worded — every
   Gherkin noun should appear in the glossary — and plumbing stays
   downstairs in the ordinary tests. Feature files are intended as
   co-authored artifacts (example-mapping sessions, humans and agents
   together), which is the condition under which Gherkin earns its keep
   rather than rotting into a test DSL nobody reads. Steps bind to the
   memory adapter for speed, with a tagged subset over real transport.

For scale: the allowlist itself is complete and tested for its v0 scope
(endpoint-only, deny-by-default, checked on every delivery, claimed
names irrelevant — pinned by test). Workshop-2 is hardening around a
sound core, not repair.

## Workshop-3 — conversations that drive agents

**The spike question: can a conversation drive an agent?** This is the
README's founding sentence finally meeting its subject — the
Turn/Party/Phone vocabulary was cut with agents in mind from the first
design session. Expected contents:

- An actor as an agent's **communications proxy**: a `Party` wrapper
  feeding incoming turns to a live agent process and returning its
  output as turns.
- An **agent-control vocabulary**: start this errand, stream me
  progress, hand me the result, stop.
- **Streaming / progress partials**, promoted from deferred nicety to
  the heart of the matter: agents are slow producers, and a protocol
  that is silent until a closing turn is exactly wrong for them.
- **Per-child capability grants**: an agent that spawns sub-agents
  should hand each one a *narrower* allowlist than its own.

The first validation errand is already designed: an agent on a remote
machine, fronted by a kamiroh actor, receiving the container check
(`docs/INCUS-CHECK.md`) as its errand over a kamiroh conversation and
returning the brief — replacing a human-driven SSH session with the
thing this project exists to build.

## Why this order

The sequencing is load-bearing, not aesthetic. An agent errand is the
most demanding conversation kamiroh will host: long-running (timeouts),
spanning machines that reboot and drop (disconnect handling), and slow
to produce answers (streaming). Hardening first means workshop-3 gets to
ask its question against a base that already survives unattended
operation, instead of tangling two spikes' questions into one.

## Still deferred beyond both

Cross-endpoint fan-out, Close-turn acknowledgments, streaming beyond
agent needs, self-hosted relays. Per the house rule
([WHY-TIERS.md](WHY-TIERS.md)): the list grows when an afternoon
demands it, not before.
