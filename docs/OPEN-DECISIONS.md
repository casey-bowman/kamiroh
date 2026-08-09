# Open decisions

Decisions that are genuinely a person's to make, deliberately **not** settled in
code. They are here because they were each raised inside a slice, recorded in
prose, and would otherwise stop being decisions simply by never being read
again.

**How this list is used.** Reviewed at every planning pass, and named in
`LOOP.md` so a session that starts by reading the loop sees it. An item leaves
this list only by being *decided* — recorded with its reasoning — not by
becoming stale.

**What does not belong here.** Anything a test could settle, and anything
already decided. Two things that look like open questions are not: the
`ScopedTempFile` disarm and the `ControllerError::Rejected` fit for an
interrupted prompt are both *settled as they are*, with reasons, in LOOP.md's
known nits.

| # | decision | raised | who can settle it |
|---|---|---|---|
| 1 | Should a malformed allowlist stop the node? | slice I | you |
| 2 | Does NAT traversal actually work? | M2 | a second machine |
| 3 | Has anyone but the author read the security posture? | F2 onward | a reviewer |

---

## 1. Should a malformed allowlist stop the node?

**Undecided:** `FileAllowlist` refuses to start a node when the allowlist file
exists but cannot be parsed. The alternative is to start, admit nobody, and
complain loudly.

**Why code cannot settle it.** It is a trade, not a fact. Fatal means one bad
edit stops every node that restarts. Non-fatal means a node that looks healthy
while admitting nobody — which reads as a network fault and is diagnosed hours
later.

**What narrows it, already true:**

- The blast radius is startup only. `reload()` keeps the previous set when the
  new file is bad, so a running fleet survives a fumbled edit until it restarts.
- The error names the file and the line.

**What would settle it properly:** a way to check a file *before* restarting.
kamiroh has no CLI surface, and this would be the first good reason to grow one
— which is itself a decision about what kamiroh is.

**Detail:** LOOP.md, slice I and the review-queue entry. ARCHITECTURE.md §6b.

---

## 2. Does NAT traversal actually work?

**Undecided:** unproven, not disputed. M2 demonstrated identity-only addressing
— a peer dialled by endpoint id with no address written down anywhere — but both
nodes were on one machine, so a relay may never have been used.

**Why code cannot settle it.** It needs a second machine on a different network.
No amount of local testing substitutes; that is the whole point of the claim.

**What would settle it:** [reachability-test.md](./reachability-test.md) — a
fifteen-minute procedure with the exact commands, what counts as proof, and the
one way to get a false pass (running both machines on the same network, where
they find each other over the LAN and it looks like success).

**Cost of leaving it open:** the README's headline case, "home ↔ cafe, behind
NAT", is the one claim in it that has never been demonstrated. Worth settling
before anyone else is invited to rely on it.

**Detail:** kamiroh-phase-2.md M2; kamiroh-phase-3.md P2.

---

## 3. Has anyone but the author read the security posture?

**Undecided:** no. Every security argument in this repo was written and checked
by the same author, and the plan's own advisor gate was never met — recorded
honestly at F2, G, I and J rather than quietly skipped.

**Why code cannot settle it.** Several arguments have been converted into
enforced properties, which is worth a lot but is not the same thing. A mutation
test proves an argument is *guarded*; it cannot tell you the argument was right.

**In priority order, what wants reading:**

1. **§5a — what `Reach::Anywhere` discloses.** That a node's addresses become
   publicly resolvable from its endpoint id is demonstrated. Whether that trade
   is acceptable, and whether opt-in is sufficient mitigation, is a judgment
   nobody else has made.
2. **The enumeration argument (F2).** Holds by ordering — authorise before
   lookup — and is mutation-tested, so a reorder fails three tests. The
   *reasoning* still deserves a second reader.
3. **The local-trust boundary (J1).** `Origin::local_front()` is claimed by
   `LocalLink` on the grounds that a pane is a process owned by whoever owns the
   node. That is an assumption about the machine, not a property of the code.

**What would settle it:** `/advisor opus` on a session, or any competent
reviewer reading ARCHITECTURE.md §5, §5a and §3.

**Detail:** LOOP.md advisor consultations; ARCHITECTURE.md §5, §5a, §3.

---

## Candidates, not yet on the list

Raised but not proposed as decisions; here so they are not lost.

- **Should kamiroh say how confident a status is?** M3 measured that a startup
  prompt is `blocked` for `claude` and `idle` for `codex`. kamiroh reports what
  Herdr detects and should not second-guess it — but it could stop *implying*
  more confidence than it has. Becomes a real decision if anyone relies on a
  sidebar.
