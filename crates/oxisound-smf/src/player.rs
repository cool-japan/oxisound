//! SMF playback: merges all tracks into a single chronological event stream.

use alloc::vec::Vec;

use crate::{SmfEvent, SmfFile, timing::TempoMap};

/// Merges all tracks from an [`SmfFile`] and provides a chronologically sorted
/// event stream with wall-clock timestamps.
pub struct SmfPlayer {
    events: Vec<(f64, SmfEvent)>,
}

impl SmfPlayer {
    /// Build a player from a parsed [`SmfFile`].
    ///
    /// Constructs a [`TempoMap`], computes absolute seconds for every event
    /// across all tracks, then sorts by time.
    pub fn new(file: SmfFile) -> Self {
        let map = TempoMap::from_file(&file);
        let mut events: Vec<(f64, SmfEvent)> = Vec::new();

        for track in &file.tracks {
            let mut abs_tick: u64 = 0;
            for ev in &track.events {
                abs_tick += ev.delta_ticks as u64;
                let t = map.tick_to_secs(abs_tick);
                events.push((t, ev.event.clone()));
            }
        }

        // Stable sort preserves relative ordering within the same tick.
        events.sort_by(|(a, _), (b, _)| a.partial_cmp(b).unwrap_or(core::cmp::Ordering::Equal));

        Self { events }
    }

    /// Iterate all events in chronological order, each paired with its
    /// wall-clock start time in seconds from the beginning of the file.
    pub fn events(&self) -> impl Iterator<Item = &(f64, SmfEvent)> {
        self.events.iter()
    }

    /// Iterate only MIDI channel/SysEx events, filtering out all meta events.
    pub fn midi_events(&self) -> impl Iterator<Item = (f64, &oxisound_core::MidiMessage)> {
        self.events.iter().filter_map(|(t, ev)| {
            if let SmfEvent::Midi(msg) = ev {
                Some((*t, msg))
            } else {
                None
            }
        })
    }

    /// Blocking playback: sends each MIDI message to `output` in real time,
    /// sleeping between events according to their timestamps.
    ///
    /// Available only with the `std` feature (requires `thread::sleep`).
    #[cfg(feature = "std")]
    pub fn play(
        &self,
        output: &mut dyn oxisound_core::MidiOutput,
    ) -> Result<(), oxisound_core::OxiSoundError> {
        let mut last_time = 0.0f64;
        for (t, ev) in &self.events {
            let delay = t - last_time;
            if delay > 0.001 {
                std::thread::sleep(std::time::Duration::from_secs_f64(delay));
            }
            if let SmfEvent::Midi(msg) = ev {
                output.send(msg)?;
            }
            last_time = *t;
        }
        Ok(())
    }
}

// ---------- Tests ----------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Division, SmfEvent, SmfFile, SmfFormat, SmfTrack, TrackEvent};
    use oxisound_core::MidiMessage;

    fn note_on(delta: u32, note: u8, vel: u8) -> TrackEvent {
        TrackEvent {
            delta_ticks: delta,
            event: SmfEvent::Midi(MidiMessage {
                status: 0x90,
                data: alloc::vec![note, vel],
                timestamp_micros: 0,
            }),
        }
    }

    fn eot() -> TrackEvent {
        TrackEvent {
            delta_ticks: 0,
            event: SmfEvent::EndOfTrack,
        }
    }

    fn simple_file(ticks_per_beat: u16, track_events: Vec<TrackEvent>) -> SmfFile {
        SmfFile {
            format: SmfFormat::SingleTrack,
            division: Division::TicksPerBeat(ticks_per_beat),
            tracks: vec![SmfTrack {
                name: None,
                events: track_events,
            }],
        }
    }

    #[test]
    fn test_midi_events_filter() {
        let file = simple_file(
            480,
            vec![
                note_on(0, 60, 64),
                TrackEvent {
                    delta_ticks: 480,
                    event: SmfEvent::Tempo(500_000),
                },
                note_on(0, 62, 80),
                eot(),
            ],
        );
        let player = SmfPlayer::new(file);
        let midi: Vec<_> = player.midi_events().collect();
        // Should contain 2 NoteOn events, not the Tempo meta or EndOfTrack.
        assert_eq!(midi.len(), 2);
        assert_eq!(midi[0].1.status, 0x90);
        assert_eq!(midi[0].1.data[0], 60);
        assert_eq!(midi[1].1.data[0], 62);
    }

    #[test]
    fn test_events_sorted_by_time() {
        // Two tracks: track A has events at ticks 0 and 960;
        // track B has events at ticks 480 and 1440.
        let track_a = SmfTrack {
            name: None,
            events: vec![note_on(0, 60, 64), note_on(960, 62, 64), eot()],
        };
        let track_b = SmfTrack {
            name: None,
            events: vec![note_on(480, 64, 64), note_on(960, 65, 64), eot()],
        };
        let file = SmfFile {
            format: SmfFormat::MultiTrack,
            division: Division::TicksPerBeat(480),
            tracks: vec![track_a, track_b],
        };
        let player = SmfPlayer::new(file);
        let times: Vec<f64> = player.midi_events().map(|(t, _)| t).collect();
        // Times should be non-decreasing.
        for pair in times.windows(2) {
            assert!(pair[0] <= pair[1], "events out of order: {:?}", times);
        }
        assert_eq!(times.len(), 4);
    }

    #[test]
    fn test_events_timing_values() {
        // At 120 BPM (500 000 µs/beat, 480 ticks/beat):
        //   tick 0   → 0.0 s
        //   tick 480 → 0.5 s
        let file = simple_file(480, vec![note_on(0, 60, 64), note_on(480, 62, 64), eot()]);
        let player = SmfPlayer::new(file);
        let midi: Vec<_> = player.midi_events().collect();
        assert_eq!(midi.len(), 2);
        let diff0 = midi[0].0.abs();
        assert!(
            diff0 < 1e-9,
            "first event should be at t=0, got {}",
            midi[0].0
        );
        let diff1 = (midi[1].0 - 0.5).abs();
        assert!(
            diff1 < 1e-9,
            "second event should be at t=0.5, got {}",
            midi[1].0
        );
    }
}
