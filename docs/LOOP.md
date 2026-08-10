# LOOP

> **Three decisions are open and waiting on a person:**
> [OPEN-DECISIONS.md](./OPEN-DECISIONS.md) — a malformed allowlist stopping the
> node, whether NAT traversal actually works, and the security posture. The
> third narrowed: the advisor gate was met once and everything factual about
> `Reach::Anywhere` is now established, so what is left is one judgment about
> whether opt-in disclosure is acceptable. Raise them at the start of a planning
> pass; an item leaves that list by being decided, not by going stale.

## Current phase

**Phase 2 is finished** — M1–M4 and the review queue. The current pass is
[kamiroh-phase-3.md](./kamiroh-phase-3.md).

### Where to pick up

**P1 is done, in code and demonstrated.** It found that the control vocabulary
was the narrowest part of the system, and closed all of it:

| | was | is |
|---|---|---|
| the read | refused whenever the agent worked | source chosen by state, with a fallback |
| `Shutdown` | claimed to stop the agent | `Detach` — stops kamiroh controlling it |
| `Interrupt` | claimed to reach the agent | `StopWaiting` — gives up kamiroh's wait |
| watching a task | ask repeatedly, or not at all | `AwaitSettled`, a long-poll |

Everything above is live-verified against a real `claude` agent, not only
unit-tested. Neither rename moved a wire byte. The README was reworded to match.

**Phase 3's own question was decided along the way**: P2's "use it" half first,
then P1. Both of P1's earlier design questions turned out to be settled by
Herdr's API rather than by preference, and the third — push versus poll — was put
to Casey with the trade laid out and answered *long-poll*.

**What is next, in the order it is worth doing:**

1. **P2's other half — the two-machine NAT test.** The only claim in the README
   that has never been demonstrated, and now also the only place the dial-cost
   numbers P1 wants can be measured. Procedure and both measurements:
   [reachability-test.md](./reachability-test.md), §5a for the timings. Needs a
   second machine on a different network; a phone hotspot is the whole trick.
2. **P3a — say plainly what kamiroh does.** Casey asked for this on
   2026-08-10, for after the coding: a document explaining exactly what kamiroh
   does, directly and simply. It is a genuine gap — every doc here is written for
   someone *building* kamiroh or resuming work on it, and none for someone
   deciding whether to use it. Scoped in kamiroh-phase-3.md §P3a, and it comes
   **before** P4: publishing without it ships the gap to strangers.
