#![forbid(unsafe_code)]
//! MIDI device I/O for OxiSound, backed by midir (CoreMIDI/WinMM/ALSA sequencer).
//!
//! # Overview
//!
//! This crate implements the [`MidiDevice`], `MidiInput`, and `MidiOutput` traits
//! defined in `oxisound-core` using the [`midir`] crate as the OS-boundary backend.
//! Platform support is automatic: CoreMIDI on macOS/iOS, WinMM on Windows, and
//! ALSA sequencer on Linux.
//!
//! # Usage
//!
//! ```no_run
//! use oxisound_core::MidiDevice;
//! use oxisound_midi::MidiDeviceImpl;
//!
//! let devices = MidiDeviceImpl::enumerate_midi().expect("MIDI enumeration failed");
//! for d in &devices {
//!     println!("{}: in={} out={}", d.name, d.is_input, d.is_output);
//! }
//! ```

#[cfg(not(target_os = "windows"))]
use midir::os::unix::{VirtualInput, VirtualOutput};
use midir::{MidiInput as MidirInput, MidiOutput as MidirOutput, MidiOutputConnection};
use oxisound_core::{
    MidiDevice, MidiDeviceInfo, MidiInput as MidiInputTrait, MidiMessage,
    MidiOutput as MidiOutputTrait, OxiSoundError,
};
use std::sync::mpsc;

// ---------------------------------------------------------------------------
// SysEx parsing helper
// ---------------------------------------------------------------------------

/// Parses raw bytes from a midir callback into a [`MidiMessage`].
///
/// Handles SysEx (0xF0..0xF7) by stripping the framing bytes and storing
/// only the payload in `data`. Returns `None` for empty byte slices.
fn parse_midi_bytes(stamp: u64, bytes: &[u8]) -> Option<MidiMessage> {
    if bytes.is_empty() {
        return None;
    }
    if bytes.first() == Some(&0xF0) {
        let payload = if bytes.last() == Some(&0xF7) {
            &bytes[1..bytes.len() - 1]
        } else {
            &bytes[1..]
        };
        let mut msg = MidiMessage::new_sysex(payload);
        msg.timestamp_micros = stamp;
        Some(msg)
    } else {
        Some(MidiMessage {
            status: bytes[0],
            data: bytes[1..].to_vec(),
            timestamp_micros: stamp,
        })
    }
}

// ---------------------------------------------------------------------------
// Type alias for the sender stored in MidiInputConnection
// ---------------------------------------------------------------------------

type MidiSender = mpsc::Sender<MidiMessage>;

// ---------------------------------------------------------------------------
// MidiDeviceImpl — implements the MidiDevice trait
// ---------------------------------------------------------------------------

/// MIDI device backend backed by midir.
///
/// This struct implements the [`MidiDevice`] trait, providing enumeration and
/// port-opening functionality for MIDI devices via platform-native backends
/// (CoreMIDI, WinMM, ALSA sequencer).
///
/// # Note on trait design
///
/// The `MidiDevice` trait methods are free functions (no `&self`), so each call to
/// [`MidiDeviceImpl::open_midi_input`] / [`MidiDeviceImpl::open_midi_output`] creates
/// a fresh midir client internally. For repeated operations, prefer [`MidiHost`], which
/// amortizes client creation across multiple port queries.
pub struct MidiDeviceImpl;

