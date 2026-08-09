# Claude Code setup (kamiroh)

Optional but recommended when developing kamiroh with [Claude Code](https://code.claude.com/).


## Files

| Path | Commit? | Purpose |
|------|---------|---------|
| `.claude/settings.json` | Yes | Shared permissions + bash sandbox baseline |
| `.claude/settings.local.json` | **No** | Machine-specific paths and personal allows |
| `.claude/settings.local.json.example` | Yes | Template for the local file |

Ensure `.gitignore` contains:

```gitignore
.claude/settings.local.json
.claude/worktrees/
```

## First-time setup

From the **git repository root** (main checkout):

```bash
cd /path/to/kamiroh

# Shared settings are already in the repo at .claude/settings.json
# Create a local file only if you need absolute paths or personal allows:
cp .claude/settings.local.json.example .claude/settings.local.json
```

Edit `.claude/settings.local.json` and replace the placeholder paths:

```bash
git rev-parse --show-toplevel
git rev-parse --git-common-dir
```

Use those absolute paths under `sandbox.filesystem.allowWrite` if sandboxed `git commit` fails with a read-only `.git` error. Native worktrees under `.claude/worktrees/` normally share that git directory.

## Launch

```bash
cd /path/to/kamiroh
claude --advisor opus
```

Fable is **not** currently selectable as an advisor; Opus is. `/advisor opus`
switches an already-running session. See the build plan's preconditions table.

**The advisor gate was met once**, for the security posture, and it found three
places where a document claimed a check the code did not perform. What it did
not do is close [open decision 3](./OPEN-DECISIONS.md): a same-session advisor
is a second reader, not an independent one. Leave it on.

Accept **workspace trust** for this repository only. Prefer starting Claude at the git root, not an unrelated parent directory.

Default workflow: **one session on the main checkout**, one slice at a time. Do not pass `--worktree` unless you intentionally want an isolated session. Claude can create native worktrees under `.claude/worktrees/` when needed.

## What the shared settings do

### Permissions

- **Allow:** `cargo check` / `test` / `build`, read-only git, `git worktree list`
- **Ask:** `git add` / `commit`, worktree add/remove, merge/rebase, `git push`, `cargo publish`
- **Deny:** force push, hard reset, force worktree remove; **Read** of `.env` and `*.key` style secrets

### Sandbox

- Bash runs in the OS sandbox (Seatbelt on macOS)
- **Writes** limited to the repo, `.git`, and `.claude/worktrees`
- **No network domain allowlist** — open enough for ad hoc docs/research; rely on write limits and secret Read denials instead
- `autoAllowBashIfSandboxed`: skip repeated Bash prompts for commands already inside the jail (still subject to allow/ask/deny)
- `allowUnsandboxedCommands`: Claude may *ask* to retry outside the sandbox if something breaks; approve those sparingly

## Verify

In a Claude Code session on the main checkout:

1. `cargo check` (or `cargo check --workspace` once the workspace exists)
2. A trivial `git commit` (approve when asked)
3. Optional: `claude -w smoke-test` — `cargo check` and a commit **inside** the worktree

If git cannot write under sandbox, fix `allowWrite` paths in `.claude/settings.local.json` rather than permanently excluding `git` from the sandbox.

## Parallel worktrees (optional)

After ports are stable, you may run workers as separate sessions:

```bash
claude -w iroh-adapter          # worker; advisor usually off
claude --advisor opus           # main conductor
```

Sessions do not share chat or advisor context. Coordinate via `docs/LOOP.md` and merges. Prefer the advisor on the main/architecture session only.

## Product note

Configuring Claude Code here is **development tooling**. It is separate from kamiroh’s optional future Herdr adapter for long-running product agents.
