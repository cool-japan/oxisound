//! Deadlines for every blocking operation this backend performs.
//!
//! The `pulseaudio` client API is `async` and has no built-in timeouts: awaiting a
//! reply from a wedged server would block forever, and OxiSound's trait surface is
//! synchronous. [`block_on_timeout`] bridges both problems — it drives a future to
//! completion on the calling thread and gives up with [`OxiSoundError::Timeout`] once a
//! deadline passes, dropping (and therefore cancelling) the future.
//!
//! This module is platform-independent, so the deadline logic is unit-tested on every
//! host rather than only on Linux.

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use oxisound_core::OxiSoundError;

/// Deadline for locating, connecting to and authenticating with the server.
///
/// Covers `connect(2)` on the unix socket plus the `AUTH` / `SET_CLIENT_NAME` handshake.
pub const PULSE_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// Deadline for a single control-plane round-trip (server info, sink/source lists,
/// stream creation, cork/uncork, delete).
pub const PULSE_OP_TIMEOUT: Duration = Duration::from_secs(5);

/// Deadline for draining the client-side ring buffer into the server.
///
/// Matches `oxisound-cpal`'s `flush()` budget so both backends behave alike.
pub const PULSE_FLUSH_TIMEOUT: Duration = Duration::from_secs(2);

/// Deadline for the server-side `DRAIN_PLAYBACK_STREAM` round-trip.
///
/// Longer than [`PULSE_OP_TIMEOUT`] because the server only replies once every queued
/// sample has actually been rendered.
pub const PULSE_DRAIN_TIMEOUT: Duration = Duration::from_secs(10);

/// A future that resolves to `None` once its deadline passes.
///
/// Arms a one-shot timer thread the first time it is polled while pending; the thread
/// sleeps until the deadline and then wakes the task. If the inner future finishes
/// first, the timer thread simply wakes a task that has already completed, which is a
/// no-op.
#[derive(Debug)]
pub struct WithDeadline<F> {
    inner: Pin<Box<F>>,
    deadline: Instant,
    armed: bool,
}

impl<F: Future> WithDeadline<F> {
    /// Wraps `future` so that it resolves to `None` after `timeout`.
    pub fn new(future: F, timeout: Duration) -> Self {
        Self {
            inner: Box::pin(future),
            deadline: Instant::now() + timeout,
            armed: false,
        }
    }
}

impl<F: Future> Future for WithDeadline<F> {
    type Output = Option<F::Output>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // Every field is `Unpin` (`Pin<Box<F>>`, `Instant`, `bool`), so the projection
        // is safe without any `unsafe` — this crate forbids it.
        let this = self.get_mut();

        if let Poll::Ready(value) = this.inner.as_mut().poll(cx) {
            return Poll::Ready(Some(value));
        }

        let now = Instant::now();
        if now >= this.deadline {
            return Poll::Ready(None);
        }

        if !this.armed {
            this.armed = true;
            let waker = cx.waker().clone();
            let remaining = this.deadline.saturating_duration_since(now);
            // A detached one-shot timer: it sleeps for a bounded time and exits.
            let spawned = std::thread::Builder::new()
                .name("oxisound-pulse-timer".into())
                .spawn(move || {
                    std::thread::sleep(remaining);
                    waker.wake();
                });
            if let Err(err) = spawned {
                // Without a timer thread the deadline can still fire, just only when
                // something else wakes the task. Re-arm on the next poll.
                log::warn!("could not spawn oxisound-pulse timer thread: {err}");
                this.armed = false;
            }
        }

        Poll::Pending
    }
}

