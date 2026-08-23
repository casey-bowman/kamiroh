# Brief: the "without a central server" claim vs. the N0 profile's n0 dependency

**For:** the agent working on kamiroh (docs/framing decision).
**Prepared by:** the Claude Cowork session building the BayWorks company site.
**Date:** 2026-08-21
**Concerns:** kamiroh's public framing (README tagline and any derived copy) — no code.

---

## TL;DR

kamiroh's headline framing — peer actors that message each other across the
internet **"without requiring a central server"** — is defensible for what it
most plausibly means (no central *application* server; actors live on the
peers; hermetic profile involves no third party at all), but **overstated for
the N0 profile**, where dialing by endpoint ID alone works because n0's
centrally-operated infrastructure mediates the introduction: their discovery
service resolves endpoint IDs to reachable addresses, and their relay servers
assist the hole punch and can carry (end-to-end-encrypted) traffic as a
fallback when no direct path forms. A reader who knows Iroh could fairly call
the unqualified claim a stretch. Casey asked that this be conveyed so the
project's own framing can be tightened at the source.

## How this surfaced

The BayWorks site (the company site featuring kamiroh as its first project)
borrowed the README's phrasing for its project copy. Reviewing that copy,
Casey asked: *"Since Iroh uses their n0 server to mediate endpoints
initially, does this contradict the claim here?"* The site is fixing its own
copy independently; this brief exists so kamiroh's canonical docs can decide
their own wording rather than inheriting the question later — a company site
gets skimmed, but a protocol README gets held to its claims.

## The analysis

What the claim gets right:

- **No central application server.** Nothing hosts the actors, holds the
  conversations, or must be operated by kamiroh (or anyone) for the protocol
  to function. Peer traffic is end-to-end encrypted QUIC between the actors'
  endpoints.
- **The hermetic profile is literally serverless.** Explicit peer addresses,
  no discovery, no relays, no third party of any kind.

Where it overstates:

- **The N0 profile's convenience is centrally mediated.** Endpoint-ID-only
  dialing depends on n0's public discovery service (ID → addresses) and
  relay fleet (hole-punch assistance; encrypted-traffic fallback). That
  infrastructure never sees plaintext and is self-hostable/replaceable — but
  it is centrally operated, sits in the default connection path, and its
  availability affects whether an introduction succeeds. "Without a central
  server" elides exactly the dependency a skeptical reader will probe.

## Suggested reframing (for whatever docs carry the claim)

Move the strong claim from "no central server" to "no server of its own,"
and let the profiles carry the nuance explicitly. One shape that survives a
skeptical reader:

- Tagline-level: "actors live on the peers themselves, not on a server" or
  "peers talk directly — no application server in the middle."
- Profile-level: "hermetic (explicit peer addresses; no third-party
  infrastructure at all) and N0 (dial by endpoint ID alone, using n0's
  public discovery and relay infrastructure for the introduction and NAT
  traversal — self-hostable if you'd rather run your own)."

The disclosure reads as a feature, not a confession: it names the trade
(convenience vs. third-party rendezvous) and points at both escape hatches
(hermetic profile; self-hosted relays/discovery).

## What is asked of you

Decide whether and how kamiroh's own README/docs should adjust their
framing. Nothing here implies a code change, and the exact wording is yours —
the suggestions above are offered, not prescribed. If the framing is already
qualified somewhere the BayWorks session didn't see (it read the public
README and repo overview only), a pointer back is a complete answer.

---

## Resolution (2026-08-23, on graduation into docs/briefs/)

Casey ruled **option A**: the claim now reads "no **application** server in
between" in both places it appears unqualified — the README status banner
and the Aims opener. The N0 profile paragraph already disclosed the
introduction infrastructure honestly and is unchanged. Rationale: the
qualified claim is precisely true (nothing hosts the actors or the
conversations), and the profile-level text carries the nuance where the
dependency is actually named. Edit made on `cowork/bayworks-framing`,
docs-only, gate run as always.
