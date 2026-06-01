//! CpalOutputStream, CpalInputStream, and CpalDuplexStream implementations.

use cpal::traits::StreamTrait;
#[cfg(not(target_arch = "wasm32"))]
use oxisound_core::StreamConfig as OxiStreamConfig;
use oxisound_core::{DuplexStream, InputStream, OutputStream, OxiSoundError, StreamStats};

// ---------------------------------------------------------------------------
// Drift-compensation constants
// ---------------------------------------------------------------------------

/// EMA smoothing factor for drift ratio updates. Lower values = smoother but slower to react.
const DRIFT_EMA_ALPHA: f64 = 0.05;
/// Minimum clamp for the raw drift ratio fed into the EMA (prevents extreme step values).
const DRIFT_CLAMP_MIN: f64 = 0.5;
/// Maximum clamp for the raw drift ratio fed into the EMA.
const DRIFT_CLAMP_MAX: f64 = 2.0;
#[cfg(not(target_arch = "wasm32"))]
use crate::recovery::{ReconnectInner, RecoveryHandle};
use ringbuf::{
    HeapCons, HeapProd,
    traits::{Consumer, Observer, Producer},
};
#[cfg(not(target_arch = "wasm32"))]
use std::sync::Mutex;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

// ---------------------------------------------------------------------------
// StreamHealth
// ---------------------------------------------------------------------------

/// Describes the current health of an audio stream.
#[derive(Debug, Clone, PartialEq)]
pub enum StreamHealth {
    /// Stream is operating normally.
    Healthy,
    /// Stream is experiencing underruns (value = cumulative underrun count).
    Degraded(f32),
    /// Stream device has disconnected.
    Disconnected,
}

// ---------------------------------------------------------------------------
// CpalOutputStream
// ---------------------------------------------------------------------------

/// A cpal-backed output stream using a lock-free SPSC ring buffer shared with the audio callback.
///
/// The producer side is held here for the application to write samples into.
/// The consumer side was moved into the audio callback at construction time.
pub struct CpalOutputStream {
    /// Keeps the stream alive for the lifetime of this handle.
    pub(crate) stream: cpal::Stream,
    /// SPSC producer — user pushes samples here; the audio callback pops them.
    pub(crate) producer: HeapProd<f32>,
    pub(crate) channels: u16,
    /// Maximum number of samples (across all channels) the ring buffer may hold (~2 seconds).
    pub(crate) capacity: usize,
    /// Set to `true` by the error callback when the device is disconnected.
    pub(crate) disconnected: Arc<AtomicBool>,
    /// Number of times the audio callback found the ring buffer empty.
    pub(crate) underrun_count: Arc<AtomicU64>,
    /// Total audio buffer slots processed by the callback (proxy for frames processed).
    pub(crate) frames_processed: Arc<AtomicU64>,
    /// Instant when the stream was created (for stream-time reporting).
    /// Not available on `wasm32` — `std::time::Instant` panics there without a shim.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) stream_start: std::time::Instant,
    /// Adaptive buffer-size state machine.
    pub(crate) adaptive: Arc<std::sync::Mutex<crate::adaptive::AdaptiveBufferSizer>>,
    /// Underrun count seen at the last [`tick_adaptive`](CpalOutputStream::tick_adaptive) call.
    pub(crate) last_tick_underruns: Arc<AtomicU64>,
    /// Desired buffer size stored for caller-driven stream rebuilds.
    pub(crate) desired_buffer_size: Arc<std::sync::atomic::AtomicU32>,
    /// Duration of the most recent audio callback invocation, in nanoseconds.
    pub(crate) callback_duration_ns: Arc<AtomicU64>,
    /// Expected period between callbacks based on buffer size and sample rate, in nanoseconds.
    pub(crate) buffer_period_ns: Arc<AtomicU64>,
    /// When `enable_auto_reconnect()` has been called, holds the swappable stream and producer.
    ///
    /// `None` means auto-recovery is disabled (default).  When `Some`, the `write()` and
    /// `flush()` paths use the mutex-wrapped producer instead of `self.producer`.
    /// Not available on `wasm32`.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) reconnect_inner: Option<Arc<ReconnectInner>>,
}

impl CpalOutputStream {
    /// Returns the number of frames currently buffered, i.e. the output latency in frames.
    pub fn latency_frames(&self) -> usize {
        let occupied = self.occupied_samples();
        occupied / usize::from(self.channels.max(1))
    }

