# Demos

Scripts that drive real `kamiroh` processes and check what they actually do.
They are how nearly every claim in `docs/LOOP.md` was verified, and several
found bugs the unit tests agreed with — so they are here rather than in a
scratch directory.

Not tests. `cargo test` covers what can be checked in-process; these cover what
only real processes, real sockets and a real Herdr can show. Nothing runs them
automatically.

## Requirements

Plain `bash` — no version-4 features, so these run on macOS's bash 3.2 and on
Linux alike. Beyond a built binary:

| | for | note |
|---|---|---|
| `jq` | every script | not installed by default anywhere; `brew install jq` / `apt install jq` |
| `nc` with `-U` | the Herdr ones only | **the one real portability trap** — see below |

`nc -U` (connect to a Unix socket) is how the Herdr scripts speak to its API.
macOS ships a BSD `nc` that supports it. On Linux it depends which netcat is
installed: `netcat-openbsd` supports `-U`, **`netcat-traditional` does not**, and
nmap's `ncat` spells it `--unixsock`. If `two_node_demo.sh` works but
`kind_live.sh` cannot reach Herdr, that is why: `apt install netcat-openbsd`.

Nothing here needs Perl or Python. An earlier version used
`perl -e 'select(...)'` in place of `sleep`, purely because the environment they
were written in blocked a foreground `sleep` — a quirk of that harness, not of
the task, and not something to inflict on a reader.

### The `nc -U` dependency should go, and there is an obvious way

These scripts hand-roll Herdr's newline-JSON protocol over `nc -U` and parse it
with `jq`. **Herdr ships a CLI that does all of it** — `herdr pane split`,
`herdr agent start`, `herdr pane get`, `herdr agent read`, `herdr pane close`
all exist and cover every call made here. Using it would drop `nc` entirely,
drop most `jq`, work wherever Herdr does — which these scripts require anyway —
and stop reimplementing someone else's wire format in bash.

Worth recording the wrong turn as well as the right one: the tempting fix is to
have the demos use *kamiroh's* Herdr client instead. That is a trap. The calls
they need are pane management — split, start, close — which kamiroh deliberately
does not do, and exposing them would push it toward being a worse Herdr. That is
a stated non-goal in `docs/kamiroh-phase-3.md`, and M3 confirmed attach-only was
right. The scaffolding is the test harness's business, not kamiroh's.

Not urgent: `nc -U` works on macOS and on Linux with `netcat-openbsd`. It
matters when someone on a Nix or minimal Linux box first tries to reproduce a
claim, and it is a small, well-defined change whenever that becomes real.

```bash
cargo build --workspace
./demos/two_node_demo.sh
```

## What each one shows

| script | shows | needs |
|---|---|---|
| `two_node_demo.sh` | two nodes over Iroh; an allowlisted peer echoes, an unlisted one is refused; identity survives restart | sockets |
| `allowlist_demo.sh` | all six allowlist sources, including the two that must refuse to start | — |
| `pane_demo.sh` | a pane console driving an agent on **another node**, and the refusal path | sockets |
| `m2_live.sh` | dialling by endpoint id with no address, **with a negative control** | internet |
| `disclosure.sh` | a stranger *finds* a published node and is refused — ARCHITECTURE.md §5a | internet |
| `m3_live.sh` | two agents on one node, driven by name from a peer | sockets |
| `kind_live.sh <kind>` | a real coding agent driven end to end through kamiroh | Herdr + that agent |
| `unblock_test.sh` | a blocked agent's question answered **remotely**, clearing it | Herdr + claude |
| `scratch_pane_test.sh` | agent state reported into a real Herdr pane | Herdr |

## Things these scripts learned the hard way

Each of these cost a debugging round. They are in the scripts; they are here so
the next rewrite does not rediscover them.

- **A node binds a wildcard** (`0.0.0.0:PORT`), not loopback. Parsing
  `listening:` for `127.0.0.1` finds nothing.
- **`endpoint id: <hex>` puts the id in the *third* whitespace field.** Taking
  the second yields the literal `id:`.
- **Anything spawned from a session inside Herdr inherits `HERDR_*`**, so a demo
  will report its test nodes' state into the pane you are sitting in. Every
  script here unsets those variables. This is not hypothetical — it happened.
- **`agent.start` fails with `agent_pane_busy`** until a freshly split pane's
  shell reaches its prompt. Wait a few seconds after `pane.split`.
- **Herdr answers one request per connection**, then closes. Three `ping`s on
  one socket produce one response.
- **macOS bash has no negative array subscripts.** `${PIDS[-1]}` is an error, not
  the last element.
- **Rebuild before running.** `cargo test` does not rebuild the binary, so a
  demo can silently exercise yesterday's code. This wasted two rounds.

## The ones that cost money

`kind_live.sh` and `unblock_test.sh` start a real coding agent, which spends
tokens. They run it in an empty temp directory rather than the repo, so an agent
has nothing of yours within reach, and they close the pane afterwards.

The first run in any new directory raises a workspace-trust dialog that a human
must answer — deliberately. Approving a consent prompt from a script is one step
from approving it in the product; see `docs/LOOP.md`.

## The ones that publish

`m2_live.sh` and `disclosure.sh` set `KAMIROH_REACH=anywhere`, which publishes
those nodes' addresses to n0's lookup service. Throwaway keys, so the records
are keyed to ephemeral ids and expire. See ARCHITECTURE.md §5a before running
them on a machine whose address you would rather not publish.

## Not kept

The `probe_*` scripts that mapped out Herdr's agent state machine were
exploratory and two had quoting bugs. What they found is recorded in
ARCHITECTURE.md §6e and `docs/LOOP.md`; the scripts themselves are not worth
carrying.
