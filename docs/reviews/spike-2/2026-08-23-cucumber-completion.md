# Completion: the cucumber-rs errand (roadmap 6)

*From Ander, 2026-08-23 (run on Casey's local date; the errand's suggested
filename said 08-24). Spike 2's last build work. Branch `code/cucumber-rs`
at `9b4a399e`, pushed; `vendor-snapshot` recut at `157c6d61`, force-pushed.*

---

## STEP 0 — the arrayref cache check on this host: **CLEAN**

Both required checks came back empty:

```
ls ~/.cargo/registry/cache/*/ | grep -E "arrayref-0.3.10|internment-0.8.7|…"   → no output
netstat -an | grep 23.254.165.112                                              → no output
```

Widened before reporting, so the result says what *is* here rather than only
what is not:

- **`arrayref-0.3.9` is present** in both `registry/cache` and
  `registry/src` — the pre-compromise version, not the malicious `0.3.10`.
- None of `internment-0.8.7`, `append-only-vec-0.1.9`, `proc-macro1`,
  `proc-macro-en*`, `aovine`, `arone`, `tinymember` appear in either
  directory.
- No connection to `23.254.165.112`, and none to anything in `23.254.165.0/24`.

Host: cargo/rustc 1.97.0. **The gate passes; the bump proceeded.**

One note while looking: the errand says "the incident record in the project
docs governs", and I could not find one — no file in `kamiroh/docs/` or beside
the repo mentions arrayref, and `docs/WORKFLOW.md` has no dep-bump hardening
section. It did not matter (the host is clean, and the errand states the
hardenings inline), but **the incident record does not appear to exist
in-repo**, and something this procedure defers to should probably live there.

---

## What shipped

| | |
|---|---|
| Branch | `code/cucumber-rs` @ `9b4a399e` (one commit on master `7defda84`) |
| Shelf | `vendor-snapshot` @ `157c6d61` — true orphan, 442 vendored crates |
| `Cargo.lock` content key | `2dba33cab2376ab4c1807cc2fb6100dcdb280746` (sha256 `f70d13ca0655e707…`) |
| Online gate | 82 ordinary tests + **25 scenarios**, clippy zero, fmt clean |
| Offline gate | same, with **zero crates.io touches** |

The shelf's `Cargo.lock` blob is byte-identical to the branch's, so the two
cannot drift.

---

## 1. The dependency, and its review

**`cucumber = "=0.23.0"`**, exact-pinned. Published 2026-04-23 — **121 days
old**, not yanked, MSRV 1.88 (well under the 1.95 sandbox floor), MIT OR
Apache-2.0. Comfortably past "more than a few days ago".

**`tokio = "=1.53.1"`** joins as a dev-dependency, also exact-pinned. It was
already in the tree via iroh and kameo, so **it adds no crate to the
lockfile** — I verified that by diffing before and after (`new: NONE,
removed: NONE`). It is there because one scenario is about real connections,
and real sockets need a reactor (§2).

### The lockfile diff, crate by crate

56 new crates, 0 removed, 393 → 449. Reviewed before anything was built:

- **Sources.** All 56 from `registry+https://github.com/rust-lang/crates.io-index`.
  No git dependencies, no path overrides, no alternative registries. Every
  one carries a checksum.
- **Build scripts.** Six: `anyhow`, `crossbeam-deque`, `gherkin`, `rustix`,
  `serde_json`, `zmij`. I pattern-checked all six for process spawning,
  network access, and `$HOME`/`.ssh` reads. The only `Command::new` hits are
  the standard `rustc --version` feature-detection idiom (anyhow, rustix,
  zmij). I read `gherkin`'s in full — it deserializes its own
  `src/languages.json` and writes keyword tables into `OUT_DIR`, textbook
  codegen with no I/O beyond that.
