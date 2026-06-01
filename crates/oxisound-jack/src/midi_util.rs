//! Pure-Rust MIDI utility types — no external dependencies, always compiled.
//!
//! This module provides:
//!
//! - [`SysExReassembler`] — re-assembles SysEx messages chunked across ring-buffer entries.
//! - [`midi_message_len`] — returns expected byte length for standard MIDI status bytes.
//! - [`is_realtime`] / [`is_status`] — MIDI byte classification helpers.

/// Events produced by [`SysExReassembler::feed`].
#[derive(Debug, PartialEq, Eq)]
pub enum SysExEvent {
    /// A complete SysEx message (F0 .. F7 inclusive).
    Complete(Vec<u8>),
    /// A realtime status byte (0xF8/FA/FB/FC/FE/FF) found inside SysEx data.
    Realtime(u8),
}

/// Reassembles MIDI SysEx messages split across multiple chunk reads.
///
/// Realtime status bytes (0xF8, 0xFA, 0xFB, 0xFC, 0xFE, 0xFF) may interleave
/// with SysEx and are yielded immediately as separate single-byte messages.
///
/// # Example
///
/// ```rust,ignore
/// let mut asm = SysExReassembler::new();
/// let _ = asm.feed(&[0xF0, 0x41]);  // begin SysEx
/// let events = asm.feed(&[0x10, 0xF7]); // finish SysEx
/// assert!(matches!(&events[0], SysExEvent::Complete(_)));
/// ```
pub struct SysExReassembler {
    buffer: Vec<u8>,
    in_sysex: bool,
}

impl SysExReassembler {
    /// Creates a new, idle reassembler.
    pub fn new() -> Self {
        Self {
            buffer: Vec::new(),
            in_sysex: false,
        }
    }

    /// Feed raw bytes from one MIDI ring-buffer entry. Returns any complete events.
    ///
    /// - SysEx framing bytes (F0 and F7) are included in the emitted `Complete` payload.
    /// - Realtime bytes interleaved inside SysEx are extracted and emitted as `Realtime`.
    /// - Any non-realtime status byte that interrupts an open SysEx cancels the partial
    ///   accumulation and is treated as the start of a new SysEx (or ignored if not 0xF0).
    pub fn feed(&mut self, bytes: &[u8]) -> Vec<SysExEvent> {
        let mut events = Vec::new();

        for &byte in bytes {
            if !self.in_sysex {
                if byte == 0xF0 {
                    // Start of a new SysEx message.
                    self.buffer.clear();
                    self.buffer.push(0xF0);
                    self.in_sysex = true;
                }
                // Non-SysEx bytes while idle are handled by the caller's main MIDI path.
                // SysExReassembler only tracks SysEx state.
            } else {
                // Currently accumulating SysEx bytes.
                if byte == 0xF7 {
                    // End-of-SysEx terminator — emit the complete message.
                    self.buffer.push(0xF7);
                    self.in_sysex = false;
                    events.push(SysExEvent::Complete(self.buffer.drain(..).collect()));
                } else if byte >= 0xF8 {
                    // Realtime byte — emit immediately, do NOT break SysEx accumulation.
                    events.push(SysExEvent::Realtime(byte));
                } else if byte >= 0x80 {
                    // Non-realtime status byte cancels the current SysEx.
                    // If it is 0xF0 it starts a new SysEx; otherwise we discard and go idle.
                    self.buffer.clear();
                    self.in_sysex = false;
                    if byte == 0xF0 {
                        self.buffer.push(0xF0);
                        self.in_sysex = true;
                    }
                } else {
                    // Regular data byte — push into accumulation buffer.
                    self.buffer.push(byte);
                }
            }
        }

        events
    }

    /// True if currently mid-SysEx (waiting for F7).
    pub fn in_sysex(&self) -> bool {
        self.in_sysex
    }

    /// Discard any partial SysEx accumulation (e.g., on stream reset).
    pub fn reset(&mut self) {
        self.buffer.clear();
        self.in_sysex = false;
    }
}