/// Drives `future` to completion on the calling thread, bounded by `timeout`.
///
/// `what` names the operation and appears in the timeout message.
///
/// # Errors
///
/// Returns [`OxiSoundError::Timeout`] when the deadline passes before the future
/// resolves. The future is dropped at that point, cancelling it.
///
/// # Examples
/// ```
/// use std::time::Duration;
/// use oxisound_pulse::block_on_timeout;
///
/// let value = block_on_timeout(async { 41 + 1 }, Duration::from_secs(1), "add").unwrap();
/// assert_eq!(value, 42);
///
/// let err = block_on_timeout(
///     std::future::pending::<()>(),
///     Duration::from_millis(20),
///     "never",
/// )
/// .unwrap_err();
/// assert_eq!(err.kind(), "timeout");
/// ```
pub fn block_on_timeout<F: Future>(
    future: F,
    timeout: Duration,
    what: &str,
) -> Result<F::Output, OxiSoundError> {
    futures_executor::block_on(WithDeadline::new(future, timeout)).ok_or_else(|| {
        OxiSoundError::Timeout(format!(
            "PulseAudio {what} did not complete within {:.1?}",
            timeout
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[test]
    fn timeout_constants_are_ordered_and_non_zero() {
        for (name, value) in [
            ("connect", PULSE_CONNECT_TIMEOUT),
            ("op", PULSE_OP_TIMEOUT),
            ("flush", PULSE_FLUSH_TIMEOUT),
            ("drain", PULSE_DRAIN_TIMEOUT),
        ] {
            assert!(!value.is_zero(), "{name} timeout must be non-zero");
            assert!(
                value <= Duration::from_secs(30),
                "{name} timeout must stay interactive"
            );
        }
        assert!(
            PULSE_DRAIN_TIMEOUT > PULSE_OP_TIMEOUT,
            "draining waits for real playback and needs the larger budget"
        );
        assert!(PULSE_FLUSH_TIMEOUT < PULSE_DRAIN_TIMEOUT);
    }

    #[test]
    fn ready_future_returns_its_value() {
        let value =
            block_on_timeout(async { "ok" }, Duration::from_secs(5), "unit").expect("ready");
        assert_eq!(value, "ok");
    }

    #[test]
    fn pending_future_times_out_with_the_timeout_variant() {
        let start = Instant::now();
        let err = block_on_timeout(
            std::future::pending::<u32>(),
            Duration::from_millis(50),
            "unit",
        )
        .expect_err("must time out");
        assert_eq!(err.kind(), "timeout");
        assert!(
            err.to_string().contains("unit"),
            "message should name the operation: {err}"
        );
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "the deadline must actually fire, not fall back to blocking forever"
        );
    }

    #[test]
    fn timed_out_future_is_dropped_and_therefore_cancelled() {
        struct DropFlag(Arc<AtomicBool>);
        impl Drop for DropFlag {
            fn drop(&mut self) {
                self.0.store(true, Ordering::SeqCst);
            }
        }

        let dropped = Arc::new(AtomicBool::new(false));
        let flag = DropFlag(Arc::clone(&dropped));
        let fut = async move {
            let _held = flag;
            std::future::pending::<()>().await;
        };

        let err =
            block_on_timeout(fut, Duration::from_millis(20), "cancel").expect_err("times out");
        assert_eq!(err.kind(), "timeout");
        assert!(
            dropped.load(Ordering::SeqCst),
            "the abandoned future must be dropped so its resources are released"
        );
    }

    /// A future that returns `Pending` once — forcing the deadline to arm its timer
    /// thread — and completes on the second poll.
    struct YieldOnce {
        yielded: bool,
    }

    impl std::future::Future for YieldOnce {
        type Output = u8;

        fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<u8> {
            if self.yielded {
                Poll::Ready(7)
            } else {
                self.yielded = true;
                cx.waker().wake_by_ref();
                Poll::Pending
            }
        }
    }

    #[test]
    fn a_future_that_parks_before_completing_still_wins_the_race() {
        let value = block_on_timeout(
            YieldOnce { yielded: false },
            Duration::from_millis(500),
            "race",
        )
        .expect("completes well inside the deadline");
        assert_eq!(value, 7);
    }
}
