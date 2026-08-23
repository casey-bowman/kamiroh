# kamiroh spike 2 review — 2026-08-23

**Reviewer:** Grok
**Target:** `kamiroh` at `c0e36604` (`master`), since yesterday's snapshot at `f997e206`
**Prior snapshot:** [2026-08-22-spike-2-review.md](2026-08-22-spike-2-review.md)
**Status:** Stories 1–5 delivered (decisions 22–29). Item 6 (cucumber runner) still unbound.
**Constraint:** inspection only. This review did not write into `kamiroh/`. Same-day local confirmations on this commit reported the workspace green (76 tests).

This is a dated snapshot. A later reader finishing the spike should read it after the 22nd review, not instead of it.

---

## Since yesterday

In one day the two remaining *behavior* items landed, both through the house cycle (mapping board → implementation → adversarial review → r2 fixes → merge):

- **Decision 28 — allowlist mutation.** `admit` / `revoke` on running actors, local API only. Revocation is positive evidence (`FailureCause::Revoked`) and fails live exchanges through the same `fail_exchange` seam as death. Phones get `on_revoked` because the runtime cannot see the app's own list. Wire mutation deferred with the operator port — the right call; a stolen key must not own its repair channel.
- **Decision 29 — same-endpoint fan-out.** `Transport::open_many` / `Phone::open_many`. One send, one request id, N ordinary Phones. Batch receipt is an ordinary `Ack` from the mailroom convention, promised at *endpoint arrival, before admission*. ALPN bumped to `kamiroh/1` so mismatched frames fail at handshake rather than hanging. Runtimes unchanged — fan-in happens in the adapters.

The spike question is now answered for every candidate implementation item except the cucumber runner. That runner was never the question; it is a standing documentation obligation.

---

## Verdict

Decisions 28 and 29 fit the existing architecture instead of bending it. Mutation reuses `fail_exchange` and the death-shaped "conversation survives." Fan-out is a transport affair that ends the moment the Phones exist. The tests that landed with them are the right shape: idempotent second revoke, multi-conversation sweep at one endpoint, key rotation without restart, denied/absent siblings as silence, refused batch creating no phones, real-QUIC one-frame fan-in.

I would still not call the reference types finished. Yesterday's two P2s are untouched, and fan-out added one new honesty gap in the memory adapter. None of those block graduation by themselves. All three will waste workshop-3's first afternoon if they are still in the copy-paste path when an agent errand starts failing for "no reason."

---

## Findings

### Still open from 2026-08-22

**1. [P2] Stock parties still keep a private `TurnState` and never `fail()` it.**
`EchoParty` / `CountdownParty` are unchanged. Survival and revoke tests still use `AskingParty`, which does not track a machine, so they stay green. A `CountdownParty` that has posed a `Continue` and then sees `Revoked` or `Disconnected` will swallow the next legal `Open`. More relevant today, not less: revoke is now a first-class cause and fan-out uses `EchoParty` as the happy-path worker (safe only because it always `Close`s in the same call).

**2. [P2] `LocalRuntime` still treats a failed handover-ack as a step error.**
`runtime.rs` still `self.send(..., ack).await?` after `on_incoming` has moved the machine and cleared waits. Party not called. `fail_exchange` not called. Kameo still ignores that ack error and continues. The toy runtime remains the less faithful of the two.

**3. [P3] README is now farther from the tree.**
Still "39 tests"; still ports = `Transport`, `Inbox`, `Registry`, `Party`. The Status block still names only decisions 22–27. A reader of the front page today would not know mutation or fan-out exist, nor that the wire is `kamiroh/1`. Adapter rustdocs still say they were written against `kameo 0.17` / `iroh 0.35` **without compiling**, on crates that have since grown `admit`/`revoke` and an ALPN bump.

**4. [P3] Death watches still never close, and dropping the Kameo handle still detaches.**
Unchanged. Fine at spike scale; still the wrong shape for a long-running embedder.

### New since `f997e206`

**5. [P2] Memory fan-out can acknowledge a batch that never reached an endpoint.**

`MemoryTransport::open_many` always synthesizes the mailroom receipt into the *sender's* mailbox, then drops unbound names, then returns `Ok`. It never asks whether `to_endpoint` has any mailbox at all.

Decision 29's receipt promises *"the batch reached the endpoint."* On Iroh that promise is real: `open_many` must `connect` before a frame can leave, and the receipt is written by the *receiving* adapter after the frame arrives. On memory there is no endpoint-liveness concept — only mailboxes — so a batch to a completely absent `zz` is indistinguishable from a live endpoint whose named actors do not exist.

The suite pins the second case (`an_absent_sibling_discloses_nothing`, including the all-unbound half against a live `bb` that still hosts `real`). It does not pin the first. Pairwise memory `send` still returns `UnknownAddress` for no-route. So a memory-backed test can "prove" endpoint arrival in a scenario Iroh would refuse or leave silent.

Either give `MemoryNet` a notion of a live endpoint (a bound sentinel, an explicit `bring_up(endpoint)`, something the receipt can require), or make `open_many` return `UnknownAddress` when no mailbox belongs to `to_endpoint`, and keep the all-unbound-*live*-endpoint case as the explicit one.

This is the same class of problem as finding 2: the test double is slightly more generous than the real transport, and the generosity is exactly on the new promise.

---

## Notes — not new findings

- Decision 28's "local lever only" and "mutation never closes connections" are load-bearing for workshop-3. The eviction recipe in `EMBEDDING.md` (revoke everywhere, then `close_connection`) is the right two-step; do not collapse it.
- Decision 29's documented residue is real and said where embedders will see it: a denied/absent/duplicate sibling surfaces at the *turn* deadline. `EMBEDDING.md` tells the pump to tighten that deadline. Listen.
- The refused-batch path (`Phone::open_many` creates no phones on transport error) is now pinned by `a_refused_batch_creates_no_phones`. Yesterday that would have been a gap; r2 closed it.
- ALPN `kamiroh/1` is a clean break. Historical `kamiroh/0` remains in archives only. Internet-check and Incus-check binaries built against `/0` will not speak to a `/1` peer — expected, and the handshake failure is the correct one.
- Accepted residues from decisions 26–27 (processing-time clocks, death fan-out race, observer non-reentrancy) are unchanged. I am not re-raising them.

---

## Spike 2 remaining

```
[x] 1. Timeouts                 decisions 22–26
[x] 2. Disconnects              decision 27
[x] 3. Allowlist mutation       decision 28   (landed today)
[x] 4. Deny observability       decision 25
[x] 5. Same-endpoint fan-out    decision 29   (landed today)
[/] 6. BDD / cucumber-rs        features written and mapped; runner not bound
```

Item 6 does not block the spike question. Bind it on the next vendor-snapshot refresh; do not hold graduation for it.

What I would still do before treating this tree as the workshop-3 base, in order:

1. `fail()` (or drop) the stock parties' private `TurnState`. Add a `CountdownParty` reconnect-after-revoke case.
2. Make `LocalRuntime`'s failed handover-ack go through `fail_exchange` (or match Kameo's ignore).
3. Stop memory `open_many` from minting a receipt for a nonexistent endpoint.
4. Sweep the README (count, ports, Status) and the adapter "without compiling" rustdoc.

Then snapshot to staging. Streaming and the agent-control vocabulary belong to workshop-3.

---

Altitude on the project as a whole is still [2026-08-22-overall-sense.md](2026-08-22-overall-sense.md). Today's work does not change that reading; it makes the "workshop-3 can ride this" half more true, provided the three P2s above do not become the first agent-errand bugs.