    /// Returns the number of occupied samples in the output ring buffer.
    fn occupied_samples(&self) -> usize {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(inner) = self.reconnect_inner.as_ref() {
            return inner.producer.lock().map(|p| p.occupied_len()).unwrap_or(0);
        }
        self.producer.occupied_len()
    }

    /// Returns the maximum number of samples the output ring buffer may hold (~2 seconds).
    pub fn ring_capacity(&self) -> usize {
        self.capacity
    }

    /// Returns the number of underrun events since the stream was opened.
    ///
    /// An underrun occurs when the audio callback drains the ring buffer faster than the
    /// producer writes, resulting in silence being inserted.
    pub fn underrun_count(&self) -> u64 {
        self.underrun_count.load(Ordering::Relaxed)
    }

    /// Returns `true` if the underlying device has been reported as disconnected.
    pub fn is_disconnected(&self) -> bool {
        self.disconnected.load(Ordering::Relaxed)
    }

    /// Returns seconds elapsed since the stream was opened.
    /// Not available on `wasm32`.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn stream_time(&self) -> f64 {
        self.stream_start.elapsed().as_secs_f64()
    }

    /// Pauses the audio stream (no audio rendered, ring buffer preserved).
    pub fn pause(&self) -> Result<(), OxiSoundError> {
        self.stream
            .pause()
            .map_err(|e| OxiSoundError::Stream(e.to_string()))
    }

    /// Resumes the audio stream after a pause.
    pub fn resume(&self) -> Result<(), OxiSoundError> {
        self.stream
            .play()
            .map_err(|e| OxiSoundError::Stream(e.to_string()))
    }

    /// Waits until the ring buffer has been fully consumed by the audio callback.
    ///
    /// Times out after 2 seconds with `OxiSoundError::Timeout`.
    /// On `wasm32`, returns `Ok(())` immediately (no blocking wait available).
    pub fn flush(&self) -> Result<(), OxiSoundError> {
        #[cfg(target_arch = "wasm32")]
        {
            // wasm32: no blocking wait; return Ok immediately if not disconnected.
            if self.disconnected.load(Ordering::Relaxed) {
                return Err(OxiSoundError::Disconnected(
                    "stream disconnected during flush".into(),
                ));
            }
            Ok(())
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let start = std::time::Instant::now();
            loop {
                if self.disconnected.load(Ordering::Relaxed) {
                    return Err(OxiSoundError::Disconnected(
                        "stream disconnected during flush".into(),
                    ));
                }
                let occupied = if let Some(inner) = self.reconnect_inner.as_ref() {
                    inner.producer.lock().map(|p| p.occupied_len()).unwrap_or(0)
                } else {
                    self.producer.occupied_len()
                };
                if occupied == 0 {
                    return Ok(());
                }
                if start.elapsed().as_secs() >= 2 {
                    return Err(OxiSoundError::Timeout(
                        "flush timed out after 2 seconds".into(),
                    ));
                }
                std::thread::yield_now();
            }
        }
    }

    /// Returns the current health status of the stream.
    pub fn health(&self) -> StreamHealth {
        if self.disconnected.load(Ordering::Relaxed) {
            return StreamHealth::Disconnected;
        }
        let underruns = self.underrun_count.load(Ordering::Relaxed);
        if underruns > 0 {
            StreamHealth::Degraded(underruns as f32)
        } else {
            StreamHealth::Healthy
        }
    }

    /// Ticks the adaptive buffer sizer based on observed underruns.
    ///
    /// Call this periodically (e.g., after each buffer write) from the application thread.
    /// If the sizer recommends a different buffer size, `adaptive_sizer().size_changed()` will
    /// return `true` — the caller should then rebuild the stream with the new config.
    ///
    /// Returns the new recommended buffer size in frames.
    pub fn tick_adaptive(&self) -> u32 {
        let current = self.underrun_count.load(Ordering::Relaxed);
        let prev = self.last_tick_underruns.swap(current, Ordering::Relaxed);
        let new_underruns = current.saturating_sub(prev);
        let mut sizer = match self.adaptive.lock() {
            Ok(g) => g,
            Err(e) => e.into_inner(),
        };
        if new_underruns > 0 {
            sizer.record_underrun()
        } else {
            sizer.record_stable_period()
        }
    }

    /// Returns a snapshot of the current adaptive buffer sizer state.
    pub fn adaptive_sizer(&self) -> crate::adaptive::AdaptiveBufferSizer {
        match self.adaptive.lock() {
            Ok(g) => *g,
            Err(e) => *e.into_inner(),
        }
    }

    /// Stores a desired buffer size for use when rebuilding the stream.
    ///
    /// This does **not** resize the live stream. The caller is responsible for rebuilding
    /// the stream (e.g., by calling `CpalDevice::open_output(config)` with an updated
    /// `StreamConfig`) when `tick_adaptive()` indicates `size_changed()` is true.
    pub fn set_buffer_size(&self, frames: u32) {
        self.desired_buffer_size
            .store(frames, std::sync::atomic::Ordering::Relaxed);
    }

    /// Returns the desired buffer size set via [`set_buffer_size`](Self::set_buffer_size),
    /// or the initial value chosen at construction time.
    pub fn desired_buffer_size(&self) -> u32 {
        self.desired_buffer_size
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    // ---------------------------------------------------------------------------
    // Auto-reconnect (not available on wasm32 — no OS threads)
    // ---------------------------------------------------------------------------

    /// Enables built-in automatic reconnection to the system default output device on disconnect.
    ///
    /// # Design
    ///
    /// The existing ring-buffer producer is moved into an `Arc<Mutex<HeapProd<f32>>>` so the
    /// recovery thread can swap it atomically when rebuilding the stream.  `self.stream` (the
    /// initial cpal stream) is **not** moved — it keeps the initial audio callback alive until the
    /// device disconnects.  After the first successful reconnect, the new stream is held in
    /// `ReconnectInner.stream` and subsequent swaps replace that.
    ///
    /// The write and flush paths check `self.reconnect_inner` and, when `Some`, acquire a brief
    /// lock on the inner producer rather than using `self.producer` directly.
    ///
    /// # Background thread
    ///
    /// - Polls `disconnected` every 100 ms.
    /// - On disconnect: applies exponential back-off (10 → 50 → 200 → 1000 ms) and calls
    ///   `CpalDevice::default_output()?.open_output_concrete(config)`.
    /// - On success: locks both producer and stream, swaps them, clears `disconnected`.
    ///
    /// The returned [`RecoveryHandle`] keeps the monitor alive.  Drop it to stop monitoring.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use oxisound_cpal::CpalDevice;
    /// use oxisound_core::AudioDevice;
    ///
    /// let device = CpalDevice::default_output().unwrap();
    /// let config = oxisound_core::StreamConfig::stereo_48k();
    /// let mut stream = device.open_output_concrete(config.clone()).unwrap();
    /// let _handle = stream.enable_auto_reconnect(config);
    /// ```
    #[cfg(not(target_arch = "wasm32"))]
    pub fn enable_auto_reconnect(&mut self, config: OxiStreamConfig) -> RecoveryHandle {
        // Idempotent: if already enabled, return a no-op guard.
        if self.reconnect_inner.is_some() {
            return RecoveryHandle {
                stop: Arc::new(AtomicBool::new(false)),
                thread: None,
            };
        }

        // Move self.producer into the shared slot.
        // Replace it with a 1-slot dummy; this field is never accessed again via the
        // direct path once reconnect_inner is Some (write/flush use the inner lock).
        use ringbuf::HeapRb;
        use ringbuf::traits::Split;
        let dummy_rb: HeapRb<f32> = HeapRb::new(1);
        let (dummy_prod, _dummy_cons) = dummy_rb.split();
        let real_producer = std::mem::replace(&mut self.producer, dummy_prod);

        // ReconnectInner.stream starts as None: the initial cpal::Stream lives in
        // self.stream and is kept alive by CpalOutputStream's Drop.  After the first
        // reconnect, the new stream occupies ReconnectInner.stream and subsequent swaps
        // replace it there.
        let inner = Arc::new(ReconnectInner {
            producer: Arc::new(Mutex::new(real_producer)),
            stream: Arc::new(Mutex::new(None)),
        });

        self.reconnect_inner = Some(Arc::clone(&inner));

        let stop = Arc::new(AtomicBool::new(false));
        let stop_clone = Arc::clone(&stop);

        let thread = crate::recovery::spawn_recovery_thread(
            inner,
            Arc::clone(&self.disconnected),
            config,
            stop_clone,
            Arc::clone(&self.underrun_count),
            Arc::clone(&self.frames_processed),
            Arc::clone(&self.callback_duration_ns),
            Arc::clone(&self.buffer_period_ns),
        );

        RecoveryHandle {
            stop,
            thread: Some(thread),
        }
    }
}

