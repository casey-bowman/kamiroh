//! In-process transport adapter.
//!
//! Will implement [`kamiroh_ports::Transport`] and [`kamiroh_ports::Inbox`]
//! over in-memory queues, so the application layer can be exercised in tests
//! with no network involved.
