# kamiroh

Kameo actors for agents over Iroh. Peer actors addressable by name and endpoint,
driven locally or across the network. A control layer for long-running coding
agents — **kamiroh routes and authorises; it never interprets what an agent
does.**

This file is loaded every session, so it stays short and points at detail rather
than repeating it. **Start by reading `docs/LOOP.md`** — it opens with what is
done, what is next, and which decisions are waiting on Casey.

## Non-negotiable

- **Dependencies point inward.** Adapters → app → ports → domain. Verify rather
  than assume: `cargo tree -p kamiroh-domain -e normal` must list nothing else.
- **`kamiroh-domain` and `kamiroh-app` contain no Iroh, Kameo or Herdr types** —
  not in signatures, not in `Cargo.toml`.
- **Agent-agnostic.** Payloads are opaque: never parsed, never logged. Log
  `?message`, never `%payload.as_text()`.
- **The composition root wires and nothing else.** Logic belongs in
  `kamiroh-app` or an adapter.
- **Deny by default** at the allowlist, always.

## Working method

One slice at a time on `master`. Verify with cargo, **update `docs/LOOP.md` in
the same commit as the slice it describes**, conventional commit subjects.

Before committing, all three must be clean:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

The full recipe, including the boundary checks, is in `docs/LOOP.md`
under *Verification*.

## Traps that have each cost a debugging round

- **`cargo test` does not rebuild the binary.** A demo can silently exercise
  yesterday's code. Run `cargo build` before any script that uses
  `target/debug/kamiroh`. This has wasted two rounds.
- **The sandbox has no network and cannot bind sockets.** Iroh and Herdr tests
  fail with "Operation not permitted" — that is the environment, not a bug.
  Re-run them outside the sandbox.
- **`grep -i iroh` matches "kam*iroh*"**, and `kameo` matches
  `kamiroh-adapter-kameo`. Use `cargo tree -i <crate>` for boundary checks.
- **A fake built from a schema agrees with whatever you assumed the schema
  meant.** Live runs have overturned three designs every unit test agreed with.
  Prefer a real run for anything touching Herdr or Iroh.
- **stdout belongs to the pane console; diagnostics go to stderr.**
- **Demos inherit `HERDR_*`** from a session running inside a pane, and will
  report test state into the pane you are working in. `demos/*.sh` unset them.

## Decisions and review

`docs/OPEN-DECISIONS.md` holds decisions that are Casey's to make. **Raise them;
do not settle them** — state what would settle each and let him choose.

The advisor is a session-level setting (`/advisor opus`). At an architecture or
security gate, if it is not on, **say so** rather than self-reviewing and calling
the gate met. It was met once, for the security posture — against the code, not
the prose, which is the distinction that made it worth anything. Open decision 3
records what held and the one judgment still outstanding.

## Where things are

| | |
|---|---|
| `docs/LOOP.md` | state, decisions, verification recipe — **read first** |
| `docs/ARCHITECTURE.md` | the design as built; §6a–§6f are the enforced rules |
| `docs/OPEN-DECISIONS.md` | waiting on a person |
| `docs/kamiroh-phase-3.md` | current plan |
| `docs/reachability-test.md` | the two-machine NAT procedure |
| `demos/` | scripts that verify claims against real processes |
| `docs/prompt.txt` | the original master prompt, for history |
