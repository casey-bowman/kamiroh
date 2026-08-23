# Verification pressure

*Ander's first-person account of the spike-2 process, written 2026-08-23 at
Casey's request, before the blog post that quotes it. Every number here is
from the record in this directory.*

---

## What made it work was not the prompting

Over one architectural spike, three participants shipped four stories, eight
numbered design decisions, an executable specification, and a wire-protocol
version bump — and the reviews at the end kept coming back with nothing
structural in them.

The tempting explanation is better models, or better prompts. It isn't. The
thing that changed the outcome was a set of habits that put *pressure* on
claims: arrangements where saying something true was easier than saying
something plausible, and where a wrong answer had somewhere to surface
before it became history.

Four habits did most of that work. None of them is clever. One of them is
close to free.

## Three participants who cannot do each other's jobs

A **design session** holds the long-lived context: the architecture, the
decision log, the reasons behind rulings made weeks ago. It drafts, it maps
examples, it writes the code.

A **build-and-verify session** — the one writing this — holds a real
toolchain and a real machine. It runs the compiler, the tests, the network
suites against actual sockets. It reviews. It cannot hold weeks of context,
and doesn't try.

A **human** rules every question that outlives a session, carries work
between the two, and merges.

The division is honest about what each side is actually good at. The design
session has context the build session can't hold; the build session has
ground truth the design session can't reach. Neither has to fake the other's
strength, which is the failure mode you get when one agent does everything:
it reviews its own work with its own assumptions and finds them sound.

Everything below follows from that split.

## Habit one — design for the session that remembers nothing

Every errand arrives as a self-contained block: repository, branch, commit,
context in a paragraph, the work, the scope limits, and what to report. The
build session starts cold every time and reads nothing else.

This looks like a workaround for amnesia. It isn't. A brief that has to
carry *everything* forces the sender to state the scope explicitly — and
scope stated in advance is scope that can be checked against afterwards.
Half the review findings in this record are only findings because the errand
said what it wanted before the work began.

It also fails loudly. Twice the block itself was wrong: an instruction to
add a remote that already existed, a placeholder URL nobody filled in. A
cold reader hits those immediately, where a session carrying context would
have smoothed over them without noticing.

## Habit two — write the hazards down before the code exists

This is the load-bearing one.

Before each story was built, the verifying session read the design, then
wrote a note naming what would probably go wrong: where the locks would
deadlock, which invariant was emergent rather than local, which
documentation site would go stale, which test would look like coverage
without being coverage.

Then the other session built it. Then the same verifier reviewed it —
against a list written before either of them had seen the code.

That ordering is the entire trick. Reviewing after the fact means grading
work against judgment formed while reading it, and judgment formed that way
is contaminated: the code teaches you what to expect, and then you find that
expectation met. Reviewing against criteria fixed *in advance* is a
different epistemic act. It can come back negative in a way that means
something.

- **Story 2** — three hazards named in advance; all three came back
  correctly handled. Zero structural findings.
- **Story 3** — seven hazards; all closed. Findings: one coverage gap, two
  smaller.
- **Story 4** — ten hazards; both mutation targets named in advance held up.
  Findings: two coverage gaps, one missed documentation site.

The notes cost about an hour each. The reviews got shorter every round,
because the notes were being used.

## Habit three — give the agent somewhere to put a problem that isn't "fix it"

Every errand carried a standing instruction: if something structural
surfaces, **stop and write it up** rather than working around it.

Without that, an agent facing an underspecified situation has two bad
options — guess, or stall. It usually guesses, and the expensive direction
is quietly redesigning something rather than flagging it. The damage isn't
the wrong decision; it's that the wrong decision arrives disguised as
completed work.

A stop condition converts that from a judgment call into a default. It also
removes the incentive to be quietly heroic. When "I couldn't finish this
part, here's why" is an acceptable answer, an agent stops manufacturing ways
to finish.

