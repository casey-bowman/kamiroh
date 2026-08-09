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
| 3 | Has anyone but the author read the security posture? | F2 onward | **read once — one judgment left for you** |

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

**Partly answered — the gate was met once, and it did not close the item.** The
advisor was on for the 2026-08-09 session and reviewed the posture against the
code rather than the prose. What it found is below. What remains is a judgment
about disclosure that no reviewer of any kind can make for you.

**Why code cannot settle it.** Several arguments have been converted into
enforced properties, which is worth a lot but is not the same thing. A mutation
test proves an argument is *guarded*; it cannot tell you the argument was right.

**What was read, and what held.** Checked in code, not in the documents that
describe it:

| claim | verdict |
|---|---|
| Authorise before dispatch (§5.1) | **holds** — early return, nothing between the guard and `dispatch` |
| `Origin` opacity (§5.5) | **holds** — a public tuple struct over a private enum; `Provenance::Local` is unreachable outside the crate |
| The front uses the *authenticated* peer (§3) | **holds** — `connection.remote_id()` straight to `Origin::remote`; nothing from the request frame touches the trust decision |
| The `local_front` audit rule (§3) | **holds** — `LocalLink`, the composition root, one test |

**What changed as a result:** §3 now states the local-trust grant as a property
of stdin rather than of the machine, §5a now answers whether the disclosure can
be reversed, and `Origin::is_local()` — a callerless predicate on the trust type
— was removed. Two stale paragraphs in §6 and §7 were corrected.

**What is still yours to decide, and it is one thing:**

**§5a — is the `Anywhere` disclosure acceptable?** Everything factual about it
is now established. A node publishes a signed record of its addresses under its
endpoint id; anyone holding that id can resolve it whether or not the allowlist
would admit them. The reversal question is answered too: publishing is a refresh
(30s TTL, 5-minute republish in `iroh 1.0.3`), so it stops when the node stops
and **does not require a new identity** — which would otherwise have invalidated
every peer's allowlist entry. The residual is n0's relay retention, which is
their policy and undocumented.

So the remaining question is not "what happens" but "is opt-in enough". That is
a judgment about your threat model — who might hold an endpoint id, and what it
costs them to learn where the node lives.

The other two items are downgraded rather than closed:

- **The enumeration argument (F2)** — the *mechanism* is confirmed structural:
  authorisation returns early, with nothing between the guard and `dispatch`,
  and it is mutation-tested besides. The *trade* is a weaker claim — that
  distinct `REFUSED` and `NO_SUCH_ACTOR` codes reaching an already-trusted peer
  is right, and that `PROTOCOL` for a malformed frame discloses nothing — was
  read and not disputed, which is not the same as independently argued.
- **The local-trust boundary (J1)** — the code was not wrong; the *description*
  was. It now says what the code grants. Whether granting the allowlist bypass
  to whoever holds this process's stdin is right for a service-managed node is a
  deployment question, and the honest wording is what makes it askable.

**What would still add something:** a reviewer with no connection to this
session reading ARCHITECTURE.md §5, §5a and §3. A same-session advisor is a
second reader, not an independent one.

**Detail:** LOOP.md advisor consultations (2026-08-09); ARCHITECTURE.md §5, §5a,
§3.

---

## Candidates, not yet on the list

Raised but not proposed as decisions; here so they are not lost.

- **Should kamiroh say how confident a status is?** M3 measured that a startup
  prompt is `blocked` for `claude` and `idle` for `codex`. kamiroh reports what
  Herdr detects and should not second-guess it — but it could stop *implying*
  more confidence than it has. Becomes a real decision if anyone relies on a
  sidebar.