impl Default for SysExReassembler {
    fn default() -> Self {
        Self::new()
    }
}

/// Returns the expected total byte length of a standard MIDI message given its status byte.
///
/// Returns `None` for SysEx (variable length) or unrecognised bytes.
///
/// # Examples
///
/// ```rust,ignore
/// assert_eq!(midi_message_len(0x90), Some(3)); // NoteOn
/// assert_eq!(midi_message_len(0xF0), None);    // SysEx — variable length
/// assert_eq!(midi_message_len(0xF8), Some(1)); // Realtime Clock
/// ```
#[must_use]
pub fn midi_message_len(status: u8) -> Option<usize> {
    match status & 0xF0 {
        0x80 | 0x90 | 0xA0 | 0xB0 | 0xE0 => Some(3), // NoteOff, NoteOn, KeyPressure, CC, PitchBend
        0xC0 | 0xD0 => Some(2),                      // ProgramChange, ChannelPressure
        0xF0 => match status {
            0xF1 | 0xF3 => Some(2), // MTC Quarter Frame, Song Select
            0xF2 => Some(3),        // Song Position Pointer
            0xF6 | 0xF8 | 0xFA | 0xFB | 0xFC | 0xFE | 0xFF => Some(1), // Tune Request, realtime
            _ => None,              // SysEx, undefined
        },
        _ => None,
    }
}

/// Returns true if `byte` is a MIDI realtime status byte (clock, start, stop, etc.).
///
/// Realtime bytes have values 0xF8–0xFF and may appear at any point in a MIDI data stream,
/// including interleaved within a SysEx message.
#[must_use]
pub fn is_realtime(byte: u8) -> bool {
    byte >= 0xF8
}