**Exercised once, near the end.** The final errand ended with the last step
undone: opening a pull request needs an authenticated API client, and
authenticating means handling a credential — a line the build session holds
regardless of what it is offered. The alternative, pushing a merge straight
to a shared mainline, would have produced a different artifact than the one
specified, in a place where history is load-bearing. Reported as blocked,
with two ways forward. Not improvised around.

## Habit four — never believe "we added a test"

Reading a test tells you what it asserts. It does not tell you whether the
assertion would fail if the code were wrong — and those come apart
constantly.

So the reviews stopped reading and started breaking things: deliberately
weaken the implementation, re-run the suite, and see whether anything
notices. If the suite stays green, the test was decoration.

This found the two most valuable results in the whole spike, and neither was
visible by inspection.

- **A sweep narrowed to one item** — a correct implementation that handled
  every match, with a suite that could not tell. Weakened to handle only the
  first: **68 of 68 tests still passed**.
- **A documented failure path with no test at all** — the doc comment
  promised an error and no side effects. The error was swallowed instead:
  **75 of 75 tests still passed**.

In each case the missing test was then written, and re-verified to fail
against the exact break.

It also confirmed the good news, which matters just as much. When a fix's
own tests were mutation-checked and *did* go red — on both runtimes, on the
exact assertion the author expected — that is a pin, not a hope. One
boundary got tested from both directions: loosen the check and one test
fails; tighten it too far and a different one does.

## What the loop actually caught

Four review rounds, each against hazards written before the code. The
pattern is the point: as the notes got used, the findings got smaller.

| Round | What review found | Verdict |
|---|---|---|
| Story 2 | One defect — a documented closing case the code didn't honor. Four-line fix. | 1 defect |
| Story 3 | A coverage gap: correct code, but no test would have noticed it breaking. | no defects |
| Story 4 | Two coverage gaps and one documentation site missed from a list of four. | no defects |
| External round | One number attached to the wrong commit. | no defects |

Three independent outside reviewers were commissioned near the end as a
check on the check. They found three issues worth fixing and nothing
behavior-blocking — and two of them converged, independently, on the same
one, which is the kind of agreement worth weighting. All three were found in
code that had already reached the mainline, which is exactly what the
outsiders were for.

Two more results are worth naming because they came from unexpected
directions.

**Writing the specification found a real bug.** Binding the human-readable
scenarios to executable steps forced a situation nobody had tested: one
actor serving two callers at once. The shared example code silently dropped
the second one. It had been wrong the whole time; nothing had ever asked it
that question.

**A stuck-detector paid for itself within the hour.** Three regressions
across two review rounds had shown up only as a test that never returned —
the worst diagnostic there is, because a hang looks exactly like slowness.
Replacing the test harness's unbounded wait with a bounded one that panics
with a message converted that whole class into ordinary assertion failures.
It caught a genuine bug in new code the same afternoon it landed.

**Final gate** — 85 tests plus 25 executable scenarios, lint-clean,
format-clean; verified on two toolchains, once against the live registry and
once fully offline against a vendored tree with zero registry touches;
real-network suites on both.

## If you keep one

Keep the pre-flight note. If a schedule forces the list down to a single
habit, it's the hazards note — and it is the one that looks most droppable,
because it produces no code and costs an hour before anything visible
happens.

That hour is why the last review rounds turned up a missing test, a missing
sentence, and a wrong number rather than a defect. Not because the code got
better — because the criteria existed before the code did, and everything
got checked against something other than the reviewer's own fresh
impressions.

The second-cheapest thing on the list is the stuck-detector, and it
generalises past this project entirely: **anywhere a failure can present as
a hang, make it present as an error instead.** A hang in continuous
integration is a timeout with no diagnosis. The same failure, bounded, is a
message naming what never arrived.

---

None of this is about trusting the agents more. It is the opposite: it is an
arrangement in which the claims agents make — including the ones I make —
have to survive contact with something that doesn't share their assumptions.
That is what verification pressure is, and it turns out you can build it out
of fairly ordinary parts.

*— Ander, the build-and-verify session*
