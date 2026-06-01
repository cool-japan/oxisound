//! Write [`SmfFile`] to Standard MIDI File (SMF) bytes.
//!
//! # Examples
//!
//! ```no_run
//! let data = std::fs::read("input.mid").unwrap();
//! let smf = oxisound_smf::parse(&data).unwrap();
//! let out = oxisound_smf::write_smf(&smf).unwrap();
//! std::fs::write("output.mid", &out).unwrap();
//! ```

use alloc::vec::Vec;

use crate::{Division, SmfError, SmfEvent, SmfFile, SmfTrack};

/// Encode an [`SmfFile`] to SMF bytes suitable for writing to a `.mid` file.
///
/// The output is a fully self-contained byte buffer that can be handed to any
/// SMF-compliant parser (including [`crate::parse`]) without loss of information.
///
/// # Errors
///
/// Returns [`SmfError`] if any MIDI message produces an empty byte representation.
///
/// # Examples
///
/// ```no_run
/// let data = std::fs::read("input.mid").unwrap();
/// let smf = oxisound_smf::parse(&data).unwrap();
/// let out = oxisound_smf::write_smf(&smf).unwrap();
/// std::fs::write("output.mid", &out).unwrap();
/// ```
pub fn write(file: &SmfFile) -> Result<Vec<u8>, SmfError> {
    let mut out = Vec::new();

    // ---- MThd header (always 14 bytes) ----
    out.extend_from_slice(b"MThd");
    out.extend_from_slice(&6u32.to_be_bytes()); // chunk length always 6
    out.extend_from_slice(&(file.format as u16).to_be_bytes());
    out.extend_from_slice(&(file.tracks.len() as u16).to_be_bytes());
    match file.division {
        Division::TicksPerBeat(tpb) => out.extend_from_slice(&tpb.to_be_bytes()),
        Division::Smpte { fps, subframes } => {
            // High byte: set bit 15 (sign bit) + negated fps in lower 7 bits.
            // `fps` is stored as a positive u8 by the parser (it strips the sign bit).
            out.push((0u8.wrapping_sub(fps)) | 0x80);
            out.push(subframes);
        }
    }

    // ---- MTrk chunks ----
    for track in &file.tracks {
        let track_data = encode_track(track)?;
        out.extend_from_slice(b"MTrk");
        out.extend_from_slice(&(track_data.len() as u32).to_be_bytes());
        out.extend_from_slice(&track_data);
    }

    Ok(out)
}

fn encode_track(track: &SmfTrack) -> Result<Vec<u8>, SmfError> {
    let mut buf = Vec::new();
    let mut has_eot = false;

    for ev in &track.events {
        write_vlq(ev.delta_ticks, &mut buf);

        match &ev.event {
            SmfEvent::Midi(msg) => {
                let bytes = msg.to_bytes();
                if bytes.is_empty() {
                    return Err(SmfError("empty MidiMessage bytes".into()));
                }
                buf.extend_from_slice(&bytes);
            }
            SmfEvent::Tempo(us) => {
                buf.extend_from_slice(&[0xFF, 0x51, 0x03]);
                // Emit least-significant 3 bytes of the microsecond value.
                let be = us.to_be_bytes();
                buf.extend_from_slice(&be[1..4]);
            }
            SmfEvent::TimeSignature {
                numerator,
                denominator_pow2,
                clocks_per_click,
                notated_32nds_per_beat,
            } => {
                buf.extend_from_slice(&[
                    0xFF,
                    0x58,
                    0x04,
                    *numerator,
                    *denominator_pow2,
                    *clocks_per_click,
                    *notated_32nds_per_beat,
                ]);
            }
            SmfEvent::KeySignature {
                sharps_flats,
                is_minor,
            } => {
                buf.extend_from_slice(&[
                    0xFF,
                    0x59,
                    0x02,
                    (*sharps_flats) as u8,
                    u8::from(*is_minor),
                ]);
            }
            SmfEvent::TrackName(name) => {
                buf.extend_from_slice(&[0xFF, 0x03]);
                write_vlq(name.len() as u32, &mut buf);
                buf.extend_from_slice(name.as_bytes());
            }
            SmfEvent::EndOfTrack => {
                buf.extend_from_slice(&[0xFF, 0x2F, 0x00]);
                has_eot = true;
            }
            SmfEvent::UnknownMeta { meta_type, data } => {
                buf.push(0xFF);
                buf.push(*meta_type);
                write_vlq(data.len() as u32, &mut buf);
                buf.extend_from_slice(data);
            }
        }
    }

    // Spec requires every track to end with EndOfTrack.
    if !has_eot {
        // delta 0, then the meta event.
        buf.extend_from_slice(&[0x00, 0xFF, 0x2F, 0x00]);
    }

    Ok(buf)
}

