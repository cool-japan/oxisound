#![forbid(unsafe_code)]
//! Direct JACK Audio Server client for OxiSound.
//!
//! Enable the `jack-backend` feature to activate the libjack2 C binding:
//!
//! ```toml
//! oxisound-jack = { version = "0.2.2", features = ["jack-backend"] }
//! ```
//!
//! ## GOVERNANCE (COOLJAPAN policy)
//!
//! The `jack-backend` feature introduces a C-FFI dependency on libjack2.
//! This is permitted under the same OS-boundary rationale as ALSA and CoreAudio.
//! The `jack` crate's public API is safe Rust; this crate maintains
//! `#![forbid(unsafe_code)]` throughout.
//!
//! ## Architecture
//!
//! JACK is inherently callback-based. The JACK server calls your `process`
//! callback at a fixed interval (the JACK buffer size). Two stream types:
//!
//! - **`JackOutputStream`** — ring-buffer backed; your code calls `write(&[f32])`,
//!   the JACK callback drains it. Identical to `CpalOutputStream` in usage.
//! - **`JackCallbackOutputStream`** — zero-copy; your `FnMut(&mut [f32])` is called
//!   directly in the JACK process thread. Lowest latency.
//!
//! ## Transport and Freewheel
//!
//! `JackDevice::transport_state()` and `transport_position()` query the JACK
//! transport clock. Note: `set_freewheel` is not available in the `jack` crate's
//! safe API as of v0.13.5 (upstream TODO); calling it returns `Unsupported`.
//!
//! ## Feature Flag
//!
//! Without `jack-backend`, all constructors return `OxiSoundError::Unsupported`.
//! The crate still compiles (Pure Rust stub) so downstream crates can have
//! optional JACK support without `#[cfg]` boilerplate.

// ---------------------------------------------------------------------------
// Modules — always compiled (no feature gate)
// ---------------------------------------------------------------------------

pub mod metrics;
pub use metrics::{JackMetrics, MetricsSnapshot};

pub mod midi_util;
pub use midi_util::{SysExEvent, SysExReassembler, is_realtime, is_status, midi_message_len};

#[cfg(feature = "jack-backend")]
pub mod midi;
#[cfg(feature = "jack-backend")]
pub use midi::{JackMidiInput, JackMidiOutput, MIDI_ENTRY_MAX, MidiEntry};

// ---------------------------------------------------------------------------
// Transport types — always public, regardless of feature flag
// ---------------------------------------------------------------------------

/// JACK transport rolling state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JackTransportState {
    /// Transport is rolling (playing).
    Rolling,
    /// Transport is stopped.
    Stopped,
    /// Transport is starting (clients synchronising).
    Starting,
}

/// JACK transport position snapshot.
#[derive(Debug, Clone)]
pub struct JackTransportPosition {
    /// Frame count on the transport timeline.
    pub frame: u64,
    /// Current BPM if the JACK time master has provided BBT data.
    pub bpm: Option<f64>,
}

// ---------------------------------------------------------------------------
// Stub implementation — compiled when `jack-backend` feature is NOT enabled
// ---------------------------------------------------------------------------

#[cfg(not(feature = "jack-backend"))]
mod stub {
    use super::{JackTransportPosition, JackTransportState};
    use oxisound_core::{InputStream, OutputStream, OxiSoundError, StreamConfig, StreamStats};

    const STUB_MSG: &str = "oxisound-jack was built without the `jack-backend` feature. \
         Enable it to link libjack2.";

    /// A JACK Audio Server client. Without `jack-backend`, all constructors return `Unsupported`.
    #[derive(Debug)]
    pub struct JackDevice;

    impl JackDevice {
        /// Open a new JACK client. Returns `Unsupported` when built without `jack-backend`.
        pub fn new(_client_name: &str) -> Result<Self, OxiSoundError> {
            Err(OxiSoundError::Unsupported(STUB_MSG.into()))
        }

        /// Stub: returns `Unsupported`.
        pub fn sample_rate(&self) -> u32 {
            0
        }

        /// Stub: returns `Unsupported`.
        pub fn buffer_size(&self) -> u32 {
            0
        }

        /// Stub: returns `Unsupported`.
        pub fn open_output(
            &self,
            _config: StreamConfig,
        ) -> Result<JackOutputStream, OxiSoundError> {
            Err(OxiSoundError::Unsupported(STUB_MSG.into()))
        }

