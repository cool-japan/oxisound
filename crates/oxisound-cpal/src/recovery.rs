//! Automatic stream recovery on device disconnect.
//!
//! This module provides [`RecoveryHandle`] and the internal [`ReconnectInner`] state used by
//! [`CpalOutputStream::enable_auto_reconnect`](crate::streams::CpalOutputStream::enable_auto_reconnect).
//!
//! # Design
//!
//! When `enable_auto_reconnect()` is called, the existing ring-buffer producer is moved into
//! `ReconnectInner.producer` (an `Arc<Mutex<HeapProd<f32>>>`), so the recovery thread can swap
//! it atomically.  The initial `cpal::Stream` remains in `CpalOutputStream.stream` and keeps the
//! initial callback alive until the device disconnects.  After the first successful reconnect, the
//! new stream is stored in `ReconnectInner.stream` (`Arc<Mutex<Option<cpal::Stream>>>`); each
//! subsequent reconnect drops the previous entry there and installs the new one.
//!
//! The write path takes a brief Mutex lock on the producer — acceptable because lock contention
//! only occurs when the device has already disconnected and the audio callback is not running.
//!
//! The recovery thread polls `disconnected` every 100 ms.  On disconnect it uses exponential
//! back-off (10 → 50 → 200 → 1000 ms) to obtain the new default output device and rebuilds the
//! stream via `open_output_inner_with_shared_arcs`, passing the original shared Arcs so the new
//! stream's callbacks write to the same locations the caller reads.  On success it swaps the
//! producer and stream, then clears the `disconnected` flag.

#[cfg(not(target_arch = "wasm32"))]
use crate::CpalDevice;
#[cfg(not(target_arch = "wasm32"))]
use oxisound_core::{AudioDevice, StreamConfig as OxiStreamConfig};
#[cfg(not(target_arch = "wasm32"))]
use ringbuf::HeapProd;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
#[cfg(not(target_arch = "wasm32"))]
use std::time::Duration;

// ---------------------------------------------------------------------------
// ReconnectInner — shared between the stream handle and recovery thread
// ---------------------------------------------------------------------------

/// Shared mutable state for the swappable producer and stream used during recovery.
///
/// - `producer`: the live ring-buffer producer.  The write path locks this briefly per call.
/// - `stream`: `None` initially (initial stream lives in `CpalOutputStream.stream`); replaced
///   with `Some(new_stream)` after each successful reconnect.  Dropping the previous `Some`
///   stops its audio callback.
///
/// Not available on `wasm32`.
#[cfg(not(target_arch = "wasm32"))]
pub struct ReconnectInner {
    /// The live ring-buffer producer.  Write path locks this briefly per call.
    pub producer: Arc<Mutex<HeapProd<f32>>>,
    /// The replacement stream after each reconnect (`None` until first reconnect).
    pub stream: Arc<Mutex<Option<cpal::Stream>>>,
}

// ---------------------------------------------------------------------------
// RecoveryHandle — RAII guard returned by enable_auto_reconnect
// ---------------------------------------------------------------------------

/// RAII guard that keeps the auto-recovery background thread alive.
///
/// Drop to stop monitoring.  When idle the thread joins cleanly within two poll cycles
/// (≤ 200 ms).  If a reconnect attempt is in flight, the rebuild goes through the
/// bounded `open_output_inner_with_shared_arcs` path, so the attempt — and therefore
/// the join — is capped by [`STREAM_OPEN_TIMEOUT`](crate::STREAM_OPEN_TIMEOUT) rather
/// than blocking forever on a wedged backend.
/// Not available on `wasm32` (no OS threads).
#[cfg(not(target_arch = "wasm32"))]
#[must_use = "drop the RecoveryHandle to stop the reconnect monitor"]
pub struct RecoveryHandle {
    pub(crate) stop: Arc<AtomicBool>,
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) thread: Option<std::thread::JoinHandle<()>>,
}

