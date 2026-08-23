# Example-mapping meetings

One subfolder per meeting, named `YYYY-MM-DD-topic/`, holding that session's
board in its final state — story cards, rule candidates, and question cards
with the rulings recorded inline as they were made — plus any other artifacts
the session produced.

Example mapping is where workshop-2's designs are born (see
[ROADMAP.md](../ROADMAP.md), item 6): humans and agents map a story's rules
and examples together, the human arbitrates every red card, and the outputs
graduate outward — accepted rules become numbered decisions in
`ARCHITECTURE.md`, examples become Gherkin scenarios in `features/`, and
implementation follows as an ordinary errand. The board itself stays here,
in full, for the same reason `docs/advisories/` exists: the decision log
says *what*, the board preserves *how the session actually reasoned* —
including the alternatives considered and declined, which are often the
most valuable part to a future reader wondering "did they think of X?"

A board is a historical record once its meeting ends: rulings recorded here
are not edited afterward. If a ruling is later overturned, that happens in
the decision log (a new numbered decision), not by rewriting the board.

Meetings so far:

- [`2026-08-19-timeouts-disconnects/`](2026-08-19-timeouts-disconnects/) —
  the first mapping session, on the two hardest roadmap items together.
  Produced decisions 22–25, `features/hung-exchanges-fail-loudly.feature`,
  and the rulings story 2 (disconnects) will be built from.
