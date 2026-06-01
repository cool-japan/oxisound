//! SMF byte-stream parser.
//!
//! Parses the chunk structure (MThd → MTrk*), variable-length quantities,
//! running status, meta events, and SysEx.

use alloc::{string::String, vec::Vec};
use oxisound_core::MidiMessage;

use crate::{Division, SmfError, SmfEvent, SmfFile, SmfFormat, SmfTrack, TrackEvent};

/// Parse a Standard MIDI File from a raw byte slice.
///
/// Returns [`SmfError`] on any structural or encoding violation.
pub fn parse(data: &[u8]) -> Result<SmfFile, SmfError> {
    let mut pos = 0usize;

    // --- MThd header ---
    let format = parse_mthd(data, &mut pos)?;

    // --- MTrk chunks ---
    let (smf_format, num_tracks, division) = format;
    let mut tracks = Vec::with_capacity(num_tracks as usize);
    for _ in 0..num_tracks {
        tracks.push(parse_mtrk(data, &mut pos)?);
    }

    Ok(SmfFile {
        format: smf_format,
        division,
        tracks,
    })
}

// Returns (format, num_tracks, division).
fn parse_mthd(data: &[u8], pos: &mut usize) -> Result<(SmfFormat, u16, Division), SmfError> {
    require_bytes(data, *pos, 14, "MThd chunk")?;

    if &data[*pos..*pos + 4] != b"MThd" {
        return Err(SmfError("missing MThd magic".into()));
    }
    *pos += 4;

    let length = read_u32_be(data, pos)?;
    if length != 6 {
        return Err(SmfError(alloc::format!(
            "MThd length must be 6, got {}",
            length
        )));
    }

    let format_word = read_u16_be(data, pos)?;
    let smf_format = match format_word {
        0 => SmfFormat::SingleTrack,
        1 => SmfFormat::MultiTrack,
        2 => SmfFormat::MultiSong,
        other => return Err(SmfError(alloc::format!("unknown SMF format {}", other))),
    };

    let num_tracks = read_u16_be(data, pos)?;
    let division_word = read_u16_be(data, pos)?;
    let division = if division_word & 0x8000 == 0 {
        Division::TicksPerBeat(division_word)
    } else {
        // High byte is negated fps (bits 14-8), low byte is subframes.
        let fps = ((division_word >> 8) & 0x7F) as u8;
        let subframes = (division_word & 0xFF) as u8;
        Division::Smpte { fps, subframes }
    };

    Ok((smf_format, num_tracks, division))
}

fn parse_mtrk(data: &[u8], pos: &mut usize) -> Result<SmfTrack, SmfError> {
    require_bytes(data, *pos, 8, "MTrk header")?;

    if &data[*pos..*pos + 4] != b"MTrk" {
        return Err(SmfError("missing MTrk magic".into()));
    }
    *pos += 4;

    let chunk_len = read_u32_be(data, pos)? as usize;
    if *pos + chunk_len > data.len() {
        return Err(SmfError("MTrk chunk extends past end of data".into()));
    }

    // Operate within a byte budget so truncated/no-EOT tracks still terminate.
    let track_end = *pos + chunk_len;
    let mut events = Vec::new();
    let mut track_name: Option<String> = None;
    let mut running_status: Option<u8> = None;

    while *pos < track_end {
        let delta_ticks = read_vlq(data, pos)?;

        if *pos >= track_end {
            return Err(SmfError("truncated event after delta time".into()));
        }

        let (event, new_status) = parse_event(data, pos, track_end, running_status)?;

        // Meta events do not update running status.
        if let Some(s) = new_status {
            running_status = Some(s);
        }

        if let SmfEvent::TrackName(ref name) = event {
            track_name = Some(name.clone());
        }
        let is_eot = matches!(event, SmfEvent::EndOfTrack);
        events.push(TrackEvent { delta_ticks, event });
        if is_eot {
            break;
        }
    }

    // Advance pos to the exact end of the chunk regardless of where parsing stopped.
    *pos = track_end;

    Ok(SmfTrack {
        name: track_name,
        events,
    })
}

