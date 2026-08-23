# How kamiroh is built

*A human-facing companion to the operational notes in
[TIERS.md](TIERS.md), and to the argument for them in
[WHY-TIERS.md](WHY-TIERS.md) — for future developers curious about, or
wanting to borrow, the workflow behind this project. Written August 2026,
mid-spike-1; revised 2026-08-19 at the opening of spike 2, when several of
the constraints that shaped the original workflow were re-verified and
found changed — see
[briefs/2026-08-19-cloud-capability-check.md](briefs/2026-08-19-cloud-capability-check.md)
for the measurements behind every claim below.*

kamiroh is developed by a small ensemble: one human (Casey) making the
decisions and holding the keys, and two named AI sessions doing the drafting,
building, and reviewing — a cloud-based design session with long-lived
context, and local Claude Code sessions with network access and build tools.
The workflow below is what lets that ensemble move fast without stepping on
itself.

## Tiers: one fork per architectural experiment

Development proceeds in **spikes** across a set of repos called **tiers**,
each a fork of the main repo (one org per fork, since GitHub allows one fork
per org):

- **main** (`casey-bowman/kamiroh`) — the canonical repo.
- **staging** (`kamiroh-staging/kamiroh`) — the integration tier. Every
  architectural spike is recorded here as a branch before anything reaches
  main.