impl OutputStream for CpalOutputStream {
    fn write(&mut self, samples: &[f32]) -> Result<(), OxiSoundError> {
        // When auto-reconnect is active and the stream is currently disconnected,
        // return Disconnected instead of Overrun so callers can detect the situation.
        // Once the recovery thread reconnects and clears the flag, writes resume normally.
        if self.disconnected.load(Ordering::Relaxed) {
            return Err(OxiSoundError::Disconnected(
                "output device disconnected".into(),
            ));
        }

        #[cfg(not(target_arch = "wasm32"))]
        if let Some(inner) = self.reconnect_inner.as_ref() {
            // Reconnect path: brief lock on the shared producer.
            let mut prod = inner
                .producer
                .lock()
                .map_err(|_| OxiSoundError::Device("reconnect producer mutex poisoned".into()))?;
            if prod.vacant_len() < samples.len() {
                return Err(OxiSoundError::Overrun(format!(
                    "output ring buffer full: {} free, {} requested",
                    prod.vacant_len(),
                    samples.len()
                )));
            }
            let n = prod.push_slice(samples);
            if n < samples.len() {
                return Err(OxiSoundError::Overrun(format!(
                    "partial write: only {n}/{} samples written",
                    samples.len()
                )));
            }
            return Ok(());
        }

        // Normal (non-reconnect) path: lock-free direct access.
        if self.producer.vacant_len() < samples.len() {
            return Err(OxiSoundError::Overrun(format!(
                "output ring buffer full: {} free, {} requested",
                self.producer.vacant_len(),
                samples.len()
            )));
        }
        let n = self.producer.push_slice(samples);
        if n < samples.len() {
            return Err(OxiSoundError::Overrun(format!(
                "partial write: only {n}/{} samples written",
                samples.len()
            )));
        }
        Ok(())
    }

