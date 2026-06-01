//! JACK MIDI port implementation — compiled only with the `jack-backend` feature.
//!
//! Provides ring-buffer-backed [`JackMidiOutput`] and [`JackMidiInput`] types,
//! plus [`JackDevice::open_midi_output`] and [`JackDevice::open_midi_input`]
//! factory methods.
//!
//! SysEx messages longer than [`MIDI_ENTRY_MAX`] bytes are automatically chunked on
//! write and can be reassembled on read using [`JackMidiInput::recv_with_sysex`].

#![cfg(feature = "jack-backend")]

use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use ringbuf::{
    HeapCons, HeapProd, HeapRb,
    traits::{Consumer, Producer, Split},
};

use oxisound_core::OxiSoundError;

use crate::client::JackDevice;
use crate::midi_util::SysExReassembler;

// ---------------------------------------------------------------------------
// Ring-buffer entry
// ---------------------------------------------------------------------------

/// Maximum bytes per MIDI ring-buffer entry.
///
/// SysEx messages longer than this value are automatically chunked by
/// [`JackMidiOutput::send_raw`] and must be reassembled by the receiver
/// using [`JackMidiInput::recv_with_sysex`].
pub const MIDI_ENTRY_MAX: usize = 16;

/// A fixed-size MIDI message entry for the realtime-safe ring buffer.
///
/// Messages longer than [`MIDI_ENTRY_MAX`] bytes are chunked (SysEx). The `len`
/// field indicates how many bytes of `data` are valid.
#[derive(Clone, Copy)]
pub struct MidiEntry {
    /// Frame timestamp (frames since buffer start).
    pub time: u32,
    /// Number of valid bytes in `data`.
    pub len: u8,
    /// Raw MIDI bytes (only `data[..len]` is meaningful).
    pub data: [u8; MIDI_ENTRY_MAX],
}

// ---------------------------------------------------------------------------
// JackMidiOutput
// ---------------------------------------------------------------------------

/// Ring-buffer-backed JACK MIDI output port.
///
/// Write MIDI messages via [`send_raw`](JackMidiOutput::send_raw). SysEx messages
/// longer than [`MIDI_ENTRY_MAX`] are automatically chunked and re-assembled by the
/// JACK process callback before being written to the port.
///
/// Constructed via [`JackDevice::open_midi_output`].
pub struct JackMidiOutput {
    producer: HeapProd<MidiEntry>,
    frames_processed: Arc<AtomicU64>,
    /// Keeps the JACK client alive; dropped when this struct is dropped.
    _active: jack::AsyncClient<(), JackMidiOutputHandler>,
}

impl JackMidiOutput {
    /// Send a MIDI message with frame timestamp `time` (frames since buffer start).
    ///
    /// Messages longer than [`MIDI_ENTRY_MAX`] (e.g. SysEx) are chunked automatically.
    /// Returns `Err(Underrun)` if the ring buffer is full.
    pub fn send_raw(&mut self, time: u32, bytes: &[u8]) -> Result<(), OxiSoundError> {
        for chunk in bytes.chunks(MIDI_ENTRY_MAX) {
            let len = chunk.len();
            let mut data = [0u8; MIDI_ENTRY_MAX];
            data[..len].copy_from_slice(chunk);
            let entry = MidiEntry {
                time,
                len: len as u8,
                data,
            };
            self.producer
                .try_push(entry)
                .map_err(|_| OxiSoundError::Underrun("JACK MIDI output ring full".to_owned()))?;
        }
        Ok(())
    }

    /// Total number of JACK process cycles completed since activation.
    pub fn frames_processed(&self) -> u64 {
        self.frames_processed.load(Ordering::Relaxed)
    }
}

/// Implementation of [`oxisound_core::MidiOutput`] for [`JackMidiOutput`].
///
/// Uses frame timestamp 0 (immediate); callers requiring precise frame alignment
/// should call [`JackMidiOutput::send_raw`] directly.
impl oxisound_core::MidiOutput for JackMidiOutput {
    fn send(&mut self, msg: &oxisound_core::MidiMessage) -> Result<(), OxiSoundError> {
        self.send_raw(0, &msg.to_bytes())
    }
}

struct JackMidiOutputHandler {
    port: jack::Port<jack::MidiOut>,
    consumer: HeapCons<MidiEntry>,
    frames_processed: Arc<AtomicU64>,
}

impl jack::ProcessHandler for JackMidiOutputHandler {
    fn process(&mut self, _client: &jack::Client, ps: &jack::ProcessScope) -> jack::Control {
        let mut writer = self.port.writer(ps);
        while let Some(entry) = self.consumer.try_pop() {
            let msg = jack::RawMidi {
                time: entry.time,
                bytes: &entry.data[..entry.len as usize],
            };
            let _ = writer.write(&msg);
        }
        self.frames_processed.fetch_add(1, Ordering::Relaxed);
        jack::Control::Continue
    }
}

// ---------------------------------------------------------------------------
// JackMidiInput
// ---------------------------------------------------------------------------

/// Ring-buffer-backed JACK MIDI input port.
///
/// Poll for incoming MIDI entries via [`try_recv`](JackMidiInput::try_recv), or
/// use [`recv_with_sysex`](JackMidiInput::recv_with_sysex) to receive all pending
/// entries and automatically reassemble chunked SysEx messages.
///
/// Constructed via [`JackDevice::open_midi_input`].
pub struct JackMidiInput {
    consumer: HeapCons<MidiEntry>,
    frames_processed: Arc<AtomicU64>,
    sysex: SysExReassembler,
    /// Keeps the JACK client alive; dropped when this struct is dropped.
    _active: jack::AsyncClient<(), JackMidiInputHandler>,
}