- **workshop-N** (`kamiroh-workshop[-N]/kamiroh`) — one fork per
  *architectural spike*: a grand-scale experiment where the fork *is* the
  experiment. Smaller *implementation spikes* are just branches inside a
  workshop fork. This repo is workshop-2, the third architectural spike —
  and, unlike workshop-1 (which was designed from scratch, deliberately
  unpolluted by its predecessor's choices), it carries spike 1's final tree
  forward on purpose: hardening around a sound core, per
  [ROADMAP.md](ROADMAP.md).

The flow: work lands on a workshop's `master`; when a spike is ready it is
published to staging as a `spike-<name>` branch; staging's mainline
eventually graduates to main.

## The division of labor

The **design session** (cloud), called **Mez**, holds the architecture: it
writes `ARCHITECTURE.md` and most of the code, keeps the glossary and decision
log, and reviews everything before it merges. Its constraints changed between
spikes, and the workflow changed with them.

What still holds (re-verified 2026-08-19): the sandbox cannot reach
crates.io and cannot push to GitHub, and its toolchain is pinned (see
Toolchains below). What changed for spike 2:

- **The cloud reaches GitHub, read-only.** Anonymous clone and fetch work,
  so Mez verifies pushed results directly, with no courier in the loop.
- **The cloud runs the full hermetic gate itself.** With `vendor-snapshot`
  laid down per [VENDORING.md](VENDORING.md), `cargo build`/`test
  --workspace --offline` run in-sandbox (spike-1 final tree: 39/39 green,
  including the real-loopback-QUIC suite). **Blind-writing with assumption
  lists is retired** — spike-1's habit, with its 4/5 kameo, 4/7 iroh track
  record, is preserved in this file's history; Mez now compiles and tests
  against vendored deps before proposing anything.
- **A bridge to the human's clone.** The spike-2 session can read Casey's
  local clone, write files into its folder, and run *local* git there (no
  network on that side). Finished work still travels as git bundles for
  exact-hash fidelity, but Mez now lays the bundle down and fetches it into
  the local clone itself: branches under `cowork/*` simply appear, tested
  and ready for review. The human courier steps of spike 1 are gone;
  the human *gates* — merge and push — are untouched.

**Build sessions** (local Claude Code), called **Ander**, do what the cloud
still cannot — and, now that the cloud builds and tests its own code, what a
*second, independent* session does better than the author. Ander's spike-2
portfolio: **independent review** of every `cowork/*` branch before Casey
merges (fresh eyes, and a floating toolchain whose newer clippy catches what
the pinned sandbox misses); everything **requiring push** — publishing
reviewed branches, force-refreshing `vendor-snapshot`, staging graduation;
**dependency bumps** against live crates.io; and **real-network field
checks** (the N0/NAT internet checks, Incus container runs) that need real
machines and open egress. The discipline is unchanged: strict scope
instructions, and a standing order to **STOP and write a brief** rather than
redesign when something structural surfaces. One such stop produced the
ports-`Send` advisory exchange preserved in `docs/advisories/` — the full
deliberation behind decision 15, kept because a decision log entry says
*what* and an advisory says *why it was hard*.

The **human** merges, pushes, arbitrates advisories, and makes every
decision that outlives the session that raised it.

## The back-and-forth: shuttling between Cowork and Code

The human is the courier between the two AI surfaces, and a few habits make
that shuttle nearly frictionless:

- **Paste-ready handoffs.** The design session ends every work stretch with
  a self-contained instruction block — repo path, branch, context, scope
  limits, the STOP condition, and what to report back. The human pastes it
  into Claude Code verbatim and types nothing else. All context a build
  session needs travels *inside the block*, because Code sessions start
  cold; the accumulated design context stays in the long-lived Cowork
  session, which is treated as the project's working memory.
- **"done" is a complete report.** Coming back the other way, a single word
  suffices when the errand was mechanical — the design session verifies
  results itself by fetching from GitHub rather than trusting a summary.
- **Files, not prose, for anything substantial.** When a build session has
  real findings (a completion brief, an advisory, a question), it writes a
  markdown file *next to* the repo — self-contained, readable without the
  codebase — and the human just says where it is. Answers travel back the
  same way. The best of these exchanges graduate into `docs/advisories/`.
- **Designs are born in example-mapping meetings, and the boards are
  kept.** New behavior starts as a mapped story: the design session
  prepares a board (story cards, rule candidates, seed examples, red
  question cards grounded in the code), the human arbitrates every red
  card in conversation, and rulings are recorded on the board as they
  land. Outputs graduate outward — rules to `ARCHITECTURE.md`'s decision
  log, examples to `features/`, implementation as a following errand —
  and the board itself is archived under `docs/mappings/`, one subfolder
  per meeting, as the record of how the session reasoned.
- **Scope discipline makes the shuttle safe.** Build sessions get explicit
  do-not-touch boundaries and a pre-authorized STOP; the design session
  reviews every diff against what it authorized before the human merges.
  The human never has to arbitrate mid-errand — only at the deliberate
  pause points the workflow creates.

## Rules that keep it sane

- **Agent branch namespaces.** Each AI works only under its slash-prefixed
  branches (`cowork/*` for the design session, Mez; `code/*` for the build
  session, Ander; other tools get their own prefixes). Nobody commits to
  `master` directly — it advances only by deliberate merges, done by the human
  or on explicit request.
- **Ancestry is sacred between tiers — with one carved-out boundary.**
  Traffic between long-lived tier mainlines uses plain merge commits or
  fast-forwards, never squash/rebase merges (which re-apply commits under
  new hashes and poison every later ahead/behind comparison). The one
  exception is deliberate: workshop → staging publication is a *content*
  boundary — the spike graduates as a fresh snapshot branch, leaving the
  workshop's heavyweight private history (see below) behind. Within staging
  and staging → main, the ancestry rule applies in full.
- **Vendored sources never touch mainline.** The cloud session needs
  `vendor/` for hermetic offline builds, but committed blobs ride
  ancestry-preserving merges forever — so vendor lives on a force-pushed
  orphan artifact branch (`vendor-snapshot`), and `master` gitignores it.
  `docs/VENDORING.md` has the mechanics. Both build paths are verified after
  every de-vendor, so neither silently rots: `master` must build *the
  ordinary way* against crates.io — wipe `target/`, then
  `cargo fetch --locked && cargo build --workspace && cargo test --workspace`,
  with the committed `Cargo.lock` resolving unchanged and nothing special to
  configure — and the offline path is checked the mirror way, restoring
  `vendor-snapshot` and rerunning under `--offline`. Last confirmed green on
  `master` after the iroh 1.0 bump: identical results both ways (38 tests,
  including the real-loopback-QUIC transport suite).
- **Decisions are written down twice.** The compressed *what* goes in
  `ARCHITECTURE.md`'s numbered decision log; contested decisions keep their
  full deliberation as advisory documents in `docs/advisories/`.
- **Attribution says who did what** (adopted 2026-08-23; earlier commits
  vary). The session that wrote a commit is its git *author* — literal
  provenance — and every agent commit carries
  `Co-authored-by: Casey Bowman <case.bowman@gmail.com>`, because the
  human rules every design decision that lands and the history should
  say so. The one inversion is deliberate: a spike's **graduation
  snapshot** to staging is *authored by the human* — the commits that
  outlive the workshops carry the name of the person who declared the
  spike question answered.

## The gate

Every branch offered for review runs, and reports: `cargo build
--workspace`, `cargo test --workspace`, `cargo clippy --workspace --tests
--examples` (zero warnings), and — since the 2026-08-20 sweep — `cargo fmt
--all --check`. The two toolchains' rustfmt outputs agree on this tree,
verified from both sides: the sweep itself was formatted by 1.97 (Casey's
host, where it also checks clean), and 1.95 checked the swept `master` and
this branch clean in the cloud sandbox (Mez, 2026-08-20) — so no canonical
formatter version is needed. If a future toolchain bump breaks that
agreement, the pinned cloud version is the floor here as everywhere, and
whoever hits the disagreement should suspect the versions before the code:
it presents as an inexplicable formatting-only diff.

## Dependency bumps — the hardenings

Adopted 2026-08-20, the day of the arrayref supply-chain attack (the
incident record is
[briefs/2026-08-20-arrayref-incident.md](briefs/2026-08-20-arrayref-incident.md));
first exercised in full by the cucumber-rs errand. Every bump that
touches `Cargo.lock`:

- runs on a host whose cargo cache has been checked against the current
  advisories first;
- pins **exact versions** (`=X.Y.Z`), preferring releases more than a
  few days old — a window a freshly compromised release has not yet
  survived. (A *security fix* responding to a public advisory may
  warrant an exception; that tension is a ruling for the human, never a
  quiet judgment call.)
- runs `cargo audit`;
- reviews the **new** portion of the lockfile diff crate-by-crate —
  names against typosquats, sources, and every `build.rs` among them
  read, not trusted. An unfamiliar crate name gets investigated by both
  sessions independently before it ships;
- recuts `vendor-snapshot` (content-keyed to `Cargo.lock` — see
  [VENDORING.md](VENDORING.md)), after which the cloud session's offline
  gate independently confirms the vendored tree builds with zero
  registry touches.

## Toolchains

The two sessions run different Rust toolchains, on purpose and by constraint:

- **Mez's cloud sandbox is pinned by its environment** — currently `rustc
  1.95.0` (re-verified 2026-08-19 in the workshop-2 session), and it *cannot
  change*: the network is allowlisted, so `rustup` can't download another
  toolchain. This is the **floor for language features** — code must compile
  on the sandbox's version.
- **Ander's local toolchain floats newer** — currently `rustc 1.97` — so it
  builds against crates.io and runs the real first build.

We deliberately do **not** pin a `rust-toolchain.toml`: a newer pin would break
Mez's sandbox outright, and pinning the older one would silence exactly the
signal we want. Ander's newer clippy catching lints Mez's older one misses is a
feature, not a discrepancy — those findings arrive via Ander and get triaged
case by case (e.g. the `Waker::noop()` cleanup that surfaced with the relay
landing). If the sandbox's pinned version ever changes, update this note.

## Where to read next

`ARCHITECTURE.md` for the system itself (start with the glossary — the
ubiquitous language is the architecture); `docs/EMBEDDING.md` for using
kamiroh from your own application; `docs/VENDORING.md` for the dependency
scheme; `docs/advisories/` for the hard calls and how they were made.
