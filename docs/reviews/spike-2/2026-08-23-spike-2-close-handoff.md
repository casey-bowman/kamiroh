# Handoff: spike-2 close-out checked, h2 landed — for Mez

*From Ander, 2026-08-23. Covers the one-commit check of `aa4c6cf6` and the
h2 fix stacked on it as `code/h2-fix`. Short, because there is one thing to
fix and it is a date.*

## Verdict

**Merge the stack.** `aa4c6cf6` is sound — the per-peer parties are
genuinely pinned, the WORKFLOW additions match what actually happened, the
incident brief is accurate against my step-0 report, and the decision-26
ordering sentence checks out against the code. **One documentation finding**,
below, and it is yours rather than Casey's.

The h2 bump is done, pushed, and `cargo audit` is clean.

## The check

**The per-peer keying is pinned.** I collapsed it back to a shared slot —
every peer keyed to one entry, the pre-fix behaviour without restructuring —
and **2 of the 3 new unit tests fail**, `a_countdown_party_serves_two_peers_at_once`
among them.

The third, `an_echo_party_serves_two_peers_at_once`, survives the mutation.
That is correct rather than a gap, and your own comment says why: Echo
returns to `Idle` inside a single `on_turn`, so it was accidentally safe
before. Worth knowing the test documents a property rather than guarding a
regression.

**My cucumber suite passes 25/25 against the fixed stock parties** — and
also passes *under* the mutation, because the harness carries its own
per-peer party. The unit tests are the pin here, not cucumber. That is the
right division: the harness proved the bug existed, your unit tests keep it
dead.

Gate on `aa4c6cf6`: 85 ordinary tests (82 + your 3) and 25 scenarios, clippy
zero, fmt clean.

**The per-peer countdown improves on the template.** `RecordingParty` shared
one counter across peers; `CountdownParty` now gives each peer its own, so
two callers count independently. That is the more honest reading of what a
per-conversation shadow means, and the shared `next_id: 100` start is fine —
ids need uniqueness only within a conversation.

**The WORKFLOW hardenings are accurate**, including the security-fix
carve-out — which this errand then immediately exercised, so the clause
earned its place the day it was written. **The decision-26 sentence holds
against the code**: `connection_to` fails with `UnknownPeer` straight from
the peer-book lookup, before any packet moves, so "a partition cannot hide
it" is literally true.

## The one finding: five dates are a day ahead

The incident brief says *"closed 2026-08-24"*, *"Ander's host: checked
2026-08-24"*, and *"cucumber-rs, 2026-08-24"*; WORKFLOW says attribution was
*"adopted 2026-08-24"*; `parties.rs` says *"ruled by Casey … 2026-08-24"*.

**It is still 2026-08-23 in Casey's timezone** — 14:40 CDT as I write this.
The cucumber errand and my step-0 host check both ran today, the 23rd, and my
completion brief is dated accordingly. The house convention is Casey's local
date, so all five want to be 08-23.

Worth fixing rather than shrugging at, because the incident brief is an
**in-repo historical record that WORKFLOW.md explicitly defers to**. A record
that exists so a procedure has something to point at should not misdate the
host check it certifies.

Smaller, same file, and a genuine question rather than a correction: the
brief says `zmij` was *"investigated independently by both sessions"*. I
investigated it; whether you did, I cannot see from here. The both-sessions
clause was written after that bump shipped, so if it was not exercised then,
"first bump under the full procedure" is slightly ahead of itself too. Your
call — you know what you checked.

## The h2 note

**Landed `h2 0.4.18`. `cargo audit` reports zero vulnerabilities** (the two
`unmaintained` warnings are long-standing and pre-existing). Lockfile-only
and minimal: one package moved, nothing added, nothing removed.

**0.4.18 rather than the advisory's stated 0.4.16 minimum, and that matters
— 0.4.16's fix was itself buggy.** It limited excessive small DATA frames but
mishandled EOS frames, corrected in 0.4.17; 0.4.18 then adds the
`data_frame_budget(n)` knob to both builders. Taking "the latest patched
0.4.x" rather than "the minimum the advisory names" was the right
instruction, and anyone pinning to 0.4.16 on the advisory's word alone would
have shipped a half-fix.

I read the mechanism rather than trusting the version number: a
per-connection budget, defaulted to the small-frame overhead threshold ×100,
charged as small DATA frames arrive, with flood tests in the crate's own
`recv.rs`.

| | |
|---|---|
| Branch | `code/h2-fix` @ `6faaaa58` |
| Shelf | `vendor-snapshot` @ `f806e3a5` — orphan, 442 crates, vendored h2 confirmed 0.4.18 |
| `Cargo.lock` key | `c294d90e1e402a90ab28dee3693d76c95a9dfea2` (sha256 `5d3d8c2b5e5d6977…`) |
| Gate | 85 ordinary + 25 scenarios, clippy zero, fmt clean |
| Offline gate | same, **zero registry touches** — run before the shelf was pushed |

h2 reaches this tree only transitively (iroh → hickory-resolver → h2, and
hyper), so the real regression check is that the real-socket iroh tests still
pass. They do.

**Attribution:** the commit is authored `Ander`, `Co-authored-by: Casey
Bowman`, per the new convention — matching the existing
`Ander <case.bowman@gmail.com>` identity already in this history rather than
inventing a new one.

One mechanical note for the merge: `cowork/spike-2-close` has no ref on the
fork, but `aa4c6cf6` rode up as an ancestor of `code/h2-fix`, so the whole
stack is there.

## Bounds kept

No behaviour change of my own anywhere — the h2 commit touches `Cargo.lock`
and nothing else. The mutation to `parties.rs` was restored from backup and
re-verified (13 pass). Local `vendor/` and `.cargo/` removed afterwards so
ordinary builds go back to crates.io. Worktree clean on `code/h2-fix` @
`6faaaa58`.

After Casey merges, the only thing left in spike 2 is the graduation
snapshot, which is mine. Engine's warm.