/// Returns true if `byte` is any MIDI status byte (high bit set).
#[must_use]
pub fn is_status(byte: u8) -> bool {
    byte & 0x80 != 0
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_midi_message_len() {
        assert_eq!(midi_message_len(0x90), Some(3)); // NoteOn
        assert_eq!(midi_message_len(0xC0), Some(2)); // ProgramChange
        assert_eq!(midi_message_len(0xF8), Some(1)); // Clock
        assert_eq!(midi_message_len(0xF0), None); // SysEx
    }

    #[test]
    fn test_sysex_reassembler_complete_in_one_chunk() {
        let mut asm = SysExReassembler::new();
        let events = asm.feed(&[0xF0, 0x41, 0x10, 0x42, 0xF7]);
        assert_eq!(events.len(), 1);
        if let SysExEvent::Complete(data) = &events[0] {
            assert_eq!(data.as_slice(), &[0xF0, 0x41, 0x10, 0x42, 0xF7]);
        } else {
            panic!("expected Complete");
        }
        assert!(!asm.in_sysex());
    }

    #[test]
    fn test_sysex_reassembler_split_across_chunks() {
        let mut asm = SysExReassembler::new();
        let e1 = asm.feed(&[0xF0, 0x41, 0x10]);
        assert!(e1.is_empty());
        assert!(asm.in_sysex());
        let e2 = asm.feed(&[0x42, 0xF7]);
        assert_eq!(e2.len(), 1);
        if let SysExEvent::Complete(data) = &e2[0] {
            assert_eq!(data.as_slice(), &[0xF0, 0x41, 0x10, 0x42, 0xF7]);
        } else {
            panic!("expected Complete");
        }
        assert!(!asm.in_sysex());
    }

    #[test]
    fn test_sysex_reassembler_interleaved_realtime() {
        let mut asm = SysExReassembler::new();
        // 0xF8 (clock) interleaved mid-SysEx
        let e1 = asm.feed(&[0xF0, 0x41]);
        assert!(e1.is_empty());
        let e2 = asm.feed(&[0xF8]); // realtime clock
        assert_eq!(e2.len(), 1);
        assert!(matches!(&e2[0], SysExEvent::Realtime(0xF8)));
        assert!(asm.in_sysex()); // still in SysEx
        let e3 = asm.feed(&[0x42, 0xF7]);
        assert_eq!(e3.len(), 1);
        if let SysExEvent::Complete(data) = &e3[0] {
            assert_eq!(data.as_slice(), &[0xF0, 0x41, 0x42, 0xF7]);
        } else {
            panic!("expected Complete");
        }
    }

    #[test]
    fn test_sysex_reassembler_back_to_back() {
        let mut asm = SysExReassembler::new();
        let e1 = asm.feed(&[0xF0, 0x01, 0xF7]);
        assert_eq!(e1.len(), 1);
        let e2 = asm.feed(&[0xF0, 0x02, 0xF7]);
        assert_eq!(e2.len(), 1);
    }

    #[test]
    fn test_is_realtime() {
        assert!(is_realtime(0xF8)); // clock
        assert!(is_realtime(0xFE)); // active sensing
        assert!(!is_realtime(0x90)); // note on
        assert!(!is_realtime(0xF0)); // sysex start
    }

    #[test]
    fn test_is_status() {
        assert!(is_status(0x90));
        assert!(is_status(0xF0));
        assert!(is_status(0xFF));
        assert!(!is_status(0x00));
        assert!(!is_status(0x7F));
    }

    #[test]
    fn test_sysex_reassembler_cancelled_by_status() {
        // A non-realtime, non-SysEx status byte mid-SysEx cancels it.
        let mut asm = SysExReassembler::new();
        let e1 = asm.feed(&[0xF0, 0x41]); // begin SysEx
        assert!(e1.is_empty());
        let e2 = asm.feed(&[0x90]); // NoteOn cancels SysEx
        assert!(e2.is_empty()); // no event emitted for partial SysEx
        assert!(!asm.in_sysex());
    }

    #[test]
    fn test_sysex_reassembler_reset() {
        let mut asm = SysExReassembler::new();
        let _ = asm.feed(&[0xF0, 0x41]); // start a SysEx
        assert!(asm.in_sysex());
        asm.reset();
        assert!(!asm.in_sysex());
        // After reset, a new SysEx should be accepted cleanly.
        let events = asm.feed(&[0xF0, 0x42, 0xF7]);
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], SysExEvent::Complete(data) if data[1] == 0x42));
    }

    #[test]
    fn test_midi_message_len_all_channels() {
        // Channel messages should work for all 16 MIDI channels.
        for ch in 0u8..16 {
            assert_eq!(midi_message_len(0x80 | ch), Some(3)); // NoteOff
            assert_eq!(midi_message_len(0x90 | ch), Some(3)); // NoteOn
            assert_eq!(midi_message_len(0xA0 | ch), Some(3)); // PolyPressure
            assert_eq!(midi_message_len(0xB0 | ch), Some(3)); // CC
            assert_eq!(midi_message_len(0xC0 | ch), Some(2)); // ProgramChange
            assert_eq!(midi_message_len(0xD0 | ch), Some(2)); // ChannelPressure
            assert_eq!(midi_message_len(0xE0 | ch), Some(3)); // PitchBend
        }
    }

    #[test]
    fn test_midi_message_len_system_messages() {
        assert_eq!(midi_message_len(0xF1), Some(2)); // MTC Quarter Frame
        assert_eq!(midi_message_len(0xF2), Some(3)); // Song Position Pointer
        assert_eq!(midi_message_len(0xF3), Some(2)); // Song Select
        assert_eq!(midi_message_len(0xF6), Some(1)); // Tune Request
        assert_eq!(midi_message_len(0xF7), None); // SysEx end (undefined as standalone)
        assert_eq!(midi_message_len(0xF8), Some(1)); // Clock
        assert_eq!(midi_message_len(0xFA), Some(1)); // Start
        assert_eq!(midi_message_len(0xFB), Some(1)); // Continue
        assert_eq!(midi_message_len(0xFC), Some(1)); // Stop
        assert_eq!(midi_message_len(0xFE), Some(1)); // Active Sensing
        assert_eq!(midi_message_len(0xFF), Some(1)); // Reset
    }
}