/// Parse a single track event (not including the delta time).
///
/// Returns the event and, for MIDI channel events, the status byte to propagate
/// as running status (None for meta/sysex events which reset nothing).
fn parse_event(
    data: &[u8],
    pos: &mut usize,
    track_end: usize,
    running_status: Option<u8>,
) -> Result<(SmfEvent, Option<u8>), SmfError> {
    let first_byte = read_byte(data, pos)?;

    if first_byte == 0xFF {
        // Meta event — never updates running status.
        let meta_type = read_byte(data, pos)?;
        let meta_len = read_vlq(data, pos)? as usize;
        if *pos + meta_len > track_end {
            return Err(SmfError("meta event data extends past track end".into()));
        }
        let meta_data = data[*pos..*pos + meta_len].to_vec();
        *pos += meta_len;
        let event = decode_meta(meta_type, meta_data)?;
        return Ok((event, None));
    }

    if first_byte == 0xF0 || first_byte == 0xF7 {
        // SysEx — read VLQ length then payload.
        let sysex_len = read_vlq(data, pos)? as usize;
        if *pos + sysex_len > track_end {
            return Err(SmfError("SysEx data extends past track end".into()));
        }
        let payload = data[*pos..*pos + sysex_len].to_vec();
        *pos += sysex_len;
        let msg = if first_byte == 0xF0 {
            MidiMessage::new_sysex(&payload)
        } else {
            // 0xF7 continuation — emit as raw bytes with status 0xF7.
            MidiMessage {
                status: 0xF7,
                data: payload,
                timestamp_micros: 0,
            }
        };
        return Ok((SmfEvent::Midi(msg), None));
    }

    // MIDI channel event (possibly with running status).
    let (status, first_data_byte) = if first_byte & 0x80 != 0 {
        // Explicit status byte.
        (first_byte, None)
    } else {
        // Running status — first_byte is actually the first data byte.
        let s = running_status
            .ok_or_else(|| SmfError("running status used before any status byte".into()))?;
        (s, Some(first_byte))
    };

    let msg_type = status & 0xF0;
    let msg = build_channel_message(data, pos, status, msg_type, first_data_byte, track_end)?;
    Ok((SmfEvent::Midi(msg), Some(status)))
}

fn build_channel_message(
    data: &[u8],
    pos: &mut usize,
    status: u8,
    msg_type: u8,
    first_data_byte: Option<u8>,
    track_end: usize,
) -> Result<MidiMessage, SmfError> {
    let read_data_byte = |data: &[u8], pos: &mut usize| -> Result<u8, SmfError> {
        if *pos >= track_end {
            return Err(SmfError("truncated MIDI data byte".into()));
        }
        read_byte(data, pos)
    };

    let b1 = match first_data_byte {
        Some(b) => b,
        None => read_data_byte(data, pos)?,
    };

    let midi_data = match msg_type {
        // Two-data-byte messages: note off, note on, poly pressure, CC, pitch bend.
        0x80 | 0x90 | 0xA0 | 0xB0 | 0xE0 => {
            let b2 = read_data_byte(data, pos)?;
            alloc::vec![b1, b2]
        }
        // One-data-byte messages: program change, channel pressure.
        0xC0 | 0xD0 => alloc::vec![b1],
        other => {
            return Err(SmfError(alloc::format!(
                "unrecognised MIDI message type 0x{:02X}",
                other
            )));
        }
    };

    Ok(MidiMessage {
        status,
        data: midi_data,
        timestamp_micros: 0,
    })
}

fn decode_meta(meta_type: u8, data: Vec<u8>) -> Result<SmfEvent, SmfError> {
    match meta_type {
        // Track name (0x03).
        0x03 => {
            let name = core::str::from_utf8(&data)
                .map(|s| s.into())
                .unwrap_or_else(|_| String::from("<invalid UTF-8>"));
            Ok(SmfEvent::TrackName(name))
        }
        // Tempo (0x51): 3 bytes, microseconds per quarter note.
        0x51 => {
            if data.len() < 3 {
                return Err(SmfError("tempo meta event too short".into()));
            }
            let us = (data[0] as u32) << 16 | (data[1] as u32) << 8 | data[2] as u32;
            Ok(SmfEvent::Tempo(us))
        }
        // Time signature (0x58): 4 bytes.
        0x58 => {
            if data.len() < 4 {
                return Err(SmfError("time signature meta event too short".into()));
            }
            Ok(SmfEvent::TimeSignature {
                numerator: data[0],
                denominator_pow2: data[1],
                clocks_per_click: data[2],
                notated_32nds_per_beat: data[3],
            })
        }
        // Key signature (0x59): 2 bytes (sharps_flats as signed, major/minor).
        0x59 => {
            if data.len() < 2 {
                return Err(SmfError("key signature meta event too short".into()));
            }
            Ok(SmfEvent::KeySignature {
                sharps_flats: data[0] as i8,
                is_minor: data[1] != 0,
            })
        }
        // End of track (0x2F): no data.
        0x2F => Ok(SmfEvent::EndOfTrack),
        _ => Ok(SmfEvent::UnknownMeta { meta_type, data }),
    }
}

// ---------- Low-level helpers ----------