impl MidiDevice for MidiDeviceImpl {
    fn enumerate_midi() -> Result<Vec<MidiDeviceInfo>, OxiSoundError> {
        // Graceful degradation: on a headless system the platform MIDI subsystem
        // (e.g. the ALSA sequencer at /dev/snd/seq) may be unavailable, in which
        // case midir's client constructor returns `InitError`. There are simply no
        // ports to enumerate, so we report an empty list rather than failing — the
        // same contract as `MidiHost::new`, which only errors when *both* clients
        // are unavailable.
        let midi_in = MidirInput::new("oxisound-enum-in").ok();
        let midi_out = MidirOutput::new("oxisound-enum-out").ok();

        let mut devices: Vec<MidiDeviceInfo> = Vec::new();

        if let Some(midi_in) = &midi_in {
            for port in &midi_in.ports() {
                let name = midi_in
                    .port_name(port)
                    .unwrap_or_else(|_| "Unknown".to_string());
                devices.push(MidiDeviceInfo {
                    name,
                    is_input: true,
                    is_output: false,
                    port_count: 1,
                });
            }
        }

        if let Some(midi_out) = &midi_out {
            for port in &midi_out.ports() {
                let name = midi_out
                    .port_name(port)
                    .unwrap_or_else(|_| "Unknown".to_string());
                // If a device of the same name already appears as input, merge it into
                // a combined in+out entry rather than creating a duplicate.
                if let Some(existing) = devices.iter_mut().find(|d| d.name == name && d.is_input) {
                    existing.is_output = true;
                } else {
                    devices.push(MidiDeviceInfo {
                        name,
                        is_input: false,
                        is_output: true,
                        port_count: 1,
                    });
                }
            }
        }

        Ok(devices)
    }

    fn open_midi_input(port: usize) -> Result<Box<dyn MidiInputTrait>, OxiSoundError> {
        let midi_in =
            MidirInput::new("oxisound-input").map_err(|e| OxiSoundError::Device(e.to_string()))?;

        let ports = midi_in.ports();
        if port >= ports.len() {
            return Err(OxiSoundError::Device(format!(
                "MIDI input port {port} not found (only {} ports available)",
                ports.len()
            )));
        }

        let (tx, rx) = mpsc::channel::<MidiMessage>();

        let connection = midi_in
            .connect(
                &ports[port],
                "oxisound-input-conn",
                move |stamp, message, sender| {
                    if let Some(msg) = parse_midi_bytes(stamp, message) {
                        // Ignore send errors: receiver dropped means the connection is
                        // being torn down and there is nothing useful to do.
                        let _ = sender.send(msg);
                    }
                },
                tx,
            )
            .map_err(|e| OxiSoundError::Device(e.kind().to_string()))?;

        Ok(Box::new(MidiInputImpl {
            _connection: connection,
            rx,
        }))
    }

    fn open_midi_output(port: usize) -> Result<Box<dyn MidiOutputTrait>, OxiSoundError> {
        let midi_out = MidirOutput::new("oxisound-output")
            .map_err(|e| OxiSoundError::Device(e.to_string()))?;

        let ports = midi_out.ports();
        if port >= ports.len() {
            return Err(OxiSoundError::Device(format!(
                "MIDI output port {port} not found (only {} ports available)",
                ports.len()
            )));
        }

        let connection = midi_out
            .connect(&ports[port], "oxisound-output-conn")
            .map_err(|e| OxiSoundError::Device(e.kind().to_string()))?;

        Ok(Box::new(MidiOutputImpl { connection }))
    }
}

// ---------------------------------------------------------------------------
// MidiInputImpl
// ---------------------------------------------------------------------------

struct MidiInputImpl {
    _connection: midir::MidiInputConnection<MidiSender>,
    rx: mpsc::Receiver<MidiMessage>,
}

impl MidiInputTrait for MidiInputImpl {
    fn receive(&mut self) -> Result<Option<MidiMessage>, OxiSoundError> {
        match self.rx.try_recv() {
            Ok(msg) => Ok(Some(msg)),
            Err(mpsc::TryRecvError::Empty) => Ok(None),
            Err(mpsc::TryRecvError::Disconnected) => Err(OxiSoundError::Disconnected(
                "MIDI input callback disconnected".to_string(),
            )),
        }
    }
}

// ---------------------------------------------------------------------------
// MidiOutputImpl
// ---------------------------------------------------------------------------

struct MidiOutputImpl {
    connection: MidiOutputConnection,
}

