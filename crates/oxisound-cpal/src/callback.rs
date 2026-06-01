//! Callback-based (zero-copy) audio streams for output and input.

use cpal::traits::StreamTrait;
use oxisound_core::OxiSoundError;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

// ---------------------------------------------------------------------------
// CpalCallbackOutputStream (zero-copy, no ring buffer)
// ---------------------------------------------------------------------------

/// A zero-copy output stream backed by a user-provided callback.
///
/// The callback is invoked on the real-time audio thread with a mutable slice of
/// `f32` samples to fill. It bypasses the ring buffer for minimal latency.
pub struct CpalCallbackOutputStream {
    pub(crate) stream: cpal::Stream,
    pub(crate) disconnect: Arc<AtomicBool>,
    pub(crate) last_callback_nanos: Arc<AtomicU64>,
}

impl CpalCallbackOutputStream {
    /// Returns the duration of the most recent callback invocation, if available.
    pub fn last_callback_duration(&self) -> Option<std::time::Duration> {
        let nanos = self.last_callback_nanos.load(Ordering::Relaxed);
        if nanos == 0 {
            None
        } else {
            Some(std::time::Duration::from_nanos(nanos))
        }
    }

    /// Returns `true` if the underlying device has been reported as disconnected.
    pub fn is_disconnected(&self) -> bool {
        self.disconnect.load(Ordering::Relaxed)
    }

    /// Pauses the callback output stream.
    pub fn pause(&self) -> Result<(), OxiSoundError> {
        self.stream
            .pause()
            .map_err(|e| OxiSoundError::Stream(e.to_string()))
    }

    /// Resumes the callback output stream after a pause.
    pub fn resume(&self) -> Result<(), OxiSoundError> {
        self.stream
            .play()
            .map_err(|e| OxiSoundError::Stream(e.to_string()))
    }
}

// ---------------------------------------------------------------------------
// CpalCallbackInputStream (zero-copy, no ring buffer)
// ---------------------------------------------------------------------------

/// A zero-copy input stream backed by a user-provided callback.
///
/// The callback is invoked on the real-time audio thread with a slice of
/// `f32` samples captured from the device.
pub struct CpalCallbackInputStream {
    pub(crate) stream: cpal::Stream,
    pub(crate) disconnect: Arc<AtomicBool>,
}

impl CpalCallbackInputStream {
    /// Returns `true` if the underlying device has been reported as disconnected.
    pub fn is_disconnected(&self) -> bool {
        self.disconnect.load(Ordering::Relaxed)
    }

    /// Pauses the callback input stream.
    pub fn pause(&self) -> Result<(), OxiSoundError> {
        self.stream
            .pause()
            .map_err(|e| OxiSoundError::Stream(e.to_string()))
    }

    /// Resumes the callback input stream after a pause.
    pub fn resume(&self) -> Result<(), OxiSoundError> {
        self.stream
            .play()
            .map_err(|e| OxiSoundError::Stream(e.to_string()))
    }
}
