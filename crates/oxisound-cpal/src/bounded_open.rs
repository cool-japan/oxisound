//! Bounded stream-open helper: every cpal open sequence runs on a disposable worker
//! thread so a wedged audio backend can never hang the caller.
//!
//! # Why every open is bounded
//!
//! Opening a stream means calling into the platform audio backend (alsa-lib /
//! CoreAudio / WASAPI) several times: configuration probing, `default_*_config`,
//! `build_*_stream` and `play`.  Those are C calls with no time bound and no
//! cancellation point.  Two real-world Linux failure modes make one of them block
//! indefinitely:
//!
//! - an ALSA PCM whose slave cannot be opened (typically an HDMI sink with no monitor
//!   attached — the source of the `ALSA lib pcm_dmix.c … unable to open slave`
//!   messages on stderr);
//! - the PipeWire ALSA plugin under concurrent open/close churn (e.g. a fully parallel
//!   test suite), which intermittently parks the open inside a `pw_thread_loop`
//!   condition wait that is never signalled.
//!
//! To make a hang impossible, [`open_bounded`] runs the whole open sequence on a
//! dedicated worker thread while the caller waits with a timeout — the design
//! `open_duplex` pioneered, generalized here so every open path in the crate shares
//! one audited implementation.
//!
//! # Timeout semantics (the deliberate leak)
//!
//! On timeout the worker thread is **detached** (its `JoinHandle` is dropped without
//! joining).  The blocking C call cannot be interrupted, so the thread stays parked
//! inside it until the backend returns; it then finds the response channel closed,
//! drops whatever half-built streams it produced (stopping any audio callback) and
//! exits.  Leaking one parked thread is the only sound option in the presence of an
//! uninterruptible FFI call — the alternative would be to block the caller forever.
//!
//! On `wasm32` there are no OS threads (`std::thread::spawn` fails at runtime) and the
//! Web Audio backend has no blocking open path, so the closure runs inline and no
//! timeout applies.

use oxisound_core::OxiSoundError;
use std::time::Duration;

/// Maximum time any stream-open entry point in this crate waits for the platform
/// audio backend to build and start a stream.
///
/// Every open sequence runs on a dedicated worker thread (see the crate-internal
/// `open_bounded` helper); if it has not produced a result within this bound the call returns
/// [`OxiSoundError::Timeout`] instead of blocking the caller forever.  15 seconds is
/// far beyond any healthy backend (a normal open completes in milliseconds) and short
/// enough that a test suite never appears to hang.
///
/// [`StreamConfig`](oxisound_core::StreamConfig) has no timeout field, so this constant
/// is the single source of truth; it is deliberately not configurable per call.
pub const STREAM_OPEN_TIMEOUT: Duration = Duration::from_secs(15);

// Compile-time proof that the values crossing the thread boundary may do so.
// If a future cpal release makes either type thread-affine, this fails here with a
// clear message instead of inside one of the worker closures.
#[cfg(not(target_arch = "wasm32"))]
const fn assert_send<T: Send>() {}
#[cfg(not(target_arch = "wasm32"))]
const _: () = assert_send::<cpal::Device>();
#[cfg(not(target_arch = "wasm32"))]
const _: () = assert_send::<cpal::Stream>();

