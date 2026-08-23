# Pre-flight handoff: same-endpoint fan-out (story 4) — for Mez

*From Ander, 2026-08-23. Cover note for `2026-08-23-fanout-hazards.md`,
which holds the detail, the line references and the reasoning. Third of
these; you know the shape. Build away — but three things want deciding
before code sets, and one of them the rulings don't name.*

## Verdict

**No STOP. The rulings stand and Q1=a / Q4=a genuinely hang together** —
one receipt can only settle N waits because one id spans them. That is a
real design property, not a coincidence, and it survived the reading.

R5 also holds better than the board hoped: the entire batch-receipt
mechanism lives in the two transports. **Neither runtime changes.** More on
that below, because it is the most useful thing in the note.

## The three to settle before code

**1. The batch receipt has no sender — and nothing has ruled what goes there.**

`Delivery.from` is an `Address`: endpoint *and* name. A receipt from "the
endpoint" has no name. And there is no reserved namespace to borrow —
`ActorName::new` accepts any non-empty string, so a real actor can be
installed under whatever sentinel you pick, and carving one out means
changing domain validation, which R5 says shouldn't be needed.

My recommendation is **the sentinel, with the collision accepted and said
out loud in decision 29**: an admitted endpoint can already forge
`Message::Ack{id}` for any id pairwise today, and `note_ack` will settle on
it — so a sentinel-named actor adds no capability anyone lacks. That keeps
`ActorName` untouched. The note lays out the four options and what each
costs if you'd rather take a different one.

This is the one I'd most want decided before the first adapter is written,
because both adapters need the *same* answer.

**2. Which unbound-name behaviour the batch copies — the wrong choice is a
roster oracle.**

Worth knowing before you write it: **the two adapters already disagree
today.** Memory errors (`UnknownAddress` → `SendFailed`); iroh drops
silently, and its own comment says why — *"an unbound name discloses
nothing."* Casey's errand framed the erroring behaviour as the current
state; that is true of the simulator only, not the wire-facing adapter.

**Copy iroh**, and not for tidiness. A batch that errors per unbound name
lets a sender probe the roster one name at a time — the same information
leak R4 exists to forbid, arrived at by a different door. The trap is that
reusing the pairwise `send` in a loop is the obvious implementation and
gets this exactly wrong, invisibly.

Same argument settles the all-unbound question you asked: **ack it.**
Withholding the receipt when nothing is bound is that oracle again, in its
purest form. The batch did reach the endpoint; the names are not part of
the promise.

**3. The relay recipe — Q1=a, Q3=a and Q4=a together put real work in app
code that no library surface guides.**

One receipt, N Phones, helper already gone. Today's pumps are single-phone
by construction, so this is new work for whoever writes the EMBEDDING
section, with three specific wrinkles in the note: `Inbound::AckReceived`
drops the origin, all N ack deadlines elapse simultaneously and
`next_or_deadline_or_death` yields one verdict for them, and there is no
app-side equivalent of the private `LocalRuntime::earliest_due`.

The generalization worth writing down once: **anything endpoint-scoped must
be relayed to every phone of the batch** — the receipt, a peer death (27),
a revocation (28), an elapsed ack deadline. All four methods already exist
and none needs changing. It is a loop, and saying so once is cheaper than
four discoveries.

## Two results that should make the build easier

**Q4=a costs the responder nothing.** I went looking for anything keyed by
`RequestId` alone and there is none — no id-keyed maps anywhere, and every
ack-matching site is scoped by peer *before* the id is compared. On the
responder side the N siblings live in separate hosts with independent
conversation maps, so N exchanges sharing one id never meet. The shared id
is only load-bearing in the initiator's pump.

**Kameo and `LocalRuntime` need no batch-ack work at all.** R5 puts fan-in
in the transport, below the runtime, so by the time a pump sees anything it
is N ordinary `Delivery` values and the receipt has long gone. Your errand
asked where kameo should ack; the answer is that it shouldn't, and that is
R5 doing its job. Worth stating positively in decision 29 — it also means
`inbound.rs`'s pairwise ack path is untouched.

## Two things wider than they look

- **Q1=a amends decision 4, not just the glossary.** Decision 4 says the ack
  "confirms delivery to the dedicated actor only" — that is the promise the
  batch receipt weakens, and it is one of the oldest entries in the log. The
  actor-level promise is stated in four places, not one; the note lists
  them. Decision 29 should qualify decision 4 *by name* rather than quietly
  diverge from it.
- **EMBEDDING's third-runtime list should NOT grow to six.** It says "all
  five" after decision 28, and the reflex will be to add fan-out. Don't —
  fan-out is a transport affair, so a third *runtime* owes nothing new. Worth
  checking deliberately, since the last two stories both did grow it.

## One residue worth a sentence and maybe a scenario

The board ruled the denial-surfaces-at-the-turn-deadline trade with eyes
open, so this is magnitude, not re-litigation: the two deadlines are
deliberately different scales — decision 22 calls the ack deadline
"transport-scale, typically short" and the turn deadline "possibly long (an
agent may think for minutes)". So a denied sibling costs the initiator
minutes where pairwise it cost seconds, for your own three-workers customer.

No ruling change needed — the caller builds the N Phones, so it sets their
`Deadlines`. **Fanned conversations have lost their short-deadline safety
net, and a tighter turn deadline is the natural compensation.** One sentence
next to the recipe, and it makes a good scenario: three fanned, one denied,
the pair close normally while the third fails by turn deadline.

The note also collects three smaller residues of the same shape (unbound
sibling, duplicate Phone against a conversation you already have, duplicate
names in one batch). They are all "the initiator waits out a turn deadline",
so they want to be one documented sentence rather than three discoveries.

## What I'll do at review

Same three-step shape, and the mutation pass earned its place last round —
it found the only real finding in story 3. I will point it specifically at
**the unbound-name behaviour** and **the N-phone relay**, because both are
the kind of thing a test can appear to cover without covering: a relay test
with one phone passes whatever the loop does, and an unbound test with one
bound sibling passes whether the batch drops or errors.

If you want a mid-flight look before the branch is finished, that is cheaper
than the review that would otherwise catch it.

## Bounds kept

No code touched, nothing pushed, no commits. The worktree is on `master` at
5f05ee3f with an empty `git status`.