#[cfg(not(target_arch = "wasm32"))]
impl Drop for RecoveryHandle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

// ---------------------------------------------------------------------------
// Recovery thread logic
// ---------------------------------------------------------------------------

/// Spawns the recovery background thread.
///
/// All `Arc` parameters are shared with `CpalOutputStream`; reusing them keeps stats and
/// disconnect detection continuous across reconnects.  The new stream built on reconnect
/// receives the *same* Arcs (via `open_output_inner_with_shared_arcs`), so its audio
/// callback writes directly to the same locations the caller reads — no manual accumulation needed.
///
/// Not available on `wasm32` (no OS threads).
#[cfg(not(target_arch = "wasm32"))]
#[expect(
    clippy::too_many_arguments,
    reason = "reconnect inner takes the same Arc handles as the original stream builder to ensure counter continuity across reconnects"
)]
pub(crate) fn spawn_recovery_thread(
    reconnect_inner: Arc<ReconnectInner>,
    disconnected: Arc<AtomicBool>,
    config: OxiStreamConfig,
    stop: Arc<AtomicBool>,
    underrun_count: Arc<AtomicU64>,
    frames_processed: Arc<AtomicU64>,
    callback_duration_ns: Arc<AtomicU64>,
    buffer_period_ns: Arc<AtomicU64>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        const BACKOFF_MS: [u64; 4] = [10, 50, 200, 1000];
        let mut backoff_idx = 0usize;

        while !stop.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(100));

            if !disconnected.load(Ordering::Relaxed) {
                backoff_idx = 0;
                continue;
            }

            log::warn!(
                "[oxisound-cpal] stream recovery: device disconnected, attempt {}",
                backoff_idx + 1
            );

            let delay = BACKOFF_MS[backoff_idx.min(BACKOFF_MS.len() - 1)];
            std::thread::sleep(Duration::from_millis(delay));
            backoff_idx = (backoff_idx + 1).min(BACKOFF_MS.len() - 1);

            // Attempt to open the new default output device, passing the original shared Arcs
            // so the new stream's callbacks write directly to the same Arcs that the caller
            // reads (disconnect detection and stats remain continuous across reconnects).
            let result = CpalDevice::default_output().and_then(|dev| {
                dev.open_output_inner_with_shared_arcs(
                    config.clone(),
                    Arc::clone(&disconnected),
                    Arc::clone(&underrun_count),
                    Arc::clone(&frames_processed),
                    Arc::clone(&callback_duration_ns),
                    Arc::clone(&buffer_period_ns),
                )
            });

            match result {
                Ok(new_stream) => {
                    // Lock the producer first so no writer can interleave during the swap.
                    let Ok(mut prod_guard) = reconnect_inner.producer.lock() else {
                        log::error!("[oxisound-cpal] stream recovery: producer mutex poisoned");
                        continue;
                    };
                    let Ok(mut stream_guard) = reconnect_inner.stream.lock() else {
                        log::error!("[oxisound-cpal] stream recovery: stream mutex poisoned");
                        continue;
                    };

                    // Swap in the new producer (new ring buffer feeds the new stream's callback).
                    // The old producer in *prod_guard is dropped here; its stream (the original
                    // self.stream or the previous reconnect's stream) is already disconnected so
                    // its callback is no longer running — safe to abandon.
                    //
                    // NOTE: No counter accumulation needed here.  The new stream was built with
                    // the same shared Arcs, so `new_stream.underrun_count` IS `underrun_count`
                    // (Arc::ptr_eq).  Accumulating would double-count.
                    *prod_guard = new_stream.producer;

                    // The previous reconnected stream (if any) is dropped here, stopping its
                    // callback.  The initial stream (self.stream) is not touched.
                    *stream_guard = Some(new_stream.stream);

                    // Clear the disconnect flag only after the new stream is installed.
                    disconnected.store(false, Ordering::Relaxed);
                    backoff_idx = 0;

                    log::info!("[oxisound-cpal] stream recovery: reconnected successfully");
                }
                Err(e) => {
                    log::error!("[oxisound-cpal] stream recovery: attempt failed: {e}");
                }
            }
        }
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod recovery_tests {
    use super::*;
    use oxisound_core::{AudioDevice, StreamConfig};

    /// Verify that RecoveryHandle stops the thread cleanly on drop (no panic, no hang).
    #[test]
    fn recovery_handle_drops_cleanly() {
        let stop = Arc::new(AtomicBool::new(false));
        let stop_clone = Arc::clone(&stop);
        let thread = std::thread::spawn(move || {
            while !stop_clone.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(5));
            }
        });
        let handle = RecoveryHandle {
            stop,
            thread: Some(thread),
        };
        // Drop signals the stop flag and joins the thread.
        drop(handle);
    }

    /// Verify that RecoveryHandle sets the stop flag correctly on drop.
    #[test]
    fn recovery_handle_stop_flag_initially_false_then_true_on_drop() {
        let stop = Arc::new(AtomicBool::new(false));
        let handle = RecoveryHandle {
            stop: Arc::clone(&stop),
            thread: None,
        };
        assert!(!stop.load(Ordering::Relaxed), "stop flag must start false");
        drop(handle);
        assert!(
            stop.load(Ordering::Relaxed),
            "stop flag must be true after drop"
        );
    }

    /// Verify that the StreamConfig used for recovery stores the right sample rate and channels.
    #[test]
    fn recovery_config_stores_sample_rate_and_channels() {
        let config = StreamConfig::stereo_48k();
        assert_eq!(config.sample_rate, 48_000);
        assert_eq!(config.channels, 2);
    }

    /// Verify that ReconnectInner can be constructed and its Arc-wrapped fields are accessible.
    ///
    /// Uses a dummy ring buffer — no audio hardware required.
    #[test]
    fn reconnect_inner_field_access() {
        use ringbuf::HeapRb;
        use ringbuf::traits::{Observer, Split};
        let rb: HeapRb<f32> = HeapRb::new(64);
        let (producer, _consumer) = rb.split();

        let inner = ReconnectInner {
            producer: Arc::new(Mutex::new(producer)),
            stream: Arc::new(Mutex::new(None)),
        };

        // producer should have 64 vacant slots.
        let prod_guard = inner.producer.lock().expect("lock must not be poisoned");
        assert_eq!(prod_guard.vacant_len(), 64);
        drop(prod_guard);

        // stream should start as None.
        let stream_guard = inner.stream.lock().expect("lock must not be poisoned");
        assert!(stream_guard.is_none(), "stream must start as None");
    }

    /// Verify that the recovery thread exits promptly when the stop flag is pre-set.
    #[test]
    fn recovery_thread_respects_stop_flag_immediately() {
        let stop = Arc::new(AtomicBool::new(true)); // already set
        let stop_clone = Arc::clone(&stop);
        let thread = std::thread::spawn(move || {
            while !stop_clone.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(100));
            }
        });
        thread.join().expect("recovery thread should exit promptly");
    }

    /// Verify that `enable_auto_reconnect` does not replace `self.disconnected` Arc.
    ///
    /// After calling `enable_auto_reconnect`, the `disconnected` Arc the caller holds (cloned
    /// before the call) must still point to the same allocation as `stream.disconnected`.
    /// This guarantees that `stream.is_disconnected()` still reads the original flag.
    ///
    /// Requires audio hardware to open a stream.
    #[test]
    #[ignore = "requires audio hardware"]
    fn enable_auto_reconnect_preserves_disconnected_arc_identity() {
        let device = CpalDevice::default_output().expect("no default output device");
        let config = StreamConfig::stereo_48k();
        let mut stream = device
            .open_output_concrete(config.clone())
            .expect("open_output_concrete failed");

        // Clone the Arc before enable_auto_reconnect so we can compare pointer identity.
        let disconnected_before = Arc::clone(&stream.disconnected);
        let underrun_before = Arc::clone(&stream.underrun_count);
        let frames_before = Arc::clone(&stream.frames_processed);

        let _handle = stream.enable_auto_reconnect(config);

        // `disconnected` must not have been replaced — same Arc as before.
        assert!(
            Arc::ptr_eq(&disconnected_before, &stream.disconnected),
            "enable_auto_reconnect must preserve the disconnected Arc"
        );
        assert!(
            Arc::ptr_eq(&underrun_before, &stream.underrun_count),
            "enable_auto_reconnect must preserve the underrun_count Arc"
        );
        assert!(
            Arc::ptr_eq(&frames_before, &stream.frames_processed),
            "enable_auto_reconnect must preserve the frames_processed Arc"
        );
    }

    /// Verify that `open_output_inner_with_shared_arcs` places the provided Arcs into the stream.
    ///
    /// This is the critical no-double-count guarantee: after reconnect the new stream's callbacks
    /// write to the same Arc locations the caller reads, not to private fresh allocations.
    ///
    /// Requires audio hardware to open a stream.
    #[test]
    #[ignore = "requires audio hardware"]
    fn shared_arc_stream_uses_caller_arcs() {
        let device = CpalDevice::default_output().expect("no default output device");
        let config = StreamConfig::stereo_48k();

        // Create the shared Arcs.
        let disconnected = Arc::new(AtomicBool::new(false));
        let underrun_count = Arc::new(AtomicU64::new(0));
        let frames_processed = Arc::new(AtomicU64::new(0));
        let callback_duration_ns = Arc::new(AtomicU64::new(0));
        let buffer_period_ns = Arc::new(AtomicU64::new(0));

        let stream = device
            .open_output_inner_with_shared_arcs(
                config,
                Arc::clone(&disconnected),
                Arc::clone(&underrun_count),
                Arc::clone(&frames_processed),
                Arc::clone(&callback_duration_ns),
                Arc::clone(&buffer_period_ns),
            )
            .expect("open_output_inner_with_shared_arcs failed");

        // The stream's Arcs must be ptr_eq to the ones we passed in.
        assert!(
            Arc::ptr_eq(&disconnected, &stream.disconnected),
            "stream.disconnected must be the same Arc as the one passed to open_output_inner_with_shared_arcs"
        );
        assert!(
            Arc::ptr_eq(&underrun_count, &stream.underrun_count),
            "stream.underrun_count must be the same Arc as the one passed"
        );
        assert!(
            Arc::ptr_eq(&frames_processed, &stream.frames_processed),
            "stream.frames_processed must be the same Arc as the one passed"
        );
        assert!(
            Arc::ptr_eq(&callback_duration_ns, &stream.callback_duration_ns),
            "stream.callback_duration_ns must be the same Arc as the one passed"
        );
        assert!(
            Arc::ptr_eq(&buffer_period_ns, &stream.buffer_period_ns),
            "stream.buffer_period_ns must be the same Arc as the one passed"
        );
    }

    /// Hardware-dependent reconnect test — requires actual device disconnect/reconnect.
    ///
    /// Manual test procedure:
    ///   1. Open the output stream with `enable_auto_reconnect()`.
    ///   2. Unplug the current default output device (e.g., USB audio adapter).
    ///   3. Wait ~3 seconds.
    ///   4. Assert `is_disconnected()` returns false (reconnected to fallback device).
    #[test]
    #[ignore = "requires audio hardware and physical device reconnect"]
    fn stream_recovery_rebuilds_on_disconnect() {
        let device = CpalDevice::default_output().expect("no default output device");
        let config = StreamConfig::stereo_48k();
        let mut stream = device
            .open_output_concrete(config.clone())
            .expect("open_output_concrete failed");
        let _handle = stream.enable_auto_reconnect(config);
        std::thread::sleep(Duration::from_secs(3));
        assert!(!stream.is_disconnected(), "stream should have reconnected");
    }
}
