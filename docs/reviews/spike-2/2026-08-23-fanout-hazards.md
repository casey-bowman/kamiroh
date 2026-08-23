# Pre-flight hazards: same-endpoint fan-out (roadmap 5)

*From Ander, 2026-08-23. Not a review — the story isn't written yet. Read
against `master` 5f05ee3f and the rulings in
`2026-08-23-mapping-board-fanout.md`. No code touched. Third of these; same
shape as the last two, so I have led with the things that would change the
build rather than the things that would change a test.*

---

## 0. The rulings stand — no STOP

I checked each for safety. **None is unsafe as ruled**, and Q1=a and Q4=a do
hang together exactly as the board says: one receipt can only settle N waits
because one id spans them.

Two things the rulings are *wider* than they look, neither a reason to stop:

- **Q1=a amends decision 4, not just the glossary.** The ruling anticipates
  "one glossary sentence". But decision 4 (`ARCHITECTURE.md:244`) says the ack
  *"confirms delivery to the dedicated actor only"* — that is the promise Q1=a
  deliberately weakens for the batch case, and it is one of the oldest
  decisions in the log. Four sites state the actor-level promise, not one
  (§8). Worth knowing before the docs commit is scoped.
- **Q1=a has no sender.** `Delivery.from` is an `Address` — endpoint *and*
  name — and a receipt from "the endpoint" has no name to put there. This is
  the first thing the build has to decide and the rulings don't name it (§1).

Everything below assumes the rulings as written.

---

## 1. The batch receipt has no sender — decide this first

`Delivery { from: Address, to: Address, message }` (`kamiroh-ports/src/lib.rs:23`)
gives the receipt nowhere to come from. Every option costs something:

| Option | Cost |
|---|---|
| Reserved sentinel name | `ActorName::new` accepts **any non-empty string** (`actor.rs:17-23`) — there is no reserved namespace, so a real actor can be installed under the sentinel. Carving one out is a **domain change**, which R5 says shouldn't be needed. |
| Echo one recipient's name | Dishonest, and breaks precisely when that recipient is the unbound one. |
| Echo `to_names[0]` as written by the sender | Deterministic and recognizable to the sender, but still claims an actor spoke when none did. |
| Not a `Delivery` at all | Then it isn't "a single ordinary Ack" and the initiator's `Inbox` never sees it — contradicts Q1=a. |

**Recommendation: the sentinel, and accept the collision.** The collision is
already inside the existing trust model — an admitted endpoint can forge
`Message::Ack{id}` for any id today, pairwise, and `note_ack` will settle on
it. A sentinel-named actor therefore adds no capability an admitted peer
lacks. Say so explicitly in decision 29 rather than leaving it as an
unexamined "reserved name", and keep `ActorName` untouched so R5 holds.

The alternative worth one minute of thought: names are "claimed, never
proven" already (`actor.rs:9-10`), so a receipt whose name is a convention
rather than an actor is not a new *kind* of lie. It is only a new *use* of an
existing one.

---

## 2. Where each adapter can honestly send the receipt

Asked directly, per adapter. The answers are not symmetric, and one of them
has a trap.

### iroh — clean, and the seam already exists

`spawn_reader` (`kamiroh-adapter-iroh/src/lib.rs:527`) is exactly the right
place: it holds the authenticated `origin`, it holds the `connection` to
reply on, and it sits **above** the router loop — so "arrived at the
endpoint, before fan-in, before admission" is literally where the code is.
Decode the batch frame → send the receipt → then route to bound names.

One new failure mode: that task has never sent anything before. A failed
reply stream loses the receipt, and the initiator falls to its ack deadline.
Same consequence as any lost ack; worth a comment, not a mechanism.

### memory — no endpoint-level seam exists at all, and there is a partition trap

`MemoryNet` has **no endpoint concept**: `mailboxes` is a flat map keyed by
full `Address`. There is no receiving-side task, so nothing can observe
"arrival at an endpoint" — the batch send *is* the arrival, in-process.

So the memory adapter must synthesize the receipt inside the batch send. Two
constraints on where:

1. **After the partition check, not before** (`lib.rs:216-227`). A partition
   holds deliveries silently and returns `Ok(())`. Synthesize the receipt at
   call entry and a *partitioned* batch gets acked — the initiator's ack wait
   settles for traffic that never arrived, and only the turn deadline catches
   it. That directly weakens the premise of
   `a_partition_that_never_heals_is_caught_by_the_deadline_backstop`. Put the
   receipt after the hold check so a held batch is acked by nobody.
