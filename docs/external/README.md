# External findings

Reports written against this repository by people outside it, published
verbatim alongside what was done about them.

The pairing is the point. A report on its own is a claim; a report next to the
commit that acted on it is evidence — of what was checked, what was accepted,
and what was not. Reports here are **input, not authority**: publication does
not mean every word was adopted, and the disposition beside each one says what
actually happened. This extends to documentation the practice spike-2 already
used for code, where three outside reviews were published verbatim with their
dispositions.

Findings land in the **same commit** as their fix, so the two are one
`git show` apart.

## Index

### `2026-08-27-vendoring-findings.md`

Two steps in `docs/VENDORING.md` that fail as written — a restore whose fetch
never creates the ref it then uses, and a refresh procedure whose last step
aborts and rewards a destructive recovery.

Received 2026-08-27 from a confidential embedder project and published verbatim
with its author's and second reader's consent — Riveter (author) and Welder
(second reader),
session codenames per the same convention that names kamiroh's own sessions
(see `WORKFLOW.md`). Both findings were reproduced independently on the kamiroh
side before publication — finding 1 by two seats, once against this repository
itself. The fixes land in the same commit as this file.

Both suggested fixes were adopted as written. Kamiroh added three things the
findings did not ask for: an explicit post-condition for the refresh
procedure, a standing rule that no step deletes anything implicitly, and a
scoped optional cleanup step. See `VENDORING.md`.
