# kamiroh Phase 3 — after it works

Phase 2 is done: M1–M4 and the review queue. kamiroh drives a real coding agent,
is reachable by endpoint id, hosts several agents, and says what it is doing
without saying what its agents said.

So this pass is not "what is left on the list" — the list is empty. It is "what
does kamiroh still not let you *do*", answered by reading the code rather than
the notes.

---

## 1. Where kamiroh is

Nine crates, 188 tests, warning-free. Every driven port resolves to an adapter
that touches the world; the only in-memory implementations left are the ones a
node deliberately asks for (`echo` as an agent target, `KAMIROH_ALLOW` as an
allowlist override).

Against the README's five aims:

| aim | state |
|---|---|
| Actor API | ✅ four verbs, one controller actor per agent |
| Reachability | ✅ allowlisted, by endpoint id — NAT traversal unproven |
| Agent-agnostic | ✅ opaque payloads, never parsed, never logged |
| Peers | ✅ no central gateway; relays are opt-in and documented |
| **Lifecycle** | ⚠ **the weakest one.** See below. |

---

## 2. The finding: the four verbs are the constraint now

`ControlMessage` is `Prompt`, `Status`, `Interrupt`, `Shutdown`. That was ample
when an agent echoed. With a real coding agent behind it, two things a person
obviously wants are unsayable.

### 2.1 You cannot watch a long task

Output reaches a caller in exactly one way: as the reply to a `Prompt`, either
`Output` or `Partial`. `Status` carries state and no output.

So when a prompt returns `Partial{Busy}` after 20 seconds — which is the normal
case for a coding agent — there is **no non-destructive way to see the rest**.
Asking again means sending another `Prompt`, which types text at an agent that
is still working. The workaround is worse than the gap.

M1 recorded this as "a caller can prompt again and read more, which is a
workaround rather than a design". Having watched a real agent, it is worse than
that: prompting again is not reading, it is interrupting.

### 2.2 `Shutdown` does not stop the agent

`ControlMessage::Shutdown` is documented as "Ask the agent to stop". What it
does is stop the *controller actor*: `abandon`, `status = Stopped`,
`stop_gracefully`. The `Agent` port has exactly two methods, `run` and `status`
— there is no way for a stop to reach the agent, so a coding agent carries on
working after kamiroh has replied `Accepted` and forgotten it.

Not a bug in the actor, which does what it says. A gap between the domain's
vocabulary and what a peer reasonably expects that word to mean. It is also why
the README's "lifecycle" aim is the least delivered: kamiroh attaches to agents
(deliberately — Herdr starts them) but cannot stop one either.

**Both gaps are the same shape.** The vocabulary was designed before there was a
real agent, and it is now the narrowest part of the system.

---

## 3. Milestones

### P1 — Say what a person needs to say

Grow `ControlMessage` to cover reading and stopping, and make the port able to
carry them.

**Done when** a caller can follow a long task without disturbing it, and can
actually stop a remote agent.

**Decisions to settle first:**

- **One verb or two for reading?** `Read` (give me output since last time) is
  the minimum. Whether it takes a cursor, or always returns "the last N", is the
  design question — a terminal has no natural cursor, and M1's output heuristic
  is already inexact.
- **What should `Shutdown` mean?** Either make it reach the agent (a `stop` on
  the `Agent` port, which `HerdrAgent` maps to something Herdr offers), or
  rename it to say what it does and add a separate verb for stopping the agent.
  Renaming is honest and cheap; reaching the agent is what people will expect.
  **Do not leave it as it is** — a verb that reports success for something it did
  not do is the worst of the three options.
- **Does reading need streaming?** Probably not first. A `Read` verb answers
  "what has it said" without a subscription, and subscriptions are a much larger
  protocol change. Prove the need before paying for it.

**Cost is known.** M1 measured it: a `ControlMessage`/`ControlReply` change
touches five compiler-caught sites, and the wire degrades to a clean
`CodecError::Discriminant` on an older peer rather than misparsing.

### P2 — Prove it where it actually matters

Two things have never been done on real hardware.

- **NAT traversal.** M2 proved identity-only addressing with both nodes on one
  machine; a relay may never have been used. This needs a second machine on a
  different network, and the procedure is written:
  [reachability-test.md](./reachability-test.md). It is the only remaining claim
  in the README that has not been demonstrated.