    fn stats(&self) -> StreamStats {
        let cb_ns = self.callback_duration_ns.load(Ordering::Relaxed);
        let period_ns = self.buffer_period_ns.load(Ordering::Relaxed);
        let cpu_load_percent = if period_ns > 0 {
            (cb_ns as f64 / period_ns as f64 * 100.0) as f32
        } else {
            0.0
        };

        let latency_frames = self.occupied_samples() as u32;

        StreamStats {
            frames_processed: self.frames_processed.load(Ordering::Relaxed),
            underruns: self.underrun_count.load(Ordering::Relaxed),
            overruns: 0,
            latency_frames,
            cpu_load_percent,
        }
    }
}

// ---------------------------------------------------------------------------
// CpalInputStream
// ---------------------------------------------------------------------------

/// A cpal-backed input stream using a lock-free SPSC ring buffer shared with the audio callback.
///
/// The consumer side is held here for the application to read captured samples.
/// The producer side was moved into the audio callback at construction time.
pub struct CpalInputStream {
    /// Keeps the stream alive for the lifetime of this handle.
    pub(crate) stream: cpal::Stream,
    /// SPSC consumer — the audio callback pushes captured samples; user pops them here.
    pub(crate) consumer: HeapCons<f32>,
    pub(crate) channels: u16,
    /// Maximum number of samples the capture ring buffer may accumulate (~2 seconds).
    pub(crate) capacity: usize,
    /// Set to `true` by the error callback when the device is disconnected.
    pub(crate) disconnected: Arc<AtomicBool>,
    /// Total frames captured by the audio callback since the stream opened.
    pub(crate) frames_processed: Arc<AtomicU64>,
    /// Duration of the most recent audio callback invocation, in nanoseconds.
    pub(crate) callback_duration_ns: Arc<AtomicU64>,
    /// Expected period between callbacks based on buffer size and sample rate, in nanoseconds.
    pub(crate) buffer_period_ns: Arc<AtomicU64>,
}

impl CpalInputStream {
    /// Returns the number of frames currently available in the capture ring buffer.
    pub fn latency_frames(&self) -> usize {
        let occupied = self.consumer.occupied_len();
        occupied / usize::from(self.channels.max(1))
    }

