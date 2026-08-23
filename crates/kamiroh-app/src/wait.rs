//! Waiting with a deadline — the one place a delivery races a timer.
//!
//! Every pump that waits on an inbox while an exchange is under way does it
//! through [`next_or_deadline`], so the choreography's racing step is written
//! once (`ARCHITECTURE.md`, decision 24). Deterministic by construction: the
//! deadline is polled first, so an already-elapsed deadline wins over an
//! already-queued delivery — a test that advances its timer past a deadline
//! observes the failure, regardless of what else is in the mailbox.

use std::future::Future;
use std::pin::pin;
use std::task::Poll;

use kamiroh_domain::endpoint::EndpointId;
use kamiroh_ports::{Delivery, Inbox};

/// What the wait produced.
#[derive(Debug)]
pub enum Waited {
    /// A delivery arrived in time (`None`: the inbox closed).
    Delivery(Option<Delivery>),
    /// The deadline elapsed first.
    DeadlineElapsed,
    /// The transport reported a peer endpoint dead first (decision 27).
    PeerDeath(EndpointId),
}

/// Wait for the next delivery, but no longer than `deadline` (a
/// [`Timer::sleep_until`](kamiroh_ports::Timer::sleep_until) future).
pub async fn next_or_deadline<I: Inbox>(
    inbox: &mut I,
    deadline: impl Future<Output = ()> + Send,
) -> Waited {
    let mut deadline = pin!(deadline);
    let mut next = pin!(inbox.next());
    std::future::poll_fn(move |cx| {
        // Deadline first: an elapsed deadline outranks a queued delivery.
        if deadline.as_mut().poll(cx).is_ready() {
            return Poll::Ready(Waited::DeadlineElapsed);
        }
        match next.as_mut().poll(cx) {
            Poll::Ready(delivery) => Poll::Ready(Waited::Delivery(delivery)),
            Poll::Pending => Poll::Pending,
        }
    })
    .await
}

/// [`next_or_deadline`], with the transport's death evidence in the race
/// too (decision 27). Poll order — deaths, deadline, delivery — makes
/// positive evidence outrank an elapsed deadline, and both outrank a
/// queued delivery, deterministically.
pub async fn next_or_deadline_or_death<I: Inbox>(
    inbox: &mut I,
    deadline: Option<impl Future<Output = ()> + Send>,
    death: impl Future<Output = Option<EndpointId>> + Send,
) -> Waited {
    let mut death = pin!(death);
    // An absent deadline waits forever — the other two still race.
    let mut deadline = pin!(async move {
        match deadline {
            Some(f) => f.await,
            None => std::future::pending::<()>().await,
        }
    });
    let mut next = pin!(inbox.next());
    // A watch that closes (`None`) drops out of the race for good: polling a
    // future after it has completed panics, and this closure is re-entered
    // on every wakeup.
    let mut watching = true;
    std::future::poll_fn(move |cx| {
        if watching {
            match death.as_mut().poll(cx) {
                Poll::Ready(Some(endpoint)) => return Poll::Ready(Waited::PeerDeath(endpoint)),
                Poll::Ready(None) => watching = false,
                Poll::Pending => {}
            }
        }
        if deadline.as_mut().poll(cx).is_ready() {
            return Poll::Ready(Waited::DeadlineElapsed);
        }
        match next.as_mut().poll(cx) {
            Poll::Ready(delivery) => Poll::Ready(Waited::Delivery(delivery)),
            Poll::Pending => Poll::Pending,
        }
    })
    .await
}
