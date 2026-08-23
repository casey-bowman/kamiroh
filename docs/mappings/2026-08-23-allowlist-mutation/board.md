# Example-mapping board: allowlist mutation on running actors

*Spike-2 roadmap item 3. Prepared by Mez, 2026-08-23, for the mapping
meeting with Casey. Red cards are questions Casey rules; rules and
examples graduate to ARCHITECTURE.md's decision log and features/ per
the house workflow. Grounding: allowlist.rs, admission.rs, inbound.rs,
runtime.rs, kameo adapter install paths — verified against master
a4184e80.*

## The gap, in one paragraph

The domain already has `Allowlist::admit` and `Allowlist::revoke`, and
enforcement runs on **every delivery** (`admission::admit`, called from
`inbound::process` in each runtime step), so a revocation *would* take
effect on a live connection — that behavior is pinned by domain tests.
But both runtimes (`LocalRuntime`, `KameoRuntime`) take the allowlist at
install time and store it in the actor's private state; no code path can
touch it afterward. ROADMAP.md item 3: "the promise is kept but
currently unreachable." The work is the **mutation channel**, not the
policy.

## Story card

> **A running actor's operator admits a new endpoint and revokes an old
> one, without restarting anything — and the changes bite immediately.**

Two named customers:

1. **Tests/ops today** — container tests and the fault-simulator memory
   adapter want to assert that revocation takes effect mid-conversation.
2. **Workshop-3's console-key story** — a leaked or rotated ops-console
   key must be evictable from every `ops.*` actor on a live fleet, or
   the console key is an unrevokable super-key (two-plane response,
   risk 3).

## Rule candidates (blue cards — Mez's proposals, Casey confirms or amends)

- **R1 — The unit of mutation is the actor**, same as the unit of trust
  (decision 2/3): you admit/revoke an endpoint *on one actor*. No
  node-wide bulk form in this spike.
- **R2 — Revocation bites on the very next delivery.** Already true by
  construction (per-delivery enforcement); the new work pins it through
  the runtime path, not just the domain.
- **R3 — Revoking the last endpoint returns the actor to
  deny-by-default silence.** Symmetric with decision 3.
- **R4 — Mutation never touches connections.** Admission is policy;
  connections are plumbing. Revoking endpoint E at actor X does not
  close E's QUIC connection — E may still be admitted by actor Y on the
  same node. (Transport-level eviction stays a separate, existing tool:
  `close_connection`.)
- **R5 — Idempotence.** Admitting an admitted endpoint and revoking an
  absent one are no-ops, not errors (BTreeSet semantics, already so in
  the domain).

## Red question cards (Casey rules, in this order)

**Q1 — Over what channel does mutation travel?**
- (a) **Local API only**: methods on the runtime
  (`admit(actor, endpoint)` / `revoke(actor, endpoint)`), callable by
  the embedding process that owns the runtime. No wire form.
- (b) **Wire form too, harness-gated**: extend the `Harness` vocabulary
  with `Admit`/`Revoke` commands, honored only by `ActorKind::Harness`
  actors (decision 6 precedent: harness admission is a privileged
  grant).
- (c) Local API now, wire form deferred to workshop-3 (where the
  operator port and the human-ack question live).

**Q2 — Does revocation fail in-flight exchanges with the revoked
endpoint at once?** Revocation is positive local evidence that the
peer's next turn will be denied. Decision 26's principle says positive
evidence fails exchanges immediately. But unlike a refused send, the
*outbound* direction still works. Options: (a) revocation also fails
the actor's live exchanges with that endpoint (cause: a new
`FailureCause` variant or reuse); (b) nothing special — the denied
reply simply never arrives and the turn deadline fails the exchange in
due course (machinery already built).

**Q3 — Is the mutation itself observable?** Deny observability
(decision 25) already shows the *effect* (`DeliveryDenied` after
revocation). Do we also emit `Event::EndpointAdmitted` /
`EndpointRevoked` at the actor, so an operator/test can assert the
policy change itself — the "revocation is observable" clause the
two-plane work asks for? Options: (a) yes, two new Event variants;
(b) no, DeliveryDenied is the observable truth and events stay
failure-shaped.

