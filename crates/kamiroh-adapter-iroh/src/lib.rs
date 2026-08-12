//! Iroh transport adapter.
//!
//! Will implement [`kamiroh_ports::Transport`] on Iroh connections: endpoint
//! setup from a [`kamiroh_domain::secret::Secret`], connection lifetimes
//! (short- or long-lived conversations), and the wire codec for
//! [`kamiroh_domain::vocabulary::Message`].
//!
//! The `iroh` dependency is added when the implementation lands, so the
//! scaffold stays light to build.
