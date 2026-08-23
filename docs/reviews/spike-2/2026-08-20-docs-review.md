# Review: `cowork/spike-2-docs` (`36a6e21`) — docs catch-up and the Q1 language pass

*From Ander, 2026-08-20. Docs-only commit on the swept `master` (`16d8c05`),
at `tiers/2-workshop/kamiroh`. Fourth review in the spike-2 cycle; reviewed
for accuracy against the code, per the errand. Self-contained.*

## Verdict

**Merge-after-fixes.** Four accuracy findings, of which **three remain** —
all small text edits. Finding 4 was settled by evidence rather than by an edit
(see its update); the sentence it questioned stands as written. Nothing
structural, and the gate is green.

Two of the first three are in enumerated lists that read as complete and are
not — the kind of thing that only shows up when the list is checked
item-by-item against the code, which is exactly what the errand asked for. The
fourth was a claim about the *toolchains* rather than the code; it has since
been tested and holds.

## The gate

Run on `cowork/spike-2-docs` with **rustc/cargo/clippy/rustfmt 1.97.0**:

```
cargo build --workspace                        # clean
cargo test  --workspace                        # 58 passed, 0 failed
cargo clippy --workspace --tests --examples    # clean — ZERO warnings
cargo fmt --all --check                        # clean — ZERO diffs
```

**58 passed, 0 failed**, as expected, and the commit is genuinely docs-only —
`ARCHITECTURE.md`, `docs/EMBEDDING.md`, `docs/WORKFLOW.md`, nothing else.

**Item 4 — the fmt claim: confirmed, see finding 4.** `cargo fmt --all --check`
is clean on 1.97 against this branch, and since master carried 29 hunks when I
last looked, the sweep did its job. The *agreement* half was untested when I
first wrote this up; Mez's 1.95 cold run has since checked it, clean. The
sentence in WORKFLOW.md stands as written.

## Findings

### 1. The ack deadline is described as covering everything you send

**`ARCHITECTURE.md:83`** (glossary) and **`docs/EMBEDDING.md:33`**.

Both now say the ack deadline bounds waiting for the delivery receipt of
*"something you sent"*. The code is narrower: an ack wait is armed only for a
turn carrying a **request half**.

```rust
let posed = turn.request().map(|r| r.id);
// ...
self.ack_due = posed.map(|id| (id, now + self.deadlines.ack));
```

`Turn::request()` returns `None` for `Turn::Close`, so a closing turn arms no
ack deadline at all. Decision 22's own body still says it precisely — *"bounds
waiting for the receipt of a sent request half"* — and the pre-pass glossary
said "the delivery ack of a request half it sent", which was correct. The
simplification lost the qualifier, and with it the meaning.

