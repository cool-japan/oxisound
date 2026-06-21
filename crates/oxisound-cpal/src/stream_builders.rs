//! Typed stream builder functions for cpal output and input streams.
//!
//! These functions create cpal streams using the lock-free SPSC ring buffer pattern.

use cpal::traits::DeviceTrait;
use cpal::{FromSample, SizedSample};
use dasp_sample::{Sample as DaspSample, ToSample};
use oxisound_core::{ChannelRouting, OxiSoundError};
use ringbuf::{
    HeapCons, HeapProd,
    traits::{Consumer, Producer},
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

use crate::error::map_build_stream_err;

// ---------------------------------------------------------------------------
// Typed output stream builder (lock-free SPSC ring buffer)
// ---------------------------------------------------------------------------

#[expect(
    clippy::too_many_arguments,
    reason = "each Arc<AtomicU64> handle is a separate shared counter that cannot be bundled without introducing a new struct in the public API"
)]
#[cfg_attr(target_arch = "wasm32", allow(unused_variables))]
pub(crate) fn build_output_stream_typed<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    mut consumer: HeapCons<f32>,
    disconnected: Arc<AtomicBool>,
    underrun_count: Arc<AtomicU64>,
    frames_processed: Arc<AtomicU64>,
    channels: u16,
    routing: Option<ChannelRouting>,
    callback_duration_ns: Arc<AtomicU64>,
) -> Result<cpal::Stream, OxiSoundError>
where
    T: SizedSample + FromSample<f32> + Send + 'static,
{
    let disc_cb = Arc::clone(&disconnected);
    let uc = Arc::clone(&underrun_count);
    let fp = Arc::clone(&frames_processed);
    #[cfg(not(target_arch = "wasm32"))]
    let cb_dur = Arc::clone(&callback_duration_ns);
    // Channel count used in closure to convert sample count → frame count.
    let ch = channels.max(1) as usize;
    // Pre-allocate scratch buffer; captured by the closure so heap allocation
    // happens at most once (on the first callback invocation when the buffer
    // grows to `sample_count`). Subsequent calls with the same size are
    // allocation-free: `resize` is a no-op when `len == capacity`.
    let mut f32_buf: Vec<f32> = Vec::new();
    device
        .build_output_stream(
            *config,
            move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
                #[cfg(not(target_arch = "wasm32"))]
                let cb_start = std::time::Instant::now();
                let sample_count = data.len();
                if let Some(ref r) = routing {
                    // Populate a temporary f32 frame buffer, apply routing, then convert.
                    // Reuses the pre-captured allocation; no heap alloc after first call.
                    f32_buf.resize(sample_count, 0.0f32);
                    for s in f32_buf.iter_mut() {
                        *s = consumer.try_pop().unwrap_or_else(|| {
                            uc.fetch_add(1, Ordering::Relaxed);
                            0.0f32
                        });
                    }
                    r.apply_interleaved(&mut f32_buf, ch);
                    for (out, f) in data.iter_mut().zip(f32_buf.iter()) {
                        *out = T::from_sample(*f);
                    }
                } else {
                    for sample in data.iter_mut() {
                        if let Some(f) = consumer.try_pop() {
                            *sample = T::from_sample(f);
                        } else {
                            uc.fetch_add(1, Ordering::Relaxed);
                            *sample = T::from_sample(0.0f32);
                        }
                    }
                }
                if sample_count > 0 {
                    fp.fetch_add((sample_count / ch) as u64, Ordering::Relaxed);
                }
                #[cfg(not(target_arch = "wasm32"))]
                cb_dur.store(cb_start.elapsed().as_nanos() as u64, Ordering::Relaxed);
            },
            move |err| {
                use cpal::ErrorKind;
                match err.kind() {
                    ErrorKind::DeviceNotAvailable
                    | ErrorKind::StreamInvalidated
                    | ErrorKind::BackendError => {
                        disc_cb.store(true, Ordering::Relaxed);
                        log::error!("[oxisound-cpal] output stream error: {err}");
                    }
                    ErrorKind::Xrun => {
                        log::warn!("[oxisound-cpal] output buffer underrun reported by driver");
                    }
                    _ => {
                        log::error!("[oxisound-cpal] output stream error: {err}");
                    }
                }
            },
            None,
        )
        .map_err(map_build_stream_err)
}

// ---------------------------------------------------------------------------
// Typed input stream builder (lock-free SPSC ring buffer)
// ---------------------------------------------------------------------------

#[cfg_attr(target_arch = "wasm32", allow(unused_variables))]
pub(crate) fn build_input_stream_typed<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    mut producer: HeapProd<f32>,
    disconnected: Arc<AtomicBool>,
    frames_processed: Arc<AtomicU64>,
    channels: u16,
    callback_duration_ns: Arc<AtomicU64>,
) -> Result<cpal::Stream, OxiSoundError>
where
    T: SizedSample + DaspSample + ToSample<f32> + Send + 'static,
{
    let disc_cb = Arc::clone(&disconnected);
    let fp = Arc::clone(&frames_processed);
    #[cfg(not(target_arch = "wasm32"))]
    let cb_dur = Arc::clone(&callback_duration_ns);
    let ch = channels.max(1) as usize;
    device
        .build_input_stream(
            *config,
            move |data: &[T], _: &cpal::InputCallbackInfo| {
                #[cfg(not(target_arch = "wasm32"))]
                let cb_start = std::time::Instant::now();
                let sample_count = data.len();
                for &sample in data.iter() {
                    // Silently discard when ring buffer is full (capacity cap).
                    let _ = producer.try_push(sample.to_sample::<f32>());
                }
                if sample_count > 0 {
                    fp.fetch_add((sample_count / ch) as u64, Ordering::Relaxed);
                }
                #[cfg(not(target_arch = "wasm32"))]
                cb_dur.store(cb_start.elapsed().as_nanos() as u64, Ordering::Relaxed);
            },
            move |err| {
                use cpal::ErrorKind;
                match err.kind() {
                    ErrorKind::DeviceNotAvailable
                    | ErrorKind::StreamInvalidated
                    | ErrorKind::BackendError => {
                        disc_cb.store(true, Ordering::Relaxed);
                        log::error!("[oxisound-cpal] input stream error: {err}");
                    }
                    ErrorKind::Xrun => {
                        log::warn!("[oxisound-cpal] input buffer underrun reported by driver");
                    }
                    _ => {
                        log::error!("[oxisound-cpal] input stream error: {err}");
                    }
                }
            },
            None,
        )
        .map_err(map_build_stream_err)
}
