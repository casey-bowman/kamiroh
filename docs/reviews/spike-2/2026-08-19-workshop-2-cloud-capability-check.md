# Workshop-2 kickoff: cloud capability check

*Brief from the workshop-2 design session (cloud), 2026-08-19. Written in the
workshop-1 style: self-contained, readable without the codebase. Verifies
which of workshop-1's environmental constraints still hold for this spike's
cloud session, before any WORKFLOW/VENDORING doctrine is updated.*

## What was verified, and how

All checks ran today in the workshop-2 cloud session's own Linux workspace
against a fresh anonymous clone of `kamiroh-workshop-2/kamiroh` at
`master` = `b960303`.

**Toolchain.** `rustc 1.95.0` / `cargo 1.95.0` / `clippy 0.1.95`, with
`rustup 1.29.0` present but unable to download anything
(`static.rust-lang.org` → 403, host not in the egress allowlist). The
sandbox is therefore still pinned at 1.95.0 — the same language-feature
floor as workshop-1's cloud session, and WORKFLOW.md's toolchain note
carries over unchanged.

**crates.io: still unreachable.** `crates.io`, `index.crates.io`, and
`static.crates.io` all return 403 ("Host not in allowlist"). A live
`cargo fetch --locked` failed exactly as expected on
`index.crates.io/config.json`. The vendoring scheme (decision 20,
`docs/VENDORING.md`) remains necessary for cloud builds, not vestigial.
The 403 body says the host list is a configurable egress setting — if
Casey can and wants to allowlist the three crates.io hosts for this
session's environment, the picture changes; until then, vendored it is.

**GitHub: reachable, anonymous, read-only.** Cloning and fetching over
HTTPS work (no credentials configured, so pushing does not). This is new
relative to workshop-1's cloud session, which could not reach GitHub at
all — the "verify by anonymous fetch" loop that previously needed a relay
now runs directly in the cloud.

**The full suite is green in the cloud.** `vendor-snapshot` was fetched
from the *workshop-1* fork (it exists there at `c827050`; workshop-2's
fork does not have one yet), laid down per VENDORING.md, and the standard
gate run offline:

```
cargo build --workspace --offline   # clean
cargo test  --workspace --offline   # 39 passed, 0 failed
```

The 39 include the real-loopback-QUIC `iroh_conversation` suite — the
whole hermetic gate, matching the counts recorded on master. Wall-clock
for the cold build+test was ~4 minutes on 2 cores. Disk is ample
(vendor 575 MB + target fit comfortably).

**The device bridge (new capability class).** Unlike workshop-1's Mez,
this session has a live bridge to Casey's machine: it can read the local
clone at `tiers/2-workshop/kamiroh`, write files into that folder, and run
shell commands there — including local git (branch, add, commit). That
device-side shell has **no network** (no push, no fetch, no crates.io) and
cannot delete files. Pushing to GitHub remains exclusively Casey's (or a
local Code session's) act, from anywhere.

## What this changes, and what it doesn't

| Workshop-1 assumption | Workshop-2 reality |
|---|---|
| Cloud cannot reach crates.io → vendor for hermetic builds | **Unchanged** — vendoring still required |
| Cloud pinned at rustc 1.95 (feature floor) | **Unchanged** — same version, rustup blocked |
| Cloud cannot reach GitHub → bundle relay outbound, human courier | **Changed** — cloud fetches/clones GitHub directly (read-only) |
| Cloud cannot run the first real build | **Softened** — cloud runs the full offline gate itself, given a current vendor-snapshot |
| Cloud cannot touch the working clone | **Changed** — device bridge allows direct file edits and local git in Casey's clone (no network there) |
| Blind-writing with assumption lists | **Mostly retired** — the cloud can now compile adapter code against vendored deps before proposing it |

Net: the *courier inbound* (bundles from cloud to human) can be replaced by
direct commits on `cowork/*` branches in the local clone, if Casey adopts
that; the *push outbound* and every merge to `master` remain human acts.
Review gates, branch namespaces, and the decision-log discipline carry over
untouched.

## Loose ends surfaced

1. **workshop-2 has no `vendor-snapshot` branch.** The cloud session
   borrowed workshop-1's, which matches the current `Cargo.lock` — fine
   until the first dependency change. First dep bump should refresh and
   push a snapshot to the workshop-2 fork (a push, so an Ander/Casey
   errand per VENDORING.md).
2. **WORKFLOW.md self-describes as workshop-1** and its constraint
   rationale is now partially stale; ARCHITECTURE.md is titled "Spike 1".
   Both want a deliberate workshop-2 update once the working mode is
   decided — doctrine should follow the decision, not this brief.
3. **Egress allowlist question for Casey**: whether to request
   crates.io/index.crates.io/static.crates.io (and optionally
   static.rust-lang.org) in the session's network egress settings. Either
   answer is workable; it decides whether vendoring stays load-bearing for
   the cloud or becomes belt-and-suspenders.

## Division of labor implication (for discussion)

With the cloud able to build, test, lint (clippy 1.95), and verify against
GitHub, Ander's distinctive contributions shift toward what only a local,
networked, newer-toolchain session can do: independent review with fresh
eyes and a floating rustc (currently 1.97) whose newer clippy catches what
1.95 misses; everything requiring push (branch publication, vendor-snapshot
refreshes, staging graduation); dependency bumps against live crates.io;
real-network field checks (N0 profile, NAT traversal, Incus containers —
the cloud sandbox has neither multiple machines nor open egress); and
first-build confirmation on an unpinned toolchain, which keeps the
two-toolchain signal WORKFLOW.md values. The STOP-and-brief discipline and
scope instructions carry over as-is.
