# kamiroh Build Plan

Kameo actors for agents over Iroh · Claude Code with an advisor model · prompt + loop  
Modular-monolith workspace · ports & adapters

> Default workflow: **one Claude Code session, one Herdr pane (optional), slice by slice on the main checkout**. Parallel worktrees are optional.

Concrete plan: **executor loop + advisor at commitment points**, building **kamiroh** as a **Cargo workspace modular monolith** with **ports & adapters**.

---

## 0. Preconditions

| Item | Choice |
|------|--------|
| Repo | Existing kamiroh (README + crates.io scaffold) |
| Tooling | **Claude Code** with repo folder access |
| Models | **Executor:** Opus (or Sonnet) · **Advisor:** Opus on main / hard decisions |
| Advisor setup | `/advisor opus` in-session, `claude --advisor opus` at launch, or `advisorModel` in settings. **Fable is not currently selectable as an advisor** — this plan originally named it, and the rest of the document has been changed to say "advisor" instead. |
| Stop rule | Each slice ends with `cargo check` (later test) green for touched crates |
| Default UI | One optional Herdr pane · one Claude session on main checkout |
| Dev config | See [claude-code-setup.md](./claude-code-setup.md) for `.claude/settings.json` sandbox and permissions |

The advisor is first-class in Claude Code. Use it on the **main** session for architecture and security gates. Worker worktrees (if any) usually run **without** an advisor unless that worker owns a hard decision.

What the gate buys is **independence**, not a model tier: a reviewer working from the code and docs rather than from the transcript of the reasoning that produced them. A same-session self-review is the weak form — it is anchored on the argument it is meant to attack. That is why the advisor is a separate session-level setting and not something the executor can decide it has already done.

---

## 1. Target architecture

**Modular monolith** = one repo, many crates, one deployable binary (for now).

```text
kamiroh/                          # workspace root
├── Cargo.toml
├── crates/
│   ├── kamiroh/                  # binary / composition root (wiring only)
│   ├── kamiroh-domain/
│   ├── kamiroh-app/
│   ├── kamiroh-ports/
│   ├── kamiroh-adapter-iroh/
│   ├── kamiroh-adapter-kameo/
│   ├── kamiroh-adapter-herdr/    # optional stub early
│   └── kamiroh-adapter-fs/
└── docs/
    ├── ARCHITECTURE.md
    ├── LOOP.md
    ├── claude-code-setup.md
    └── kamiroh-build-plan.md     # this file
```

**Rules**

- Dependencies point **inward**: adapters → app → domain; ports define boundaries.
- No Iroh/Kameo types in domain.
- Composition root wires adapters to ports.
- One controller actor per agent; fronts = Iroh + optional Herdr.

---

## 2. Master prompt (paste once at session start)

```text
You are building kamiroh in this repo.

Product (aims, not all implemented yet):
- Kameo actors for agents over Iroh
- Peer actors addressable by name and endpoint; message locally or across the network to drive agents
- Optional Herdr: local UX/lifecycle; same controller actor as Iroh fronts
- Allowlisted EndpointIds only; key custody per node
- Agent-agnostic: no assumption about what agents do
- No central application control gateway; Iroh relays may assist NAT only

Architecture constraints (non-negotiable):
- Cargo workspace modular monolith
- Ports and adapters (hexagonal)
- Domain and app have no Iroh/Kameo/Herdr concrete types
- Thin composition root
- MIT license; copyright personal name for now

Working method:
- Default: one session on main checkout; one slice at a time
- Executor implements and verifies; consult the advisor ONLY at architecture boundaries,
  public port traits, security-sensitive paths (allowlist, keys), and before declaring a phase done
- After every slice: cargo check -p <touched> (and cargo test when tests exist)
- Update docs/LOOP.md with: done / next / blockers after each slice
- Stubs for Herdr/Iroh are OK if ports are real

First deliverable:
1) Workspace layout + ARCHITECTURE.md
2) kamiroh-ports traits: transport, allowlist, key store, agent controller messages
3) No-op or in-memory adapters so the bin compiles
4) cargo check workspace green
```

---

## 3. The loop and slices

```text
1. READ  docs/LOOP.md + README aims
2. PICK  one vertical slice
3. ADVISOR if architectural/security
4. IMPLEMENT  ports → app → one adapter (as needed)
5. VERIFY  cargo check / test
6. ADVISOR if claiming phase done
7. WRITE  LOOP.md + commit
8. STOP or next slice
```

### Slice order

| # | Slice | Done when | Status |
|---|--------|-----------|--------|
| A | Workspace + crate graph + ARCHITECTURE.md | `cargo check` stub crates | ✅ |
| B | kamiroh-ports traits | Traits compile; no IO | ✅ |
| C | kamiroh-domain | Pure tests | ✅ (folded into A/B) |
| D | kamiroh-app use cases + mock ports | App tests green | ✅ (folded into A/B) |
| E | adapter-fs / keyring KeyStore | Load/create secret | ✅ |
| F | adapter-iroh stub | Compiles; optional smoke | ✅ split F1 identity / F2 transport + front |
| G | adapter-kameo controller | Local tell/ask works | ✅ |
| H | Composition root wires E+F+G | Binary starts; prints EndpointId | **absorbed — see below** |
| I | Allowlist adapter (may move earlier if F needs it) | Unknown peers rejected | next |
| J | Herdr adapter stub | Port exists | pending |

**Default path (one pane):** A→B→C→D→E→F→G→I→J serially on main with a single Claude Code session.