/// Read a variable-length quantity (VLQ) from the byte stream.
///
/// VLQ encodes values up to 28 bits in 1–4 bytes using the MSB as the
/// continuation flag. Enforces the 4-byte limit.
pub(crate) fn read_vlq(data: &[u8], pos: &mut usize) -> Result<u32, SmfError> {
    let mut val: u32 = 0;
    for _ in 0..4 {
        let b = data
            .get(*pos)
            .ok_or_else(|| SmfError("unexpected EOF in VLQ".into()))?;
        *pos += 1;
        val = (val << 7) | ((*b & 0x7F) as u32);
        if *b & 0x80 == 0 {
            return Ok(val);
        }
    }
    Err(SmfError("VLQ too long (exceeds 4 bytes)".into()))
}

fn read_byte(data: &[u8], pos: &mut usize) -> Result<u8, SmfError> {
    let b = data
        .get(*pos)
        .ok_or_else(|| SmfError("unexpected EOF".into()))?;
    *pos += 1;
    Ok(*b)
}

fn read_u16_be(data: &[u8], pos: &mut usize) -> Result<u16, SmfError> {
    require_bytes(data, *pos, 2, "u16")?;
    let val = u16::from_be_bytes([data[*pos], data[*pos + 1]]);
    *pos += 2;
    Ok(val)
}

fn read_u32_be(data: &[u8], pos: &mut usize) -> Result<u32, SmfError> {
    require_bytes(data, *pos, 4, "u32")?;
    let val = u32::from_be_bytes([data[*pos], data[*pos + 1], data[*pos + 2], data[*pos + 3]]);
    *pos += 4;
    Ok(val)
}

fn require_bytes(data: &[u8], pos: usize, n: usize, ctx: &str) -> Result<(), SmfError> {
    if pos + n > data.len() {
        Err(SmfError(alloc::format!(
            "need {} bytes for {}, only {} available",
            n,
            ctx,
            data.len().saturating_sub(pos)
        )))
    } else {
        Ok(())
    }
}

