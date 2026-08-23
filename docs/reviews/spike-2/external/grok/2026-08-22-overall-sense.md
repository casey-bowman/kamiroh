# kamiroh overall sense — 2026-08-22

Companion to [2026-08-22-spike-2-review.md](2026-08-22-spike-2-review.md).

It is unusually well-shaped for a spike. The project is not mainly proving that Kameo can talk over Iroh. It is carving a vocabulary — conversation, exchange, turn, deadline, death — and then making illegal states unrepresentable, with tests that pin those words from a pure domain out to real QUIC.

The strongest part is the failure discipline. Deadlines are mandatory and local. Silence and positive evidence are different things. Failures never cross the wire. Conversations survive dead exchanges. That set of choices is coherent, and it is already more real than “toy spike” suggests.

The risk is not missing architecture. It is overfitting to spike-era survival notes: processing-time vs arrival-time, observer non-reentrancy, death fan-out races, connection generations. None of that looked accidental, which is good. The next pass has to decide which of those become product constraints and which get simplified before the public API hardens.

In short: promising, thoughtful, and already a substrate rather than a demo. Workshop-3 can ride this if the remaining hardening (live allowlist mutation, the two reference-implementation traps in the review) lands. The danger is starting agents on top of notes that were true for echo parties and then discovering they were the architecture.