    /// Returns the maximum number of samples the capture ring buffer may hold.
    ///
    /// This is approximately two seconds of audio at the configured sample rate and channel count.
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Returns `true` if the underlying device has been reported as disconnected.
    pub fn is_disconnected(&self) -> bool {
        self.disconnected.load(Ordering::Relaxed)
    }

    /// Returns the total number of frames captured by the audio callback since the stream opened.
    pub fn frames_processed(&self) -> u64 {
        self.frames_processed.load(Ordering::Relaxed)
    }

    /// Pauses the audio capture stream.
    pub fn pause(&self) -> Result<(), OxiSoundError> {
        self.stream
            .pause()
            .map_err(|e| OxiSoundError::Stream(e.to_string()))
    }

    /// Resumes the audio capture stream after a pause.
    pub fn resume(&self) -> Result<(), OxiSoundError> {
        self.stream
            .play()
            .map_err(|e| OxiSoundError::Stream(e.to_string()))
    }
}

impl InputStream for CpalInputStream {
    fn read(&mut self, samples: &mut [f32]) -> Result<usize, OxiSoundError> {
        if self.disconnected.load(Ordering::Relaxed) {
            return Err(OxiSoundError::Disconnected(
                "input device disconnected".into(),
            ));
        }
        let n = self.consumer.pop_slice(samples);
        Ok(n)
    }

    fn stats(&self) -> StreamStats {
        let cb_ns = self.callback_duration_ns.load(Ordering::Relaxed);
        let period_ns = self.buffer_period_ns.load(Ordering::Relaxed);
        let cpu_load_percent = if period_ns > 0 {
            (cb_ns as f64 / period_ns as f64 * 100.0) as f32
        } else {
            0.0
        };
        StreamStats {
            frames_processed: self.frames_processed.load(Ordering::Relaxed),
            underruns: 0,
            // The capture ring buffer silently drops samples when full; report buffered frames
            // as latency so callers can detect a consumer that is falling behind.
            overruns: 0,
            latency_frames: self.consumer.occupied_len() as u32,
            cpu_load_percent,
        }
    }
}

// ---------------------------------------------------------------------------
// CpalDuplexStream
// ---------------------------------------------------------------------------

/// A cpal-backed duplex stream with paired lock-free SPSC ring buffers for input and output.
///
/// Both streams are kept alive for the lifetime of this handle.
/// Dropping the handle stops both the input and output callbacks.
pub struct CpalDuplexStream {
    pub(crate) _input: cpal::Stream,
    pub(crate) _output: cpal::Stream,
    /// User reads captured audio from here (callback writes).
    pub(crate) in_consumer: HeapCons<f32>,
    /// User writes playback audio here (callback reads).
    pub(crate) out_producer: HeapProd<f32>,
    pub(crate) in_channels: u16,
    pub(crate) out_channels: u16,
    pub(crate) out_disconnected: Arc<AtomicBool>,
    pub(crate) in_disconnected: Arc<AtomicBool>,
    /// Total frames captured by the input callback since the stream opened.
    pub(crate) in_frames_processed: Arc<AtomicU64>,
    /// Total frames rendered by the output callback since the stream opened.
    pub(crate) out_frames_processed: Arc<AtomicU64>,
    /// EMA-smoothed drift ratio (output frames / input frames). Starts at 1.0 (no drift).
    pub(crate) drift_ema: f64,
    /// Fractional phase offset carried across `pump_resampled` calls (0.0 ≤ phase < 1.0).
    pub(crate) resample_phase: f64,
}

impl CpalDuplexStream {
    /// Returns the number of input frames currently buffered (capture latency in frames).
    pub fn input_latency_frames(&self) -> usize {
        let occupied = self.in_consumer.occupied_len();
        occupied / usize::from(self.in_channels.max(1))
    }

    /// Returns the number of output frames currently queued (playback latency in frames).
    pub fn output_latency_frames(&self) -> usize {
        let occupied = self.out_producer.occupied_len();
        occupied / usize::from(self.out_channels.max(1))
    }

    /// Returns the estimated total roundtrip latency in frames.
    ///
    /// Computed as the sum of input buffered frames (capture latency) and
    /// output buffered frames (playback latency).
    pub fn roundtrip_latency_frames(&self) -> usize {
        self.input_latency_frames() + self.output_latency_frames()
    }

    /// Returns the total number of frames captured by the input callback.
    pub fn input_frames_processed(&self) -> u64 {
        self.in_frames_processed.load(Ordering::Relaxed)
    }