- **Typosquats.** I ran an edit-distance scan of all 56 new names against
  every pre-existing name (the `proc-macro1`-vs-`proc-macro2` shape that the
  incident turned on). Thirteen pairs came back within distance 2, all short-
  name coincidences (`nom`/`log`, `clap`/`slab`, `bstr`/`ctr`). I checked the
  identity of every name I did not recognise on sight:
  - **`zmij`** — dtolnay, "double-to-string based on Schubfach", i.e. ryu's
    successor, pulled by `serde_json`.
  - **`synthez`** — tyranron/arcane-rs, who also maintains cucumber-rs itself.
  - **`sealed`**, **`inflections`**, **`smawk`**, **`peg`** — all real crates
    with matching upstream repos.
  - **`noq`** (distance 1 from the new `nom`, and pre-existing) — n0-computer's
    own "UDP sockets with ECN for QUIC", pulled by iroh. A coincidence, not a
    squat.

**Nothing looked off, so nothing stopped.**

### `cargo audit`: one finding, and it is not from this bump

`cargo-audit` was not installed on this host; I installed it. The scan
reports:

> **h2 0.4.15 — RUSTSEC-2026-0258, "h2 unbounded empty DATA frames"**,
> published 2026-08-17. Solution: upgrade to >= 0.4.16.

**This is pre-existing, not introduced here.** `h2` was already in the
lockfile before cucumber (I checked the before-snapshot), reached via
`iroh → iroh-relay/iroh-dns → hickory-resolver → hickory-net → h2`, and via
`hyper`. The advisory is six days old — newly disclosed against a dependency
we already had.

**I did not bump it, deliberately, and this wants your ruling:**

- It is outside this errand's scope: a pre-existing transitive dependency,
  not part of the cucumber subtree the STOP condition covers.
- Every fixed version is **inside the freshness window the errand itself
  tells me to avoid**: 0.4.16 is 6 days old, 0.4.17 is 4, 0.4.18 is 3. The
  fix is fresh precisely because the vulnerability is. That tension is real
  and it is yours to resolve, not mine to resolve silently.
- Exposure here is slight: it is a DoS against HTTP/2 *servers*, reached in
  this tree only through iroh's DNS resolution and relay client paths, in a
  spike that ships to nobody.

But the shelf I just pushed carries `h2 0.4.15`, so the decision should not
sit long. The fix would be a lockfile-only `cargo update -p h2` plus a shelf
recut — cheap, and I can do it in a follow-up the moment you say so.

Two `unmaintained` warnings (`atomic-polyfill`, `paste`) are long-standing,
also pre-existing, and allowed by the audit's own configuration.

---

## 2. All four feature files are bound — 25 scenarios, 118 steps

`tests/cucumber.rs` (~1,000 lines) binds every scenario in all four files.
The whole suite runs in **about 1.5 seconds** and is part of
`cargo test --workspace`.

Plumbing is the ordinary tests' machinery — memory adapter, hand-cranked
`TestTimer`, the `stage`/`open_live_exchange` shapes — and the scenarios were
not touched except to correct their own headers, which still said "NOT YET
EXECUTABLE".

**Story 3's binding note was heeded.** The allowlist-mutation scenarios bind
`LocalRuntime`-side, so "the very next delivery" is literally true rather than
true-if-you-await; the reasoning is recorded in the file header and in
`features/README.md` so the next reader does not have to rediscover it.

**One scenario runs on real QUIC.** *A conversation spans connections* is
about connections, and the memory net has none — it routes by address and
never dials. Binding it to the simulator would have asserted something the
simulator cannot witness. It runs on the iroh adapter over loopback with a
nested reactor inside the step, exactly as `iroh_disconnects.rs` pins it.
This is the only reason `tokio` is a dev-dependency. I judged this better
than the alternatives (skip it, and break the very rule step 3 asks me to
preserve; or fake it against the simulator) — but it is a judgement call, and
the honest summary is that 24 scenarios are memory-backed and one is not.

### A finding the binding surfaced: the stock parties are single-conversation

Binding *"a running actor that admits two endpoints / and a conversation in
progress with each"* wedged, and the reason is worth recording:

