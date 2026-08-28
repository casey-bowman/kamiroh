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
(second reader), session codenames per the same convention that names kamiroh's
own sessions (see `WORKFLOW.md`). Both findings were reproduced independently
on the kamiroh side before publication — finding 1 by two seats, once against
this repository itself. The fixes land in the same commit as this file.

Both suggested fixes were adopted as written. Kamiroh added three things the
findings did not ask for: an explicit post-condition for the refresh
procedure, a standing rule that no step deletes anything implicitly, and a
scoped optional cleanup step. See `VENDORING.md`.

**One point where this report and `VENDORING.md` disagree, said out loud so a
reader does not have to guess whether it is a transcription error.** Finding 2
describes `git checkout -f` as discarding untracked files, and warns the reader
about files they had added. Reproduced here with both cases planted, it does
the opposite of what that warning implies: **a file you added survives, and an
edit you made to a file already on the target branch is silently discarded.**
Since `git rm --cached .` leaves every source file untracked, `-f` overwrites
them all from the target tree without a word, and the result is
indistinguishable from a clean return. The hazard is narrower than the report
states and worse — the mitigation inverts. The report stands as received;
`VENDORING.md` carries the corrected wording, and the correction has gone back
to its authors.

The other difference is deliberate and documented in place: the report puts its
`git status` check before `symbolic-ref`, kamiroh runs it after, and
`VENDORING.md` names the source placement as equally safe.