/// Runs `open` on a named worker thread ("oxisound-open-`what`") and waits at most
/// `timeout` for its result.
///
/// `what` is a short noun ("output", "input", "duplex", …) used in the thread name,
/// the timeout log line and every error message, so a hang report immediately names
/// the path that wedged.
///
/// # Errors
///
/// - The closure's own `Err` is passed through unchanged.
/// - [`OxiSoundError::Timeout`] if the backend did not return within `timeout`; the
///   worker thread is detached and cleans up after itself when (if ever) the backend
///   call completes — see the module docs for why this leak is deliberate.
/// - [`OxiSoundError::Device`] if the worker thread could not be spawned, or if it
///   died without reporting a result (a panic inside the open sequence unwinds the
///   worker, closing the channel without a send).
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn open_bounded<T: Send + 'static>(
    what: &'static str,
    timeout: Duration,
    open: impl FnOnce() -> Result<T, OxiSoundError> + Send + 'static,
) -> Result<T, OxiSoundError> {
    let (tx, rx) = std::sync::mpsc::sync_channel::<Result<T, OxiSoundError>>(1);
    // Detached on timeout: the JoinHandle is dropped without joining.
    let _worker = std::thread::Builder::new()
        .name(format!("oxisound-open-{what}"))
        .spawn(move || {
            let result = open();
            // `send` fails only when the caller already timed out and dropped `rx`;
            // the streams are then dropped here, on this thread, which stops them.
            let _ = tx.send(result);
        })
        .map_err(|e| OxiSoundError::Device(format!("{what} open thread spawn: {e}")))?;

    match rx.recv_timeout(timeout) {
        Ok(result) => result,
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            log::warn!(
                "[oxisound-cpal] {what} open timed out after {}s; the audio backend did \
                 not return. The worker thread stays parked until the backend call \
                 completes, then cleans up.",
                timeout.as_secs()
            );
            Err(OxiSoundError::Timeout(format!(
                "{what} open did not complete within {}s (audio backend did not return)",
                timeout.as_secs()
            )))
        }
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => Err(OxiSoundError::Device(
            format!("{what} open worker thread died without a result (panic?)"),
        )),
    }
}

/// wasm32 variant: no OS threads exist, so the closure runs inline on the caller.
///
/// The Web Audio backend has no blocking open path, so the missing timeout is not a
/// regression — this mirrors the wasm32 special case `open_duplex` always had.  The
/// `Send + 'static` bounds are dropped along with the thread; the signature otherwise
/// matches the threaded variant so call sites need no `cfg`.
#[cfg(target_arch = "wasm32")]
pub(crate) fn open_bounded<T>(
    _what: &'static str,
    _timeout: Duration,
    open: impl FnOnce() -> Result<T, OxiSoundError>,
) -> Result<T, OxiSoundError> {
    open()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// A fast closure's value must pass through unchanged — the bound is transparent
    /// on the happy path.
    #[test]
    fn fast_open_passes_value_through() {
        let result = open_bounded("test-fast", Duration::from_secs(5), || Ok(42u32));
        assert_eq!(result.expect("fast open must succeed"), 42);
    }

    /// A closure's own `Err` must pass through unchanged (not be re-wrapped).
    #[test]
    fn open_error_passes_through() {
        let result = open_bounded::<u32>("test-err", Duration::from_secs(5), || {
            Err(OxiSoundError::NoDevice)
        });
        assert!(
            matches!(result, Err(OxiSoundError::NoDevice)),
            "closure error must pass through unchanged; got: {result:?}"
        );
    }

    /// A closure that outlives the timeout must yield `Err(Timeout)` promptly — the
    /// caller must never wait for the (deliberately leaked) worker to finish sleeping.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn slow_open_times_out_quickly() {
        let start = std::time::Instant::now();
        let result = open_bounded("test-slow", Duration::from_millis(50), || {
            // Far past the 50ms bound; the detached worker finishes this sleep on its
            // own after the test has already returned.
            std::thread::sleep(Duration::from_secs(2));
            Ok(1u32)
        });
        assert!(
            matches!(result, Err(OxiSoundError::Timeout(_))),
            "slow open must yield Timeout; got: {result:?}"
        );
        assert!(
            start.elapsed() < Duration::from_secs(1),
            "timeout must fire near the 50ms bound, not wait for the worker \
             (elapsed: {:?})",
            start.elapsed()
        );
    }

    /// A worker that panics unwinds without sending, closing the channel; the caller
    /// must map that to `Err(Device)` rather than hanging or propagating the panic.
    /// The unwind stays inside the worker thread — no FFI boundary is crossed.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn worker_panic_yields_device_error() {
        let result = open_bounded::<u32>("test-panic", Duration::from_secs(5), || {
            panic!("deliberate test panic (must surface as Err(Device), not a crash)");
        });
        assert!(
            matches!(result, Err(OxiSoundError::Device(_))),
            "worker panic must yield Device error; got: {result:?}"
        );
    }
}