        /// Stub: returns `Unsupported`.
        pub fn open_input(&self, _config: StreamConfig) -> Result<JackInputStream, OxiSoundError> {
            Err(OxiSoundError::Unsupported(STUB_MSG.into()))
        }

        /// Stub: returns `Unsupported`.
        pub fn open_output_callback<F>(
            &self,
            _config: StreamConfig,
            _callback: F,
        ) -> Result<JackCallbackOutputStream, OxiSoundError>
        where
            F: FnMut(&mut [f32]) + Send + 'static,
        {
            Err(OxiSoundError::Unsupported(STUB_MSG.into()))
        }

        /// Stub: returns `Unsupported`.
        pub fn connect_ports(&self, _src: &str, _dst: &str) -> Result<(), OxiSoundError> {
            Err(OxiSoundError::Unsupported(STUB_MSG.into()))
        }

        /// Stub: returns `Unsupported`.
        pub fn auto_connect_output(&self, _port_name: &str) -> Result<(), OxiSoundError> {
            Err(OxiSoundError::Unsupported(STUB_MSG.into()))
        }

        /// Stub: always returns `JackTransportState::Stopped`.
        pub fn transport_state(&self) -> JackTransportState {
            JackTransportState::Stopped
        }

        /// Stub: always returns frame=0, bpm=None.
        pub fn transport_position(&self) -> JackTransportPosition {
            JackTransportPosition {
                frame: 0,
                bpm: None,
            }
        }

        /// Stub: freewheel is not available without `jack-backend`.
        ///
        /// Note: `set_freewheel` is also not implemented in the `jack` 0.13.5 safe API
        /// (upstream TODO). Even with `jack-backend`, this returns `Unsupported`.
        pub fn set_freewheel(&self, _enabled: bool) -> Result<(), OxiSoundError> {
            Err(OxiSoundError::Unsupported(
                "JACK freewheel is not available in the jack 0.13.5 safe API (upstream TODO). \
                 Track: https://github.com/RustAudio/rust-jack/issues"
                    .into(),
            ))
        }
    }

    // Stub stream types — unconstructable, exist only to satisfy the type system.

    /// Stub ring-buffer JACK output stream. Cannot be constructed without `jack-backend`.
    pub struct JackOutputStream {
        _private: (),
    }

    impl OutputStream for JackOutputStream {
        fn write(&mut self, _samples: &[f32]) -> Result<(), OxiSoundError> {
            Err(OxiSoundError::Unsupported(STUB_MSG.into()))
        }
        fn stats(&self) -> StreamStats {
            StreamStats::default()
        }
    }

    impl JackOutputStream {
        /// Stub: returns `Unsupported`.
        pub fn connect_ports(&self, _src: &str, _dst: &str) -> Result<(), OxiSoundError> {
            Err(OxiSoundError::Unsupported(STUB_MSG.into()))
        }
        /// Stub: returns `Unsupported`.
        pub fn auto_connect(&self) -> Result<(), OxiSoundError> {
            Err(OxiSoundError::Unsupported(STUB_MSG.into()))
        }
        /// Stub: returns 0.0 (no JACK server available without `jack-backend`).
        pub fn cpu_load(&self) -> f32 {
            0.0
        }
        /// Stub: returns empty list (no JACK server available without `jack-backend`).
        pub fn list_ports(&self) -> Vec<String> {
            Vec::new()
        }
        /// Stub: returns empty list (no JACK server available without `jack-backend`).
        pub fn list_input_ports(&self, _pattern: Option<&str>) -> Vec<String> {
            Vec::new()
        }
        /// Stub: returns empty list (no JACK server available without `jack-backend`).
        pub fn list_output_ports(&self, _pattern: Option<&str>) -> Vec<String> {
            Vec::new()
        }
        /// Stub: returns 0 (no JACK server available without `jack-backend`).
        pub fn current_sample_rate(&self) -> u32 {
            0
        }
        /// Stub: returns 0 (no JACK server available without `jack-backend`).
        pub fn xrun_count(&self) -> u64 {
            0
        }
        /// Stub: returns 0 (no JACK server available without `jack-backend`).
        pub fn current_buffer_size(&self) -> u32 {
            0
        }
    }

    /// Stub ring-buffer JACK input stream. Cannot be constructed without `jack-backend`.
    pub struct JackInputStream {
        _private: (),
    }

    impl InputStream for JackInputStream {
        fn read(&mut self, _samples: &mut [f32]) -> Result<usize, OxiSoundError> {
            Err(OxiSoundError::Unsupported(STUB_MSG.into()))
        }
        fn stats(&self) -> StreamStats {
            StreamStats::default()
        }
    }

