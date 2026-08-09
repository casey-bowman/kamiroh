# LOOP

## Current phase

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

**Not yet verified live.** Proving `anywhere` works means actually publishing
this machine's addresses to n0's service, which is the disclosure described
above. Left for an explicit decision rather than assumed.

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
cargo test  --workspace                        # 151 passed, 0 failed (as of J2)
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

- `FileKeyStore::default_path()` returns `KeyStoreError::Malformed` when neither
  `XDG_CONFIG_HOME` nor `HOME` is set. Nothing is malformed there — the
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
- The two-process demo lives in the session scratchpad, not the repo. It reads
  `endpoint id:` and `listening:` out of each node's stdout, and both fields
  have bitten it: the id is the *third* whitespace field, and the node binds a
  wildcard (`0.0.0.0:PORT`), not loopback. If it is worth keeping, it belongs in
  the repo next to the integration tests rather than being rewritten each time.