**H has no content on the serial path, by construction.** It was written as the reassembly step for the parallel path below, where E, F and G are built in separate worktrees behind stable ports and nothing is wired until after the merges. Serially, `ARCHITECTURE.md` §7 governs instead — *"each is a constructor swap in `crates/kamiroh/src/main.rs`"* — so every slice integrates as it lands. E swapped in `FileKeyStore`, F2 added `bind_endpoint` / `front::serve` / `IrohTransport`, G swapped in `KameoController`. By the end of G the composition root already held all three, and H's whole content had been paid for in installments.

Keep the row: on the parallel path H is real work. Just do not go looking for it after a serial G.

**Optional parallel path:** After D (ports stable), run E, F, G in separate native worktrees (`claude -w …`), merge to main, then do **H on main**. I may run with F or soon after. J stays last. Parallel sessions do not auto-coordinate; you + `LOOP.md` are the bus. Workers usually omit the advisor; main keeps it.

---

## 4. When to call the advisor

**Do:** port traits, endpoint-per-node vs per-app types, allowlist/keys, actor message enum, “phase done” claims.

**Don’t:** renames, formatting, obvious compile fixes, filling stubs behind stable ports.

```text
Decision: <one sentence>
Constraints: modular monolith, ports/adapters, allowlist, agent-agnostic
Options: <A/B>
Evidence: <paths, cargo output>
Ask: ship / fix-first / rethink — ≤300 words
```

---

## 5. docs/LOOP.md

```markdown
# LOOP
## Current phase
## Done
## Next slice
## Advisor consultations
## Blockers
```

---

## 6. Claude Code wiring (default + optional parallel)

### 6.1 Default — one pane, serial slices

This is the intended baseline. One terminal or Herdr pane is enough for the entire plan.

```bash
# Pane: claude-main
cd /path/to/kamiroh          # git repo root (main checkout)
claude --advisor opus

# Paste master prompt (or rely on CLAUDE.md).
# Then each turn:
#   Continue the kamiroh loop: read docs/LOOP.md, execute the Next slice only,
#   verify with cargo, update LOOP.md, commit with conventional commits.
#   Consult the advisor only if this slice is architectural or security-related.

# Do NOT pass --worktree for the default path.
```

Optional second pane: a plain **shell** for `cargo check --workspace` and `git status`—not a second Claude.

### 6.2 Optional — parallel adapter slices (E, F, G)

Use only after slice D, when ports are stable, and only if you want speed over simplicity.

```bash
# Pane: claude-main (conductor — advisor on)
cd /path/to/kamiroh
claude --advisor opus
# Owns LOOP.md, merge order, architecture; does not control other Claudes.

# Pane: wt-keystore  (worker — advisor off)
cd /path/to/kamiroh && claude -w keystore

# Pane: wt-iroh
cd /path/to/kamiroh && claude -w iroh-adapter

# Pane: wt-kameo
cd /path/to/kamiroh && claude -w kameo-adapter

# After merges: H on main only
cd /path/to/kamiroh && claude --advisor opus
```

Two Claude sessions never share chat or advisor context automatically. Coordinate via `docs/LOOP.md`, short ADRs, and paste. One worktree per agent; never two Claudes on the same worktree directory. Prefer Claude’s default `.claude/worktrees/` (gitignored).

### 6.3 Recommended safeguards

| Safeguard | Practice |
|-----------|----------|
| Workspace trust | Accept trust once for the kamiroh git root before long runs. |
| Start from git root | Launch Claude from the main checkout, not an unrelated parent folder. |
| Advisor scope | Main / hard decisions only; workers implement behind agreed ports. |
| Permissions | Prefer ask on destructive git; allow routine cargo / read-only git as you build trust. See `.claude/settings.json`. |
| Sandbox | `sandbox.enabled: true` in project settings; optional `/sandbox` once to verify. Ensure `git commit` works on main and in a native worktree. |
| Secrets | Keep keys out of git; never commit endpoint secrets. Read-deny `.env` / `*.key` in settings. |
| Worktree hygiene | `.claude/worktrees/` in `.gitignore`; remove worktrees after merge. |
| Verify every slice | `cargo check` (then tests) before marking LOOP.md done. |
| Conventional commits | Small commits per slice; easy revert. |
| No parallel on main | Don’t run two implementer Claudes both editing the main checkout. |

### 6.4 Herdr vs product Herdr

Using Herdr as a **terminal multiplexer for Claude Code** is optional UX while building. That is separate from kamiroh’s future optional Herdr adapter for long-running product agents. Scaffold does not require product-Herdr panes.

---

## 7. Success criteria — foundation complete

- [ ] `cargo check --workspace`
- [ ] Ports crate has no adapter dependencies
- [ ] Domain has no Iroh/Kameo deps
- [ ] Bin wires KeyStore + Allowlist/stub + Transport + controller
- [ ] README aims intact; ARCHITECTURE.md matches code
- [ ] At least one advisor review on ports + security

---

## 8. What not to do in the first loops

- Full Iroh production hardening; real Herdr protocol; multi-node CI
- Premature kamiroh-cli surface; domain types coupled to Kameo/Iroh
- Parallel Claudes before ports (B) are stable

---

## Summary

One master prompt locks product + hexagonal modular monolith. Default = **one Claude Code session, optional Herdr pane, serial slices on main** with the advisor at gates. Optional parallel native worktrees only for E–G after D; **H stays on main** *if you take that path* — see the slice table. Safeguards: trust the repo, limit advisor scope, project sandbox/permissions, no dual editors on main, verify every slice.

*kamiroh — “Let’s be awesome!”*
