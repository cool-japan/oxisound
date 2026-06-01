//! Polyphonic sine-wave synthesizer driven by a Standard MIDI File.
//!
//! Parses an SMF file with oxisound-smf, converts each NoteOn/NoteOff event into
//! sine oscillator start/stop actions, renders the result to a Vec<f32>, and plays
//! it through the default output device.
//!
//! NoteOn with velocity 0 is treated as NoteOff (standard MIDI running-status convention).
//!
//! Usage: cargo run --example smf_synth --features smf -- <file.mid>
//! Without a file argument: plays a built-in polyphonic demo sequence.

use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy)]
enum MidiNoteEvent {
    On { note: u8, _vel: u8 },
    Off { note: u8 },
}

fn midi_note_to_freq(note: u8) -> f32 {
    440.0 * 2.0f32.powf((note as f32 - 69.0) / 12.0)
}

fn render(
    timed_events: Vec<(f64, MidiNoteEvent)>,
    duration_secs: f64,
    sample_rate: u32,
    channels: u16,
) -> Vec<f32> {
    let n_samples = (duration_secs * sample_rate as f64).ceil() as usize;
    let ch = channels as usize;
    let mut out = vec![0.0f32; n_samples * ch];

    // Pre-compute sample index for each event and sort (events must already be sorted by time).
    struct TimedEvent {
        sample: usize,
        event: MidiNoteEvent,
    }
    let indexed: Vec<TimedEvent> = timed_events
        .into_iter()
        .map(|(t, ev)| TimedEvent {
            sample: (t * sample_rate as f64).round() as usize,
            event: ev,
        })
        .collect();

    // note → (freq, phase_cycles)
    let mut active: BTreeMap<u8, (f32, f32)> = BTreeMap::new();
    let mut ev_idx = 0usize;

    for i in 0..n_samples {
        // Apply all events whose sample index falls at or before this sample.
        while ev_idx < indexed.len() && indexed[ev_idx].sample <= i {
            match indexed[ev_idx].event {
                MidiNoteEvent::On { note, .. } => {
                    let freq = midi_note_to_freq(note);
                    active.insert(note, (freq, 0.0));
                }
                MidiNoteEvent::Off { note } => {
                    active.remove(&note);
                }
            }
            ev_idx += 1;
        }

        let mut sample = 0.0f32;
        for (freq, phase) in active.values_mut() {
            // 0.2 gain per voice — up to ~5 simultaneous notes at full scale
            sample += (2.0 * std::f32::consts::PI * *phase).sin() * 0.2;
            *phase += *freq / sample_rate as f32;
            // Phase wraps in cycles; >= handles exact integer
            while *phase >= 1.0 {
                *phase -= 1.0;
            }
        }
        let s = sample.clamp(-1.0, 1.0);
        for c in 0..ch {
            out[i * ch + c] = s;
        }
    }

    out
}

fn collect_smf_events(data: &[u8]) -> Result<Vec<(f64, MidiNoteEvent)>, String> {
    let smf = oxisound::parse_smf(data).map_err(|e| e.to_string())?;
    let player = oxisound::SmfPlayer::new(smf);

    let mut events: Vec<(f64, MidiNoteEvent)> = player
        .midi_events()
        .filter_map(|(t, msg)| {
            let status_type = msg.status & 0xF0;
            if msg.data.len() < 2 {
                return None;
            }
            let note = msg.data[0];
            let vel = msg.data[1];

            match status_type {
                0x90 if vel > 0 => Some((t, MidiNoteEvent::On { note, _vel: vel })),
                // NoteOn with vel=0 is NoteOff (running-status convention)
                0x90 | 0x80 => Some((t, MidiNoteEvent::Off { note })),
                _ => None,
            }
        })
        .collect();

    // Events from SmfPlayer::midi_events() are already sorted by time.
    events.sort_by(|(a, _), (b, _)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    Ok(events)
}

fn demo_events() -> Vec<(f64, MidiNoteEvent)> {
    // Polyphonic demo: C major triad arpeggiated then held together.
    // Beat 1 (0.0–0.4s): C4 solo
    // Beat 2 (0.5–0.9s): E4 joins C4 (two notes overlap)
    // Beat 3 (1.0–1.4s): G4 joins C4+E4 (full triad)
    // Beat 4 (1.5–2.5s): full C-E-G-C5 chord held together (4-voice polyphony)
    vec![
        (0.0, MidiNoteEvent::On { note: 60, _vel: 80 }), // C4 on
        (0.5, MidiNoteEvent::On { note: 64, _vel: 80 }), // E4 on (C+E overlap)
        (1.0, MidiNoteEvent::On { note: 67, _vel: 80 }), // G4 on (C+E+G overlap)
        (1.5, MidiNoteEvent::On { note: 72, _vel: 80 }), // C5 on (C+E+G+C5 chord)
        (2.5, MidiNoteEvent::Off { note: 60 }),
        (2.5, MidiNoteEvent::Off { note: 64 }),
        (2.5, MidiNoteEvent::Off { note: 67 }),
        (2.5, MidiNoteEvent::Off { note: 72 }),
    ]
}

fn main() {
    let args: Vec<String> = std::env::args().collect();

    let config = oxisound::StreamConfig::stereo_48k();
    let sr = config.sample_rate;
    let ch = config.channels;

    let timed_events: Vec<(f64, MidiNoteEvent)> = if args.len() > 1 {
        let path = &args[1];
        let data = match std::fs::read(path) {
            Ok(d) => d,
            Err(e) => {
                println!("Failed to read {path}: {e}");
                return;
            }
        };
        match collect_smf_events(&data) {
            Ok(evs) => {
                println!("Parsed SMF: {} note events from {path}", evs.len());
                evs
            }
            Err(e) => {
                println!("SMF parse error: {e}");
                return;
            }
        }
    } else {
        println!("No file argument. Playing built-in polyphonic C-major demo.");
        println!("Usage: cargo run --example smf_synth --features smf -- <file.mid>");
        demo_events()
    };

    let duration = timed_events.last().map(|(t, _)| t + 1.0).unwrap_or(3.0);

    println!("Rendering {duration:.1}s of polyphonic audio...");

    let samples = render(timed_events, duration, sr, ch);

    let mut output = match oxisound::open_output(config) {
        Ok(o) => o,
        Err(e) => {
            println!("Failed to open output: {e}");
            return;
        }
    };

    if let Err(e) = output.write(&samples) {
        println!("Write failed: {e}");
        return;
    }

    std::thread::sleep(std::time::Duration::from_secs_f64(duration + 0.5));
    println!("Done.");
}