// ---------- Tests ----------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Division, SmfEvent, SmfFormat};

    // 480 ticks/beat = 0x01E0.
    // Track payload:
    //   00 90 3C 40          → delta=0, NoteOn ch0 n60 v64           (4 bytes)
    //   83 60 80 3C 00       → delta=480 (VLQ 0x83,0x60), NoteOff ch0 n60 v0 (5 bytes)
    //   00 FF 2F 00          → delta=0, EndOfTrack                   (4 bytes)
    // Total payload = 13 = 0x0D
    fn minimal_smf_bytes() -> Vec<u8> {
        vec![
            // MThd
            0x4D, 0x54, 0x68, 0x64, // 'MThd'
            0x00, 0x00, 0x00, 0x06, // length = 6
            0x00, 0x00, // format 0
            0x00, 0x01, // 1 track
            0x01, 0xE0, // 480 ticks/beat
            // MTrk
            0x4D, 0x54, 0x72, 0x6B, // 'MTrk'
            0x00, 0x00, 0x00, 0x0D, // length = 13
            // Events
            0x00, 0x90, 0x3C, 0x40, // delta=0, NoteOn ch0 n60 v64
            0x83, 0x60, 0x80, 0x3C, 0x00, // delta=480 VLQ, NoteOff ch0 n60 v0
            0x00, 0xFF, 0x2F, 0x00, // delta=0, EndOfTrack
        ]
    }

    #[test]
    fn test_parse_minimal_format0() {
        let data = minimal_smf_bytes();
        let file = parse(&data).expect("parse should succeed");

        assert_eq!(file.format, SmfFormat::SingleTrack);
        assert_eq!(file.division, Division::TicksPerBeat(480));
        assert_eq!(file.tracks.len(), 1);

        let track = &file.tracks[0];
        // 3 events: NoteOn, NoteOff, EndOfTrack.
        assert_eq!(track.events.len(), 3);

        // First event: delta=0, NoteOn ch0 n60 v64.
        let ev0 = &track.events[0];
        assert_eq!(ev0.delta_ticks, 0);
        if let SmfEvent::Midi(ref msg) = ev0.event {
            assert_eq!(msg.status, 0x90);
            assert_eq!(msg.data, &[0x3C, 0x40]);
        } else {
            panic!("expected Midi event, got {:?}", ev0.event);
        }

        // Second event: delta=480, NoteOff ch0 n60 v0.
        let ev1 = &track.events[1];
        assert_eq!(ev1.delta_ticks, 480);
        if let SmfEvent::Midi(ref msg) = ev1.event {
            assert_eq!(msg.status, 0x80);
            assert_eq!(msg.data[0], 0x3C);
        } else {
            panic!("expected Midi event, got {:?}", ev1.event);
        }

        // Third event: EndOfTrack.
        assert!(matches!(track.events[2].event, SmfEvent::EndOfTrack));
    }

    #[test]
    fn test_vlq_decode() {
        let cases: &[(Vec<u8>, u32)] = &[
            (vec![0x00], 0),
            (vec![0x01], 1),
            (vec![0x7F], 127),
            (vec![0x81, 0x00], 128),
            (vec![0xFF, 0x7F], 16383),
            (vec![0x81, 0x80, 0x00], 16384),
            (vec![0xFF, 0xFF, 0xFF, 0x7F], 0x0FFF_FFFF),
        ];
        for (bytes, expected) in cases {
            let mut pos = 0;
            let val = read_vlq(bytes, &mut pos).expect("VLQ decode failed");
            assert_eq!(val, *expected, "VLQ mismatch for bytes {:?}", bytes);
            assert_eq!(pos, bytes.len(), "VLQ should consume all bytes");
        }
    }

    #[test]
    fn test_vlq_too_long() {
        // 5-byte VLQ is illegal.
        let bad = vec![0x80, 0x80, 0x80, 0x80, 0x00];
        let mut pos = 0;
        assert!(read_vlq(&bad, &mut pos).is_err());
    }

    #[test]
    fn test_vlq_eof() {
        let truncated = vec![0x80]; // continuation bit set, no follow-up byte
        let mut pos = 0;
        assert!(read_vlq(&truncated, &mut pos).is_err());
    }

    #[test]
    fn test_wrong_magic_header() {
        let bad: Vec<u8> = vec![
            b'X', b'X', b'X', b'X', // wrong magic
            0x00, 0x00, 0x00, 0x06, 0x00, 0x00, 0x00, 0x01, 0x01, 0xE0,
        ];
        assert!(parse(&bad).is_err());
    }

    #[test]
    fn test_truncated_header() {
        // Only 8 bytes — MThd magic + partial length field.
        let truncated: Vec<u8> = vec![0x4D, 0x54, 0x68, 0x64, 0x00, 0x00, 0x00];
        assert!(parse(&truncated).is_err());
    }

    #[test]
    fn test_wrong_mtrk_magic() {
        // Valid MThd, but MTrk magic is broken.
        let mut data = minimal_smf_bytes();
        // Byte 14 = first byte of 'MTrk'.
        data[14] = b'X';
        assert!(parse(&data).is_err());
    }

    #[test]
    fn test_running_status() {
        // Two NoteOn events; second uses running status (no repeated 0x90).
        // Payload:
        //   00 90 3C 40   → NoteOn ch0 n60 v64  (4 bytes)
        //   00 3E 50      → running status NoteOn ch0 n62 v80 (3 bytes)
        //   00 FF 2F 00   → EndOfTrack  (4 bytes)
        // Total = 11 = 0x0B
        let data: Vec<u8> = vec![
            0x4D, 0x54, 0x68, 0x64, 0x00, 0x00, 0x00, 0x06, 0x00, 0x00, 0x00, 0x01, 0x01, 0xE0,
            0x4D, 0x54, 0x72, 0x6B, 0x00, 0x00, 0x00, 0x0B, 0x00, 0x90, 0x3C, 0x40, 0x00, 0x3E,
            0x50, 0x00, 0xFF, 0x2F, 0x00,
        ];
        let file = parse(&data).expect("running status parse should succeed");
        assert_eq!(file.tracks[0].events.len(), 3);

        // Second event should be NoteOn ch0 n62 v80.
        if let SmfEvent::Midi(ref msg) = file.tracks[0].events[1].event {
            assert_eq!(msg.status, 0x90);
            assert_eq!(msg.data, &[0x3E, 0x50]);
        } else {
            panic!("expected Midi event");
        }
    }

    #[test]
    fn test_tempo_meta() {
        // Tempo = 500000 µs/beat = 0x07A120.
        // Payload:
        //   00 FF 51 03 07 A1 20   → Tempo (7 bytes)
        //   00 FF 2F 00            → EndOfTrack (4 bytes)
        // Total = 11 = 0x0B
        let data: Vec<u8> = vec![
            0x4D, 0x54, 0x68, 0x64, 0x00, 0x00, 0x00, 0x06, 0x00, 0x00, 0x00, 0x01, 0x01, 0xE0,
            0x4D, 0x54, 0x72, 0x6B, 0x00, 0x00, 0x00, 0x0B, 0x00, 0xFF, 0x51, 0x03, 0x07, 0xA1,
            0x20, 0x00, 0xFF, 0x2F, 0x00,
        ];
        let file = parse(&data).expect("tempo parse should succeed");
        if let SmfEvent::Tempo(us) = file.tracks[0].events[0].event {
            assert_eq!(us, 500_000);
        } else {
            panic!("expected Tempo event");
        }
    }
}