3. **P4 — publishing**, whose blocking item (the README's lifecycle claim) is
   now fixed. What remains on that checklist is the crates.io logo URL and the
   repository-owner question, both written up in kamiroh-phase-3.md §P4.
4. **Nothing else is queued.** Phase 3's milestones are otherwise complete.

**One thing raised and not taken up:** whether kamiroh should say how *confident*
a status is, given M3 measured `codex` reporting a permission prompt as `idle`
where `claude` reports `blocked`. It is a candidate at the foot of
OPEN-DECISIONS.md, and it matters more now that `AwaitSettled` exists — a kind
whose manifest misses `blocked` makes that verb quietly useless. Measuring a
second agent kind would settle whether it is real.

**The recurring defect this pass kept finding, worth carrying forward.** Six
times, the code was right and something *describing* it promised more — two verb
names, two pane mappings, three cached statuses, and the README. Every one leaned
the same way: telling an operator an agent was fine when it was not. When
touching this codebase, the doc comment and the sidebar label are as likely to be
wrong as the logic, and they are the parts no test looks at.

**The lettered plan is finished.** A→J are all complete.

| | slice | what it left behind |
|---|---|---|
| A/B | workspace, port traits | the crate graph and the boundary |
| C/D | domain, app | folded into A/B rather than run separately |
| E | `kamiroh-adapter-fs` | key custody on disk |
| F1 | `kamiroh-adapter-iroh` | real endpoint identity |
| F2 | `kamiroh-adapter-iroh` | codec, transport, inbound front |
| G | `kamiroh-adapter-kameo` | one controller actor per agent |
| H | — | **absorbed**: each slice wired itself in as it landed |
| I | `kamiroh-adapter-fs` | the allowlist file |
| J1 | `kamiroh-adapter-herdr` | the pane console, local or remote |
| J2 | `kamiroh-adapter-herdr` | agent state reported into Herdr |

Two slices were split (F, J) and one dissolved (H) — each recorded where it
happened rather than quietly.

Every driven port resolves to an adapter that touches the world.
`kamiroh-adapter-memory` is now test doubles plus one production caller:
`InMemoryAllowlist`, for the `KAMIROH_ALLOW` override.

**M1 removed the last stand-in.** With `KAMIROH_AGENT_TARGET` set, a prompt
reaches a coding agent that Herdr is managing. `EchoAgent` remains for nodes
with no agent runtime, and for tests.

## Done

**The README now says what the verbs say**

Phase 3's pre-publish checklist flagged it: the README promised *"start and
manage long-running agents"*, and kamiroh does neither. It attaches to an agent
Herdr started, and cannot stop one.

**The fix was not only rewording aim 1, it was moving it out of the aims.** As an
unmet aim under a status line reading "not all of the aims above exist yet",
starting agents looked like unfinished work. It is a **stated non-goal** — the
one place kamiroh could grow that would make it a worse Herdr — so it now sits in
*Non-goals* with the reason, next to the fact that no way to stop an agent exists
that does not mean managing panes or sending per-kind keystrokes.

The rest followed from the same standard the verbs were held to:

- Aim 2 lists what a caller can actually say, in the verbs' own words.
- The Herdr bullet no longer offers "start/attach/monitor"; a pane is a terminal
  you type at, and starting and supervising are Herdr's.
- The status section separates *built* from *unproven* from *never*. Aims 1, 2, 4
  and 5 are built and exercised against real processes; aim 3 carries the one
  claim that has never been demonstrated — NAT traversal, which is the headline
  home ↔ cafe case — and points at open decision 2. And it says plainly that a
  non-goal is not a thing arriving later.

Worth noting the shape, because it is the sixth of these: the code was right and
the description promised more, in the direction a reader would act on. The
difference here is that the reader is a stranger rather than an operator.

**P1 (third slice) — `AwaitSettled`, the verb for "tell me when it needs me"**

P2 measured what a coding agent's work actually looks like: long stretches of
running punctuated by stops that need a human — three live runs, three permission
dialogs. Until now the only way to learn about one was to keep asking. Now a
caller says "tell me when it settles" and the node holds the request until it
does, or for twenty seconds, whichever comes first.

**Long-poll rather than push, and Casey chose it after the trade was laid out.**
The alternative was a streaming front: keep the console's connection open and let
the host send events down it. Two arguments landed the same way against it.

- **Asking for state is self-healing; delivering events is not.** A caller in a
  tunnel misses a *transition* permanently, and recovering it needs
  state-on-reconnect — which is this verb again, bolted onto a push. Asking "what
  is it now" is idempotent: come back, ask, be correct.
- **A connection that closes after each reply keeps the allowlist checked per
  request.** That is what makes `SIGHUP` revocation immediate today. A
  subscription is authorised *once*, so a peer removed at hour two keeps
  receiving an agent's state until someone notices — a trust change wearing the
  costume of a performance feature, and the same shape as everything else this
  pass has been removing.

Push's remaining win is narrow and real: no round trip every twenty seconds, so a
laptop's radio idles. What that round trip costs was read out of Iroh rather than
guessed — one `Endpoint` is held for the life of the process, so a redial reuses
the peer's addresses and hole-punched path and pays about one handshake. The real
numbers get taken during the two-machine test, which is the only place they mean
anything: [reachability-test.md §5a](./reachability-test.md).

**No new reply, and no timeout parameter.** It answers with
`ControlReply::Status`, which is already unambiguous — `Blocked` means it needs a
human, `Idle` means it finished, `Busy` means patience ran out and ask again. And
the verb carries no timeout, because how long a node holds an actor open is the
node's business; a peer that could name the number could name a large one.

**The §6d hazard was the hard part, and it is the one this repo keeps walking
into.** Twenty seconds of inline await inside the handler would freeze the
mailbox for twenty seconds — `Status`, `StopWaiting` and `Detach` all
unanswerable. Slice G wrote that rule; M1 broke it while citing it. So an await
is **spawned**, exactly as a prompt is, and reports back through the mailbox. A
test dispatches `Status` and `Detach` while an await is outstanding and requires
both to answer.

**Three consequences that had to be decided rather than discovered:**

- **`abandon` covers awaits too.** §6d says nobody waiting is left hanging, and
  an await is the longest wait this actor holds — after a `Detach` the actor is
  gone, so anything left would never be answered by anyone. It also keeps
  `StopWaiting` honest: a verb named for giving up a wait that left one running
  would be its own small lie.
- **The controller bounds a no-opinion agent, not the port.** An agent whose
  state only moves when it runs answers instantly, so a caller long-polling one
  would spin at full speed — and `EchoAgent` is the production stand-in for a
  node with no agent runtime, not only a test double. The spawned task sleeps out
  the remainder. The port cannot do this: it has no clock, and the actor must not
  sleep in a handler.
- **One waiter at a time, refused rather than queued** — two would both be
  answered by one settle and the second would hang. Same reasoning as concurrent
  prompts.

**The substrate was checked before the design, and it moved the design twice.**
`events.subscribe` looked like the primitive; `agent.wait` turned out to fit
kamiroh's existing client exactly — one request, one reply, connection closes —
and it takes a *set* of states rather than one. And it **answers at once when the
agent is already in one of them**, rather than waiting for a fresh transition,
which is the difference between reporting an already-blocked agent and never
reporting it. Verified against herdr 0.8.0, because the two behaviours are
indistinguishable from the documentation.

**A measurement trap worth recording, because it invalidated an earlier
conclusion.** Probing that socket with `printf | nc` half-closes stdin, and Herdr
drops a long-running request when a client does that. Every long call looked like
it returned nothing. Held open — which is what kamiroh's own client does — the
same requests work. That retroactively invalidates the evidence used to conclude
the first P1 live run's failure "was not kamiroh's": that probe proved nothing.
Corrected below.

**And the fifth instance of the same defect, fixed on a follow-up.** The
`Finished` handler set `status = Idle` when a run *fails*, directly beneath a
comment saying that claiming `Idle` would invite another prompt into the same
failure — the comment and the code disagreeing in the same file. The case that
makes it concrete is the read refusal this pass began with: the agent is working
perfectly well and only kamiroh's question failed, so reporting it free is wrong
exactly when a remote operator would act on it. The cached `Busy` stands now.

Mutation-tested, and the first attempt at that mutation was itself wrong:
`Backend(Box::new(error))` appears twice in the file and the edit hit the
`Settled` handler instead, producing a green run that proved nothing. Anchored on
the surrounding comment, the mutation fails exactly one test. Worth recording —
a mutation that silently applies somewhere else is indistinguishable from a
property that is guarded.

**Verified live, and the half a fake could not show.** Against a real `claude`
agent: the prompt returned `[still working]`, `/await` answered `blocked`, and
Herdr's own timeline agreed (`idle → working at 10s → blocked at 15s`). The agent
had hit a permission dialog and the verb reported it, end to end through domain,
codec, actor and Herdr client.

That run only exercised the *already-settled* path, though — by the time kamiroh
read `/await`, the agent was blocked already. The claim the whole design rests on
is that a wait fires **on the transition**, and that was measured separately and
for free, by pointing `agent.wait` at this session's own pane and letting the
turn end:

```
start, pane state: working
agent.wait until=[idle], timeout 120s  ->  {"status":"idle"}
```

A status rather than `code: timeout`, so the wait was released by the change
rather than by expiry. That is the latency claim, demonstrated.

**Verified:** 204 tests, fmt and clippy clean. The await tests run under paused
time — the first version genuinely slept twenty real seconds while its own
comment claimed otherwise, so the actor now measures against `tokio::time` rather
than `std::time`, the clock it actually sleeps on.

**P1 (second slice) — the verbs now say what they do**

Two of the four verbs claimed to reach the agent and reached only kamiroh's own
machinery. `Shutdown` is now `Detach` and `Interrupt` is now `StopWaiting`.

`ControlMessage::Shutdown` was documented as "ask the agent to stop" and stopped
the *controller actor*. The P2 run measured the gap: answered at 17:22:01, and
the agent wrote 297 lines at 17:27:32. Renamed rather than reimplemented, because
Herdr has **no method that stops an agent** — the only routes are `pane.close`
(pane management, a stated non-goal) or `agent.send_keys` (per-kind keystrokes,
which agent-agnostic forbids). A verb that reports success for something it did
not do is the worst of the options, and the other two were unavailable.

**Why `Detach` and not `Abandon` or `Release`.** It is the exact inverse of the
word this repo already uses — *"Herdr starts agents; kamiroh attaches"*. The
objection is that "detach" reads reversible and today it is not:
`KameoController::dispatch` looks up an existing actor and never re-creates one,
so the name answers `Stopped` for the life of the process. That is stated in the
doc comment rather than papered over, and it is an implementation limit rather
than a different meaning — if re-attaching ever arrives, the name is still right.

**The wire byte did not move.** `request_kind::DETACH` is still `4`, and the
codec round-trip tests passed untouched. That is the evidence this is a rename
and not a protocol change: an older peer is unaffected.

**The same lie was in the sidebar, which is the part a rename alone would have
left.** `state_after` mapped this verb's `Accepted` to Herdr's `done`, so a pane
announced that an agent had finished at the exact moment kamiroh stopped being
able to know. It reports `unknown` now — the rule §6d already applied to an
unreachable peer, applied to the case that broke it. The old behaviour had a test
named `shutdown_reports_done` asserting it was right; the new one says why it is
not.

**Two smaller places the old word was still doing damage.** The abandoned-prompt
reason string said *"the agent was shut down"* — and that one reaches a caller,
as `ControllerError::Rejected { reason }`, so it repeated the exact claim the
rename exists to remove. It says "kamiroh detached from the agent" now. And
`/shutdown` at the console answered "unknown command", which is a poor reply to a
word that worked an hour ago: it now names its replacement and says why, while
**sending nothing**. An alias would have kept the misleading word working, which
is the opposite of the point.

**`Interrupt` became `StopWaiting` on a follow-up.** It was held back for one
slice, deliberately: it has the same defect — it said "ask the agent to abandon
its current work" and only abandons the run kamiroh is waiting on — but the
controller survives it, so `Status` can still be asked, where `Detach` left
nothing able to answer. That asymmetry is real, and it turned out to be an
argument about *how bad*, not about whether it was true. Held to the same
standard as the other three, the name had to go.

`StopWaiting` over `Abandon`, the other candidate, because it needs no doc
comment to be honest — "abandon" leaves *abandon what?* open, and needing a
paragraph to explain a name is the failure this pass exists to remove. Its byte
did not move either (`3`), and the console command follows the same pattern:
`/stop-waiting`, with `/interrupt` explaining itself and sending nothing.

**`StopWaiting`'s status guess went too.** The handler set the
cached status to `Idle`, which claimed the agent was ready for work when all that
had happened was kamiroh giving up its wait — the same guess in the same
direction as the two above. It now touches the status at all: the value stays
`Busy` when a run was abandoned and unchanged when there was nothing to abandon,
and `Status` corrects it from the agent whenever the agent has an opinion.

**What made the choice easy is that the status is a report, not a gate.** `start`
refuses a second prompt on `running.is_some()`, never on the status, so a stale
`Busy` blocks nothing — the cost of being conservative is an inaccurate reading
until the next refresh, against the cost of being wrong, which is telling a
remote operator an agent is free when it is mid-task. The repo's existing rules
already pick that way twice ("a failure reports `unknown`, never `idle`"; "an
unknown state becomes `Busy`, not `Idle`").

The pane mapping went with it: an accepted `StopWaiting` reports `unknown` rather
than `idle`, for the same reason the detach mapping does. It differs in being
self-correcting — the controller survives an interrupt, so the next `Status` puts
it right — but `idle` is not true at the moment it is said.

`EchoController` in `kamiroh-adapter-memory` keeps stop-waiting → `Idle`, and that
is not an inconsistency: it is a `HashMap` with no agent behind it, so its status
is its whole model rather than a claim about something else.

Mutation-tested: restoring the `Idle` assignment fails exactly one test, the one
whose name now says what it is for.

**Verified:** 193 tests, fmt and clippy clean, and the compiler found every
propagation site — 26 references across six crates, exactly the shape M1
predicted for a `ControlMessage` change. The doc's promise that a detached name
is "refused as stopped" is pinned by a test asserting `ControllerError::Stopped`
specifically, not merely an error.

**P1 (first slice) — the read that could not run**

P2 found that a prompt to a working agent fails outright: kamiroh asks
`agent.read` for `recent`, and Herdr refuses that source while an agent is
working. So M1's `Partial{Busy}` path — the normal case for a coding agent —
could never execute. Fixed.

**The source is chosen by the settled state, *and* a refusal falls back.** Both,
because they cover different things:

- **Choosing** avoids a round trip that would always fail. A working or unknown
  agent is read `visible`; a settled one `recent`, which still includes what has
  scrolled away and is what a finished answer wants.
- **Falling back** covers what choosing cannot. The settled state is a
  moment-old observation of something that moves on its own — the same reasoning
  that put `Agent::status` in M1 — so an agent that had settled may be working
  again by the time the read lands. It also removes an assumption about
  `Blocked`: a blocked agent is not working, so it is asked for `recent`, and if
  Herdr declines to scroll a dialog too, the retry answers instead of the run
  failing. Nobody had tested that case; now nobody needs to.

**Only `agent_not_idle` is retried.** Falling back on any refusal would turn a
broken target into a plausible-looking answer, so `pane_not_found` stays a
failure. Matched on the **code**, never the message — established practice here,
and the prose is Herdr's to reword.

**And the fallback corrects the status, which is the part that was nearly got
wrong.** The first version kept `settled` and built the outcome from it, so the
race case answered `finished` — handing a caller a mid-task screen as if it were
an answer. But `agent_not_idle` is Herdr saying *at read time* that this agent is
not idle, and that outranks a state observed a moment earlier. The fallback
returns `Working`, so the outcome is `Partial{Busy}`. Reporting `Idle` for a
working agent is the direction §6e calls dangerous, and it was one assertion away
from being pinned as correct.

**The real deliverable is the test double.** `FakeHerdr::scripted` answered
positionally and never read the request, which is precisely why nine tests
agreed with a read the daemon rejects: *a fake that cannot disagree with a
request cannot catch a request being wrong*. `FakeHerdr::answering` now sees
each request, so a test can refuse one source and serve another, and
`read_sources()` asserts which was asked for. `scripted` is kept, implemented on
top of it.

**Mutation-tested, and how it fails is the interesting part.** Pinning the
source back to `Recent` fails the new test on the *source* assertion —
`["recent", "visible"]` — not on the outcome. The fallback rescues the result,
so the mutation costs a wasted round trip on every prompt rather than breaking
anything. Asserting the outcome alone would not have caught it; asserting which
source was requested does.

**What a `Partial{Busy}` now carries is weaker, and that is stated rather than
implied.** The screen mid-task may be a spinner and a half-drawn diff rather
than anything the agent has "said". The alternative was the prompt failing,
which is what it did. ARCHITECTURE.md §6e says so.

**Two corrections to the P2 write-up below.** It said kamiroh "has never passed"
a source. It did — `recent`, with a comment giving a good reason that is right
about a *finished* agent. Better than nobody setting it, and the correction
matters in a file that had just been through a stale-docs pass. And `lines`
alongside `visible` is a maximum, not a request: herdr 0.8.0 answered a 200-line
ask with the 57 lines on screen, so it is kept and documented rather than
dropped.

**Verified:** 192 tests, fmt and clippy clean. Both halves rest on observed
real-daemon behaviour rather than on the schema — the refusal code
`agent_not_idle` came out of the live P2 run, and `visible` + `lines` was checked
against herdr 0.8.0 directly, where a 200-line ask returned the 57 lines on
screen.

**Verified live, both directions, over two runs.** The first reached only the
`recent` half, because its agent never worked at all; a second run in a directory
where an agent had worked before reached the half that matters.

```
herdr:   5s idle   10s–55s working   60s blocked
kamiroh: prompt -> [still working]   /status -> busy · busy · busy   /detach -> ok
```

**That `[still working]` is the whole slice.** It is `Partial{Busy}` carrying the
screen, delivered while the agent was genuinely mid-task — the exact call that
answered `controller backend failed` on every prompt before the fix. The three
`busy` replies line up with Herdr's `working` throughout, so kamiroh was not
merely returning *something*, it was returning the truth.

**What the screen actually carried is worth recording, since §6e made a
prediction about it:**

```
⏺ I'll look at the crate first.
  Reading 1 file, listing 1 directory…
  ⎿  Cargo.toml
· Julienning… (19s · ↓ 309 tokens · thinking)
```

Thin on *answer* and rich on *progress* — elapsed time, tokens, what it had read.
The §6e caveat holds ("may carry a spinner rather than anything the agent has
said"), and the useful part is the shape: mid-task, a terminal tells you an agent
is **getting somewhere**, not what it concluded. One more argument that the thing
worth pushing to a remote operator is state rather than scraped text.

**A correction to what this file said an hour ago.** The first run's failure was
attributed here to the agent coming up with `manual mode on`. That is wrong: the
successful run showed `manual mode on` too, and worked. Manual mode is normal, so
the cause of the first run's no-op is unknown and was never kamiroh's — a direct
`agent.prompt` bypassing kamiroh behaved identically, with an empty response and
`agent.get` reporting `kind: null` for a pane `agent.start` had accepted as
`claude`. Recorded as unexplained rather than left with a plausible cause
attached, which is how a wrong explanation becomes settled fact.

**And that evidence was itself invalid**, found while probing `agent.wait` for
the third P1 slice. The "direct `agent.prompt` returned nothing" probe used
`printf | nc`, which half-closes stdin — and Herdr drops a long-running request
when a client does that. Held open, the same call works. So the probe showed
nothing about that pane, and the only honest statement left is the narrow one:
the agent did not act on the prompt, and why is unknown.

**The re-run needed a change to the script, and why is itself the finding.**
`demos/use_it.sh` took a fixed work directory, and everything that decides
whether a run can work is keyed to that *path* — Claude Code's workspace trust,
and whatever else it remembers per project. `KAMIROH_DEMO_DIR` now overrides it,
which is the difference between reproducing a failure and guessing at it.

**And the run found something worse than the bug it was checking.** Because the
prompt settled `idle` immediately, kamiroh answered with `Output` — a *finished*
reply — carrying the Claude Code splash screen. A confident, complete-looking
answer to a task that never ran. M1 recorded "what counts as output is a
heuristic"; this is that heuristic producing a false success rather than a
truncated one, and a false success is the harder failure to notice. It is also
the strongest argument yet that what a control layer should deliver is *state*,
not scraped terminal text — which is the open question at the end of P1.

**P2 — using it, and the premise that did not survive**

Phase 3 §5 recommended the "use it" half of P2 before P1, on the grounds that
live use has twice overturned a design every unit test agreed with. It happened
a third time, on the **first prompt**.

A real `claude` agent in a scratch pane, given a task that takes minutes — an
RPN calculator with twelve tests, work until `cargo test` passes — and driven
through kamiroh's console.

**1. A prompt to a working agent does not return `Partial{Busy}`. It fails.**

```
controller backend failed: the agent runtime is unavailable: Herdr refused the
request: agent_not_idle: cannot read 200 lines while w1:pN is working: its
alternate-screen history can only be captured by scrolling while idle.
Wait and retry, or use --source visible
```

M1's design is: patience runs out, report `Busy` *plus whatever the agent had
said so far*. The read that fetches "whatever it had said" is **refused by Herdr
exactly when the agent is still working** — an alternate-screen TUI has no
scrollback to capture until it goes idle. So the path built for the common case
is the one that cannot run, and the caller gets an `AgentError` where the design
promised a `Partial`.

Nine `HerdrAgent` tests cover every outcome path against a fake daemon that
answers the read. The real one rejects it. This is the trap already written down
in CLAUDE.md — *a fake built from a schema agrees with whatever you assumed the
schema meant* — landing on the one behaviour nobody thought to doubt.

**`source` is a correctness requirement, not a refinement.** Herdr names the fix
in its own error text. And the shape of the mistake is better than "nobody set
it": M1 *did* pass one — `recent`, with a comment giving the reason, *"what the
agent produced, not what happens to fit on the screen right now"*. That
reasoning is right, and it is right about the case where the agent has stopped.
It walked into a constraint nobody knew existed: `recent` is the source Herdr
cannot serve while an agent is working.

**2. `Shutdown` not reaching the agent is now demonstrated, with timestamps.**

| | |
|---|---|
| 17:22:01 | kamiroh answers `/shutdown` with `ok`, prints `controller actor agent has stopped` |
| 17:27:32 | the agent writes `src/lib.rs` — **297 lines** |

Five and a half minutes of real work by an agent kamiroh reported as stopped,
and it was still alive and waiting on a human after that. §2.2 was read out of
the code; this is the same claim with a filesystem timestamp behind it. It also
settles the tone of P1's naming question: this is not a verb that is merely
imprecise, it is one that reports success for something it did not do.

**3. What a long task actually consists of, which nothing had measured.** Herdr's
view across the run:

```
  5s idle    10s–60s working    65s onwards blocked    (kamiroh shut down at ~60s)
```

The agent hit a permission dialog — "create lib.rs?" — then another on running
`cargo test`. A real coding task is not one long `working` stretch with output at
the end; it is **working punctuated by stops that need a human**. That reframes
what a control layer is for. The scarce event to deliver to a remote operator is
not "here is more output", it is "**it needs you, now**" — and kamiroh has that
information already, since `Blocked` is in the domain and M1 proved a remote
operator can clear one.

Which makes the third of P1's design questions the interesting one after all.
Herdr has `events.subscribe`, and `pane_agent_status_changed` carries the full
status. Pushing *state* is nearly free; it is pushing *output* that is expensive,
and output is the thing this run suggests matters less.

**One run, one task, one agent kind** — so this is a hypothesis, not a
measurement. It also depends on `Blocked` being detected, and M3 already found
that `codex` reports the same permission prompt as `idle`. A design that pushes
"it needs you" would simply stay quiet for a kind whose manifest misses it,
which is the failure mode kamiroh cannot fix from its side. Recorded in
kamiroh-phase-3.md against P1 so it is not assumed away.

**4. Two things that held, recorded because one nearly became a false finding.**
kamiroh answered `/status` with `busy` three times while Herdr said `working` —
checked against the timeline rather than assumed, and all three fall inside the
`working` window. No repeat of M3's `codex` divergence here; kamiroh told the
truth. And `/quit` ends the console but not the node, exactly as J1 decided —
correct for a person at a pane, and a trap for anything piping input in, which
waits forever for a process that is never going to exit.

**What this pass deliberately did not do.** Nothing was implemented. Casey chose
P2 so that P1's design would not be a third guess, and turning the read bug into
a fix mid-pass would have been the fourth. It is written up in
[kamiroh-phase-3.md](./kamiroh-phase-3.md) as the first thing P1 must do.

**M3 — several agents, and a second agent kind**

An `agents` file next to `allow` and `node.key`: `<name> <target>` per line.
`KAMIROH_AGENT_TARGET` still overrides it with a single agent, the same shape as
`KAMIROH_ALLOW` overriding the allowlist — which is why every existing demo and
test kept working untouched.

Verified with two agents on one node, driven **by name** from a peer:

```
agents:      agent = echo, reviewer = echo
"agent"     -> hello agent
"reviewer"  -> hello reviewer
"nonesuch"  -> no actor named nonesuch on peer be3ec8be…
```

`KAMIROH_PEER_AGENT` names the agent to address on the peer, because a home node
hosting `reviewer` cannot be driven by a laptop whose pane agent is `agent`
without saying so. The demo then caught an inconsistency: the startup greet was
still probing this node's *own* agent name, so a pane bound to a peer's
`nonesuch` reported a healthy greet against `agent` — a reachability check for a
different thing than the one you are about to use. Greet now probes the same
agent the pane will drive.

**A pane still shows one agent**, so the reporter filters to the pane's own.
Letting every hosted agent report would have them overwrite each other in
Herdr's list, which tells an operator less than reporting nothing.

**The second agent kind found what it was meant to find.** `codex` is installed,
so the per-kind risk was testable rather than only guardable. Started in a fresh
directory it shows its own confirmation prompt — and **Herdr reports that as
`idle`, where the equivalent `claude` prompt is `blocked`**. The same situation,
two answers, and the wrong one is wrong in the dangerous direction: kamiroh
would call a stuck agent ready.

This is Herdr's detection manifest, not something kamiroh can fix from its side.
Inferring state from terminal output would mean a parser per kind, which is
precisely what agent-agnostic forbids. So it is recorded as a limitation in
ARCHITECTURE.md §6e: the accuracy of `Blocked` is Herdr's to own, and kamiroh's
job is not to make it worse. Worth knowing before trusting a sidebar.

One smaller thing, found by a test whose own input made the mistake: a target
containing spaces is now refused, because `my agent w1:p2` otherwise parses as
the agent `my` with target `agent w1:p2` — accepted, wrong, and silent.

**M4 — the rest: reporting from both directions, reload, and the nits**

**A serving node's pane was permanently idle.** J2 decorated `Link`, which is
what a *console* drives; a node serving peers has nobody at its console and its
work arrives through the Iroh front. `AgentController` is decorated too now.

J2's note said to run a second reporter and order the two with Herdr's optional
`seq`. Sharing one channel is better: it removes the race rather than sequencing
it, and leaves one connection and one order — the channel's. `seq` stays
available if a genuinely independent source ever turns up. What remains
uncovered is one pane showing one agent; a node hosting several would have them
overwrite each other, which is M3's to solve.

**`FileAllowlist::reload()` finally has a caller: `SIGHUP`.** A signal rather
than a `/reload` console command, because the node that needs it is the one with
nobody at its pane. Verified against a running node:

```
good edit + SIGHUP ->  INFO  allowlist reloaded on SIGHUP  peers=2
bad edit  + SIGHUP ->  WARN  allowlist reload failed; keeping the previous one
                             … line 1: "this is not an endpoint id" … got 26
```

The second line is the one that matters — it is the difference between a fumbled
edit costing a log line and costing every peer, and it is why the composition
root keeps the concrete handle while everything else uses the port.

**The nits.** `KeyStoreError` gained `Unconfigured { reason }`, so an
unconfigured environment stops being reported as a corrupt key file — and the
two error types stop disagreeing, which was the observation recorded in I. The
other two stand as decisions rather than debt: `ScopedTempFile` still has no
disarm because nothing needs one, and an interrupted prompt still answers
`ControllerError::Rejected` — M1's `ControlReply::Partial` did not resolve it,
since an abandoned prompt has no output to carry.

**M4 — observability**

`tracing` across the app and adapters, a subscriber in the binary, and two rules
that are specific to what kamiroh is.

**Agent content is never logged — only its shape.** kamiroh is agent-agnostic;
it has no business knowing what a prompt says, still less writing it down. This
holds by construction because `Payload`'s `Debug` reports content type and
length, but that is a domain property and the mistake would be made in
`kamiroh-app`, so the guard lives there:
`a_prompts_content_never_reaches_the_log` captures this crate's own output at
`TRACE` and asserts the secret is absent and a length is present. Mutation-
tested — swapping `?message` for `%payload.as_text()` fails it.

Demonstrated end to end at maximum verbosity, with a prompt containing a
distinctive string:

```
stdout (the console): 1 occurrence     <- where it belongs
stderr (the log):     0 occurrences
log said instead:     Prompt(Payload { content_type: "text/plain; …", len: 28 })
```

**Diagnostics go to stderr, because stdout belongs to the pane console.** Since
J1 stdout is where an agent's answers appear and where a person is typing; a log
line there lands in the middle of someone's conversation. Not a detail — it is
the reason the split exists at all.

Smaller decisions: `Targets` rather than `EnvFilter`, giving per-crate filtering
(`KAMIROH_LOG=kamiroh_adapter_iroh=debug`) without pulling regex in for dynamic
matching kamiroh does not use. Dependencies default to `warn` and kamiroh's own
crates to `info`, since iroh at `debug` would drown the events worth seeing. A
malformed `KAMIROH_LOG` falls back loudly rather than silently disabling
logging. `kameo`'s `tracing` feature stays off — dropped in G as an unadopted
facade, and now that the facade is adopted the reason changes but the answer
does not: actor-internal spans would be noise.

**One thing found on the way in:** `kamiroh-adapter-iroh` had carried a
`tracing` dependency since F2 and *was* already emitting three events. So this
was less a beginning than a completion — the facade was in the tree, used by one
crate, subscribed to by nobody, which is the worst of the three states.

**The review queue, worked through**

Five items had accumulated since F2, all flagged for an advisor nobody has
turned on. Rather than write five opinions, each was pushed to whichever of
these it could reach: **demonstrated**, **enforced by a test**, **fixed**, or
**still a decision for a human**.

| # | item | outcome |
|---|---|---|
| 1 | F2's enumeration argument | **enforced** — mutation-tested |
| 2 | malformed-allowlist-is-fatal (I) | **still yours** — a genuine judgment call |
| 3 | `Agent` as an adapter trait (G) | **resolved** by M1; struck |
| 4 | bounded-mailbox reasoning (G) | **a real bug found and fixed** |
| 5 | §5a's disclosure claim (M2) | **demonstrated** |

**1 — the enumeration argument is enforced, not merely argued.** It rests on
statement order (authorise before lookup), so the question was whether anything
would notice a reorder. Answered by mutation: dispatch was moved above the
allowlist check, keeping `?`, which is the realistic version of the mistake.
Three tests failed, one at the wire level — `[1,0,2]` (`NO_SUCH_ACTOR`) against
`[1,0,1]` (`REFUSED`). The mutation was reverted; the property is guarded.

Worth noting what the strongest of those tests actually checks:
`unlisted_remote_endpoint_is_rejected_without_dispatching` catches something
worse than enumeration — an unlisted peer's prompt *reaching the agent*. Since
M1 that means a stranger spending the operator's tokens, so the property is
worth more now than when it was written.

**4 — the bounded-mailbox reasoning caught a bug I had just introduced.** Slice
G established that this actor must not await inside a handler, because nothing
else in the mailbox moves while it does. M1 then added exactly that: `Status`
awaits `agent.status()` inline, and the Herdr client's `read_line` has no
timeout. A runtime that accepts a connection and never answers would have
wedged the actor permanently — `Interrupt` and `Shutdown` unreachable, the agent
not even stoppable. Bounded now at `STATUS_TIMEOUT`, with a test that hangs an
agent's `status()` on `pending()` and asserts the controller still answers and
still shuts down. Note the shape of the mistake: the reasoning was *cited* in
the very commit that violated it.

**5 — §5a's disclosure is demonstrated.** The claim was that a node holding only
an endpoint id can locate a published node whether or not the allowlist would
admit it. Run with a stranger node absent from the allowlist and given nothing
but the id:

```
peer c6f5488f… -> peer c6f5488f… refused the connection
```

`refused`, not `unreachable` — so it found the node and was then rejected. Both
halves at once: the disclosure is real, and reachable is not admitted.

**2 — malformed-is-fatal is still a decision, and it is yours.** Nothing here
can settle it, because it is a trade rather than a fact: a malformed allowlist
stops every node that restarts, against the alternative of a node that comes up
admitting nobody while looking healthy. Two things narrow it. The blast radius
is startup only — `FileAllowlist::reload()` already keeps the previous set on a
bad read, so a running fleet survives a bad edit until it restarts. And the
error names the file and line. What would settle it properly is a way to check a
file *before* restarting; kamiroh has no CLI surface for that, and adding one is
a slice, not a nit.

**M2 — reachable from anywhere (implementation)**

`Reach`, chosen by `KAMIROH_REACH`:

- `direct` (default) — Iroh's `Minimal` preset. No relays, no lookup, nothing
  published. Exactly today's behaviour, so every test and demo is unchanged.
- `anywhere` — Iroh's `N0` preset. Relays and address lookup, so a peer can be
  dialled by endpoint id with no address written down. `KAMIROH_PEER` now
  accepts a bare id for that reason.

**Opt-in, deliberately.** `anywhere` publishes a signed record of this node's
addresses to n0's DNS/pkarr under its endpoint id, so anyone holding the id can
learn where the node is — whether or not the allowlist would admit them.
Reachable is not admitted, but "unlisted peers cannot even find me" stops being
true, and a node should not start announcing where it lives because someone
failed to set a variable. The startup line always says which mode is in force,
and the transport's error names the fix rather than just failing.

What a relay can see is worth stating once: QUIC is end-to-end encrypted, so a
relay carries ciphertext and learns which endpoints talk, when, and how much.
Not content, and it cannot admit itself — it is on nobody's allowlist. Written
up in ARCHITECTURE.md §5a, along with the note that self-hosted relays and a
self-hosted pkarr server would remove the third party entirely.

Tests are `Direct` by construction, including the two-node integration tests:
publishing to a public service is not a decision a test suite makes on its own.

**Verified live**, with a negative control — which is the part that makes it
mean anything, since two processes on one host could plausibly have found each
other some other way:

```
reach=direct,   KAMIROH_PEER=<id>  -> unreachable: no address for this peer, and
                                      this node resolves nothing by id
                                      (set the reach to `anywhere` to look peers up)
reach=anywhere, KAMIROH_PEER=<id>  -> Output(Payload { len: 11 })
```

Same id, same two nodes, one variable changed. B was given A's endpoint id and
nothing else — no host, no port, at any point.

**What it does not prove:** NAT traversal. Both nodes were on this machine, so
no relay was needed and none was necessarily used. What it proves is the thing
that actually blocked the product — that identity is sufficient to address a
peer. A real home-to-cafe run needs a second machine on a different network, and
is the remaining gap in M2's claim.

**M1 — a real agent**

`HerdrAgent`: a prompt goes to a coding agent in a Herdr pane and what it says
comes back. `agent.prompt` with a bounded wait, then `agent.read`, then an
`AgentOutcome`. kamiroh does not start, supervise or parse the agent — Herdr
does the first two and nobody does the third.

**The domain changed for the first time since slice B**, exactly as planned, and
the three-part decision held up:

| | change | where |
|---|---|---|
| 1 | `Agent::run` returns an outcome, and is fallible | ports |
| 2 | `AgentStatus::Blocked` | domain |
| 3 | `ControlReply::Partial { output, status }` | domain |

The compiler found all five propagation sites — `encode_reply`, `decode_reply`,
`console::render`, `report::state_after`, and the pane-state mapping — which is
what made the earlier "nine files across six crates" estimate wrong.

`run` being **fallible** was not in the plan and should have been. Without it an
unreachable Herdr socket could only be reported as agent *output*, which would
arrive at the caller looking like something the agent said. Infrastructure
failure and agent speech must not share a channel.

**`Agent` moved to `kamiroh-ports`**, reversing the note written in slice G. That
note argued the ports crate describes kamiroh's boundaries while `Agent`
described how one adapter runs the thing behind one. True while a single crate
both defined and implemented it; false the moment `kamiroh-adapter-herdr`
arrived to implement it, since the alternative was an adapter depending on an
adapter. `EchoAgent` moved to `kamiroh-adapter-memory` with it — an in-memory
implementation of a driven port is what that crate is for.

**The timeout collision was the predicted hazard and it was real.** The Iroh
front gives a request 30s and the transport gives a reply 30s; a coding agent
works for minutes. `DEFAULT_PATIENCE` is 20s, running out of it yields `Busy`
plus whatever the agent had said, and a test pins the relationship so the two
cannot drift apart silently.

Two limits worth stating rather than hiding. `agent.read` returns the last N
lines of a terminal, which has no marker for "this is the answer to that
prompt", so what counts as output is a heuristic. And there is no verb for
asking for the *rest* of a long answer — `ControlMessage` has none — so a caller
prompts again and reads more, which is a workaround rather than a design.

**Verified** by 9 `HerdrAgent` tests against a fake daemon covering every
outcome path, 160 workspace-wide, plus live evidence that `agent.prompt` and
`agent.read` are real methods whose parameters kamiroh's requests satisfy: the
daemon rejected a custom-reported pane with `agent_not_ready` — "not an active
named agent" — which is target resolution, downstream of method and parameter
validation.

**Then verified live, against a real `claude` agent** started in a scratch pane
rooted in an empty temp directory. A prompt typed at kamiroh's console reached
the agent and its answer came back:

```
> Reply with exactly this and nothing else: KAMIROH-OK
❯ Reply with exactly this and nothing else: KAMIROH-OK
⏺ KAMIROH-OK
```

**The live run found three things no unit test could have.**

1. **A real agent returns to `idle` when it finishes, not `done`.** Waiting on
   `until: [done, blocked]` therefore never matched, and every prompt expired
   instead of completing. The observed sequence is `idle → working → idle`, with
   `blocked` appearing when it wants a human. The wait list now includes `idle`,
   and the test asserts membership rather than order so it pins the property
   instead of the spelling.
2. **Herdr reports an expired wait as an `error`, not as a state.** Code
   `timeout`, message "timed out waiting for agent status". Mapping it to a
   failure made a slow agent indistinguishable from a broken socket — precisely
   the confusion `AgentError` was introduced to prevent. An expired wait now
   means `Busy`, which is what it is.
3. **`local_smoke` was prompting the real agent at every startup.** Harmless
   with `EchoAgent`; with a coding agent behind the port it spends tokens on
   every launch and puts words in the agent's mouth. It sends `Status` now,
   which proves the same path — front, allowlist bypass, controller — and costs
   nothing.

**The live test has a human precondition, and that is correct.** Claude Code
asks for workspace trust the first time an agent runs in a given directory, and
records the answer per path as `hasTrustDialogAccepted` in `~/.claude.json`. So
it is asked once per scratch directory, not once per run — which is why re-runs
of the same script were silent.

Worth being clear about the risk and the non-fix. The risk is not the manual
step; it is that the test is green partly because of consent a human gave
earlier, and nothing in the script knew it. On a fresh machine or a new path the
agent sits at the dialog, kamiroh correctly returns `Partial{blocked}`, and the
script would have shown a confusing near-miss. It now detects a
blocked-before-we-sent-anything agent and says what it is.

The non-fix is automating the approval — writing the flag into the config, or
reaching for a skip-permissions switch. That prompt exists because an agent in a
directory can read and act on everything in it, and scripting around a consent
prompt is a habit that leaks: M3 contemplates kamiroh calling `agent.start`
itself, at which point "who approves workspace trust" stops being a test
convenience and becomes a question about a node starting agents on someone's
behalf. `claude-code-setup.md` already treats workspace trust as a one-time
human step for this repo; this is the same shape.

**It is also `Blocked` in the wild — and answering it remotely works.** A trust
dialog is exactly "cannot proceed without a human". Driven from a kamiroh
console against an unapproved directory:

```
/status   -> blocked          # before kamiroh has sent anything
1         -> (dialog cleared)
/status   -> idle
"Reply with exactly: KAMIROH-UNBLOCKED"  ->  KAMIROH-UNBLOCKED
```

So the answer to "can a remote operator unblock an agent" is **yes**, and the
consent still happens — a human sees the question and answers it, from
somewhere else. That is remote consent, not a bypass, and it is the difference
between kamiroh telling you an agent is stuck and letting you unstick it.

**Two bugs found getting there, both about telling the truth.**

1. **`Status` returned a cached value.** The controller answered from its own
   view, which only updates when a run completes, so kamiroh reported `Idle` for
   an agent sitting at a dialog. `Agent::status` now exists, returning
   `Option<AgentStatus>` — `None` meaning "no better answer than yours", so
   agents whose state only moves when run keep the default.
2. **`impl Agent for Arc<dyn Agent>` forwarded `run` but not `status`.** A
   defaulted method a forwarding impl does not override is answered by the
   default — silently, no compile error. Every unit test passed because they
   called the agent directly; only the live run went through the wrapper the
   composition root uses. The fix is one method; the guard is a test that
   asserts the wrapper and the agent agree.

Two smaller operational notes. `agent.start` fails with `agent_pane_busy` until
the new pane's shell reaches its prompt, so a freshly split pane needs a moment.
And `DEFAULT_LINES = 200` returns the whole terminal, splash screen included:
the "output is a heuristic" caveat is not theoretical, it is the first thing you
see. Tightening it wants more than one agent kind to generalise from.

**Only `claude` has been exercised.** Herdr can start `codex`, `gemini`,
`cursor`, `devin`, `opencode` and a dozen more, each with its own detection
manifest and readiness behaviour. Finding #1 — which state means "finished" — is
exactly the kind of thing that will differ per kind, and kamiroh currently
assumes one answer for all of them.

**Slice J2 — reporting the pane's agent state to Herdr**

kamiroh now tells Herdr what its agent is doing, so a pane shows `working` while
a prompt runs and `idle` when it lands. Herdr's local socket API, method
`pane.report_agent`, over `$HERDR_SOCKET_PATH`.

**It decorates `Link`, not `AgentController` — the opposite of what this file
said to do.** The plan here was a decorator over the controller port, which sees
every message any front delivers. That is wrong for the case J1 exists for: a
pane driving an agent on another node never touches the local controller, so the
controller decorator would report nothing precisely when there is something to
report. A pane's state is the state of the agent *that pane is bound to*, which
is what `Link` names. Decorating it covers local and remote uniformly and
changes no port.

**Two things the documentation did not say, found by experiment.**

1. **Herdr answers one request per connection, then closes it.** Three `ping`s
   written to one socket produce one response, not three. The first
   implementation held a connection open, which worked for the opening report
   and then failed forever with `Broken pipe`. Each report now opens its own
   connection — no waste worth caring about, since state changes at human speed.
2. **The error codes are more specific than the docs suggest.** A bogus method
   gives `invalid_request` ("unknown variant"), a missing field gives
   `invalid_request` ("missing field `source`"), and a bad pane gives
   `pane_not_found`. That they are *distinct* is what makes a `pane_not_found`
   reply positive evidence: the method name and the whole required parameter set
   were accepted, and only the pane was wrong.

Settled before writing:

- **Reporting never delays or fails a control message.** Bounded channel,
  `try_send`, drop when full. A sidebar label is not worth a slower prompt, and
  Herdr being down is not a reason for kamiroh to stop working.
- **Absence is normal.** No `HERDR_PANE_ID` → the link is returned untouched and
  the startup line says so. kamiroh runs outside Herdr as a matter of course.
- **Asking does not change the answer.** Only a prompt reports `working` first;
  `/status` must not briefly claim the agent is busy just by being asked.
- **A failure reports `unknown`, not `idle`.** An unreachable peer says nothing
  about the agent behind it.
- **`Starting → unknown`, not `idle`.** "Idle" invites someone to prompt an
  agent that is not ready. Dead today: `KameoController` spawns actors idle.
- **Nothing maps to Herdr's `blocked`** ("waiting on a human"). No kamiroh agent
  waits on input yet; when one does, that is where it surfaces.
- **`serde_json`, adapter-local.** F2's hand-written codec was about keeping
  `kamiroh-domain` dependency-free for a protocol kamiroh *defines*. This is
  someone else's JSON, and the pane id arrives from the environment unvalidated,
  so it must be escaped by something that knows the rules.

**A mistake worth recording, because it touched a live system.** The demo
scripts inherited `HERDR_*` from the session running them — which is inside a
real Herdr pane — so the first run had test nodes reporting into the pane being
worked in. The scripts now `unset` those variables. Anything spawned from a
session inside a tool's environment inherits that environment; a demo must not
touch a live session.

Verified by 19 unit tests against a fake socket that closes after each response
(the shape that would have caught the connection bug), and then end to end
against the **real `herdr 0.8.0` daemon** in a scratch pane split for the
purpose and closed again afterwards:

```
fresh pane                      agent=null     agent_status="unknown"
kamiroh's opening report        agent="agent"  agent_status="idle"
prompting an unreachable peer   agent="agent"  agent_status="working"
```

The `working` observation is the one that needed setting up: with `EchoAgent` a
local prompt finishes too fast to catch, so the pane was pointed at a peer that
does not exist and the state was polled while the dial ran.

**That attempt found a real wart, in code older than the slice.** `greet()` —
the startup reachability smoke from F2 — was awaited before the console was
built, so an unreachable peer blocked startup for the full dial timeout, 16
seconds measured. Harmless when the binary was a server; not harmless now that
a person opens a pane and waits at it. A laptop whose home node is asleep would
look hung rather than offering the prompt where `/status` explains the problem.
`greet` is now spawned. The two-node demo waits on the `peer ` line rather than
`serving`, since the two are no longer ordered.

**Slice J1 — the pane console**

`kamiroh-adapter-herdr`: one pane, one agent, typed at like a chat window. A
bare line is a prompt; `/status`, `/interrupt`, `/shutdown` carry the other
three `ControlMessage` verbs, because a local console that could only prompt
would be strictly weaker than the remote path for no reason.

**The plan's description of J was wrong, and following it would have built the
wrong thing.** The slice table said "a second front calling the same
`ControlApi`" — inbound, driving an agent on *this* node. What is actually
wanted is the opposite direction: sit at a pane on a laptop and drive an agent
that has been running on the home node for a week. That is not a front at all;
nothing arrives. It is kamiroh as a *client*, over the `Transport` port.

Both directions are here, behind a `Link` the console cannot see through:

| | port | direction | trust |
|---|---|---|---|
| `LocalLink` | `ControlApi` (driving) | inbound — a *front* | `Origin::local_front()` |
| `RemoteLink` | `Transport` (driven) | outbound — a *console* | the peer's allowlist decides |

`LocalLink` is what finally tests the claim the architecture has made since
slice A: it holds the same `Arc<dyn ControlApi>` as the Iroh front, so both
reach one controller actor. Until now there was only ever one front, so the
claim was untested. ARCHITECTURE.md gained §3a for the front/console
distinction, since the old wording is what caused the mistake.

**The `local_front` audit rule changed, deliberately.** It was "no adapter may
appear in `grep -r local_front`", which held while every adapter was a
transport. `LocalLink` is the case that constructor was written for in slice B.
The rule is now "only `kamiroh-adapter-herdr`'s `LocalLink` and the composition
root", and a transport adapter appearing there is still a bug. Note `RemoteLink`
does **not** call it: a console does not get to vouch for the far end.

Two smaller decisions worth keeping:

- **Ending the console does not end the node.** A node serving agents for peers
  has nobody at its pane and may have a closed stdin from the moment it starts.
  The console is spawned, and EOF or `/quit` just ends that task. This is what
  keeps the two-node demo working unchanged.
- **The prompt string is the caller's choice.** A terminal echoes the newline
  when someone presses Enter, so `> ` lands on a fresh line; piped input echoes
  nothing, so every blank line stacked another prompt on the same one — visible
  as `> > unknown command` in the first demo run. The composition root passes
  `"> "` only when `stdin().is_terminal()`.

Nothing in the crate knows about Herdr. A pane is a terminal, so `serve` takes
an `AsyncBufRead` and an `AsyncWrite` and is tested with a string and a
`Vec<u8>`.

Verified by 14 unit tests and a two-process run: a pane on node B sends a prompt
to node A's agent over Iroh and gets the echo back, `/status` returns `idle`
from A's Kameo actor, and once A empties its allowlist the same pane shows
`refused the connection` and carries on.

**Slice I — the allowlist file**

`FileAllowlist` in `kamiroh-adapter-fs`, beside the key store: one endpoint id
per line, `#` comments, blank lines ignored. The `Allowlist` port did not
change — it is still a synchronous, infallible `bool` — because loading is a
separate act from checking, so the load errors are adapter-local.

**Its custody rules are deliberately not the key store's**, and the contrast is
the interesting part of the slice. A node secret is secret; an allowlist is
public keys. Demanding `0600` on it would be theatre that only makes the file
harder to inspect. What matters is *integrity*: group- or other-writable is
refused on both the file and its directory, since an account that can append a
line can admit itself. Permissions are checked before contents, because a file
anyone can rewrite is not evidence of anything.

Settled before writing:

- **Malformed is fatal; a partial list is never used.** One bad line rejects the
  whole file. Both ways of guessing are wrong: admitting the lines that parsed
  enforces a policy nobody wrote, and admitting nobody while looking healthy
  hides the mistake behind what looks like a network problem.
- **Absent is not malformed.** A missing file means what an empty one means —
  admit nobody. That is the port's deny-by-default, and the state of a fresh
  node.
- **`KAMIROH_ALLOW` survives as an outright override**, which is what keeps the
  two-process demo and multi-node local testing working. The startup line names
  the source either way; an operator editing a file the node never read is the
  failure worth designing against.
- **`reload()` ships, a trigger does not.** The atomic swap under the lock is
  the part that is hard to retrofit; a trigger — signal, file watch, Herdr
  command — is not. A failed reload keeps the previous set *and* returns the
  error, because retaining a stale list can miss a revocation while emptying one
  locks out every peer over a typo, and only a caller knows which risk it runs.

**A defect the slice exposed, in code older than the slice.** A
`Result`-returning `main` prints the error's `Debug`, so the first run of the
refusal path produced `Error: Malformed { path: "...", line: 2, entry:
"truncated-id", source: Length { got: 12 } }` — a struct dump, with the
carefully written sentence nowhere in sight. "Refuse to start" is worth exactly
as much as the reason it gives. `main` now returns `ExitCode` and prints
`Display`:

```
kamiroh: allowlist /…/allow line 2: "truncated-id" is not an endpoint id:
         endpoint id must be 64 hex characters, got 12
```

Only the top level is printed, no source chain: every error type here already
embeds its source in its own message, so walking the chain would print it twice.

Verified by 15 new unit tests and a six-case run of the real binary — file with
peers, file absent, file of only comments, env override, malformed file, and
world-writable file — checking both the `allowing:` line and the exit code.

**Slice G — the Kameo controller**

`kamiroh-adapter-kameo`: one Kameo actor per agent, behind the unchanged
`AgentController` port. `EchoController` was not merely replaced — the thing it
faked was the *controller*, so an agent's lifecycle was simulated by a
`HashMap`. Now the controller, its mailbox and its lifecycle are real.

- `Agent` is the seam for the work itself, with `EchoAgent` as the stand-in.
  It lives in the adapter, **not** in `kamiroh-ports`: the ports crate describes
  kamiroh's boundaries, and promoting `Agent` would make every future controller
  adapter adopt one notion of "an agent" — the assumption kamiroh exists not to
  make.
- A prompt runs as its own task and reports back **through the mailbox**. Run
  inline, the actor could not answer `Status` while working and `Interrupt`
  would have nothing to arrive at. Reporting through the mailbox is what makes a
  completion racing an interrupt into two messages in an order already fixed.
- **`AgentStatus::Busy` is reachable for the first time.** It has been in the
  domain since slice A and no adapter could ever produce it. Pinned by a test
  that observes an agent mid-prompt.
- Every `ControllerError` variant now has a real producer, which is the
  after-the-fact evidence that the port was drawn right in B.

**A hazard found by reading, not by failing.** Kameo's default mailbox is
*bounded* (`Actor::spawn` → `mailbox::bounded(DEFAULT_MAILBOX_CAPACITY)`). The
obvious `Shutdown` handler awaits `stop_gracefully()` inline, which sends into
the actor's own mailbox — and an actor inside a handler cannot drain that
mailbox to make room, so a full mailbox deadlocks it against itself. Stopping is
requested from a spawned task instead.

That fix opens a second question it would be easy to miss: if stopping is
asynchronous, does a message sent just after `Shutdown` get `Stopped` or a
normal reply? It would depend on whether the actor had finished stopping. So the
actor holds an explicit `Stopped` state *and* `KameoController` maps a send to a
dead actor onto the same `ControllerError::Stopped`. Both paths agree, and a
test covers each side of the race.

Verified in-process (12 tests) and then through the two-process demo, where a
remote peer's prompt crosses the wire into a real actor and its output comes
back — the swap changed nothing a peer can observe, which is the point.

**Slice F2 — the Iroh transport and front**

- `codec`: a hand-written, length-prefixed wire format inside the adapter.
  **This is the decision F2 was told to settle first.** `postcard` + `serde`
  derives would have given `kamiroh-domain` a serde dependency and given up the
  zero-dep property held since slice A, to save code in one adapter. `cargo tree
  -p kamiroh-domain -e normal` still lists nothing but the crate itself.
- `front::serve`: the accept loop, calling `ControlApi` with an `Origin` built
  from the connection's authenticated peer.
- `IrohTransport`: the `Transport` port — dial, send, await the reply.
- Composition root binds an endpoint, serves the front, and reads
  `KAMIROH_ALLOW` / `KAMIROH_PEER`.

**The enumeration obligation holds by ordering, not by collapsing error codes.**
`REFUSED` and `NO_SUCH_ACTOR` *are* distinct on the wire — but authorisation
runs before the actor is looked up, so an unlisted peer receives byte-identical
`REFUSED` whether or not the agent it names exists. The distinction only ever
reaches a peer that is already trusted, which is where it is useful and
harmless. Worth stating precisely: "the two errors serialise identically" would
be the wrong summary and would be falsified by the next person to read
`wire_code`.

Verified by six two-node integration tests, then by a two-process run: an
allowlisted peer gets its echo back, the same peer is refused once the allowlist
is emptied, and the node's identity is stable across restarts.

**Slice F1 — real endpoint identity**

F was split. F1 is identity derivation, fully offline-testable and independently
revertible; F2 is the transport, which needs a wire format, an accept-loop
lifecycle, and UDP sockets the sandbox denies. Bundling them would have made the
small high-value piece wait on the large one.

- New crate `kamiroh-adapter-iroh` with one function, `endpoint_id_for`:
  `SecretKey::from_bytes(secret).public()` → `kamiroh_domain::EndpointId`.
  Infallible — any 32 bytes are a valid ed25519 secret scalar.
- Depends on `iroh-base` (`default-features = false`, `features = ["key"]`), not
  `iroh`: ~155 crates instead of ~375. Going through the type Iroh itself uses
  makes agreement structural, and one test asserts it directly.
- **`placeholder_endpoint_for` deleted**, not merely orphaned. `cargo check
  --workspace` proves the removal is complete.
- Composition root now prints a real id. It changed from `e0c520ae…` (the old
  byte inversion) to `b4cfcb25…` from the *same* persisted secret — expected,
  and the visible proof the derivation is no longer fake.

Boundary verified: `iroh-base` has exactly one consumer (`cargo tree -i
iroh-base`), `kamiroh-domain` still has zero dependencies, and `kamiroh-app`'s
tree contains no iroh crate. Beware `grep -i iroh` here — it matches "kam*iroh*";
check the reverse-dependency tree instead.

**Slice E — `kamiroh-adapter-fs` key custody**

- `FileKeyStore`: OS entropy via `getrandom`, stored as hex at
  `$XDG_CONFIG_HOME/kamiroh/node.key`, overridable with `KAMIROH_KEY_FILE`.
- Publish via temp file + `hard_link` — see the race below.
- Permission checks before any read, on both the file (`0600`) and its parent
  directory (not group/other writable), on the create *and* load paths.
- Domain gained `NodeSecret::{from_fill, from_hex, write_hex_into}` and
  `ParseNodeSecretError`, plus a private `hex` module shared with `EndpointId`.
  The `KeyStore` port and `kamiroh-app` were **not** touched — the port held.
- Composition root swapped `InMemoryKeyStore` → `FileKeyStore`; that swap was a
  one-line constructor change, which is the evidence the boundary is right.

**A real race, found and closed.** The first implementation created the key with
`O_CREAT | O_EXCL` at its final path, then wrote. That is non-clobbering but not
atomically published: the name exists before the contents do, so a concurrently
starting process reads a **zero-length** file and reports a spurious `Malformed`.
Reproduced as `got 0` — 4 failures in 5 full-suite runs. Replaced with temp file
→ fsync → `hard_link`, which is non-clobbering *and* publishes atomically.
Verified with the same loop harness that caught it: **30/30 clean**.

Two follow-on details from that fix: temp paths need a per-attempt counter as
well as the pid, or threads in one process stage onto a single path and delete
each other's candidate; and the temp file is removed by a scope guard on every
exit path, since a stranded one is a live secret loose in the key directory.

**Slice A — workspace + crate graph + ARCHITECTURE.md**

- Root `Cargo.toml` converted from a single package to a workspace
  (`resolver = "3"`, `[workspace.package]`, `[workspace.dependencies]`). The old
  `src/lib.rs` scaffold (`add`/`it_works`) was deleted, not carried forward.
- Five crates created: `kamiroh-domain`, `kamiroh-ports`, `kamiroh-app`,
  `kamiroh-adapter-memory`, `kamiroh` (bin). crates.io metadata moved to the bin.
- `docs/ARCHITECTURE.md` written against the code as built.

**Slice B — port traits**

- Driving: `ControlApi` with an opaque `Origin` (`remote()` / `local_front()`).
- Driven: `Transport`, `Allowlist`, `KeyStore`, `AgentController`.
- Per-port `thiserror` error enums; `#[async_trait]` on the async ports for
  dyn-compatibility; `Allowlist` synchronous and infallible.

**Beyond the two slices, to make the bin real**

- `kamiroh-domain`: `EndpointId` (hex, round-tripping), `ActorName` (validated),
  `PeerAddress`, `NodeSecret` (redacted `Debug`, zeroed on drop), `Payload` /
  `ControlMessage` / `ControlReply` / `AgentStatus`.
- `kamiroh-app`: `ControlService` — authorise, then dispatch.
- `kamiroh-adapter-memory`: `InMemoryAllowlist`, `InMemoryKeyStore`,
  `EchoController`, `LoopbackTransport`.
- `crates/kamiroh/src/main.rs`: wiring only; prints the endpoint id and runs a
  smoke path in both the allowed and refused directions.

**Verification** — the standing recipe, kept current rather than per-slice

```
cargo fmt --all --check                        # clean
cargo clippy --workspace --all-targets -- -D warnings   # zero warnings
cargo test  --workspace --no-fail-fast         # 204 passed, 0 failed (as of P1)
cargo tree  -p kamiroh-domain -e normal        # no dependencies at all
cargo tree  -p kamiroh-ports  -e normal        # kamiroh-domain + async-trait + thiserror only
cargo tree  -i kameo -e normal                 # exactly one consumer: kamiroh-adapter-kameo
cargo tree  -i iroh-base -e normal             # exactly one consumer: kamiroh-adapter-iroh
# Since J1: the composition root, kamiroh-app's tests, and LocalLink only.
# A *transport* adapter appearing here is a bug.
grep -rn local_front crates/ --include='*.rs'

# Need sockets, so these run OUTSIDE the sandbox. The Herdr client tests bind a
# Unix socket, which the sandbox denies with "Operation not permitted" — that is
# the sandbox, not a bug.
cargo test -p kamiroh-adapter-iroh  --test two_nodes
cargo test -p kamiroh-adapter-herdr

# The key-store race is timing-dependent — one green run proves nothing.
for i in $(seq 1 30); do cargo test -p kamiroh-adapter-fs || echo "FAIL $i"; done
```

Beware `grep -i kameo` and `grep -i iroh` when checking boundaries: both match
the crate names `kamiroh-adapter-kameo` and `kamiroh-adapter-iroh`, and "iroh"
matches "kam*iroh*" besides. Use the reverse-dependency tree, which cannot be
fooled this way.

## Decisions

| Decision | Choice | Why |
|---|---|---|
| Crate count | 5 now, not the plan's 8 | Empty adapter crates prove nothing and get rewritten; each arrives with its slice. Stated, not silent. |
| Async style | `#[async_trait]` | Native AFIT is not dyn-compatible; the composition root needs `Arc<dyn Port>`. |
| Errors | Per-port `thiserror` enums | `anyhow` in a port leaks adapter detail into the app and makes failures untestable. |
| `EndpointId` | Opaque `[u8; 32]`, hex display | Naming `iroh::NodeId` in the domain breaks the dependency rule; `String` is wrong forever. |
| Agent-agnostic | Fixed verbs, opaque `Payload` | kamiroh routes and authorises; it never parses agent content. |
| Allowlist | Sync `bool`, no enumeration, deny-by-default | An allowlist check is set membership; a fallible one invites treating an error as "allow". |
| `Origin` | Opaque, built via `remote()` / `local_front()` | A public `Local` variant is constructible by every adapter; local trust must be a deliberate, greppable act. |
| Lints | `deny(missing_docs)`, `forbid(unsafe_code)` | The docs promised a warning-free build; the lint level now enforces it. |
| Key publish | temp + `hard_link` | The only option that is both non-clobbering and atomically published; `rename` clobbers, `O_EXCL`-in-place exposes an empty file. |
| Key format | Hex + newline | Inspectable, never mistaken for corrupt binary, reads like an `EndpointId`. |
| Key entropy | `getrandom` only | A node secret is generated once; a seeded PRNG layer would add surface for nothing. |
| Split F | F1 identity, F2 transport | F1 is offline-testable and independently revertible; bundling made it wait on the large, socket-dependent half. |
| Identity dep | `iroh-base` `["key"]`, not `iroh` | ~155 crates instead of ~375, and the public key comes from the type Iroh uses, so agreement is structural. |
| Wire format | Hand-written codec in the adapter | `postcard` + serde derives would put serde in `kamiroh-domain`, trading the zero-dep property for less code in one adapter. |
| Enumeration defence | Authorise before lookup | Collapsing `REFUSED` and `NO_SUCH_ACTOR` into one code would also blind trusted peers. Ordering gives the property without that cost. |
| `Agent` trait location | In `kamiroh-adapter-kameo`, not `kamiroh-ports` | Ports are kamiroh's boundaries; this is how one adapter runs the thing behind one. A port would force one notion of "an agent" on every future controller. |
| Prompt execution | Own task, reports via the mailbox | Inline, the actor cannot answer `Status` while working and `Interrupt` has nothing to reach. Reporting through the mailbox orders completion against interruption. |
| Concurrent prompts | Refused, not queued | The mailbox would queue them, but silently serialising makes `Busy` a lie — the caller cannot tell queued from running. |
| Stopping an actor | From a spawned task, plus an explicit `Stopped` state | The mailbox is bounded, so awaiting `stop_gracefully()` inside a handler can deadlock the actor against itself; the explicit state then keeps the answer independent of stop timing. |
| `kameo` features | `default-features = false` | `remote` pulls libp2p, and a second p2p stack invites routing agent traffic over the wrong one. `macros` and `tracing` are unused. |
| Allowlist home | `kamiroh-adapter-fs`, beside the key store | It is a file, and that crate is the filesystem adapter. A new crate would exist only to hold one type. |
| Allowlist permissions | Writable refused, readable fine | Public keys need integrity, not secrecy. A `0600` allowlist would be theatre that makes the file harder to inspect. |
| Malformed allowlist | Fatal, never partial | Both guesses are wrong: a partial list enforces a policy nobody wrote; a silent empty one looks like a network fault. |
| Absent allowlist | Deny everyone, not an error | Same meaning as an empty file, and the state of a fresh node. |
| `KAMIROH_ALLOW` | Kept as an outright override | Explicit beats ambient, and the two-process demo and multi-node local testing depend on it. |
| Reload | `reload()` now, trigger later | The atomic swap is hard to retrofit; a signal or watch is not. A failed reload retains and reports rather than choosing a risk for the caller. |
| `main`'s return | `ExitCode`, printing `Display` | A `Result`-returning `main` prints `Debug`, so every refusal-to-start message was a struct dump. |
| Split J | J1 console, J2 Herdr reporting | Inbound console and outbound status reporting are different kinds of thing; J1 alone meets the plan's bar. |
| J's real shape | A console, not only a front | The wanted case is driving a *remote* agent from a local pane. The plan said "front", which is the other direction. |
| Pane input syntax | Bare line = prompt; slash commands for the rest | One pane means one agent, so no line ever has to name one. The other three verbs would otherwise be unreachable locally. |
| Console lifetime | Spawned; its end is not the node's | A serving node has nobody at its pane and may start with a closed stdin. |
| Prompt string | Caller's choice, `""` for none | A tty echoes the newline; piped input does not, so a per-line prompt stacked on one line. |
| Herdr coupling | None in J1 | A pane is a terminal. Taking `AsyncBufRead`/`AsyncWrite` keeps the crate testable without Herdr installed. |
| Reporting hook | Decorate `Link`, not `AgentController` | A pane driving a *remote* agent never touches the local controller, so a controller decorator reports nothing in the case that matters. |
| Herdr connections | One per report | Herdr closes after each response. A held connection succeeds once, then fails forever. Found by experiment, not documented. |
| Report backpressure | Bounded channel, drop when full | A sidebar label must never slow a prompt or fail one. |
| Herdr absent | Silent no-op, one startup line | kamiroh runs outside Herdr as a matter of course; that is not a degraded mode. |
| `Starting` state | `unknown`, not `idle` | "Idle" invites prompting an agent that is not ready. |
| Failed send | `unknown`, not `idle` | An unreachable peer says nothing about the agent behind it. |
| `serde_json` | Adapter-local, accepted | Herdr's JSON is Herdr's to change, and the pane id is unvalidated environment input needing real escaping. |
| Startup `greet` | Spawned, not awaited | An unreachable peer costs the full dial timeout (16s measured), and awaiting it holds up the console a person is waiting at. |

## Advisor consultations

- **Before slice A/B** — crate graph, the `EndpointId` trap, async-vs-generic,
  error types, allowlist semantics, agent-agnostic payloads, workspace mechanics,
  and what "green" must mean. All adopted; the deferred-crate decision is
  recorded above and in ARCHITECTURE.md §2.
- **Before declaring the phase done** — found one real hole and three points of
  polish, all fixed:
  1. **`Origin::Local` was constructible by any crate depending on
     `kamiroh-ports`**, putting a future Iroh adapter one typo away from silently
     disabling the allowlist. `Origin` is now opaque with named constructors. The
     tell was a runtime `panic!` in a loopback test standing in for a type-level
     guarantee; that assertion is now about the adapter, not the type.
  2. `InMemoryKeyStore` copied bytes out of `NodeSecret` into a `Debug`-deriving
     struct, stripping both the redacted `Debug` and the zero-on-drop. It stores
     the `NodeSecret` itself now, pinned by a test.
  3. ARCHITECTURE.md claimed a no-enumeration property the *wire* protocol does
     not yet owe; §7's slice-F row now records that obligation explicitly.
  4. `missing_docs` promoted from `warn` to `deny`, so the doc's promise of a
     warning-free build is enforced rather than aspirational.
- **Before slice E** — endorsed the design (hex storage, permission check before
  read, `0o022` mask on the parent directory rather than requiring `0o700`,
  wiped intermediate buffers, `NodeSecret::from_fill` so no plaintext copy
  exists outside the type) and set the scope boundary: E is custody only, the
  ed25519 endpoint id belongs to F.
- **On the race, mid-slice** — the pre-slice advice had been to prefer `O_EXCL`
  on the final path and *not* to use `hard_link`. The reproduction overrode it:
  that framing weighed clobbering against non-clobbering and missed the
  atomic-publish axis. Reconciled, then switched. Worth remembering as a pattern
  — a reproduced failure outranks a design preference.
- **Before slice F** — split F into F1/F2, use `iroh-base` rather than `iroh` for
  identity, and check the derivation is not the old inversion. All adopted.
- **F2 and G — not consulted.** Both were flagged for it in advance: F2 as an
  architecture *and* security gate, G because §4 lists "actor message enum". The
  gate was not met on either. Recording it because the alternative is that the
  omission disappears and the plan quietly becomes something nobody follows.

  What stood in for it: the two decisions that would have gone to the advisor
  were instead written down with their alternatives and the reason for
  rejecting them — the wire format in F2 (Decisions table), and the location of
  the `Agent` trait in G. Both are reversible without touching the app layer.
  The security-relevant half of F2 is pinned by a test that states the property
  as what an observer can distinguish, which is the form that survives being
  re-read.

  Still worth a review when one is available, in this order: the enumeration
  argument in F2 (it rests on ordering, so it breaks quietly if authorisation
  and lookup are ever reordered), then `Agent` as an adapter trait, then the
  bounded-mailbox reasoning in G.

  **All three were worked through later — see "The review queue, worked
  through" above.** The `Agent` item was resolved by M1 rather than reviewed;
  the other two produced a mutation test and a real bug fix respectively. A
  human review is still worth having, but these are no longer unexamined.
- **Slice I — flagged at the gate, before the design was fixed, and still not
  consulted.** The improvement over F2/G is only in timing: the four decisions
  (fatal-on-malformed, absent-is-empty, env override, reload scope) were put up
  as explicit alternatives with their trade-offs *before* anything was written,
  rather than justified afterwards. That is what the plan's step 3 is for, and
  it is the closest a single session gets to the gate on its own.

- **Slice J1 — not consulted, and it is the one where a review would have paid.**
  Not for the trust boundary, which is the part I flagged in advance: `LocalLink`
  calling `Origin::local_front()` is exactly what that constructor was added for.
  For the *shape*. Two rounds of questions went out framed as "which kind of
  second front", because the plan and ARCHITECTURE.md both said "front", and
  both rounds were the wrong question. It took Casey saying plainly that the
  point is driving a **remote** agent from a pane.

  The lesson is not about advisors. A design gate would probably have repeated
  the plan's framing, since the plan is what it would have read. What actually
  caught it was the person who knew the intent. Worth remembering: when the
  written plan and the README disagree — the README said "locally or across the
  network" all along — the disagreement is the signal, and asking beats
  reconciling them alone.

  Add to the review queue, above the G items: whether **malformed-is-fatal** is
  the right call. It is the one decision here that can take a running fleet
  down — a bad edit to a config-managed file stops every node that restarts —
  and the alternative (start, admit nobody, complain loudly) is defensible in a
  way the other three alternatives are not.

- **The security posture, finally read by something other than its author.** The
  advisor was on for this session, so the gate that F2, G, I and J all recorded
  as unmet was met — against the *code*, not the prose, because this repo has
  twice shipped a violation of a rule the same commit cited.

  Four claims were checked where they actually live. All held: authorisation
  returns early before any lookup (`control_service.rs`), `Origin` is a public
  struct over a private enum so `Local` is unconstructible outside
  `kamiroh-ports`, the Iroh front passes `connection.remote_id()` straight into
  `Origin::remote` with nothing from the request frame touching the trust
  decision, and the `local_front` grep still shows only `LocalLink`, the
  composition root and one test.

  **Three findings, all about documents disagreeing with code.**

  1. **The local-trust grant is stdin, not the machine.** §3 justified the
     allowlist bypass with "a pane is a process on this machine, started by
     whoever owns the node". The code grants it to whoever can write to this
     process's stdin — the composition root hands `console::serve` a reader over
     `tokio::io::stdin()` and `LocalLink` stamps `local_front()` on everything
     arriving there. The same set on a desktop; not the same under a service
     manager holding a pipe, a shared account, or `kamiroh < file`. Nothing is
     wrong in the code; the claim was about the wrong thing, and the narrower one
     is checkable — to know who holds local trust, ask what is on the other end
     of stdin.
  2. **Two paragraphs described a node that no longer exists.** §6 said the
     loopback transport delivers the smoke path via `Origin::remote(local
     endpoint)` and "the binary self-allows for that reason" — pre-F2 behaviour;
     `local_smoke` has sent `local_front()` + `Status` since M1 and
     `build_allowlist` never inserts this node. §7 still claimed the
     `local_front` grep showed the composition root as its only caller, which J1
     changed. Both corrected. Worth noting the shape: a *security* document going
     stale in the direction of claiming a check that is not performed.
  3. **The disclosure had no documented exit, and the runbook's safety argument
     was unsupported.** §5a covered what `Anywhere` publishes and never said
     whether a node can stop. `reachability-test.md` asserted the throwaway
     records "expire on their own" as the reason publishing during the test is
     harmless — an assertion nothing in the repo established.

  Settled by reading `iroh 1.0.3` rather than reasoning: `DEFAULT_PKARR_TTL` is
  30s and `DEFAULT_REPUBLISH_INTERVAL` is 5 minutes. So publishing is a
  **refresh, not a broadcast** — it stops when the node stops or `KAMIROH_REACH`
  returns to `direct`, and crucially **the exit needs no new identity**. That was
  the answer worth having: rotating an endpoint id invalidates every peer's
  allowlist entry, so a disclosure undoable only that way would have been
  permanent in practice for a fleet. The residual — what n0's relay retains after
  a node stops refreshing — is their policy and is now stated as unknown rather
  than implied to be fine.

  **One removal.** `Origin::is_local()` had zero callers. A convenient predicate
  on the trust type invites an adapter to branch on trust somewhere other than
  `ControlService`, which §5 names as the one place authorisation happens. Gone,
  with the reason recorded on `remote_endpoint`, which is now its only reader.

  **The gate is met once; the item does not close.** A same-session advisor is a
  second reader, not an independent one, and the question §5a actually asks —
  whether opt-in is sufficient mitigation — is a judgment about a threat model
  and is Casey's. OPEN-DECISIONS #3 is rewritten from "nobody has read this" to
  that single remaining question.

## Next slice

**The lettered plan is finished.** A→J are done. What follows was deferred by
the plan rather than specified by it, so this is a planning decision, not a
queue to work through in order.

The candidates, most valuable first as they look from here:

1. **A real agent runtime behind `Agent`.** Everything below the console is
   real: identity, allowlist, transport, front, controller actors. What an
   agent *does* is still `EchoAgent`. This is the last stand-in in the system
   and the only one a user would notice. `Agent::run` is already the seam, and
   its cancel-safety contract is already written down.
2. **Reporting for serving nodes.** J2 covers a pane driving an agent. A node
   whose agent is driven by a *remote* peer shows nothing, because inbound
   messages arrive through the Iroh front rather than any `Link`. That is the
   `AgentController` decorator after all, as a second reporter —
   `pane.report_agent` takes an optional `seq` for ordering two sources on one
   pane.
3. **An allowlist reload trigger.** `FileAllowlist::reload()` exists, is tested,
   and has no caller. A signal handler or a `/reload` console command would give
   it one. The atomic swap — the hard part — is done.
4. **The review queue**, which has been accumulating since F2 and is the only
   item here that is about existing code rather than new code. See *Advisor
   consultations*: the F2 enumeration argument, malformed-is-fatal in I, `Agent`
   as an adapter trait, the bounded-mailbox reasoning in G.

**That pass is done: [kamiroh-phase-2.md](./kamiroh-phase-2.md).** It supersedes
the list above, which was a leftovers pile rather than a plan. Two findings
changed the ordering:

- **The README's headline case does not work.** `bind_endpoint` uses Iroh's
  `Minimal` preset — no relays, no discovery — and `KAMIROH_PEER` needs a
  routable `host:port`. "Home ↔ cafe, behind NAT" has no such address. Every
  demo so far has used `127.0.0.1`, which hid it.
- **The missing agent runtime is already installed.** Herdr's socket API has
  `agent.start`, `agent.prompt`, `agent.wait` and `agent.read`. `Agent::run`
  maps onto them, so a kamiroh agent is *an agent Herdr is managing* — which is
  what the README always described, and why `Agent` went in the adapter rather
  than in the ports crate.

Phase 2 is four outcome-shaped milestones: a real agent, reachability, several
agents, then the accumulated debt. Its open question — what `blocked` means when
an agent is waiting for a human — is the one that decides whether Phase 2 has to
touch `kamiroh-domain`, which nothing since slice B has.

## Known nits (not worth their own commit)

- ~~`FileKeyStore::default_path()` returns `KeyStoreError::Malformed`~~ —
  **fixed in M4** with a new `KeyStoreError::Unconfigured { reason }`, which is
  what the note below concluded was needed. Kept for the reasoning: Nothing is malformed there — the
  environment is unconfigured — and it reads as a corrupt-key-file error to
  whoever hits it.

  **The fix this note used to propose does not work.** `KeyStoreError::Missing`
  is a unit variant whose message is "no node secret available and this key
  store cannot create one" — about a missing *secret*, not a missing
  environment, and carrying no detail. Swapping it in would trade a misleading
  message for a differently misleading one. The real options are to add a
  variant (`Unconfigured { reason }`, additive but a port change) or to leave
  it. I is the second slice to look at this and decline: it added
  `AllowlistError::Unconfigured` for the sibling path rather than widen the port
  mid-slice, so the two now disagree with each other, which is its own small
  argument for doing it properly next time.
- `ScopedTempFile` has no disarm: `Drop` always removes the temp. Correct today.
  If a future change needs the temp to survive (retrying a link, say), add a
  disarm rather than restructuring the guard.
- An interrupted prompt answers its caller with `ControllerError::Rejected`,
  whose doc says the controller "cannot accept this message in its current
  state". The message *was* accepted and then abandoned, so the fit is loose.
  Nothing else fits better without a domain change, and no caller can act on the
  difference. Revisit if `ControlReply` ever grows a cancellation case.

## Blockers

None. Note the sandbox has no network: Iroh work builds and unit-tests inside
it, but the two-node integration tests and the two-process demo need a run
outside it. `kameo` and its three new crates resolve from the local registry
cache, so slice G needed no network at all.

## Notes for the next session

- The sandbox has no network. Cargo resolves from the local registry cache;
  append `--offline` if a fetch is attempted.
- `cargo clippy --workspace --all-targets` is warning-free today. `missing_docs`
  is `deny` in every library crate, so undocumented public items — including enum
  struct fields — break the build rather than nagging.
- Commits use conventional-commit subjects, one per slice, on `master` — the
  working method is explicitly "one session on the main checkout".
- **Slice headings carry no commit hash.** This file is written before the commit
  exists, so any hash in it is either stale or missing — and a file where some
  slices have one and others don't reads as "that slice isn't committed yet".
  `git log --oneline -- docs/LOOP.md` is the mapping.
- `KAMIROH_KEY_FILE` overrides the key path. Use it when running the binary
  inside a sandbox, or to run several nodes on one machine.
- **The advisor is Opus, and it is a session-level setting** — `/advisor opus`,
  or `claude --advisor opus` at launch. The docs used to say Fable throughout;
  Fable is not currently selectable as an advisor, and they now say "advisor".
  If it is off at a gate, say so rather than self-reviewing and calling the gate
  met: what the gate buys is a reviewer that is not anchored on the reasoning it
  is meant to attack, which a same-session pass cannot be.
- **Update this file in the same commit as the slice it describes.** F2 shipped
  without its entry and the gap was only noticed a slice later, at which point
  the details had to be recovered from the diff. The plan's step 7 is "write
  LOOP.md + commit", in that order.
- **Anything spawned from a session inside Herdr inherits `HERDR_*`.** The demo
  scripts started nodes that reported their agent state into the *real* pane
  being worked in, because `HERDR_PANE_ID` was in the environment. They now
  `unset` it. The general form: a demo run from inside a live tool inherits that
  tool's environment, and a demo must not touch a live session.
- **The demos are in the repo now**, under `demos/`, with a README recording
  what each shows and the traps that cost a debugging round apiece — the
  wildcard bind, the third whitespace field, inherited `HERDR_*`, one request
  per Herdr connection, and rebuilding before running. They used to live in a
  session scratchpad and be rewritten from memory each time, which is how those
  traps kept being rediscovered.