**Q4 — If a wire form exists (Q1 ≠ a): who may aim it at whom?**
Harness actors only, per decision 6? And may the controller that
spawned an actor mutate *that actor's* allowlist remotely (natural for
the fan-out/narrower-grants future), or only its own endpoint's actors?

## Seed examples (green cards — will become Gherkin after rulings)

1. **Warming up to a new peer.** An actor installed with an empty
   allowlist (silence). Its operator admits endpoint C. C's next
   request is delivered and acked. No restart happened.
2. **Revocation bites mid-conversation.** Actor admitted A and B; B is
   mid-conversation. Operator revokes B. B's next turn is denied — and
   observed (`DeliveryDenied`). A's traffic is untouched.
3. **Console-key rotation (workshop-3 flavor, memory adapter).** Actor
   admits old-console. Operator admits new-console, revokes
   old-console. Old key's next command: denied, observed. New key:
   delivered. The actor never stopped serving.
4. **Revoking a stranger is a quiet no-op.** Revoke an endpoint that
   was never admitted: no error, no event beyond what Q3 rules, no
   change.
5. **(Depends Q2.)** B mid-exchange when revoked: either the exchange
   fails at once with a revocation-shaped cause, or B's reply is denied
   and the exchange fails by turn deadline — whichever way Q2 goes, the
   scenario pins it.

## Estimated build after rulings

Lightest of the spike (domain complete; enforcement already
per-delivery): runtime methods + kameo message-based equivalent +
events (if Q3=a) + tests/features. Wire form (if Q1=b) adds vocabulary
+ harness execute arms + admission tests. Ander pre-flight hazards note
before implementation, per the plan.

---

## Rulings (Casey, 2026-08-23, in conversation)

- **Q1 = (c).** Local API only this spike: `admit`/`revoke` methods on
  the runtimes, callable by the embedding process. The wire form (and
  Q4, who may aim it) defers to workshop-3, where the operator port and
  the human-ack question live. Deliberately security-load-bearing: the
  repair channel for a stolen console key must not be the channel the
  stolen key controls.
- **Q2 = (a).** Revocation is positive local evidence (decision 26's
  principle): the actor's live exchanges with the revoked endpoint fail
  at once, new cause `FailureCause::Revoked`, flowing through both
  existing failure surfaces. The conversation survives — a fresh
  exchange is legal if the endpoint is ever re-admitted (mirrors
  decision 27's shape).
- **Q3 = (b).** No policy-change events now. The lever is local, so the
  caller knows what it did; the effects are already observable twice
  (ExchangeFailed{Revoked} at once, DeliveryDenied thereafter). Audit
  events travel with the wire form to workshop-3, keeping the Event
  enum uniformly failure-shaped meanwhile.
- **Q4 — retired unruled**: ships intact to workshop-3 with the wire
  form.
- **R1–R5 all confirmed as written.**

### Additional ruling: no automatic connection closing

Casey probed for a natural point to close connections no actor admits.
The natural point exists (the post-revoke census) but is deliberately
NOT wired in: (1) it would leak policy onto the wire — denial is
silent by design (decision 25, never-on-wire), and an auto-close hands
the revoked peer positive, timely evidence of eviction; (2) the census
answer is transient while policy is not — coupling teardown to it puts
a race where R4 puts a separation; (3) idle connections die by QUIC
idle timeout, and deliberate eviction already has its lever
(`close_connection`). **R4 stands.**

### Documentation obligation on the build

EMBEDDING.md gets the **eviction recipe**: *evict = revoke at every
actor that admits the key (policy: nothing further is heard) +
`close_connection` (plumbing: the pipe dies now, wire-visibly — the
right trade when the key is stolen and stealth is worthless).* Routine
key rotation is the same two calls (admit-new, revoke-old) minus the
close, rehearsing the theft response for free.

### Outputs

Decision-log entry (next number) for the rulings; two feature-file
scenarios or additions binding examples 1–3 and 5(a-shape); this board
archives under `docs/mappings/2026-08-23-allowlist-mutation/` on the
implementation branch. Next step per plan: Ander pre-flight hazards
note BEFORE implementation.
