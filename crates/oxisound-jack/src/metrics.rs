//! Pure-Rust observability metrics for JACK streams.
//!
//! [`JackMetrics`] holds a set of atomic counters shared between the realtime
//! JACK process callback and main-thread readers. It is always compiled, even
//! without the `jack-backend` feature, so unit tests can exercise the logic
//! without libjack.

use std::sync::{
    Arc,
    atomic::{AtomicU32, AtomicU64, Ordering},
};

/// Snapshot of JACK stream metrics at a point in time.
#[derive(Debug, Clone)]
pub struct MetricsSnapshot {
    pub sample_rate: u32,
    pub xrun_count: u64,
    pub buffer_size: u32,
    pub latency_frames: u32,
}

/// Shared observability state for a JACK stream.
///
/// Create with [`JackMetrics::new`]; clone the [`Arc`] handles into the
/// realtime handler, and call [`snapshot`](JackMetrics::snapshot) from the
/// main thread.
#[derive(Clone, Debug)]
pub struct JackMetrics {
    pub(crate) sample_rate: Arc<AtomicU32>,
    pub(crate) xrun_count: Arc<AtomicU64>,
    pub(crate) buffer_size: Arc<AtomicU32>,
    pub(crate) latency_frames: Arc<AtomicU32>,
}

impl JackMetrics {
    pub fn new() -> Self {
        Self {
            sample_rate: Arc::new(AtomicU32::new(0)),
            xrun_count: Arc::new(AtomicU64::new(0)),
            buffer_size: Arc::new(AtomicU32::new(0)),
            latency_frames: Arc::new(AtomicU32::new(0)),
        }
    }

    pub fn record_sample_rate(&self, rate: u32) {
        self.sample_rate.store(rate, Ordering::Relaxed);
    }

    pub fn record_xrun(&self) {
        self.xrun_count.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_buffer_size(&self, size: u32) {
        self.buffer_size.store(size, Ordering::Relaxed);
    }

    pub fn record_latency(&self, frames: u32) {
        self.latency_frames.store(frames, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> MetricsSnapshot {
        MetricsSnapshot {
            sample_rate: self.sample_rate.load(Ordering::Relaxed),
            xrun_count: self.xrun_count.load(Ordering::Relaxed),
            buffer_size: self.buffer_size.load(Ordering::Relaxed),
            latency_frames: self.latency_frames.load(Ordering::Relaxed),
        }
    }
}

impl Default for JackMetrics {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_initial_zero() {
        let m = JackMetrics::new();
        let s = m.snapshot();
        assert_eq!(s.sample_rate, 0);
        assert_eq!(s.xrun_count, 0);
        assert_eq!(s.buffer_size, 0);
        assert_eq!(s.latency_frames, 0);
    }

    #[test]
    fn test_metrics_record_sample_rate() {
        let m = JackMetrics::new();
        m.record_sample_rate(48_000);
        assert_eq!(m.snapshot().sample_rate, 48_000);
    }

    #[test]
    fn test_metrics_record_xrun() {
        let m = JackMetrics::new();
        m.record_xrun();
        m.record_xrun();
        assert_eq!(m.snapshot().xrun_count, 2);
    }

    #[test]
    fn test_metrics_record_buffer_size() {
        let m = JackMetrics::new();
        m.record_buffer_size(512);
        assert_eq!(m.snapshot().buffer_size, 512);
    }

    #[test]
    fn test_metrics_record_latency() {
        let m = JackMetrics::new();
        m.record_latency(256);
        assert_eq!(m.snapshot().latency_frames, 256);
    }

    #[test]
    fn test_metrics_clone_shares_state() {
        let m1 = JackMetrics::new();
        let m2 = m1.clone();
        m1.record_sample_rate(44_100);
        assert_eq!(m2.snapshot().sample_rate, 44_100);
    }
}
