//! Standard MIDI File (SMF / `.mid`) parser and player.
//!
//! # Usage
//!
//! ```rust
//! use oxisound_smf::{parse, SmfFormat, SmfEvent};
//!
//! // Minimal Format-0 SMF (1 track, 480 ticks/beat, one NoteOn)
//! let bytes: Vec<u8> = vec![
//!     0x4D, 0x54, 0x68, 0x64, 0x00, 0x00, 0x00, 0x06,
//!     0x00, 0x00, 0x00, 0x01, 0x01, 0xE0,
//!     0x4D, 0x54, 0x72, 0x6B, 0x00, 0x00, 0x00, 0x08,
//!     0x00, 0x90, 0x3C, 0x40,
//!     0x00, 0xFF, 0x2F, 0x00,
//! ];
//! let file = parse(&bytes).unwrap();
//! assert_eq!(file.format, SmfFormat::SingleTrack);
//! assert_eq!(file.tracks.len(), 1);
//! ```

#![forbid(unsafe_code)]
#![cfg_attr(not(feature = "std"), no_std)]
extern crate alloc;

use alloc::{string::String, vec::Vec};

mod parser;
mod player;
mod timing;
mod writer;

pub use parser::parse;
pub use player::SmfPlayer;
pub use timing::{TempoMap, ticks_to_seconds};
pub use writer::write as write_smf;

/// SMF file format variant (header word 0).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum SmfFormat {
    SingleTrack = 0,
    MultiTrack = 1,
    MultiSong = 2,
}

/// SMF time division — either ticks-per-beat or SMPTE frame-based.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Division {
    TicksPerBeat(u16),
    Smpte { fps: u8, subframes: u8 },
}

/// A fully parsed SMF file.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SmfFile {
    pub format: SmfFormat,
    pub division: Division,
    pub tracks: Vec<SmfTrack>,
}

/// One track extracted from an SMF file.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SmfTrack {
    pub name: Option<String>,
    pub events: Vec<TrackEvent>,
}

/// A single event within a track, with its delta-tick offset from the previous event.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TrackEvent {
    pub delta_ticks: u32,
    pub event: SmfEvent,
}

/// All event kinds that can appear in an SMF track.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum SmfEvent {
    /// A MIDI channel message. The channel nibble is encoded in `MidiMessage::status`.
    Midi(oxisound_core::MidiMessage),
    /// Tempo change in microseconds per quarter note.
    Tempo(u32),
    TimeSignature {
        numerator: u8,
        /// Log₂ of the denominator (e.g. 2 → quarter note = denominator 4).
        denominator_pow2: u8,
        clocks_per_click: u8,
        notated_32nds_per_beat: u8,
    },
    KeySignature {
        /// Negative = flats, positive = sharps.
        sharps_flats: i8,
        is_minor: bool,
    },
    TrackName(String),
    EndOfTrack,
    /// Unknown/unsupported meta event — preserved for round-trip fidelity.
    UnknownMeta {
        meta_type: u8,
        data: Vec<u8>,
    },
}

/// Parse error returned by [`parse`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SmfError(pub String);

impl core::fmt::Display for SmfError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "SMF parse error: {}", self.0)
    }
}

#[cfg(feature = "std")]
impl std::error::Error for SmfError {}