impl MidiOutputTrait for MidiOutputImpl {
    fn send(&mut self, msg: &MidiMessage) -> Result<(), OxiSoundError> {
        let bytes = msg.to_bytes();
        self.connection
            .send(&bytes)
            .map_err(|e| OxiSoundError::Device(e.to_string()))
    }
}

// ---------------------------------------------------------------------------
// MidiHost — amortised host builder for repeated port discovery
// ---------------------------------------------------------------------------

/// A MIDI host that reuses the same midir client across multiple port queries.
///
/// Unlike the free functions in [`MidiDevice`] which create a fresh midir client
/// per call, `MidiHost` holds clients for its lifetime, reducing initialization
/// overhead when repeatedly querying available ports.
///
/// # Examples
///
/// ```no_run
/// use oxisound_midi::MidiHost;
///
/// let host = MidiHost::new().expect("MIDI unavailable");
/// for name in host.input_port_names() {
///     println!("MIDI input: {name}");
/// }
/// ```
pub struct MidiHost {
    input: Option<MidirInput>,
    output: Option<MidirOutput>,
}

impl MidiHost {
    /// Create a new [`MidiHost`], initializing both input and output clients.
    ///
    /// On platforms where MIDI is unavailable, one or both of the internal clients
    /// may be absent; the port-name methods will simply return empty lists.
    ///
    /// # Errors
    ///
    /// Returns [`OxiSoundError::Device`] if neither input nor output client can be
    /// initialized (i.e., MIDI is completely unavailable on this platform).
    pub fn new() -> Result<Self, OxiSoundError> {
        let input = MidirInput::new("oxisound-host-in").ok();
        let output = MidirOutput::new("oxisound-host-out").ok();
        if input.is_none() && output.is_none() {
            return Err(OxiSoundError::Device(
                "MIDI unavailable: could not initialize input or output client".to_string(),
            ));
        }
        Ok(Self { input, output })
    }

