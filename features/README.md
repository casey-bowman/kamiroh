# Feature files — the executable specification

These `.feature` files are co-authored artifacts of example-mapping sessions
(humans and agents together, per `docs/ROADMAP.md` item 6): the claims a
reader needs to believe, in glossary words a non-implementer can read and a
machine can falsify. Scenarios stay declarative and glossary-worded; plumbing
stays downstairs.

**They are executable.** Every scenario is bound to real step definitions in
`tests/cucumber.rs` and runs on every `cargo test`:

```
cargo test --test cucumber          # just the specification
cargo test --workspace              # the ordinary tests, and this
```

The interim scenario → pinning-test tables that used to live here are
retired. They existed because the scenarios could not run, and a table
mapping a claim to the test that happened to cover it is a promissory note.
The note has been paid: what each scenario asserts is now its own step
definitions.

## The standing rule, unchanged

**A scenario without a step is a gap, not a decoration.** The runner now
enforces it rather than trusting it: `tests/cucumber.rs` runs with
`fail_on_skipped()`, so a scenario whose steps are not all defined fails the
suite. Adding a scenario means adding its steps in the same change.

## What the runner is, and is not

`cucumber` 0.23, pinned exactly (`=0.23.0`) under the dependency-bump
hardenings — see `docs/briefs/` for the review that pin went through.

- **The ordinary `tests/*.rs` remain the fast gate.** They are finer-grained,
  they pin things no scenario should ever mention (mailbox arithmetic, stale
  timer reports, epoch bumps), and they are where a regression is diagnosed.
  Nothing was deleted when the scenarios became executable.
- **Cucumber sits on top and answers a different question**: does the spec a
  non-implementer can read still hold? It is the layer that fails when the
  *claims* stop being true, in the words the claims were made in.
- **Plumbing stays downstairs.** Steps reuse the ordinary tests' machinery —
  the memory adapter, the hand-cranked `TestTimer`, the `stage` /
  `open_live_exchange` shapes — so a scenario never has to say "mailbox" or
  "waker". If binding a step needs a word the glossary lacks, that is a
  finding about the scenario, not a licence to invent vocabulary.

## Two deliberate choices worth knowing

- **The allowlist-mutation scenarios are bound `LocalRuntime`-side.** Story
  3's binding note (decision 28) warns that "the very next delivery" means
  *the next delivery processed after the revocation resolves*. That is true
  of the toy runtime by construction — it mutates strictly between steps —
  and true of the Kameo runtime only if the caller sends after awaiting the
  revoke. Binding these steps to the reference runtime keeps the scenario
  honest without the Gherkin having to carry a mailbox-ordering caveat.
- **One scenario runs on real sockets.** *A conversation spans connections*
  is about connections, and the memory net has none — it routes by address
  and never dials. Binding it to the simulator would assert something the
  simulator cannot witness, so that scenario alone runs on the iroh adapter
  over loopback, exactly as `iroh_disconnects.rs` pins it. Everything else is
  memory-backed and virtual-clocked, which is why the whole suite finishes in
  about a second.

## The four stories

| Feature file | Story | Design record |
|---|---|---|
| `hung-exchanges-fail-loudly.feature` | 1 — timeouts | decisions 22–25 |
| `vanished-peers-fail-loudly.feature` | 2 — disconnects | decision 27 |
| `guest-lists-change-while-running.feature` | 3 — allowlist mutation | decision 28 |
| `one-send-opens-many-conversations.feature` | 4 — same-endpoint fan-out | decision 29 |

Boards for each session are archived under `docs/mappings/`.