2. **Be honest in the doc comment** that the memory receipt proves "the send
   call ran in this process", not "bytes reached a remote endpoint". It is a
   simulator; the pairwise ack has always had the same property. Say it once.

### kameo — not involved, and that is the finding

R5 puts fan-in in the *transport*, below the runtime. By the time a
`KameoRuntime` pump sees anything it is already N ordinary `Delivery` values,
and the receipt was sent by the responding node's transport before any host
existed in the story. **The Kameo adapter needs no change for the batch
receipt at all** — and neither does `LocalRuntime`.

That is worth stating positively in decision 29: the entire batch-ack
mechanism lives in the two transports, which is R5 doing its job. It also
means the pairwise ack path in `inbound.rs:72` is untouched — the batch
receipt is a *different* message from a *different* layer, not a modified
version of the existing one.

### An all-unbound batch: **ack it**

Under Q1=a's own definition the receipt means "the batch reached the
endpoint" — and it did. The names are not part of the promise.

The stronger argument is R4's. R4 forbids wildcards because "all actors at
the endpoint" would let a peer enumerate the roster. **Withholding the
receipt when no name is bound hands the sender exactly that oracle**: batch a
single name, see whether a receipt comes back, learn whether that actor
exists. Ack unconditionally and the oracle closes.

Consequence to document: all N conversations then die at their turn
deadlines, having been acked. That is the same residue as a denial, which is
what Q1=a already accepts.

---

## 3. Unbound siblings: the two adapters already disagree

The errand's premise — *"pairwise send to an unbound name errors at the
transport today (SendFailed on our side)"* — **is true of memory only.**

- **memory**: `mailboxes.get_mut(to).ok_or(UnknownAddress)?`
  (`lib.rs:230`) → the sender gets an error → `FailureCause::SendFailed`.
- **iroh**: `if let Some(tx) = router.bound.get(&delivery.to.name)`
  (`lib.rs:548`), whose own comment reads *"Unknown or closed bindings drop
  silently: an unbound name discloses nothing."* The sender's `send` already
  returned `Ok(())` — the frame was written. The deadline catches it.

So the wire-facing adapter is already silent and the simulator is already
loud. Fan-out will codify whichever one the batch path copies.

**Copy iroh: deliver to bound names, drop unbound ones silently.** Not for
consistency's sake — for R4's. A batch that *errors* on an unbound name is a
roster oracle in the same family as the wildcard R4 exists to forbid, and it
is worse than the all-unbound case in §2 because it discriminates per name.
The memory batch path therefore must **not** reuse the pairwise
lookup-or-error; that is an easy and invisible mistake to make, because
reusing `send` in a loop is the obvious implementation.

Answering the second half directly: **yes, the initiator's story for a
missing sibling is just the turn deadline**, identical to a denial, and yes I
think that is acceptable residue — provided it is documented next to the
denial residue rather than discovered. The two are the same shape and should
be one sentence, not two.

---

## 4. The shared request id: clean where it matters

Asked directly, and this is the reassuring section. **Nothing in the
workspace keys on `RequestId` alone.** Every ack-matching site is already
scoped by peer before the id is compared:

- `runtime.rs:471` — inside `conversations.get_mut(&origin)`.
- `kameo lib.rs:459` → `settle_ack` — inside `conversations.get_mut(&origin)`.
- `kameo lib.rs:741` — the `DeadlineElapsed` staleness check, likewise
  peer-scoped first.
- `phone.rs:146` — a `Phone` *is* one conversation.

No `HashMap<RequestId, _>` exists anywhere. On the responder side the N
siblings live in different hosts (or different `LocalActor`s) with
independent `conversations` maps, so N exchanges sharing one id never meet.
**Q4=a costs the responder nothing.**

The one place the shared id becomes load-bearing is the **initiator's pump**,
which is application code, not library code — see §5. And there the library
gives no help at all: `RequestId` is a bare `pub [u8; 16]`
(`vocabulary.rs:11`) that kamiroh never generates. Uniqueness is entirely
caller discipline, and in the test suite ids are hand-picked constants —
`RequestId([200; 16])` is reused across parties today. A pump that relays a
batch receipt **by id to every phone it holds** will cross-settle the moment
two batches pick the same id. Relay to *this batch's* phones (§5), not by id
across all phones, and the hazard closes.

---

