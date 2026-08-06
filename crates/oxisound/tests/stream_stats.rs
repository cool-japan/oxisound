//! Regression coverage for `oxisound::stream_stats`.
//!
//! `stream_stats` used to conflate "the backend exposes no stats" with "the stream is healthy
//! but hasn't processed anything yet": it returned `None` whenever every `StreamStats` field
//! happened to read zero, which is exactly the state of a freshly opened, perfectly healthy
//! stream. Since `OutputStream::stats()` is an infallible trait method (it defaults to
//! `StreamStats::default()`), there is no such thing as "stats truly unavailable" to
//! distinguish from — `stream_stats` must always report `Some`.
//!
//! These tests need no audio hardware: `FreshStream` is a minimal `OutputStream` that never
//! overrides `stats()`, so it always yields the all-zero default — precisely the case that used
//! to be misreported as `None`.

use oxisound::{OutputStream, OxiSoundError, StreamStats};

/// An `OutputStream` that accepts writes but never overrides `stats()`, so `stats()` always
/// returns `StreamStats::default()` (all fields zero) — the "freshly opened, healthy, idle
/// stream" case.
struct FreshStream;

impl OutputStream for FreshStream {
    fn write(&mut self, _samples: &[f32]) -> Result<(), OxiSoundError> {
        Ok(())
    }
    // `stats()` intentionally not overridden: inherits the trait default
    // (`StreamStats::default()`), matching a backend that hasn't recorded anything yet.
}

#[test]
fn stream_stats_is_some_for_an_idle_all_zero_stream() {
    let stream = FreshStream;
    let stats = oxisound::stream_stats(&stream);
    assert!(
        stats.is_some(),
        "a freshly opened, healthy stream must report Some(..) stats — infallible stats() \
         must never be hidden behind a heuristic None just because every field reads zero"
    );
}

#[test]
fn stream_stats_all_zero_fields_are_preserved_not_swallowed() {
    let stream = FreshStream;
    let stats = oxisound::stream_stats(&stream).expect("stream_stats must always return Some");
    assert_eq!(stats.frames_processed, 0);
    assert_eq!(stats.underruns, 0);
    assert_eq!(stats.overruns, 0);
    assert_eq!(stats.latency_frames, 0);
}

/// A stream that *has* processed real audio — the pre-existing, never-broken case — must keep
/// reporting `Some` too, so the fix doesn't accidentally special-case the all-zero path only.
struct ActiveStream;

impl OutputStream for ActiveStream {
    fn write(&mut self, _samples: &[f32]) -> Result<(), OxiSoundError> {
        Ok(())
    }

    fn stats(&self) -> StreamStats {
        StreamStats {
            frames_processed: 48_000,
            underruns: 1,
            overruns: 0,
            latency_frames: 256,
            cpu_load_percent: 3.5,
        }
    }
}

#[test]
fn stream_stats_is_some_for_an_active_stream() {
    let stream = ActiveStream;
    let stats = oxisound::stream_stats(&stream).expect("stream_stats must always return Some");
    assert_eq!(stats.frames_processed, 48_000);
    assert_eq!(stats.underruns, 1);
}