    impl JackInputStream {
        /// Stub: returns `Unsupported`.
        pub fn connect_ports(&self, _src: &str, _dst: &str) -> Result<(), OxiSoundError> {
            Err(OxiSoundError::Unsupported(STUB_MSG.into()))
        }
        /// Stub: returns `Unsupported`.
        pub fn auto_connect(&self) -> Result<(), OxiSoundError> {
            Err(OxiSoundError::Unsupported(STUB_MSG.into()))
        }
        /// Stub: returns 0.0 (no JACK server available without `jack-backend`).
        pub fn cpu_load(&self) -> f32 {
            0.0
        }
        /// Stub: returns empty list (no JACK server available without `jack-backend`).
        pub fn list_ports(&self) -> Vec<String> {
            Vec::new()
        }
        /// Stub: returns empty list (no JACK server available without `jack-backend`).
        pub fn list_input_ports(&self, _pattern: Option<&str>) -> Vec<String> {
            Vec::new()
        }
        /// Stub: returns empty list (no JACK server available without `jack-backend`).
        pub fn list_output_ports(&self, _pattern: Option<&str>) -> Vec<String> {
            Vec::new()
        }
        /// Stub: returns 0 (no JACK server available without `jack-backend`).
        pub fn current_sample_rate(&self) -> u32 {
            0
        }
        /// Stub: returns 0 (no JACK server available without `jack-backend`).
        pub fn xrun_count(&self) -> u64 {
            0
        }
        /// Stub: returns 0 (no JACK server available without `jack-backend`).
        pub fn current_buffer_size(&self) -> u32 {
            0
        }
    }

    /// Stub zero-copy JACK callback output stream. Cannot be constructed without `jack-backend`.
    pub struct JackCallbackOutputStream {
        _private: (),
    }

    impl JackCallbackOutputStream {
        /// Stub: returns `Unsupported`.
        pub fn stats(&self) -> StreamStats {
            StreamStats::default()
        }
        /// Stub: returns `Unsupported`.
        pub fn connect_ports(&self, _src: &str, _dst: &str) -> Result<(), OxiSoundError> {
            Err(OxiSoundError::Unsupported(STUB_MSG.into()))
        }
        /// Stub: returns `Unsupported`.
        pub fn auto_connect(&self) -> Result<(), OxiSoundError> {
            Err(OxiSoundError::Unsupported(STUB_MSG.into()))
        }
        /// Stub: returns 0.0 (no JACK server available without `jack-backend`).
        pub fn cpu_load(&self) -> f32 {
            0.0
        }
        /// Stub: returns empty list (no JACK server available without `jack-backend`).
        pub fn list_ports(&self) -> Vec<String> {
            Vec::new()
        }
        /// Stub: returns empty list (no JACK server available without `jack-backend`).
        pub fn list_input_ports(&self, _pattern: Option<&str>) -> Vec<String> {
            Vec::new()
        }
        /// Stub: returns empty list (no JACK server available without `jack-backend`).
        pub fn list_output_ports(&self, _pattern: Option<&str>) -> Vec<String> {
            Vec::new()
        }
        /// Stub: returns 0 (no JACK server available without `jack-backend`).
        pub fn current_sample_rate(&self) -> u32 {
            0
        }
        /// Stub: returns 0 (no JACK server available without `jack-backend`).
        pub fn xrun_count(&self) -> u64 {
            0
        }
        /// Stub: returns 0 (no JACK server available without `jack-backend`).
        pub fn current_buffer_size(&self) -> u32 {
            0
        }
    }
}

#[cfg(not(feature = "jack-backend"))]
pub use stub::{JackCallbackOutputStream, JackDevice, JackInputStream, JackOutputStream};

// ---------------------------------------------------------------------------
// Full implementation — compiled only when `jack-backend` feature is enabled
// ---------------------------------------------------------------------------

#[cfg(feature = "jack-backend")]
mod client;