impl JackMidiInput {
    /// Poll for a single incoming MIDI entry. Returns `None` if the ring buffer is empty.
    ///
    /// Raw entries may be chunked SysEx fragments. Use [`recv_with_sysex`](Self::recv_with_sysex)
    /// for automatic reassembly.
    pub fn try_recv(&mut self) -> Option<MidiEntry> {
        self.consumer.try_pop()
    }

    /// Receive all pending entries and reassemble SysEx.
    ///
    /// Returns `(regular_entries, sysex_events)`:
    /// - `regular_entries` — non-SysEx MIDI entries (note on/off, CC, etc.)
    /// - `sysex_events` — completed SysEx messages or interleaved realtime bytes
    pub fn recv_with_sysex(&mut self) -> (Vec<MidiEntry>, Vec<crate::midi_util::SysExEvent>) {
        let mut regular = Vec::new();
        let mut sysex_events = Vec::new();
        while let Some(entry) = self.consumer.try_pop() {
            let bytes = &entry.data[..entry.len as usize];
            if !bytes.is_empty() && (bytes[0] == 0xF0 || self.sysex.in_sysex()) {
                sysex_events.extend(self.sysex.feed(bytes));
            } else {
                regular.push(entry);
            }
        }
        (regular, sysex_events)
    }

    /// Total number of JACK process cycles completed since activation.
    pub fn frames_processed(&self) -> u64 {
        self.frames_processed.load(Ordering::Relaxed)
    }
}

struct JackMidiInputHandler {
    port: jack::Port<jack::MidiIn>,
    producer: HeapProd<MidiEntry>,
    frames_processed: Arc<AtomicU64>,
}

impl jack::ProcessHandler for JackMidiInputHandler {
    fn process(&mut self, _client: &jack::Client, ps: &jack::ProcessScope) -> jack::Control {
        for msg in self.port.iter(ps) {
            let len = msg.bytes.len().min(MIDI_ENTRY_MAX);
            let mut data = [0u8; MIDI_ENTRY_MAX];
            data[..len].copy_from_slice(&msg.bytes[..len]);
            let entry = MidiEntry {
                time: msg.time,
                len: len as u8,
                data,
            };
            let _ = self.producer.try_push(entry);
        }
        self.frames_processed.fetch_add(1, Ordering::Relaxed);
        jack::Control::Continue
    }
}

// ---------------------------------------------------------------------------
// JackDevice factory methods
// ---------------------------------------------------------------------------

impl JackDevice {
    /// Open a JACK MIDI output port with the given client/port name.
    ///
    /// The returned [`JackMidiOutput`] owns a JACK async client. When dropped,
    /// the client is deactivated and the port is unregistered.
    pub fn open_midi_output(&self, port_name: &str) -> Result<JackMidiOutput, OxiSoundError> {
        let (client, _status) = jack::Client::new(port_name, jack::ClientOptions::NO_START_SERVER)
            .map_err(|e| OxiSoundError::Device(format!("JACK client open failed: {e}")))?;

        let port = client
            .register_port("midi_out", jack::MidiOut::default())
            .map_err(|e| OxiSoundError::Device(format!("JACK port register failed: {e}")))?;

        let capacity = 512usize;
        let rb = HeapRb::<MidiEntry>::new(capacity);
        let (producer, consumer) = rb.split();

        let frames_processed = Arc::new(AtomicU64::new(0));
        let handler = JackMidiOutputHandler {
            port,
            consumer,
            frames_processed: Arc::clone(&frames_processed),
        };

        let active = client
            .activate_async((), handler)
            .map_err(|e| OxiSoundError::Device(format!("JACK activate_async failed: {e}")))?;

        Ok(JackMidiOutput {
            producer,
            frames_processed,
            _active: active,
        })
    }

    /// Open a JACK MIDI input port with the given client/port name.
    ///
    /// The returned [`JackMidiInput`] owns a JACK async client. When dropped,
    /// the client is deactivated and the port is unregistered.
    pub fn open_midi_input(&self, port_name: &str) -> Result<JackMidiInput, OxiSoundError> {
        let (client, _status) = jack::Client::new(port_name, jack::ClientOptions::NO_START_SERVER)
            .map_err(|e| OxiSoundError::Device(format!("JACK client open failed: {e}")))?;

        let port = client
            .register_port("midi_in", jack::MidiIn::default())
            .map_err(|e| OxiSoundError::Device(format!("JACK port register failed: {e}")))?;

        let capacity = 512usize;
        let rb = HeapRb::<MidiEntry>::new(capacity);
        let (producer, consumer) = rb.split();

        let frames_processed = Arc::new(AtomicU64::new(0));
        let handler = JackMidiInputHandler {
            port,
            producer,
            frames_processed: Arc::clone(&frames_processed),
        };

        let active = client
            .activate_async((), handler)
            .map_err(|e| OxiSoundError::Device(format!("JACK activate_async failed: {e}")))?;

        Ok(JackMidiInput {
            consumer,
            frames_processed,
            sysex: SysExReassembler::new(),
            _active: active,
        })
    }
}

// ---------------------------------------------------------------------------
// Integration test stubs (require JACK daemon — run manually with --include-ignored)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    #[test]
    #[ignore]
    fn test_jack_midi_output_integration() {
        // Requires a running JACK daemon — execute manually:
        // cargo test -p oxisound-jack --features jack-backend -- --include-ignored test_jack_midi_output_integration
    }

    #[test]
    #[ignore]
    fn test_jack_midi_input_integration() {
        // Requires a running JACK daemon — execute manually:
        // cargo test -p oxisound-jack --features jack-backend -- --include-ignored test_jack_midi_input_integration
    }
}
