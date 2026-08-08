# kamiroh

Kameo actors for agents over Iroh

Peer actors, addressable by name and endpoint, that message each other—-locally or across the network—-to drive agents.

*kamiroh — “Let’s be awesome!”*

> **Status: early / WIP** — API and behavior may change.

## What it aims to be

kamiroh is meant to be a peer-oriented control layer for long-running agents. The aim is to combine:

1. **Lifecycle** — start and manage long-running agents on a home-office node via Herdr locally, or from a client
2. **Actor API** — Kameo-style actors as the control surface (prompt, status, interrupt, and similar messages)
3. **Reachability** — Iroh connections accepted only from **allowlisted** endpoints (for example home ↔ cafe, behind NAT)
4. **Agent-agnostic control** — no assumption about what each agent does; coding, testing, delegated app control, or anything else stays outside kamiroh
5. **Peers** — agent control is peer-to-peer (no central control gateway)'. Iroh relays may still assist with NAT traversal; they do not command agents.


**Control model:** each agent is driven by one long-lived controller actor. Multiple fronts can talk to that same actor:

- **Iroh** — remote peers (allowlisted `EndpointId` + actor name)
- **Herdr** (optional) — local panes for start/attach/monitor, and pane input that the process bridges into the same actor

Herdr is optional local UX and lifecycle aid; it is not required.

Adjacent tools cover pieces of this (P2P transport, agent chat, remote shells, messaging gateways). kamiroh aims at this specific combination.

## Addressing model

- **Local:** actor name
- **Remote:** `EndpointId` + actor name
- **Trust:** endpoint allowlist on accept; key custody for each node’s Iroh secret
- **Same actor, multiple fronts:** Herdr input and Iroh messages can both target the controller for a given agent

## Non-goals 

- Replacing Herdr, Claude Code, OpenCode, or other harnesses
- Defining what agents do inside their own apps
- A hosted multi-tenant cloud control plane
- Automatic trust of arbitrary public endpoints

## Status

This is an early design/implementation. Expect breaking changes. Not all of the aims above exist yet.

## License

MIT