    /// List all available MIDI input port names.
    ///
    /// Returns an empty `Vec` on systems without MIDI input devices.
    #[must_use]
    pub fn input_port_names(&self) -> Vec<String> {
        self.input
            .as_ref()
            .map(|i| {
                i.ports()
                    .iter()
                    .map(|p| i.port_name(p).unwrap_or_else(|_| "Unknown".to_string()))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// List all available MIDI output port names.
    ///
    /// Returns an empty `Vec` on systems without MIDI output devices.
    #[must_use]
    pub fn output_port_names(&self) -> Vec<String> {
        self.output
            .as_ref()
            .map(|o| {
                o.ports()
                    .iter()
                    .map(|p| o.port_name(p).unwrap_or_else(|_| "Unknown".to_string()))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Creates a virtual MIDI input port with the given name.
    ///
    /// Supported on macOS (CoreMIDI) and Linux (ALSA sequencer).
    ///
    /// The provided callback is called with each incoming message.
    /// The returned [`Box<dyn MidiInputTrait>`] can be polled with [`MidiInputTrait::receive`].
    #[cfg(not(target_os = "windows"))]
    pub fn create_virtual_input<F>(
        &self,
        name: &str,
        callback: F,
    ) -> Result<Box<dyn MidiInputTrait>, OxiSoundError>
    where
        F: FnMut(MidiMessage) + Send + 'static,
    {
        let midi_in = MidirInput::new(name).map_err(|e| {
            OxiSoundError::Device(format!("Failed to create MIDI input for virtual port: {e}"))
        })?;
        let (tx, rx) = mpsc::channel::<MidiMessage>();
        // The callback is passed as the `data` argument so midir holds it
        // inside the connection, keeping both alive without a forwarder thread.
        let mut user_cb = callback;
        let connection = midi_in
            .create_virtual(
                name,
                move |stamp, bytes, sender| {
                    if let Some(msg) = parse_midi_bytes(stamp, bytes) {
                        user_cb(msg.clone());
                        let _ = sender.send(msg);
                    }
                },
                tx,
            )
            .map_err(|e| {
                OxiSoundError::Device(format!("Failed to create virtual input port: {e}"))
            })?;
        Ok(Box::new(MidiInputImpl {
            _connection: connection,
            rx,
        }))
    }

    /// Creates a virtual MIDI input port with the given name.
    ///
    /// Always returns [`OxiSoundError::Unsupported`] on Windows (WinMM does not
    /// support virtual ports).
    #[cfg(target_os = "windows")]
    pub fn create_virtual_input<F>(
        &self,
        _name: &str,
        _callback: F,
    ) -> Result<Box<dyn MidiInputTrait>, OxiSoundError>
    where
        F: FnMut(MidiMessage) + Send + 'static,
    {
        Err(OxiSoundError::Unsupported(
            "Virtual MIDI ports are not supported on Windows (WinMM)".to_string(),
        ))
    }

    /// Creates a virtual MIDI output port with the given name.
    ///
    /// Supported on macOS (CoreMIDI) and Linux (ALSA sequencer).
    #[cfg(not(target_os = "windows"))]
    pub fn create_virtual_output(
        &self,
        name: &str,
    ) -> Result<Box<dyn MidiOutputTrait>, OxiSoundError> {
        let midi_out = MidirOutput::new(name).map_err(|e| {
            OxiSoundError::Device(format!(
                "Failed to create MIDI output for virtual port: {e}"
            ))
        })?;
        let conn = midi_out.create_virtual(name).map_err(|e| {
            OxiSoundError::Device(format!("Failed to create virtual output port: {e}"))
        })?;
        Ok(Box::new(MidiOutputImpl { connection: conn }))
    }

    /// Creates a virtual MIDI output port with the given name.
    ///
    /// Always returns [`OxiSoundError::Unsupported`] on Windows (WinMM does not
    /// support virtual ports).
    #[cfg(target_os = "windows")]
    pub fn create_virtual_output(
        &self,
        _name: &str,
    ) -> Result<Box<dyn MidiOutputTrait>, OxiSoundError> {
        Err(OxiSoundError::Unsupported(
            "Virtual MIDI ports are not supported on Windows (WinMM)".to_string(),
        ))
    }
}

impl Default for MidiHost {
    fn default() -> Self {
        Self::new().unwrap_or(Self {
            input: None,
            output: None,
        })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use oxisound_core::MidiDevice;

    #[test]
    fn enumerate_midi_does_not_panic() {
        // On a headless box this returns Ok([]) — just verify it doesn't panic.
        let result = MidiDeviceImpl::enumerate_midi();
        assert!(
            result.is_ok(),
            "enumerate_midi should return Ok: {result:?}"
        );
    }

    #[test]
    fn midi_message_byte_roundtrip() {
        let msg = MidiMessage {
            status: 0x90,
            data: vec![0x40, 0x7F],
            timestamp_micros: 12345,
        };
        // Verify that the send-path to_bytes() produces the expected output.
        let bytes = msg.to_bytes();
        assert_eq!(bytes, vec![0x90_u8, 0x40, 0x7F]);
        assert_eq!(msg.timestamp_micros, 12345);
    }

    #[test]
    fn open_midi_input_nonexistent_port_returns_error() {
        // Port 9999 should not exist on any machine.
        let result = MidiDeviceImpl::open_midi_input(9999);
        assert!(result.is_err(), "opening nonexistent port should fail");
    }

    #[test]
    fn open_midi_output_nonexistent_port_returns_error() {
        let result = MidiDeviceImpl::open_midi_output(9999);
        assert!(result.is_err(), "opening nonexistent port should fail");
    }

    #[test]
    fn midi_host_new_succeeds_or_returns_structured_error() {
        // On macOS (CI and dev), CoreMIDI is available so new() should succeed.
        // On a truly MIDI-less system it may fail — both outcomes are acceptable.
        let result = MidiHost::new();
        // If it errors, it must be a Device error (not a panic).
        if let Err(e) = result {
            assert_eq!(
                e.kind(),
                "device",
                "MidiHost error must be Device variant: {e:?}"
            );
        }
    }

    #[test]
    fn midi_host_default_does_not_panic() {
        let _ = MidiHost::default();
    }

    #[test]
    fn midi_host_input_port_names_returns_vec() {
        // May be empty on headless systems; must not panic.
        if let Ok(host) = MidiHost::new() {
            let names = host.input_port_names();
            // All names must be non-empty strings.
            for name in &names {
                assert!(!name.is_empty(), "port name should not be empty");
            }
        }
    }

    #[test]
    fn midi_host_output_port_names_returns_vec() {
        if let Ok(host) = MidiHost::new() {
            let names = host.output_port_names();
            for name in &names {
                assert!(!name.is_empty(), "port name should not be empty");
            }
        }
    }

    #[test]
    fn midi_device_info_fields_consistent() {
        let devices = MidiDeviceImpl::enumerate_midi().expect("enumerate_midi failed");
        for d in &devices {
            assert!(
                d.is_input || d.is_output,
                "device '{}' must be at least input or output",
                d.name
            );
            assert_eq!(d.port_count, 1, "port_count must be 1 for midir devices");
        }
    }

    #[test]
    fn error_kind_for_device_error() {
        // Verify that port-not-found errors are mapped to Device variant.
        let result = MidiDeviceImpl::open_midi_input(9999);
        assert!(result.is_err(), "opening port 9999 should fail");
        if let Err(err) = result {
            assert_eq!(err.kind(), "device", "expected 'device' kind, got {err:?}");
        }
    }

    #[test]
    fn sysex_roundtrip_via_to_bytes() {
        let payload = &[0x41_u8, 0x10, 0x42];
        let msg = MidiMessage::new_sysex(payload);
        assert!(msg.is_sysex());
        let bytes = msg.to_bytes();
        assert_eq!(bytes[0], 0xF0);
        assert_eq!(bytes.last(), Some(&0xF7));
        assert_eq!(&bytes[1..bytes.len() - 1], payload);
    }

    #[test]
    fn send_uses_to_bytes_for_sysex() {
        // Verify the to_bytes() path produces the correct framing for SysEx.
        let msg = MidiMessage::new_sysex(&[0x41, 0x10]);
        let bytes = msg.to_bytes();
        assert_eq!(bytes, vec![0xF0_u8, 0x41, 0x10, 0xF7]);
    }

    #[test]
    fn parse_midi_bytes_handles_sysex() {
        // SysEx with proper F0..F7 framing.
        let raw = &[0xF0_u8, 0x41, 0x10, 0x42, 0xF7];
        let msg = parse_midi_bytes(99, raw).expect("should parse");
        assert!(msg.is_sysex());
        assert_eq!(msg.data, vec![0x41_u8, 0x10, 0x42]);
        assert_eq!(msg.timestamp_micros, 99);
    }

    #[test]
    fn parse_midi_bytes_returns_none_for_empty() {
        assert!(parse_midi_bytes(0, &[]).is_none());
    }

    #[test]
    fn virtual_port_windows_cfg_returns_unsupported() {
        // This test is only meaningful on Windows; on non-Windows we skip it.
        // The Windows cfg branch always returns Unsupported.
        #[cfg(target_os = "windows")]
        {
            let host = MidiHost::new().expect("MidiHost::new failed");
            let result = host.create_virtual_output("test-virtual");
            assert!(
                matches!(result, Err(OxiSoundError::Unsupported(_))),
                "expected Unsupported on Windows, got {result:?}"
            );
        }
        // On non-Windows, either success or a device error is acceptable.
        #[cfg(not(target_os = "windows"))]
        {
            // Just verify the method compiles and does not panic.
            let _ = MidiHost::new().map(|h| h.create_virtual_output("oxisound-test-virt-out"));
        }
    }
}