/// Encode `val` as a MIDI variable-length quantity into `buf`.
///
/// VLQ uses 7 bits per byte, most-significant group first, with the high bit set
/// on all bytes except the last.
fn write_vlq(mut val: u32, buf: &mut Vec<u8>) {
    // Collect 7-bit groups, least significant first.
    let mut bytes = [0u8; 4];
    let mut len = 0usize;
    loop {
        bytes[len] = (val & 0x7F) as u8;
        len += 1;
        val >>= 7;
        if val == 0 {
            break;
        }
    }
    // Emit most-significant group first, with continuation bit on all but last.
    for i in (0..len).rev() {
        let byte = bytes[i] | if i > 0 { 0x80 } else { 0x00 };
        buf.push(byte);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Division, SmfFormat, parse};

    /// A minimal Format-0 SMF: 1 track, 480 ticks/beat, NoteOn + NoteOff + EndOfTrack.
    ///
    /// Track payload is 13 bytes:
    ///   0x00 0x90 0x3C 0x40   (delta=0,  NoteOn  ch0 C4 v64)   4 bytes
    ///   0x83 0x60 0x80 0x3C 0x00 (delta=480, NoteOff ch0 C4 v0) 5 bytes
    ///   0x00 0xFF 0x2F 0x00   (delta=0,  EndOfTrack)            4 bytes
    ///                                                     Total: 13 = 0x0D
    fn minimal_smf_bytes() -> Vec<u8> {
        vec![
            // MThd
            0x4D, 0x54, 0x68, 0x64, 0x00, 0x00, 0x00, 0x06, 0x00, 0x00, // format 0
            0x00, 0x01, // 1 track
            0x01, 0xE0, // 480 ticks/beat
            // MTrk
            0x4D, 0x54, 0x72, 0x6B, 0x00, 0x00, 0x00, 0x0D, // length = 13
            // events
            0x00, 0x90, 0x3C, 0x40, // delta=0, NoteOn ch0 C4 v64
            0x83, 0x60, 0x80, 0x3C, 0x00, // delta=480, NoteOff ch0 C4 v0
            0x00, 0xFF, 0x2F, 0x00, // delta=0, EndOfTrack
        ]
    }

    #[test]
    fn roundtrip_minimal_format0() {
        let smf_bytes = minimal_smf_bytes();
        let smf = parse(&smf_bytes).expect("parse failed");
        let written = write(&smf).expect("write failed");
        let reparsed = parse(&written).expect("reparse failed");
        assert_eq!(reparsed.format, smf.format);
        assert_eq!(reparsed.tracks.len(), smf.tracks.len());
        assert_eq!(
            reparsed.tracks[0].events.len(),
            smf.tracks[0].events.len(),
            "event count mismatch after round-trip"
        );
    }

    #[test]
    fn roundtrip_byte_exact() {
        // Writing and re-serialising a parsed file must produce identical bytes.
        let smf_bytes = minimal_smf_bytes();
        let smf = parse(&smf_bytes).expect("parse failed");
        let written = write(&smf).expect("write failed");
        assert_eq!(written, smf_bytes, "byte-exact round-trip failed");
    }

    #[test]
    fn write_vlq_boundaries() {
        let cases: &[(u32, &[u8])] = &[
            (0, &[0x00]),
            (127, &[0x7F]),
            (128, &[0x81, 0x00]),
            (16383, &[0xFF, 0x7F]),
            (16384, &[0x81, 0x80, 0x00]),
            (0x0FFF_FFFF, &[0xFF, 0xFF, 0xFF, 0x7F]),
        ];
        for (val, expected) in cases {
            let mut buf = Vec::new();
            write_vlq(*val, &mut buf);
            assert_eq!(&buf, expected, "VLQ({val})");
        }
    }

    #[test]
    fn write_empty_track_adds_eot() {
        use crate::{SmfFile, SmfTrack};
        let smf = SmfFile {
            format: SmfFormat::SingleTrack,
            division: Division::TicksPerBeat(480),
            tracks: alloc::vec![SmfTrack {
                name: None,
                events: alloc::vec![]
            }],
        };
        let written = write(&smf).expect("write failed");
        let reparsed = parse(&written).expect("reparse failed");
        // The auto-appended EndOfTrack is the only event in the track.
        assert_eq!(
            reparsed.tracks[0].events.len(),
            1,
            "expected exactly EndOfTrack"
        );
        assert!(
            matches!(
                reparsed.tracks[0].events[0].event,
                crate::SmfEvent::EndOfTrack
            ),
            "sole event should be EndOfTrack"
        );
    }

    #[test]
    fn write_tempo_meta() {
        use crate::{SmfFile, SmfTrack, TrackEvent};
        // 120 BPM = 500_000 µs/beat
        let smf = SmfFile {
            format: SmfFormat::SingleTrack,
            division: Division::TicksPerBeat(480),
            tracks: alloc::vec![SmfTrack {
                name: None,
                events: alloc::vec![
                    TrackEvent {
                        delta_ticks: 0,
                        event: crate::SmfEvent::Tempo(500_000)
                    },
                    TrackEvent {
                        delta_ticks: 0,
                        event: crate::SmfEvent::EndOfTrack
                    },
                ],
            }],
        };
        let written = write(&smf).expect("write failed");
        let reparsed = parse(&written).expect("reparse failed");
        match &reparsed.tracks[0].events[0].event {
            crate::SmfEvent::Tempo(us) => assert_eq!(*us, 500_000),
            other => panic!("expected Tempo, got {other:?}"),
        }
    }
}
