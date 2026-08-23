# Handoff: the cucumber errand is done — for Mez

*From Ander, 2026-08-23. Cover note for `2026-08-23-cucumber-completion.md`,
which holds the detail — the crate-by-crate lockfile review, the step-0
sweep, and the gate numbers. Roadmap 6 is closed; two things want a decision
before graduation, and one of those is Casey's rather than yours.*

## Step 0 first, because it gates everything else

**Clean.** Both required checks came back empty on this host. Widened before
reporting, so the result says what *is* here: `arrayref-0.3.9` is cached —
the pre-compromise version, not the malicious `0.3.10` — none of the other
flagged packages appear, and there is no contact with the C2 address or
anything in its `/24`.

The bump proceeded on a clean host.

## What to fetch

| | |
|---|---|
| Branch | `code/cucumber-rs` @ `9b4a399e` (one commit on master `7defda84`) |
| Shelf | `vendor-snapshot` @ `157c6d61` — true orphan, 442 crates, 596 MB |
| `Cargo.lock` key | `2dba33cab2376ab4c1807cc2fb6100dcdb280746` |

The shelf's `Cargo.lock` blob is byte-identical to the branch's, so the two
cannot drift.

**Your offline build should just work, and I checked rather than hoped.** I
ran the full offline gate against the vendored tree *before* pushing the
shelf: 82 ordinary tests + 25 scenarios, and **zero** crates.io touches
(grepped the build log for `Downloading` and `Updating crates.io`). Online on
1.97 gives the same numbers, clippy zero, fmt clean.

## The specification is executable

All 25 scenarios across all four feature files are bound to real steps in
`tests/cucumber.rs`, running in about 1.5 seconds as part of
`cargo test --workspace`. The mapping tables in `features/README.md` are
retired — the promissory note is paid — and the rule they carried is now
**enforced rather than trusted**: the runner uses `fail_on_skipped()`, so an
unbound scenario fails the suite rather than quietly counting as covered.

Two binding choices you should know about, both documented where a reader
meets them rather than only here:

- **Story 3's note was heeded.** The allowlist-mutation scenarios bind
  `LocalRuntime`-side, so "the very next delivery" is literally true instead
  of true-if-the-caller-awaits. Your Gherkin needs no mailbox-ordering caveat.
- **One scenario runs on real QUIC.** *A conversation spans connections* is
  about connections, and the memory net has none — it routes by address and
  never dials. Binding it to the simulator would have asserted a fiction, so
  that one runs on the iroh adapter over loopback with a nested reactor, as
  `iroh_disconnects.rs` already pins it. It is the only reason `tokio` is now
  a dev-dependency, and tokio was already in the tree, so it adds no crate.

  This is a judgement call and I want it stated plainly rather than buried:
  24 scenarios are memory-backed and one is not. The alternatives were to
  skip it — breaking the very rule the README asks me to preserve — or to
  fake it. Neither seemed better.

## Two decisions

**1. The `h2` advisory — Casey's, not yours.**

`cargo audit` (not previously installed here; I installed it) reports
RUSTSEC-2026-0258 against `h2 0.4.15`. **It is not from this bump**: `h2` was
already in the lockfile, reached via `iroh → hickory-resolver → h2` and via
`hyper`. The advisory is six days old — newly disclosed against a dependency
we already had.

I did not bump it, deliberately. It is outside the errand's scope, and every
fixed version (0.4.16/17/18) sits **inside the few-days freshness window the
errand itself says to avoid** — the fix is fresh precisely because the
vulnerability is. That tension is a ruling, not a judgement I should make
quietly. Exposure is slight (a DoS against HTTP/2 servers, reached here only
through DNS resolution and relay client paths, in a spike that ships to
nobody) — but **the shelf I just pushed carries the vulnerable version**, so
it should not sit long. The fix is a lockfile-only `cargo update -p h2` plus
a shelf recut, and I can do both the moment Casey rules.

**2. The stock parties are single-conversation — a follow-up errand.**

Binding *"a running actor that admits two endpoints / and a conversation in
progress with each"* wedged, and the cause is worth your attention:

**`EchoParty` and `CountdownParty` keep one shadow `TurnState` for the whole
party, not one per peer.** With two peers on one actor, the first
conversation leaves the shadow in `AwaitingTheirTurn`, so the second peer's
opening turn is refused as `MustAnswerFirst` and silently swallowed. The
runtime keys conversations per peer (decision 17); the parties' shadow of
that state does not.

This is adjacent to but distinct from the external review's Grok P2. That
finding was that the parties never *failed* their shadow; this is that the
shadow is single-tenant to begin with. Same reason it matters: these are the
examples embedders copy, and one copied for a two-peer actor loses turns
without a sound.

I did **not** touch the stock parties — a behaviour change in `crates/` is
outside this errand's scope. The cucumber harness carries its own per-peer
party with the reasoning in a comment. Worth a small errand to either make
the stock parties per-peer or document them as single-conversation examples.

## The bounded `block_on` landed, and paid immediately

My carry-forward from the last two reviews: `block_on` now panics after 30s
rather than parking forever, with a message naming what a stuck future means
("something that should have arrived never did: an ack, a turn, a reply the
peer owed, or a delivery a party swallowed"). Two unit tests pin it,
including a `#[should_panic]` one that would hang the suite without it.

It caught two of my own binding bugs within the hour — as a bounded stall
with a diagnosis instead of an infinite one. That is exactly the class that
cost time in the fan-out and external-review rounds.

One honest limit: when the bound fires inside the cucumber runner it aborts
the run without a scenario summary. Better than hanging, worse than a single
failing scenario; the runner gets a generous 120s ceiling for that reason.

## One thing I could not find

The errand says "the incident record in the project docs governs". There
isn't one — nothing in `kamiroh/docs/` or beside the repo mentions arrayref,
and `docs/WORKFLOW.md` has no dep-bump hardening section. It did not block
anything (the host was clean, and the errand states the hardenings inline),
but a procedure that defers to a record probably wants that record to exist,
especially with graduation next.

## Bounds kept

Master untouched. No behaviour change anywhere in `crates/` beyond the
bounded `block_on` in the testing module, as scoped. Local `vendor/` and
`.cargo/` removed afterwards so ordinary builds go back to crates.io —
leaving the source-replacement config behind is a trap the moment
dependencies next change. Worktree clean on `code/cucumber-rs` @ `9b4a399e`.

Roadmap 6 is done. What is left before graduation is the two decisions above
and the snapshot to staging.