## 5. One receipt, N phones — and the same shape for everything endpoint-scoped

Q3=a hands back N ordinary Phones and the helper is gone. Q1=a sends one
receipt. Nothing in the library connects them, so the app's pump must, and
today's pumps are single-phone by construction (`tests/deadlines.rs:246`,
`tests/turns_conversation.rs:99` — match on shape, call `phone.note_ack`).

Three concrete wrinkles for whoever writes the EMBEDDING recipe:

1. **`Inbound::AckReceived` drops the origin.** `process` destructures the
   delivery and returns `Inbound::AckReceived(ack)` with no `from`
   (`inbound.rs:66`). A pump holding both fanned and pairwise phones cannot
   tell which peer an ack came from without capturing `delivery.from` *before*
   calling `process` — and `process` takes the delivery by value. Either
   document the clone-first idiom or give `AckReceived` its origin. The
   latter is an app-layer change, not a domain one, so R5 survives it.
2. **All N ack deadlines are simultaneous.** The helper arms them in one
   instant with one `Deadlines`, so they elapse together. `next_or_deadline_or_death`
   (`wait.rs:53`) takes **one** deadline future and yields **one** `Waited`,
   and `Phone::on_deadline_elapsed` fails one phone. The pump must fold N
   `next_due()` values into the earliest and then apply the verdict to *every*
   phone that was due — there is no app-side equivalent of
   `LocalRuntime::earliest_due`, which is private.
3. **The delivery is consumed once**, which is correct and not a hazard: the
   receipt is one delivery that the pump then *applies* N times. Worth saying
   out loud because "relay to all" sounds like it needs the delivery twice.

**The generalization worth putting in the docs:** *anything endpoint-scoped
must be relayed to every phone of the batch.* That is already three things
and will be four —

| Event | Per-phone call | Why it fans |
|---|---|---|
| batch receipt | `note_ack(&ack)` | one endpoint, one receipt |
| peer death (27) | `on_peer_death(&endpoint)` | all N share the peer endpoint |
| revocation (28) | `on_revoked(&endpoint)` | ditto |
| ack deadline | `on_deadline_elapsed()` | armed in one instant |

All four already exist and none needs changing. The recipe is a loop, and
saying so once is cheaper than four discoveries.

---

## 6. The helper can collide with a conversation you already have

`Phone::converse` is purely local (decision 11) and nothing prevents two
Phones to the same peer. So a fan-out naming a sibling the caller is
*already* conversing with produces a **second Phone for one conversation**,
both believing they own the turn state.

The responder settles it unhelpfully: its single `TurnState` for that peer is
mid-exchange, so the fanned `Open` hits `MustAnswerFirst` → dropped, observed
at home as `IllegalTurnDropped`. The initiator's new Phone waits and dies at
the turn deadline. A third residue of the same shape as §2 and §3.

Cheap mitigations, in order of preference: document the helper as "for
opening fresh conversations" and leave it; or have the helper take the
caller's existing peers to assert against. Not worth a mechanism — worth a
doc sentence and possibly a scenario.

---

## 7. The residue is longer than pairwise, and the turn deadline is the long one

The board named this trade and ruled on it, so this is not a re-litigation —
it is a note on magnitude, because the two deadlines are deliberately very
different scales. Decision 22's own words: the ack deadline is
"transport-scale, typically short"; the turn deadline is "party-scale,
possibly long (an agent may think for minutes)".

So under Q1=a a denied, unbound, or dropped sibling costs the initiator a
**turn** deadline where pairwise it cost an **ack** deadline. For the board's
own customer — a controller opening work with three spawned workers — one
denied worker means that exchange hangs for minutes rather than seconds.

No ruling change needed, and the fix is already in the caller's hands: the
helper builds the N Phones, so the caller passes their `Deadlines`. **Fanned
conversations have lost their short-deadline safety net, so a tighter turn
deadline is the natural compensation.** One sentence in EMBEDDING next to the
recipe. Worth a scenario too: three fanned, one denied, assert the survivor
pair close normally while the third fails by turn deadline.

---

## 8. Documentation blast radius

Q1=a's "one glossary sentence" is four sites, because the actor-level promise
is stated four times:

| Site | What's there |
|---|---|
| `ARCHITECTURE.md:244` | **decision 4** — "confirms delivery to the dedicated actor only" |
| `ARCHITECTURE.md:135` | glossary **Ack** — "from the remote *actor*" |
| `vocabulary.rs:22` | `Ack` doc — same words |
| `deadline.rs:40` | `DeadlineKind::Ack` — "the delivery ack of a sent request half" |

