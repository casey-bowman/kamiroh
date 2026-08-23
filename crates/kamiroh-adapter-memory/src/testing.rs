//! Test affordances: a minimal executor, and a hand-cranked timer.
//!
//! The workspace core is executor-agnostic; tests need *something* to poll
//! with. This thread-parking `block_on` is ~20 lines of std, which keeps the
//! test path dependency-free. Production adapters bring their own runtimes.
//!
//! [`TestTimer`] implements the [`Timer`] port on a virtual clock that moves
//! only when a test calls [`TestTimer::advance`] — so a sixty-second deadline
//! elapses in microseconds, deterministically, with no real sleeping
//! (`ARCHITECTURE.md`, decision 24).

use std::future::Future;
use std::pin::{Pin, pin};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Wake, Waker};
use std::thread::{self, Thread};
use std::time::{Duration, Instant};

use kamiroh_ports::Timer;

struct ThreadWaker(Thread);

impl Wake for ThreadWaker {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.0.unpark();
    }
}

/// How long [`block_on`] waits for progress before declaring a future stuck.
///
/// Deliberately generous: every future driven here is memory-backed and
/// virtual-clocked, so a healthy one finishes in microseconds. The bound is a
/// hang detector, not a performance budget.
pub const STUCK_AFTER: Duration = Duration::from_secs(30);

/// Drive `future` to completion on the current thread, parking between polls.
///
/// **Bounded** (see [`STUCK_AFTER`]): a future that stops making progress
/// panics with a message instead of parking forever. Reviews of spike 2
/// repeatedly turned up regressions whose only symptom was a test that never
/// returned — a reply that should have arrived never did, nothing woke the
/// thread, and `cargo test` sat there. A hang is detected only as a CI
/// timeout with no diagnostic; a panic names the test and fails the suite in
/// the ordinary way. Use [`block_on_bounded`] for a different ceiling.
pub fn block_on<F: Future>(future: F) -> F::Output {
    block_on_bounded(STUCK_AFTER, future)
}

/// [`block_on`] with an explicit stuck-detection bound.
///
/// Panics if `future` is still pending `bound` after the call began. The
/// bound is wall-clock since entry, not accumulated parked time, so a healthy
/// future that is repeatedly woken is never penalised for being slow to
/// schedule — it is only ever compared against a ceiling no honest test
/// approaches.
pub fn block_on_bounded<F: Future>(bound: Duration, future: F) -> F::Output {
    let mut future = pin!(future);
    let waker = Waker::from(Arc::new(ThreadWaker(thread::current())));
    let mut cx = Context::from_waker(&waker);
    let started = Instant::now();
    loop {
        match future.as_mut().poll(&mut cx) {
            Poll::Ready(output) => return output,
            Poll::Pending => {
                let waited = started.elapsed();
                let Some(remaining) = bound.checked_sub(waited) else {
                    panic!(
                        "block_on: future still pending after {waited:?} (bound {bound:?}) \
                         — nothing woke this thread, so it is stuck rather than slow. \
                         Something that should have arrived never did: an ack, a turn, \
                         a reply the peer owed, or a delivery a party swallowed."
                    );
                };
                // park_timeout may return spuriously; the loop re-polls, and
                // the ceiling is re-checked against wall time either way.
                thread::park_timeout(remaining);
            }
        }
    }
}

/// A point on the [`TestTimer`]'s virtual clock: nanoseconds since its start.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct TestInstant(u128);

impl std::ops::Add<Duration> for TestInstant {
    type Output = TestInstant;

    fn add(self, rhs: Duration) -> TestInstant {
        TestInstant(self.0 + rhs.as_nanos())
    }
}

#[derive(Debug, Default)]
struct TestClock {
    now: u128,
    /// Wakers of sleeps not yet due, with their due instants.
    sleepers: Vec<(u128, Waker)>,
}

/// A [`Timer`] on a virtual clock that moves only when the test says so.
///
/// Clone handles freely; they share the clock. `sleep_until` futures complete
/// exactly when [`TestTimer::advance`] (or a competing waker) moves the clock
/// to or past their due instant — no real time is ever spent.
#[derive(Debug, Clone, Default)]
pub struct TestTimer {
    clock: Arc<Mutex<TestClock>>,
}

impl TestTimer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Move the virtual clock forward, waking every sleep now due.
    pub fn advance(&self, by: Duration) {
        let woken: Vec<Waker> = {
            let mut clock = self.clock.lock().expect("test clock poisoned");
            clock.now += by.as_nanos();
            let now = clock.now;
            let (due, pending): (Vec<_>, Vec<_>) =
                clock.sleepers.drain(..).partition(|(at, _)| *at <= now);
            clock.sleepers = pending;
            due.into_iter().map(|(_, waker)| waker).collect()
        };
        for waker in woken {
            waker.wake();
        }
    }
}

impl Timer for TestTimer {
    type Instant = TestInstant;

    fn now(&self) -> TestInstant {
        TestInstant(self.clock.lock().expect("test clock poisoned").now)
    }

    fn sleep_until(&self, deadline: TestInstant) -> impl Future<Output = ()> + Send {
        TestSleep {
            clock: Arc::clone(&self.clock),
            due: deadline.0,
        }
    }
}

struct TestSleep {
    clock: Arc<Mutex<TestClock>>,
    due: u128,
}

impl Future for TestSleep {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let mut clock = self.clock.lock().expect("test clock poisoned");
        if clock.now >= self.due {
            return Poll::Ready(());
        }
        // Re-registering on every poll is fine: stale wakers wake a future
        // that simply re-checks the clock. Known cost, accepted for a test
        // affordance: entries for *dropped* sleep futures are not removed
        // until the clock advances past their due instant, so a loop that
        // builds a fresh sleep per iteration grows `sleepers` until the next
        // `advance`. Bounded by test length; revisit only if a real suite
        // notices.
        clock.sleepers.push((self.due, cx.waker().clone()));
        Poll::Pending
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_healthy_future_completes() {
        assert_eq!(block_on(async { 7 }), 7);
    }

    #[test]
    #[should_panic(expected = "still pending")]
    fn a_stuck_future_panics_instead_of_hanging() {
        // The regression class this bound exists for: nothing will ever wake
        // this thread, so unbounded parking would hang the suite forever.
        block_on_bounded(Duration::from_millis(50), std::future::pending::<()>());
    }
}