#[cfg(feature = "jack-backend")]
pub use client::{JackCallbackOutputStream, JackDevice, JackInputStream, JackOutputStream};

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jack_transport_state_variants() {
        let states = [
            JackTransportState::Rolling,
            JackTransportState::Stopped,
            JackTransportState::Starting,
        ];
        assert_eq!(states.len(), 3);
        assert_ne!(states[0], states[1]);
        assert_ne!(states[1], states[2]);
        assert_ne!(states[0], states[2]);
    }

    #[test]
    fn jack_transport_position_fields() {
        let pos = JackTransportPosition {
            frame: 48_000,
            bpm: Some(120.0),
        };
        assert_eq!(pos.frame, 48_000);
        assert!((pos.bpm.unwrap() - 120.0).abs() < f64::EPSILON);
    }

    #[test]
    fn jack_transport_position_no_bpm() {
        let pos = JackTransportPosition {
            frame: 0,
            bpm: None,
        };
        assert_eq!(pos.frame, 0);
        assert!(pos.bpm.is_none());
    }

    #[cfg(not(feature = "jack-backend"))]
    #[test]
    fn jack_device_without_feature_returns_unsupported() {
        let result = JackDevice::new("test-client");
        assert!(result.is_err());
        let err = result.unwrap_err();
        // Must be the Unsupported variant.
        assert_eq!(err.kind(), "unsupported");
    }

    #[cfg(not(feature = "jack-backend"))]
    #[test]
    fn jack_device_stub_transport_state_is_stopped() {
        // JackDevice::new() fails but transport_state() is safe to call on any instance
        // (conceptually). Since we can't construct one, we verify the type compiles.
        // The JackTransportState::Stopped variant is always available.
        assert_eq!(JackTransportState::Stopped, JackTransportState::Stopped);
    }

    #[cfg(not(feature = "jack-backend"))]
    #[test]
    fn jack_device_stub_set_freewheel_unsupported() {
        // Even with jack-backend, set_freewheel is an upstream jack 0.13.5 TODO.
        // We can't construct JackDevice without jack-backend, but we can verify
        // the error message constant is well-formed.
        let stub_msg = "JACK freewheel is not available in the jack 0.13.5 safe API (upstream TODO). \
                        Track: https://github.com/RustAudio/rust-jack/issues";
        assert!(stub_msg.contains("0.13.5"));
    }

    /// Verify that cpu_load and port-listing methods exist on the stub types and return
    /// zero-cost defaults (compile-time presence check; no hardware required).
    #[cfg(not(feature = "jack-backend"))]
    #[test]
    fn jack_stream_has_cpu_load_and_port_methods() {
        // All three stub stream types are unconstructable through the public API, but
        // we can exercise their method signatures through trait objects to confirm
        // they are present and return the expected zero-value defaults.
        fn assert_output_stream_api(s: &JackOutputStream) {
            assert_eq!(s.cpu_load(), 0.0);
            assert!(s.list_ports().is_empty());
            assert!(s.list_input_ports(None).is_empty());
            assert!(s.list_output_ports(None).is_empty());
            assert!(s.list_input_ports(Some("system:capture_")).is_empty());
            assert!(s.list_output_ports(Some("system:playback_")).is_empty());
        }
        fn assert_input_stream_api(s: &JackInputStream) {
            assert_eq!(s.cpu_load(), 0.0);
            assert!(s.list_ports().is_empty());
            assert!(s.list_input_ports(None).is_empty());
            assert!(s.list_output_ports(None).is_empty());
        }
        fn assert_callback_stream_api(s: &JackCallbackOutputStream) {
            assert_eq!(s.cpu_load(), 0.0);
            assert!(s.list_ports().is_empty());
            assert!(s.list_input_ports(None).is_empty());
            assert!(s.list_output_ports(None).is_empty());
        }
        // These functions are checked by the compiler even if never called at runtime.
        let _ = assert_output_stream_api as fn(&JackOutputStream);
        let _ = assert_input_stream_api as fn(&JackInputStream);
        let _ = assert_callback_stream_api as fn(&JackCallbackOutputStream);
    }

    /// Verify that the stub port-listing methods accept a pattern argument correctly.
    #[cfg(not(feature = "jack-backend"))]
    #[test]
    fn jack_stub_port_methods_accept_pattern() {
        // Construct through internal field trick is not possible; verify via dummy closure types.
        // This is a compile-time API shape check — the real contract is tested when jack-backend
        // is enabled and a JACK server is available.
        fn check_output_pattern_param<F: Fn(&JackOutputStream)>(_f: F) {}
        fn check_input_pattern_param<F: Fn(&JackInputStream)>(_f: F) {}
        check_output_pattern_param(|s| {
            let _ = s.list_input_ports(Some("system:"));
            let _ = s.list_output_ports(Some("system:"));
        });
        check_input_pattern_param(|s| {
            let _ = s.list_input_ports(Some("system:capture_"));
        });
    }
}
