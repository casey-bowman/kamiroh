//! kamiroh application layer.
//!
//! Use cases expressed purely against `kamiroh-ports` traits and `kamiroh-domain`
//! types. No Iroh, no Kameo, no Herdr, no runtime — this crate must stay
//! testable with hand-written fake ports, and its tests are where security
//! invariants such as deny-by-default are pinned.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod control_service;

pub use control_service::ControlService;