This matters more than a usual wording slip for two reasons. The glossary is
the one place the document says the words *are* the architecture ("in a
ports-and-adapters design the ubiquitous language is the architecture"). And
`EMBEDDING.md` contradicts itself about sixty lines apart: the Party example
at line 98 comments `turn.request()?` with `// Close → None: done`, which is
the very distinction the new sentence erases.

Suggested repair — plain words, qualifier restored:

> for the delivery receipt of a **request you sent** (the *ack deadline*)

### 2. The list of places `Deadlines` is required is missing one

**`ARCHITECTURE.md:359`** (decision 22's new plain-terms lead) and
**`docs/EMBEDDING.md:31`**.

Both enumerate the construction sites as `Phone::converse` and
`install`/`install_party`. There is a third: **the runtime constructors**.
`LocalRuntime::new` and `KameoRuntime::new` each take a mandatory
`spawn_deadlines` — the patience given to harness-spawned actors — and
decision 22's own body says so ("Runtimes take `spawn_deadlines` for
harness-spawned actors").

`EMBEDDING.md` never shows a runtime being constructed anywhere in its 167
lines, so an embedder following it meets this as a compile error rather than
as documentation. That is a soft landing, but the point of the section is to
stop the surprise, and the list is phrased as complete.

### 3. The third-runtime obligation list omits decision 26

**`docs/EMBEDDING.md:157`.**

The errand asked me to confirm this list is complete against what
`LocalRuntime` actually does. It is not. The section names three spike-2
obligations — the deadline choreography (22–24), the dual failure surface
(25), and the death fan-out (27) — and skips **decision 26 entirely**.

`LocalRuntime` calls `fail_exchange` with four causes:

| Cause | `runtime.rs` | In the list? |
|---|---|---|
| `DeadlineElapsed` | 381 | yes (22–24) |
| `Disconnected` | 291 | yes (27) |
| `IllegalReply` | 514 | **no** |
| `SendFailed` | 527 | **no** |

A third runtime built from this list would not fail an exchange when the
transport refuses a send, nor when its own party returns an illegal reply —
reintroducing precisely the silent wedge and the un-deadlined hang that
findings 2 and 3 of the timeouts review closed. Those two are arguably the
*most* important obligations to state, because both are invisible until
something goes wrong.

Suggested repair: add "positive evidence fails an exchange at once — a refused
send or an illegal reply from your own party (decision 26)" to the list, and
change "the readable reference for all three" to "all four".

### 4. WORKFLOW.md states a two-toolchain agreement that nothing has tested — **RESOLVED, no edit needed**

> **Update, later the same day.** Settled by evidence, and the sentence stands
> exactly as written. Mez's cold offline run on the fresh clone reported
> `cargo fmt --all --check` **clean, exit 0**, under a genuinely 1.95
> toolchain (rustc/cargo 1.95.0, rustfmt 1.9.0-stable). A tree formatted by
> 1.97's rustfmt is therefore also already-formatted by 1.95's — which is
> precisely the claim. Verified from both sides rather than asserted, so
> **WORKFLOW.md needs no change**, and this drops off the fix list.
>
> One point of scope worth keeping in view: what is now verified is that the
> two rustfmts agree **on this tree**, not that they agree in general. Future
> code could hit a construct where they diverge. That is exactly why the
> fallback the section already carries — a bump that breaks agreement defers
> to the pinned cloud version — earns its place, and it should stay.
>
> The original finding follows, kept for the record.

**`docs/WORKFLOW.md`**, "The gate": *"The two toolchains' rustfmt outputs were
verified to agree (1.95 and 1.97), so no canonical formatter version is
needed."*

Casey confirms he ran the sweep himself, from the local repo — so on **1.97**.
That means the sweep is not evidence of agreement: it is one toolchain's
output, and 1.95's rustfmt has not formatted or checked this tree at all. The
1.97-clean result I report above is the *same* toolchain again, so it adds
nothing to the claim either.

This matters more than it would have yesterday, because the same commit makes
`cargo fmt --all --check` a **standing gate item**. If the two rustfmts differ
anywhere in the workspace, the gate fails for whichever session is not the one
that last formatted — and the natural repair (reformat on the failing side)
flips the failure to the other session. That is a ping-pong between Casey's
1.97 and Mez's 1.95 sandbox, and it would present as a mysterious
formatting-only diff rather than as a version disagreement.

Cheap to settle rather than soften: **Mez runs `cargo fmt --all --check` on
1.95 in the cloud against the merged tree.** Clean means the sentence was
right all along and is now actually verified. Any diff means the workspace
does need a canonical formatter version, which is exactly what the sentence
currently says it does not.

Until one of those happens, the honest wording is that the tree is formatted
with 1.97 and 1.95 has not been checked. The fallback the section already
carries — a future bump that breaks agreement defers to the pinned cloud
version — is the right shape and should stay either way.

*(My earlier draft of this review attributed the 1.95 half to a run of Mez's.
Nothing I saw supported that; I should have asked whose run it was instead of
assuming there had been one.)*

## The Q1 language pass, otherwise

Simplified and unchanged in meaning, apart from finding 1. Checked
phrase-by-phrase against the decision bodies:

- *"you choose your own time limits ... there is no way to say 'wait
  forever'"* — matches "no `Default`, no `Option`, and no unbounded variant"
  exactly, in words a non-implementer can hold.
- *"the other side never sees your limits — it has its own"* — matches "each
  side's deadlines bound its own waiting only and never cross the wire".
- Glossary *"chosen where the surface is constructed"* for "configured per
  conversation surface" — same claim, plainer.
- Glossary *"fails the exchange — **never** the conversation"* (was "not the
  conversation") — this is an improvement, not drift: decision 23's own
  wording is "terminal for the *exchange*, never the conversation", so the
  pass moved the glossary *closer* to the decision.

**Item 3 — the spike wobble is settled.** *"hosts the third architectural
spike — spike 2, counting from spike 0, which preceded the workshop forks"*
does the job for a cold reader: it names both numbers and explains why they
differ, in one clause, without a detour.

## Nits

- **`docs/EMBEDDING.md:53`** — "`Timer` is **not object-safe**" is true, and I
  verified it rather than trusting my own earlier claim: `Box<dyn Timer>` fails
  to compile. But on 1.97 the actual error an embedder sees reads *"the trait
  `Timer` is not dyn compatible"* — rustc renamed the concept. A parenthetical
  ("newer toolchains call this *dyn compatible*") would save someone a search.
- **`docs/EMBEDDING.md:72`** — "the observer is called under the runtime's
  lock" is true of `KameoRuntime`; `LocalRuntime` has no lock at all (its
  `emit` takes `&mut self`, and the borrow checker makes re-entry impossible
  anyway). The `Observer` trait's own doc hedges correctly with "runtimes
  *may*"; EMBEDDING states it flatly. The operative rule — observe, record,
  return — is right for both, so this is presentation, not substance.
- **`docs/EMBEDDING.md:79`** — "Get a `DeathWatch` from your transport
  (`net.death_watch()`…)". The code shown is right, but `death_watch()` lives
  on the net/registry object, not on the `Transport` handle. "from your
  network" would match the call.

## What I checked and found accurate

Skimmed as an embedder, flagging anything unimplementable or contradicted:

- `Deadlines { ack, turn }` field names, and both deadlines' scale
  characterisations (transport-scale / thinking-scale).
- `on_exchange_failed(&mut self, from, cause)` and its no-op default — the
  Party example at line 98 implements only `on_turn` and is consistent with it.
- `FailureCause` really has exactly the four variants the prose lists: a
  deadline elapsed, a send refused, an illegal reply, a disconnect.
- `set_observer`, the `FnMut(Event) + Send` blanket impl, and all three event
  variants — `ExchangeFailed`, `DeliveryDenied`, `IllegalTurnDropped`.
- `net.death_watch()` on both nets; `LocalRuntime::watch_deaths` and
  `KameoRuntime::watch_deaths`; `wait::next_or_deadline_or_death`;
  `Phone::on_peer_death` — all exist, all public, all spelled as written.
- `kill_endpoint` as positive evidence and `partition`/`heal` as buffered
  silence — matches the adapter, including the "silence is never evidence"
  framing.
- `TokioTimer` from the kameo adapter and `TestTimer` from the memory
  adapter's `testing` module.
- The "judged against the peer's clock, which you cannot see" warning, which
  is both accurate and the most useful new sentence in the document for
  someone about to write a slow party.

## Suggested disposition

Three text edits and the branch is merge-ready; none of the three needs a
design decision. Finding 1 is the one I would not skip on the code side — it
is in the glossary, and the glossary is load-bearing by this document's own
account.

Finding 4 is closed: the 1.95 check was run and came back clean, so the gate
rule now rests on tested ground rather than an untested premise. Nothing to do
for it.

Still open, unchanged: workshop-2's fork has no `vendor-snapshot`, so every
green result across all four reviews — including this gate run — is from my
1.97.0 macOS host and none has been confirmed by an offline cloud build.
