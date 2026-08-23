# Example-mapping board: same-endpoint fan-out (roadmap 5)

*Prepared by Mez, 2026-08-23, for the mapping meeting with Casey. The
smallest board of the spike — the roadmap already carries the central
design observation; what remains is mostly confirming its consequences.
Grounding: ROADMAP.md item 5, Transport::send (ports lib.rs:33),
inbound.rs's ack flow, decision 28 — against master 5f05ee3f.*

## The roadmap's own words (the design is half-done already)

> One send reaching several actors that all live on a single endpoint.
> Colocation collapses partial failure — one connection, one delivery
> event, one ack covering the batch — and the load-bearing constraint is
> *same-endpoint*, which is mechanically checkable, not
> *supervisor-and-children*, which would import a kinship concept the
> transport cannot verify and does not need. Admission stays per-actor:
> siblings may individually deny, and that residue is deterministic
> policy, not network luck. Cross-endpoint fan-out — the genuinely hard
> version — stays deferred.

## Story card

> **A controller says one thing to several of its actors at one endpoint
> with one send — and gets N ordinary conversations, not a new kind of
> conversation.**

Customer today: harness-driven tests and the spawner pattern (decision
6's controller spawns several workers at one endpoint and wants to open
work with all of them without N wire round-trips). Customer tomorrow:
workshop-3's console addressing a node's ops actors.

## Rule candidates (blue cards)

- **R1 — Same-endpoint only, checked mechanically at the sending
  surface.** The fan-out call takes ONE endpoint and a list of names; a
  mixed-endpoint batch is unrepresentable, not merely rejected.
- **R2 — Fan-out exists only at the moment of opening.** The result is N
  ordinary pairwise conversations; turns, deadlines, failure causes, and
  decision 28's mutation all apply downstream completely unchanged. No
  new conversation kind enters the glossary.
- **R3 — Admission stays per-actor.** Each sibling's allowlist judges the
  fanned delivery independently; a denial is silent on the wire and
  observable at the receiving home (`DeliveryDenied`, decision 25) —
  exactly as if the sends had been separate.
- **R4 — No wildcard, ever in v1.** The sender names every recipient.
  "All actors at the endpoint" would let any admitted peer enumerate the
  roster — an information leak the deny-by-default ethos forbids.
- **R5 — The batch is a transport affair.** One wire frame goes out; the
  receiving adapter fans it into N ordinary `Delivery` values before
  admission. Domain vocabulary is untouched; the ports grow at most one
  method.

## Red question cards (Casey rules)

**Q1 — What does the one ack promise?** Today an ack is sent per actor,
*after* admission (`inbound.rs`: denied traffic is dropped before any
ack), so pairwise ack means "your request reached this actor and was
admitted." A batch has three options:
- (a) **One ack = the batch reached the endpoint**, sent before fan-in
  to the siblings. Truly one ack, as the roadmap sketches. Cost: it
  promises less than today's pairwise ack — a denied sibling's silence
  now surfaces at the *turn* deadline, not the ack deadline. (Denial
  stays indistinguishable from slowness either way; only the deadline
  that catches it changes.)
- (b) **N acks, one per admitting sibling.** The wire batch is still one
  frame, but "one ack covering the batch" is given up; initiator-side
  bookkeeping is today's, unchanged.
- (c) One ack listing who was admitted — REJECTED before ruling:
  admission decisions would cross the wire, violating never-on-wire.

**Q2 — What fans out?** (a) Opening turns only — `Open` to N names, N
exchanges born, the clear customer; (b) also harness commands (ping-all,
stop-all — test convenience); (c) any request-bearing turn — rejected
before ruling: a `Continue` belongs to exactly one exchange by
construction.

**Q3 — The initiator-side surface.** (a) A helper that performs one wire
send and hands back **N ordinary Phones**, one per conversation — the
Phone stays pairwise and the glossary intact; (b) a new "conference"
abstraction owning N conversations — more surface, and R2 says fan-out
shouldn't outlive the opening moment.

**Q4 — May the fanned request ids differ per sibling, or one id for
all?** One send means one request; ids need uniqueness only within a
conversation, so a single id shared across the N new conversations is
legal today. (a) One id — simplest, matches "one send"; the batch ack
(if Q1=a) settles every sibling's ack wait by that one id. (b) Fresh id
per sibling — no wire saving on ids, but each conversation looks fully
independent in logs/tests.

## Seed examples (green cards)

1. **Three workers, one send.** A controller opens work with three
   spawned workers at one endpoint via one fan-out send; all admitted;
   three independent conversations proceed and close on their own
   schedules.
2. **One sibling denies.** Same batch, but worker two's allowlist never
   admitted the controller. Two conversations proceed; the denial is
   observed at the workers' home; the controller's second exchange fails
   by deadline — silence, as deny always looks from outside.
3. **The batch ack settles every wait at once** (shape depends on Q1).
   The controller's N ack-waits settle from the single receipt; the
   timer never moves.
4. **Mixed endpoints are unrepresentable.** There is no way to write a
   fan-out call naming two endpoints — the constraint is the type, not a
   runtime check (R1).
5. **Fan-out then revoke** (decision 28 interplay). After the batch
   opens three conversations, the controller's endpoint is revoked at
   one sibling: that conversation's exchange fails with `Revoked` at the
   sibling, the other two are untouched — nothing about fan-out changes
   mutation, or vice versa.

## Estimated build after rulings

Small-to-medium: one ports method (batch send), memory + iroh + kameo
adapter fan-in, initiator helper returning N Phones, ack settling per
Q1, feature file + tests (including example 5's interplay pin). No
domain changes if R5 holds. Same three-step shape: this board → Ander
pre-flight hazards note → build → review with mutation testing.

---

## Rulings (Casey, 2026-08-23, in conversation)

- **Q1 = (a).** The batch receipt is one ack meaning "the batch reached
  the endpoint," sent before admission. A denied sibling surfaces at the
  turn deadline instead of the ack deadline — denial is silence either
  way; only the deadline that catches it changes. One glossary sentence
  distinguishes the batch receipt from the pairwise ack. (Option (c) —
  an ack listing admissions — was pre-rejected: admission never crosses
  the wire.)
- **Q2 = (a).** Opening turns only. Harness fan-out (ping-all, stop-all)
  is a DEFINITE later item, Casey's words: "we'd like to address the
  rest sometime later for sure" — recorded in the deferred list as a
  commitment, not a maybe. (Fanning a Continue was pre-rejected: a
  continuation belongs to one exchange by construction.)
- **Q3 = (a).** The caller holds N ordinary Phones; the helper that did
  the fanning is gone before the first reply arrives, making R2
  structural. No conference abstraction — if await-all becomes
  everyone's afternoon, it builds on top of N Phones without touching
  the protocol.
- **Q4 = (a).** One request id shared by all N conversations: legal (ids
  are unique per conversation), and it is what lets the single batch
  receipt settle every Phone's ack wait as an ordinary Ack{id}. Q1=(a)
  and Q4=(a) hang together as one design.
- **R1–R5 all confirmed** (each ruling leaned on them; Casey raised no
  objection).

### Outputs

Decision-log entry (29) for the rulings; feature file (story 4) with the
five seed examples including the decision-28 interplay pin; this board
archives under `docs/mappings/2026-08-23-fanout/` on the implementation
branch. Next step per the proven shape: Ander pre-flight hazards note
BEFORE implementation, then build, then review with mutation testing.