- **Use it.** Everything to date has been verified by scripts written alongside
  the code. Nobody has driven a real agent through kamiroh for a day's work.
  That is the cheapest remaining source of true information about what is wrong,
  and P1's design questions above are exactly the sort that real use answers
  better than reasoning does.

**This is a legitimate plan item, not a gap in the plan.** The next most
valuable thing kamiroh can receive is not a feature.

### P3 — Trust

*The first two items below are standing decisions rather than tasks, and live in
[OPEN-DECISIONS.md](./OPEN-DECISIONS.md) — the list that outlives this plan. The
third is a candidate on that list, not yet proposed.*

- **A security review by someone who is not its author.** The posture has never
  been read by anyone else: the enumeration argument, the local-trust boundary,
  and §5a's disclosure trade in particular. Demonstrating that the disclosure
  happens is not the same as agreeing it is acceptable.
- **Malformed-allowlist-is-fatal**, still undecided since I. What would settle
  it is a way to check a file before restarting — which is a small CLI surface
  kamiroh does not have, and the first good reason to grow one.
- **Per-kind detection quality.** Measured in M3: a startup prompt is `blocked`
  for `claude` and `idle` for `codex`. kamiroh cannot fix Herdr's manifests, but
  it could stop *implying* more confidence than it has — a status that says
  where it came from would be honest.

### P4 — Let someone else run it

Only worth doing after P2. The README currently promises lifecycle management
kamiroh does not perform; publishing before P1 would ship that mismatch to
strangers.

**Before publishing — a checklist, because each item is invisible until it is
too late:**

1. **The logo will not render on crates.io.** `README.md` uses a relative
   `<img src="kamiroh.jpg">`. GitHub resolves that; crates.io does not, because
   it renders the README outside the repository. It needs an absolute URL, and
   plain markdown rather than the `<p align="center">` wrapper, since crates.io
   sanitises HTML:

   ```markdown
   ![kamiroh](https://raw.githubusercontent.com/casey-bowman/kamiroh/<tag>/kamiroh.jpg)
   ```

   **Pin `<ref>` to a tag, not `master`.** A published version's README is
   immutable, but an image URL pointing at a branch is not — the logo of
   version 0.1.0 would change the next time `master` did.

2. **The owner is `casey-bowman`, and the apparent discrepancy is not a bug.**
   `Cargo.toml`'s `repository` and `git remote origin` name different repos:

   | | | |
   |---|---|---|
   | `Cargo.toml` `repository` | `casey-bowman/kamiroh` | the canonical home — **use this** |
   | `git remote origin` | `kamiroh-workshop/kamiroh` | the fork work is pushed from, for PRs |

   Both are correct; they mean different things. Written down because it reads
   as a mistake, and the wrong guess would put a crates.io repository link on a
   fork — not fixable without publishing a new version.

   **Sequencing:** a raw URL only resolves once the image exists *upstream* at
   the pinned tag. Publishing from a fork whose logo commit has not been merged
   would ship a README with a broken image.

3. **The README's lifecycle claim.** It promises starting and managing agents;
   kamiroh attaches and cannot stop one (§2.2). Fix in P1, or reword before
   strangers rely on it.

---

## 4. What this deliberately does not do

- **No streaming protocol** until P1's `Read` proves insufficient.
- **No agent starting.** Herdr starts agents; kamiroh attaches. Reversing that
  makes kamiroh a worse Herdr, and M3 confirmed attach-only was right.
- **No parsing agent output.** Per-kind detection is Herdr's job; a parser per
  kind is exactly what agent-agnostic forbids.
- **No new transport.** Iroh works.

---

## 5. The decision this plan needs

**Is the next step P1 or P2?**

They are genuinely close, and the argument runs both ways. P1 fixes two known,
specific gaps and its cost is measured. P2 costs no code and would tell us
whether P1's design questions have the answers we assume — in particular whether
"read the last N lines" is enough, which is a guess right now.

**Recommended: P2 first, specifically the "use it" half.** Not because the gaps
are in doubt — they are structural and confirmed — but because their *shape* is
not. Twice now, live use has overturned a design that unit tests agreed with:
`done`-versus-`idle` in M1, and the `Arc<dyn Agent>` forwarding default. A third
guess about what a `Read` verb should carry is worth less than a day of actually
wanting one.

If you would rather build than use, take P1 and start with `Shutdown` — it is
the one where the current behaviour is actively misleading rather than merely
absent.