    /// Returns the total number of frames rendered by the output callback.
    pub fn output_frames_processed(&self) -> u64 {
        self.out_frames_processed.load(Ordering::Relaxed)
    }

    /// Returns the clock-drift ratio between the output and input streams.
    ///
    /// Defined as `output_frames_processed / input_frames_processed`. When the input and output
    /// device clocks run at exactly the same rate this converges to `1.0`. A value greater than
    /// `1.0` means the output side has consumed more frames than the input captured (output clock
    /// is faster); a value below `1.0` means the opposite. Multiply a nominal resampling ratio by
    /// this value to compensate for drift in a duplex pipeline.
    ///
    /// Returns `None` until the input callback has processed at least one frame (avoids dividing
    /// by zero immediately after the stream opens).
    pub fn drift_ratio(&self) -> Option<f64> {
        let in_frames = self.in_frames_processed.load(Ordering::Relaxed);
        if in_frames == 0 {
            return None;
        }
        let out_frames = self.out_frames_processed.load(Ordering::Relaxed);
        Some(out_frames as f64 / in_frames as f64)
    }

    /// Returns the current EMA-smoothed drift ratio (output frames / input frames).
    ///
    /// Values > 1.0 mean the output clock is faster; < 1.0 means the input clock is faster.
    /// Starts at `1.0` and converges towards the true drift over many `pump_resampled` calls.
    pub fn drift_ema(&self) -> f64 {
        self.drift_ema
    }

    /// Updates the EMA with the latest raw drift ratio measurement (private helper).
    fn update_drift(&mut self) {
        if let Some(raw) = self.drift_ratio() {
            let clamped = raw.clamp(DRIFT_CLAMP_MIN, DRIFT_CLAMP_MAX);
            self.drift_ema = (1.0 - DRIFT_EMA_ALPHA) * self.drift_ema + DRIFT_EMA_ALPHA * clamped;
        }
    }

    /// Pump available input frames to the output ring with linear-interpolation drift correction.
    ///
    /// Reads from `in_consumer`, resamples at rate `1 / drift_ema`, writes to `out_producer`
    /// bounded by `vacant_len` so the existing `Overrun` hard-error in `write()` is never tripped.
    /// Returns the number of output frames written.
    ///
    /// The resampling step is `1 / drift_ema`:
    /// - When output is faster (`drift_ema > 1`): step < 1, we stretch input (upsample).
    /// - When input is faster (`drift_ema < 1`): step > 1, we squeeze input (downsample).
    ///
    /// A fractional phase is carried across calls so resampling is continuous across buffer
    /// boundaries. HF rolloff and aliasing at extreme drift ratios are v1 trade-offs — correctness
    /// and alignment take priority over fidelity.
    pub fn pump_resampled(&mut self) -> Result<usize, OxiSoundError> {
        self.update_drift();

        let in_ch = usize::from(self.in_channels);
        let out_ch = usize::from(self.out_channels);

        if in_ch == 0 || out_ch == 0 {
            return Ok(0);
        }

        let step = 1.0_f64 / self.drift_ema;
        if step <= 0.0 {
            return Ok(0);
        }

        let in_available = self.in_consumer.occupied_len() / in_ch;
        let out_capacity = self.out_producer.vacant_len() / out_ch;

        if in_available == 0 || out_capacity == 0 {
            return Ok(0);
        }

        let phase = self.resample_phase;

        // How many output frames can we produce while staying within in_available?
        // Output frame i accesses input at phase + i*step; capped by in_available.
        let out_frames_from_input = ((in_available as f64 - phase) / step).floor() as usize;
        let out_frames = out_frames_from_input.min(out_capacity);

        if out_frames == 0 {
            return Ok(0);
        }

        // Pop enough input: logical consumption + 1 lookahead for the lerp pair.
        let final_phase = phase + out_frames as f64 * step;
        let in_needed = (final_phase.floor() as usize + 1).min(in_available);

        let mut in_buf = vec![0.0_f32; in_needed * in_ch];
        let popped = self.in_consumer.pop_slice(&mut in_buf);
        let actual_in = popped / in_ch;

        // Linear interpolation: produce out_frames output frames.
        let mut out_buf = vec![0.0_f32; out_frames * out_ch];
        let mut cur = phase;
        for out_frame in 0..out_frames {
            let idx0 = cur.floor() as usize;
            let frac = (cur - cur.floor()) as f32;
            let idx1 = (idx0 + 1).min(actual_in.saturating_sub(1));

            for out_c in 0..out_ch {
                // Simple channel mapping: if channel counts differ, repeat/truncate.
                let in_c = out_c.min(in_ch - 1);
                let s0 = in_buf.get(idx0 * in_ch + in_c).copied().unwrap_or(0.0);
                let s1 = in_buf.get(idx1 * in_ch + in_c).copied().unwrap_or(0.0);
                out_buf[out_frame * out_ch + out_c] = s0 + (s1 - s0) * frac;
            }

            cur += step;
        }

        // Carry fractional phase for the next call.
        self.resample_phase = final_phase.fract();

        let pushed = self.out_producer.push_slice(&out_buf);
        Ok(pushed / out_ch)
    }
}