**`EchoParty` and `CountdownParty` keep one shadow `TurnState` for the whole
party, not one per peer.** With two peers on one actor, the first
conversation leaves the shadow in `AwaitingTheirTurn`, so the second peer's
opening turn is refused as `MustAnswerFirst` and silently swallowed. The
runtime keys conversations per peer (decision 17); the stock parties' shadow
of that state does not.

This is adjacent to — but distinct from — the external review's Grok P2 (that
the parties never *failed* their shadow, fixed on `cowork/external-reviews`).
That fix made the shadow honest about failure; this is about the shadow being
single-tenant in the first place. It matters for the same reason: these are
the examples embedders copy, and an embedder copying `CountdownParty` for an
actor with two peers would lose turns silently.

I did **not** change the stock parties — that is a behaviour change to
`crates/`, outside this errand's scope. The cucumber harness has its own
per-peer party, with the reasoning in a comment. **Recommend a follow-up
errand** to either make the stock parties per-peer or document them as
single-conversation examples.

---

## 3. `features/README.md` rewritten

The interim scenario → pinning-test tables are retired: they existed because
the scenarios could not run, and the promissory note is paid. The file now
describes the runner, and the standing rule it carried — *a scenario without
a step is a gap, not a decoration* — is now **enforced rather than trusted**:
the runner uses `fail_on_skipped()`, so an unbound scenario fails the suite.

It also records that the ordinary tests remain the fast gate (nothing was
deleted), and the two deliberate binding choices above.

---

## 4. The bounded `block_on` — my carry-forward

`kamiroh_adapter_memory::testing::block_on` now panics rather than parking
forever: `STUCK_AFTER = 30s`, with `block_on_bounded(bound, future)` for a
different ceiling. The bound is wall-clock since entry, so a healthy future
that is repeatedly woken is never penalised — every future driven here is
memory-backed and virtual-clocked, so a healthy one finishes in microseconds.
The panic names what a stuck future means:

> nothing woke this thread, so it is stuck rather than slow. Something that
> should have arrived never did: an ack, a turn, a reply the peer owed, or a
> delivery a party swallowed.

Pinned by two unit tests, including a `#[should_panic]` one that would hang
the suite forever without the bound.

**It earned its keep within the hour.** Two of my own binding bugs surfaced
as a 120-second stall instead of an infinite one, and the message told me the
future was stuck rather than slow. That is precisely the class of regression
that cost diagnosis time in the last two review rounds.

One honest limit: when the bound fires inside the cucumber runner it aborts
the whole run without a scenario summary — better than hanging, worse than a
single failing scenario. The runner gets a generous 120s ceiling for that
reason.

---

## 5. Gate and snapshot

**Online** (rustc 1.97.0, crates.io): fmt clean, clippy zero
(`--workspace --all-targets -D warnings`), 82 ordinary tests + 25 scenarios,
all passing.

**Offline** (the shelf, as the sandbox will use it): the same 82 + 25, with
**zero crates.io touches** — verified by grepping the build log for
`Downloading`/`Updating crates.io`. I ran this *before* pushing the shelf, so
the snapshot is known-good rather than hoped-good.

The shelf: 442 vendored crates, 596 MB, 22,053 files, `.cargo/config.toml` +
`Cargo.lock` + `vendor/`, true orphan (0 parents), force-pushed as documented.
Verified after pushing by fetching it back.

Local `vendor/` and `.cargo/` were removed afterwards so ordinary builds go
back to crates.io — leaving the source-replacement config behind is a trap
the moment dependencies next change.

---

## Bounds kept

Master untouched. No behaviour change anywhere in `crates/` except the
bounded `block_on` in the testing module, as scoped. `git status` clean,
worktree on `code/cucumber-rs` @ `9b4a399e`.

## What is left before graduation

The roadmap's item 6 is done. Outstanding, in your court rather than mine:

1. **The `h2` advisory ruling** (§1) — the only thing I would not decide alone.
2. **The stock-party finding** (§2) — a follow-up errand, small.
3. The snapshot to staging, per the graduation boundary in `docs/VENDORING.md`.
