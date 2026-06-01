//! Tempo map: converts absolute tick positions to real time in seconds.
//!
//! Handles mid-track tempo changes by accumulating real-time intervals between
//! consecutive tempo entries.

use alloc::vec::Vec;

use crate::{Division, SmfEvent, SmfFile};

/// A single tempo change recorded in the tempo map.
#[derive(Debug, Clone)]
pub struct TempoEntry {
    /// Absolute tick position where this tempo takes effect.
    pub tick: u64,
    /// Tempo in microseconds per quarter note.
    pub tempo_us: u32,
}

/// Accumulated tempo map built from all tracks in an [`SmfFile`].
///
/// Only meaningful when [`Division`] is [`Division::TicksPerBeat`]; SMPTE
/// files use a fixed frame rate and [`TempoMap::tick_to_secs`] returns 0.0 for them.
#[derive(Debug, Clone)]
pub struct TempoMap {
    entries: Vec<TempoEntry>,
    ticks_per_beat: u16,
}

impl TempoMap {
    /// Build a tempo map by scanning all tracks for [`SmfEvent::Tempo`] events.
    ///
    /// Defaults to 120 BPM (500 000 µs/beat) when no tempo event is present —
    /// the MIDI spec mandates this default.
    pub fn from_file(file: &SmfFile) -> Self {
        let ticks_per_beat = match file.division {
            Division::TicksPerBeat(t) => t,
            Division::Smpte { .. } => {
                // Tempo map is unused for SMPTE — return an empty sentinel.
                return Self {
                    entries: Vec::new(),
                    ticks_per_beat: 0,
                };
            }
        };

        // Collect (absolute_tick, tempo_us) from every track.
        let mut raw: Vec<(u64, u32)> = Vec::new();
        for track in &file.tracks {
            let mut abs_tick: u64 = 0;
            for ev in &track.events {
                abs_tick += ev.delta_ticks as u64;
                if let SmfEvent::Tempo(us) = ev.event {
                    raw.push((abs_tick, us));
                }
            }
        }

        // Sort by tick position; for simultaneous changes keep the last one (stable sort).
        raw.sort_by_key(|(tick, _)| *tick);
        raw.dedup_by(|later, earlier| {
            if later.0 == earlier.0 {
                earlier.1 = later.1; // keep the later-sorted (higher-priority) value in `earlier`
                true // remove `later`
            } else {
                false
            }
        });

        // Always start with the MIDI default tempo unless the file sets one at tick 0.
        let mut entries: Vec<TempoEntry> = Vec::with_capacity(raw.len() + 1);
        if raw.first().is_none_or(|(tick, _)| *tick != 0) {
            entries.push(TempoEntry {
                tick: 0,
                tempo_us: 500_000,
            });
        }
        for (tick, tempo_us) in raw {
            entries.push(TempoEntry { tick, tempo_us });
        }

        Self {
            entries,
            ticks_per_beat,
        }
    }

    /// Convert an absolute tick position to wall-clock seconds from the start.
    ///
    /// Walks tempo segments, accumulating real time for each span.
    pub fn tick_to_secs(&self, tick: u64) -> f64 {
        if self.ticks_per_beat == 0 || self.entries.is_empty() {
            return 0.0;
        }
        let mut elapsed_secs = 0.0f64;
        let mut prev_tick: u64 = 0;
        let mut prev_tempo_us: u32 = 500_000;

        for entry in &self.entries {
            if entry.tick >= tick {
                break;
            }
            // Accumulate the segment from prev_tick to entry.tick.
            let span = entry.tick - prev_tick;
            elapsed_secs += ticks_to_seconds(span, prev_tempo_us, self.ticks_per_beat);
            prev_tick = entry.tick;
            prev_tempo_us = entry.tempo_us;
        }

        // Remaining ticks after the last tempo change up to `tick`.
        let span = tick - prev_tick;
        elapsed_secs += ticks_to_seconds(span, prev_tempo_us, self.ticks_per_beat);
        elapsed_secs
    }
}

/// Convert a tick duration to seconds given a fixed tempo (convenience function).
///
/// # Arguments
/// - `ticks`: duration in ticks
/// - `tempo_us`: microseconds per quarter note
/// - `ticks_per_beat`: ticks per quarter note from the SMF header
#[inline]
pub fn ticks_to_seconds(ticks: u64, tempo_us: u32, ticks_per_beat: u16) -> f64 {
    if ticks_per_beat == 0 {
        return 0.0;
    }
    ticks as f64 * tempo_us as f64 / (ticks_per_beat as f64 * 1_000_000.0)
}

// ---------- Tests ----------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Division, SmfEvent, SmfFile, SmfFormat, SmfTrack, TrackEvent};

    fn make_smf_with_tempos(ticks_per_beat: u16, tempos: &[(u32, u32)]) -> SmfFile {
        // Each tuple: (delta_ticks, tempo_us).
        let events: Vec<TrackEvent> = tempos
            .iter()
            .map(|(delta, tempo)| TrackEvent {
                delta_ticks: *delta,
                event: SmfEvent::Tempo(*tempo),
            })
            .chain(core::iter::once(TrackEvent {
                delta_ticks: 0,
                event: SmfEvent::EndOfTrack,
            }))
            .collect();

        SmfFile {
            format: SmfFormat::SingleTrack,
            division: Division::TicksPerBeat(ticks_per_beat),
            tracks: vec![SmfTrack { name: None, events }],
        }
    }

    #[test]
    fn test_ticks_to_seconds_basic() {
        // 500 000 µs/beat, 480 ticks/beat → 1 beat = 0.5 s.
        let secs = ticks_to_seconds(480, 500_000, 480);
        let diff = (secs - 0.5).abs();
        assert!(diff < 1e-9, "expected 0.5 s, got {}", secs);
    }

    #[test]
    fn test_tempo_map_default_120bpm() {
        // No explicit tempo → defaults to 500 000 µs/beat.
        let file = make_smf_with_tempos(480, &[]);
        let map = TempoMap::from_file(&file);
        // 480 ticks = 1 beat = 0.5 s.
        let secs = map.tick_to_secs(480);
        let diff = (secs - 0.5).abs();
        assert!(diff < 1e-9, "expected 0.5 s, got {}", secs);
    }

    #[test]
    fn test_tempo_map_single_change() {
        // At tick 0: 500 000 µs/beat (120 BPM).
        // At tick 480: change to 1 000 000 µs/beat (60 BPM).
        // After 480 more ticks (960 total) = 0.5 + 1.0 = 1.5 s.
        let file = make_smf_with_tempos(480, &[(0, 500_000), (480, 1_000_000)]);
        let map = TempoMap::from_file(&file);

        let at_beat1 = map.tick_to_secs(480);
        let diff1 = (at_beat1 - 0.5).abs();
        assert!(diff1 < 1e-9, "at beat 1 expected 0.5 s, got {}", at_beat1);

        let at_beat2 = map.tick_to_secs(960);
        let diff2 = (at_beat2 - 1.5).abs();
        assert!(diff2 < 1e-9, "at beat 2 expected 1.5 s, got {}", at_beat2);
    }

    #[test]
    fn test_tempo_map_tick_zero() {
        let file = make_smf_with_tempos(480, &[]);
        let map = TempoMap::from_file(&file);
        assert_eq!(map.tick_to_secs(0), 0.0);
    }

    #[test]
    fn test_ticks_to_seconds_zero_tpb() {
        // Guard against division-by-zero.
        assert_eq!(ticks_to_seconds(100, 500_000, 0), 0.0);
    }
}