Decision 4 is the load-bearing one and it is foundational, so decision 29
should **qualify it by name** rather than quietly diverge. `phone.rs:144`
happens to already say "from the peer's runtime", which is closer to the
batch meaning than the others — no edit needed there.

Also expected, from the previous stories' pattern: `EMBEDDING.md`'s
third-runtime obligation list currently says **"all five"**. Fan-out is a
transport affair, not a runtime one (§2), so **this list should NOT grow** —
worth checking deliberately, because the reflex after decision 28 will be to
add a sixth.

---

## 9. Interplay: what the existing mechanisms assume about how a conversation was born

Asked directly. **Nothing** — and I checked rather than assuming.

Conversations are created lazily by `conversations.entry(peer).or_default()`
on first turn, identically in both runtimes. A fanned `Open` arrives as an
ordinary `Delivery` (R5), so no responder-side mechanism can tell it from a
pairwise one. Specifically:

- **Revoke (28)** — `revoke_here`'s sweep is per-host and filters on
  `peer.endpoint` plus live turn-state. Revoking the initiator at sibling *k*
  fails sibling *k*'s exchange and nothing else, because each sibling is a
  separate host with a separate map. Board example 5 pins it; the mechanism
  needs no help.
- **Death (27)** — the responder side fans out per host as today. The
  *initiator* side is the interesting direction: all N phones share one peer
  endpoint, so one death report must reach all N — §5's table.
- **Denial (25)** — `DeliveryDenied` is emitted per sibling at the receiving
  home, unchanged. The only new thing is ordering: with Q1=a the receipt has
  already gone out before admission runs, so the initiator sees *ack, then
  silence*. That is the intended shape, but it is the first case in the
  system where an ack precedes a denial, and the feature file should say so
  in the scenario rather than leaving it implied.

One genuinely new state to think about: a fanned `Open` to a sibling whose
conversation with the initiator is in `Failed` (revoked earlier, say) is
**legal** — `Failed → Open` is allowed in both directions
(`protocol.rs:165`, `:204`). So fan-out after a revocation-and-re-admission
works, and example 5's neighbourhood is already covered by the turn machine.

---

## 10. Smaller things

- **R1's "unrepresentable" needs the signature to carry it.**
  `batch_send(&mut self, from: &Address, to_endpoint: &EndpointId, to_names:
  &[ActorName], message)` makes mixed endpoints unrepresentable at the call
  site, as ruled. Taking `&[Address]` and checking would satisfy the words
  and lose the point.
- **Empty name list.** `to_names: &[]` — is that an error, or a no-op that
  still acks? Falls out of §2's rule (the batch reached the endpoint, so ack),
  but it is a one-line decision someone will otherwise make by accident.
- **Duplicate names in the batch.** Two copies of the same name → two
  deliveries to one actor → the second `Open` hits `MustAnswerFirst`. Dedupe
  at the sending surface, or document. Cheaper to dedupe.
- **`MAX_FRAME_BYTES` is 1 MiB** (`iroh lib.rs:75`). A batch frame carries N
  names plus one message; N is small in every story on the board, so this is
  not a limit today — but a batch frame is the first message whose size grows
  with a caller-supplied list, so the check now matters where it never did.
- **The receipt bypasses admission on the way out, not on the way in.** The
  initiator's own allowlist still judges the receipt by
  `delivery.from.endpoint` when its pump calls `process`. So an initiator that
  fans out to an endpoint it does not admit will send the batch, receive the
  receipt, and **deny its own receipt**. Symmetric with the pairwise case and
  correct, but it will look like a bug the first time someone hits it.
- **Story numbering.** The board calls this story 4 in `features/`; decision
  29 is next in the log. Worth keeping those straight in the archive path
  (`docs/mappings/2026-08-23-fanout/`).

---

The three I would most want settled before code sets: **§1** (the receipt's
sender — every adapter needs the same answer), **§3** (which unbound
behaviour the batch copies — the wrong choice is a roster oracle and it is
the natural implementation), and **§5's relay recipe** (because Q1=a, Q3=a
and Q4=a together put the work in app code that no library surface guides).

As before, a partial branch mid-flight is fine, and I will mutation-test
whatever the suite claims to pin — §3 and §5 in particular are the kind of
thing a test can appear to cover without covering.
