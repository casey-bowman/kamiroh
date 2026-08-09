//! kamiroh port traits — the boundary between application logic and adapters.
//!
//! Ports come in two directions:
//!
//! - **Driving** (the outside calls in): [`ControlApi`]. Implemented by
//!   `kamiroh-app`, called by transport and UX adapters.
//! - **Driven** (the inside calls out): [`Transport`], [`Allowlist`],
//!   [`KeyStore`], [`AgentController`]. Declared here, implemented by adapters,
//!   wired by the composition root.
//!
//! Every trait is dyn-compatible so the composition root can hold
//! `Arc<dyn Port>` and swap implementations without touching the app layer.
//! Async ports use `#[async_trait]` for that reason.
//!
//! Each port owns a `thiserror` error enum rather than a catch-all error type,
//! so the application can react to specific failures and tests can produce them.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod allowlist;
pub mod control_api;
pub mod controller;
pub mod key_store;
pub mod transport;

pub use allowlist::Allowlist;
pub use control_api::{ControlApi, ControlApiError, Origin};
pub use controller::{AgentController, ControllerError};
pub use key_store::{KeyStore, KeyStoreError};
pub use transport::{Transport, TransportError};