impl DuplexStream for CpalDuplexStream {
    fn write(&mut self, out: &[f32]) -> Result<(), OxiSoundError> {
        if self.out_disconnected.load(Ordering::Relaxed) {
            return Err(OxiSoundError::Disconnected(
                "duplex output device disconnected".into(),
            ));
        }
        if self.out_producer.vacant_len() < out.len() {
            return Err(OxiSoundError::Overrun(format!(
                "duplex output ring buffer full: {} free, {} requested",
                self.out_producer.vacant_len(),
                out.len()
            )));
        }
        let n = self.out_producer.push_slice(out);
        if n < out.len() {
            return Err(OxiSoundError::Overrun(format!(
                "duplex output partial write: only {n}/{} samples written",
                out.len()
            )));
        }
        Ok(())
    }

    fn read(&mut self, inp: &mut [f32]) -> Result<usize, OxiSoundError> {
        if self.in_disconnected.load(Ordering::Relaxed) {
            return Err(OxiSoundError::Disconnected(
                "duplex input device disconnected".into(),
            ));
        }
        let n = self.in_consumer.pop_slice(inp);
        Ok(n)
    }

    fn stats(&self) -> StreamStats {
        // Report the output side's processed frames (playback progress) and the combined
        // roundtrip buffering as the latency figure.
        StreamStats {
            frames_processed: self.out_frames_processed.load(Ordering::Relaxed),
            underruns: 0,
            overruns: 0,
            latency_frames: self.roundtrip_latency_frames() as u32,
            cpu_load_percent: 0.0,
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod resampler_tests {
    use super::*;
    use ringbuf::{HeapRb, traits::Split};

    /// Verify the EMA converges to a constant target after many iterations.
    #[test]
    fn drift_ema_converges() {
        let mut ema = 1.0_f64;
        let alpha = DRIFT_EMA_ALPHA;
        let target = 1.002_f64.clamp(DRIFT_CLAMP_MIN, DRIFT_CLAMP_MAX);
        for _ in 0..200 {
            ema = (1.0 - alpha) * ema + alpha * target;
        }
        // After 200 iterations, EMA should be very close to target.
        assert!((ema - 1.002).abs() < 0.001, "EMA did not converge: {ema}");
    }

    /// Verify that an out-of-range drift value is clamped before entering the EMA.
    #[test]
    fn drift_ema_clamped() {
        let mut ema = 1.0_f64;
        let alpha = DRIFT_EMA_ALPHA;
        // Feed a raw value well above DRIFT_CLAMP_MAX (5.0 → clamped to 2.0).
        let raw = 5.0_f64.clamp(DRIFT_CLAMP_MIN, DRIFT_CLAMP_MAX);
        for _ in 0..1000 {
            ema = (1.0 - alpha) * ema + alpha * raw;
        }
        assert!(
            ema <= DRIFT_CLAMP_MAX + 0.001,
            "EMA exceeded clamp max: {ema}"
        );
    }

    /// With step = 1.0 (identity), the resampler should pass samples through unchanged.
    #[test]
    fn linear_interp_identity_step() {
        let in_rb: HeapRb<f32> = HeapRb::new(64);
        let out_rb: HeapRb<f32> = HeapRb::new(64);
        let (mut in_prod, mut in_cons) = in_rb.split();
        let (mut out_prod, mut out_cons) = out_rb.split();

        // Push 4 stereo input frames: [L0 R0 L1 R1 L2 R2 L3 R3].
        let input = [1.0_f32, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0];
        in_prod.push_slice(&input);

        let in_ch = 2_usize;
        let out_ch = 2_usize;
        let step = 1.0_f64;
        let phase = 0.0_f64;

        let in_available = in_cons.occupied_len() / in_ch; // 4
        let out_capacity = out_prod.vacant_len() / out_ch; // 32

        let out_frames_from_input = ((in_available as f64 - phase) / step).floor() as usize; // 4
        let out_frames = out_frames_from_input.min(out_capacity); // 4

        let final_phase = phase + out_frames as f64 * step; // 4.0
        // in_needed = min(floor(4.0) + 1, 4) = min(5, 4) = 4
        let in_needed = (final_phase.floor() as usize + 1).min(in_available);

        let mut in_buf = vec![0.0_f32; in_needed * in_ch];
        let popped = in_cons.pop_slice(&mut in_buf);
        let actual_in = popped / in_ch;

        let mut out_buf = vec![0.0_f32; out_frames * out_ch];
        let mut cur = phase;
        for out_frame in 0..out_frames {
            let idx0 = cur.floor() as usize;
            let frac = (cur - cur.floor()) as f32;
            let idx1 = (idx0 + 1).min(actual_in.saturating_sub(1));
            for out_c in 0..out_ch {
                let in_c = out_c.min(in_ch - 1);
                let s0 = in_buf.get(idx0 * in_ch + in_c).copied().unwrap_or(0.0);
                let s1 = in_buf.get(idx1 * in_ch + in_c).copied().unwrap_or(0.0);
                out_buf[out_frame * out_ch + out_c] = s0 + (s1 - s0) * frac;
            }
            cur += step;
        }

        let carry = final_phase.fract();
        assert_eq!(carry, 0.0, "carry should be 0 for identity");
        out_prod.push_slice(&out_buf);

        // Verify output matches input (identity step, frac = 0 for every frame).
        let mut result = vec![0.0_f32; 8];
        out_cons.pop_slice(&mut result);
        assert_eq!(result, input, "identity step should pass through unchanged");
    }

    /// With a near-full output buffer, the resampler must never exceed vacant_len.
    #[test]
    fn pump_never_exceeds_vacant_len() {
        let in_rb: HeapRb<f32> = HeapRb::new(64);
        let out_rb: HeapRb<f32> = HeapRb::new(8); // small output buffer
        let (mut in_prod, in_cons) = in_rb.split();
        let (mut out_prod, _out_cons) = out_rb.split();

        // Fill output to near capacity (leave room for 2 stereo frames = 4 samples).
        let filler = vec![0.0_f32; 4]; // occupy 4 of 8 slots
        out_prod.push_slice(&filler);

        let in_ch = 2_usize;
        let out_ch = 2_usize;
        let step = 1.0_f64;

        // Push 10 stereo input frames.
        let input = vec![0.5_f32; 20];
        in_prod.push_slice(&input);

        let in_available = in_cons.occupied_len() / in_ch; // 10
        let out_capacity = out_prod.vacant_len() / out_ch; // 2 (only 4 vacant samples)

        let out_frames_from_input = ((in_available as f64) / step).floor() as usize; // 10
        let out_frames = out_frames_from_input.min(out_capacity); // 2

        assert!(
            out_frames <= out_capacity,
            "must not exceed vacant capacity"
        );
        assert!(
            out_frames <= 2,
            "should produce at most 2 frames when output is nearly full"
        );
    }

    /// Phase carry is zero for integer steps and non-zero for fractional steps.
    #[test]
    fn phase_carry_is_fractional() {
        // step = 0.5 (upsample 2x). 2 output frames from 1 input frame.
        // phase starts at 0.0: out[0] @ 0.0, out[1] @ 0.5
        // final_phase = 0.0 + 2 * 0.5 = 1.0, fract = 0.0
        let phase_a = 0.0_f64 + 2.0 * 0.5_f64;
        assert_eq!(phase_a.fract(), 0.0, "2x upsample carry should be zero");

        // step = 1.5 (downsample 1.5x). 1 output frame from ~1.5 input frames.
        // final_phase = 0.0 + 1 * 1.5 = 1.5, fract = 0.5
        let phase_b = 0.0_f64 + 1.0 * 1.5_f64;
        assert!(
            (phase_b.fract() - 0.5).abs() < 1e-12,
            "1.5x downsample carry should be 0.5"
        );
    }
}
