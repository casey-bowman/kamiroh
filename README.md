# kamiroh

Kameo actors for agents over Iroh

Peer actors, addressable by name and endpoint, that message each other—-locally or across the network—-to drive agents.

*kamiroh — “Let’s be awesome!”*

> **Status: early / WIP** — API and behavior may change.

## Aims

kamiroh is for putting two parties in conversation across the internet without a server
in between. Each side of a conversation is an **actor** — a named party living at an
[Iroh](https://www.iroh.computer/) endpoint. One endpoint can host many actors, each
with its own unique name, and any actor can open a conversation with another by naming
its endpoint and actor name.

Where an AI agent takes part, one actor is dedicated to that agent as its
communications proxy: everything the agent sends or receives flows through its actor.
But agents are optional — either end of a conversation can just as well be an
application embedding kamiroh as a library. Conversations may be one quick
request-and-acknowledgment or a long-lived back-and-forth, following small, defined
**protocols** built from a constrained, agent-agnostic vocabulary.

Security is allowlist-based and deny-by-default: an actor receives messages only from
endpoints it has explicitly admitted, and an empty allowlist means silence.

Under the hood, kamiroh is a modular monolith in the ports-and-adapters style — a Rust
workspace whose core knows nothing about the network or the actor runtime, with
[Kameo](https://crates.io/crates/kameo) animating the actors and Iroh carrying the
conversations. See [ARCHITECTURE.md](ARCHITECTURE.md) for the full picture.

## Status

This is an early design/implementation. Expect breaking changes. Not all of the aims above exist yet.

## License

MIT
