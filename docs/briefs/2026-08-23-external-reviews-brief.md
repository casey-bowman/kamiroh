# Brief: three external reviews of spike 2, and what they changed

**Date:** 2026-08-23 (end of spike 2's feature work, before graduation).
**Prepared by:** Mez (the design session), from reviews Casey commissioned.
**Reviewers:** Gemini, Codex, and Grok — each given the repo read-only,
each reporting twice (mid-spike at `f997e206`, near-completion at
`c0e36604`). Their full reports live next to the repo in `gemini/`,
`codex/`, and `grok/`.

## Why this brief exists

The house rule for outside findings is the same as for inside ones:
verify against the code, then rule, then fix with a pinning test. Three
findings survived verification and changed the tree; the rest is
recorded context. This brief is the disposition record.

## What the reviewers agreed on

All three called the architecture sound and the failure discipline the
strongest part — mandatory deadlines, silence vs. positive evidence,
never-on-wire verdicts, conversations surviving failed exchanges — and
none found a behavior-blocking defect. Codex and Grok converged
independently on one new P2 (the memory batch receipt, below), which is
exactly the kind of agreement worth weighting.

## Findings verified and fixed (this branch)

1. **Stock parties never failed their shadow turn machines** (Grok, P2,
   both days). `EchoParty`/`CountdownParty` tracked private `TurnState`
   but did not override `on_exchange_failed`, so after a mid-exchange
   death or revocation the runtime's machine said "fresh Open legal"
   while the party's shadow said `MustAnswerFirst` and swallowed the
   surviving Open — in the examples embedders copy. **Fixed:** both
   parties `fail()` their shadow in `on_exchange_failed`; pinned by
   `a_countdown_party_answers_a_fresh_open_after_its_peer_died`.

2. **`LocalRuntime` wedged on a refused handover-ack** (Grok, P2). The
   `?` on the ack send sat after `on_incoming` had moved the machine
   and cleared the waits: an ack-send failure aborted the step — party
   never called, no failure declared, no deadline armed. Kameo ignored
   the same error and behaved correctly; the documented reference
   runtime was the less faithful one. **Ruled by Casey:** ignore the
   refused ack and let the reply send be the loud verdict (the arriving
   turn already proved delivery; a blip recovers; a real breakage fails
   via decision 26 seconds later). Pinned both ways:
   `a_refused_handover_ack_does_not_wedge_the_exchange` (broken
   transport → loud SendFailed) and
   `a_blipped_ack_alone_interrupts_nothing` (ack-only blip → exchange
   continues). Recorded as an addendum to decision 26.

3. **Memory `open_many` minted a receipt for a nonexistent endpoint**
   (Codex and Grok independently, P2). On iroh the batch receipt cannot
   exist without a successful connect; the simulator synthesized one
   unconditionally — more generous than the wire on exactly decision
   29's new promise. **Fixed:** a batch to an endpoint with no
   mailboxes at all errors (`UnknownAddress`), mirroring the pairwise
   no-route behavior, while a live endpoint whose named actors don't
   exist still acks (the names were never part of the promise). Pinned
   by `a_batch_to_an_absent_endpoint_is_refused`; the live-endpoint
   case stays pinned by `an_absent_sibling_discloses_nothing`.

## Documentation debt paid (flagged by Codex twice, Grok twice)

README: test count (39 → 80 as of decision 29), ports list grown to all
seven, status block now covers decisions 22–29 and names `kamiroh/1`.
Both adapters' "written without compiling" rustdoc blocks converted to
historical notes — the blind-writing era is workflow history, not live
uncertainty. EMBEDDING gained the death-watch lifetime note (watches
never close; dropping the Kameo handle detaches — abort is the off
switch), and `watch_deaths`' rustdoc now says so too.

## Recorded, not acted on

- Grok's altitude warning, seconded by Codex: don't let spike-era
  survival notes (processing-time clocks, fan-out races, observer
  non-reentrancy) ossify into architecture — decide in workshop-3 which
  become product constraints and which get simplified. This is already
  the standing shape of the two mandatory post-streaming revisits
  (decisions 26–27).
- Gemini's reports found nothing new; their value is independent
  confirmation that the architecture reads from outside as intended,
  plus a workshop-3 readiness framing worth keeping.
- Old binaries speaking `kamiroh/0` will refuse to connect to `/1`
  peers at the handshake — expected, and the correct failure (that was
  the point of the bump).

## Disposition

With these fixes the tree is what graduates: the reference
implementations model the discipline, the simulator is no more generous
than the wire, and the front page tells the truth. Remaining before
graduation: the cucumber-rs errand (unchanged), then the snapshot to
staging.
