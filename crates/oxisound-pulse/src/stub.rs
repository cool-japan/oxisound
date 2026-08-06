//! Pure-Rust stub compiled on every non-Linux target.
//!
//! The PulseAudio native protocol only exists on Linux (PulseAudio itself, or PipeWire's
//! `pipewire-pulse` shim), so the `pulseaudio` dependency is declared under
//! `[target.'cfg(target_os = "linux")'.dependencies]`. This module keeps the public API
//! shape identical everywhere so downstream crates can depend on `oxisound-pulse`
//! unconditionally and skip the `#[cfg]` boilerplate; every constructor returns
//! [`OxiSoundError::Unsupported`].
//!
//! This mirrors the stub pattern of the `oxisound-jack` quarantine crate.

use oxisound_core::{
    AudioDevice, DeviceInfo, DuplexStream, HostApi, InputStream, NegotiatedConfig, OutputStream,
    OxiSoundError, StreamConfig, StreamStats,
};

use crate::format::PulseSampleFormat;
use crate::model::{PulseBufferAttrs, PulseDeviceDescriptor, PulseDeviceRole};

/// Client name reported to the server when the caller does not choose one.
pub const DEFAULT_CLIENT_NAME: &str = "oxisound";

const STUB_MSG: &str = "oxisound-pulse speaks the PulseAudio native protocol, which is only \
                        available on Linux (pulseaudio, or pipewire-pulse). This target compiles \
                        the Pure-Rust stub, where every constructor returns Unsupported.";

fn unsupported<T>() -> Result<T, OxiSoundError> {
    Err(OxiSoundError::Unsupported(STUB_MSG.into()))
}

/// A PulseAudio sink or source. On this platform it cannot be constructed.
#[derive(Debug)]
pub struct PulseDevice {
    _private: (),
}

impl PulseDevice {
    /// Stub: returns [`OxiSoundError::Unsupported`].
    ///
    /// # Errors
    ///
    /// Always.
    pub fn open_default(_role: PulseDeviceRole, _client_name: &str) -> Result<Self, OxiSoundError> {
        unsupported()
    }

    /// Stub: returns [`OxiSoundError::Unsupported`].
    ///
    /// # Errors
    ///
    /// Always.
    pub fn open_named(
        _name: &str,
        _role: PulseDeviceRole,
        _client_name: &str,
    ) -> Result<Self, OxiSoundError> {
        unsupported()
    }

    /// Stub: returns [`OxiSoundError::Unsupported`].
    ///
    /// # Errors
    ///
    /// Always.
    pub fn enumerate_details(
        _client_name: &str,
    ) -> Result<Vec<PulseDeviceDescriptor>, OxiSoundError> {
        unsupported()
    }

    /// Stub: returns [`OxiSoundError::Unsupported`].
    ///
    /// # Errors
    ///
    /// Always.
    pub fn enumerate_all_as(_client_name: &str) -> Result<Vec<DeviceInfo>, OxiSoundError> {
        unsupported()
    }

    /// Stub: returns [`OxiSoundError::Unsupported`].
    ///
    /// # Errors
    ///
    /// Always.
    pub fn enumerate_output() -> Result<Vec<DeviceInfo>, OxiSoundError> {
        unsupported()
    }

    /// Stub: returns [`OxiSoundError::Unsupported`].
    ///
    /// # Errors
    ///
    /// Always.
    pub fn enumerate_input() -> Result<Vec<DeviceInfo>, OxiSoundError> {
        unsupported()
    }

    /// Stub: unreachable — no `PulseDevice` can be constructed on this platform.
    #[must_use]
    pub fn descriptor(&self) -> &PulseDeviceDescriptor {
        unreachable!("PulseDevice cannot be constructed on a non-Linux target")
    }

    /// Stub: always `None`.
    #[must_use]
    pub fn device_info(&self) -> Option<DeviceInfo> {
        None
    }

    /// Returns [`HostApi::PulseAudio`].
    #[must_use]
    pub fn host_api(&self) -> HostApi {
        HostApi::PulseAudio
    }

    /// Stub: returns [`OxiSoundError::Unsupported`].
    ///
    /// # Errors
    ///
    /// Always.
    pub fn open_output_concrete(
        &self,
        _config: StreamConfig,
    ) -> Result<PulseOutputStream, OxiSoundError> {
        unsupported()
    }

    /// Stub: returns [`OxiSoundError::Unsupported`].
    ///
    /// # Errors
    ///
    /// Always.
    pub fn open_input_concrete(
        &self,
        _config: StreamConfig,
    ) -> Result<PulseInputStream, OxiSoundError> {
        unsupported()
    }

    /// Stub: returns [`OxiSoundError::Unsupported`].
    ///
    /// # Errors
    ///
    /// Always.
    pub fn open_duplex_concrete(
        &self,
        _config: StreamConfig,
    ) -> Result<PulseDuplexStream, OxiSoundError> {
        unsupported()
    }
}

impl AudioDevice for PulseDevice {
    fn enumerate() -> Result<Vec<DeviceInfo>, OxiSoundError> {
        unsupported()
    }

    fn default_output() -> Result<Self, OxiSoundError> {
        unsupported()
    }

    fn default_input() -> Result<Self, OxiSoundError> {
        unsupported()
    }

    fn open_output(&self, _config: StreamConfig) -> Result<Box<dyn OutputStream>, OxiSoundError> {
        unsupported()
    }

    fn open_input(&self, _config: StreamConfig) -> Result<Box<dyn InputStream>, OxiSoundError> {
        unsupported()
    }

    fn open_duplex(&self, _config: StreamConfig) -> Result<Box<dyn DuplexStream>, OxiSoundError> {
        unsupported()
    }

    fn negotiate_output(&self, config: StreamConfig) -> Result<NegotiatedConfig, OxiSoundError> {
        // Negotiation is pure arithmetic and works everywhere, but there is no device to
        // negotiate against on this platform.
        let _ = config;
        unsupported()
    }
}

/// Stub playback stream. Cannot be constructed on this platform.
pub struct PulseOutputStream {
    _private: (),
}

impl std::fmt::Debug for PulseOutputStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PulseOutputStream(unsupported)")
    }
}

impl PulseOutputStream {
    /// Stub: unreachable.
    #[must_use]
    pub fn negotiated(&self) -> NegotiatedConfig {
        unreachable!("PulseOutputStream cannot be constructed on a non-Linux target")
    }

    /// Stub: unreachable.
    #[must_use]
    pub fn wire_format(&self) -> PulseSampleFormat {
        unreachable!("PulseOutputStream cannot be constructed on a non-Linux target")
    }

    /// Stub: unreachable.
    #[must_use]
    pub fn requested_buffer_attrs(&self) -> PulseBufferAttrs {
        unreachable!("PulseOutputStream cannot be constructed on a non-Linux target")
    }

    /// Stub: always 0.
    #[must_use]
    pub fn ring_capacity_bytes(&self) -> usize {
        0
    }

    /// Stub: always 0.
    #[must_use]
    pub fn latency_frames(&self) -> usize {
        0
    }

    /// Stub: always 0.
    #[must_use]
    pub fn underrun_count(&self) -> u64 {
        0
    }

    /// Stub: always `true` — there is no connection on this platform.
    #[must_use]
    pub fn is_disconnected(&self) -> bool {
        true
    }

    /// Stub: always 0.
    #[must_use]
    pub fn channel(&self) -> u32 {
        0
    }

    /// Stub: returns [`OxiSoundError::Unsupported`].
    ///
    /// # Errors
    ///
    /// Always.
    pub fn flush(&self) -> Result<(), OxiSoundError> {
        unsupported()
    }

    /// Stub: returns [`OxiSoundError::Unsupported`].
    ///
    /// # Errors
    ///
    /// Always.
    pub fn drain(&self) -> Result<(), OxiSoundError> {
        unsupported()
    }

    /// Stub: returns [`OxiSoundError::Unsupported`].
    ///
    /// # Errors
    ///
    /// Always.
    pub fn discard_buffered(&self) -> Result<(), OxiSoundError> {
        unsupported()
    }

    /// Stub: returns [`OxiSoundError::Unsupported`].
    ///
    /// # Errors
    ///
    /// Always.
    pub fn pause(&self) -> Result<(), OxiSoundError> {
        unsupported()
    }

    /// Stub: returns [`OxiSoundError::Unsupported`].
    ///
    /// # Errors
    ///
    /// Always.
    pub fn resume(&self) -> Result<(), OxiSoundError> {
        unsupported()
    }
}

impl OutputStream for PulseOutputStream {
    fn write(&mut self, _samples: &[f32]) -> Result<(), OxiSoundError> {
        unsupported()
    }

    fn stats(&self) -> StreamStats {
        StreamStats::default()
    }
}

/// Stub capture stream. Cannot be constructed on this platform.
pub struct PulseInputStream {
    _private: (),
}

impl std::fmt::Debug for PulseInputStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PulseInputStream(unsupported)")
    }
}

impl PulseInputStream {
    /// Stub: unreachable.
    #[must_use]
    pub fn negotiated(&self) -> NegotiatedConfig {
        unreachable!("PulseInputStream cannot be constructed on a non-Linux target")
    }

    /// Stub: unreachable.
    #[must_use]
    pub fn wire_format(&self) -> PulseSampleFormat {
        unreachable!("PulseInputStream cannot be constructed on a non-Linux target")
    }

    /// Stub: unreachable.
    #[must_use]
    pub fn requested_buffer_attrs(&self) -> PulseBufferAttrs {
        unreachable!("PulseInputStream cannot be constructed on a non-Linux target")
    }

    /// Stub: always 0.
    #[must_use]
    pub fn ring_capacity_bytes(&self) -> usize {
        0
    }

    /// Stub: always 0.
    #[must_use]
    pub fn latency_frames(&self) -> usize {
        0
    }

    /// Stub: always 0.
    #[must_use]
    pub fn overrun_count(&self) -> u64 {
        0
    }

    /// Stub: always `true` — there is no connection on this platform.
    #[must_use]
    pub fn is_disconnected(&self) -> bool {
        true
    }

    /// Stub: always 0.
    #[must_use]
    pub fn channel(&self) -> u32 {
        0
    }

    /// Stub: returns [`OxiSoundError::Unsupported`].
    ///
    /// # Errors
    ///
    /// Always.
    pub fn discard_buffered(&self) -> Result<(), OxiSoundError> {
        unsupported()
    }

    /// Stub: returns [`OxiSoundError::Unsupported`].
    ///
    /// # Errors
    ///
    /// Always.
    pub fn pause(&self) -> Result<(), OxiSoundError> {
        unsupported()
    }

    /// Stub: returns [`OxiSoundError::Unsupported`].
    ///
    /// # Errors
    ///
    /// Always.
    pub fn resume(&self) -> Result<(), OxiSoundError> {
        unsupported()
    }
}

impl InputStream for PulseInputStream {
    fn read(&mut self, _samples: &mut [f32]) -> Result<usize, OxiSoundError> {
        unsupported()
    }

    fn stats(&self) -> StreamStats {
        StreamStats::default()
    }
}

/// Stub duplex stream. Cannot be constructed on this platform.
pub struct PulseDuplexStream {
    _private: (),
}

impl std::fmt::Debug for PulseDuplexStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PulseDuplexStream(unsupported)")
    }
}

impl PulseDuplexStream {
    /// Stub: always 0.
    #[must_use]
    pub fn output_latency_frames(&self) -> usize {
        0
    }

    /// Stub: always 0.
    #[must_use]
    pub fn input_latency_frames(&self) -> usize {
        0
    }

    /// Stub: always 0.
    #[must_use]
    pub fn roundtrip_latency_frames(&self) -> usize {
        0
    }

    /// Stub: always `true`.
    #[must_use]
    pub fn is_disconnected(&self) -> bool {
        true
    }
}

impl DuplexStream for PulseDuplexStream {
    fn write(&mut self, _out: &[f32]) -> Result<(), OxiSoundError> {
        unsupported()
    }

    fn read(&mut self, _inp: &mut [f32]) -> Result<usize, OxiSoundError> {
        unsupported()
    }

    fn stats(&self) -> StreamStats {
        StreamStats::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_constructor_reports_unsupported() {
        assert_eq!(
            PulseDevice::open_default(PulseDeviceRole::Sink, DEFAULT_CLIENT_NAME)
                .expect_err("stub")
                .kind(),
            "unsupported"
        );
        assert_eq!(
            PulseDevice::open_named("sink", PulseDeviceRole::Sink, DEFAULT_CLIENT_NAME)
                .expect_err("stub")
                .kind(),
            "unsupported"
        );
        assert_eq!(
            PulseDevice::enumerate().expect_err("stub").kind(),
            "unsupported"
        );
        assert_eq!(
            PulseDevice::enumerate_details(DEFAULT_CLIENT_NAME)
                .expect_err("stub")
                .kind(),
            "unsupported"
        );
        assert_eq!(
            PulseDevice::enumerate_all_as(DEFAULT_CLIENT_NAME)
                .expect_err("stub")
                .kind(),
            "unsupported"
        );
        assert_eq!(
            PulseDevice::enumerate_output().expect_err("stub").kind(),
            "unsupported"
        );
        assert_eq!(
            PulseDevice::enumerate_input().expect_err("stub").kind(),
            "unsupported"
        );
        assert_eq!(
            PulseDevice::default_output().expect_err("stub").kind(),
            "unsupported"
        );
        assert_eq!(
            PulseDevice::default_input().expect_err("stub").kind(),
            "unsupported"
        );
    }

    #[test]
    fn stub_error_message_explains_the_platform_constraint() {
        let err = PulseDevice::default_output().expect_err("stub");
        let msg = err.to_string();
        assert!(
            msg.contains("Linux"),
            "message must name the platform: {msg}"
        );
        assert!(
            msg.contains("pipewire-pulse"),
            "message should mention the PipeWire shim: {msg}"
        );
    }

    #[test]
    fn stub_stream_types_are_send_and_implement_the_traits() {
        fn assert_send<T: Send>() {}
        assert_send::<PulseOutputStream>();
        assert_send::<PulseInputStream>();
        assert_send::<PulseDuplexStream>();

        fn assert_output<T: OutputStream>() {}
        fn assert_input<T: InputStream>() {}
        fn assert_duplex<T: DuplexStream>() {}
        assert_output::<PulseOutputStream>();
        assert_input::<PulseInputStream>();
        assert_duplex::<PulseDuplexStream>();
    }

    #[test]
    fn stub_stats_are_zeroed_defaults() {
        // The stream types are unconstructable, so exercise the shape through fn items
        // the compiler still type-checks.
        fn check_output(s: &PulseOutputStream) {
            assert_eq!(s.stats().frames_processed, 0);
            assert_eq!(s.underrun_count(), 0);
            assert!(s.is_disconnected());
        }
        fn check_input(s: &PulseInputStream) {
            assert_eq!(s.stats().overruns, 0);
            assert_eq!(s.overrun_count(), 0);
            assert!(s.is_disconnected());
        }
        fn check_duplex(s: &PulseDuplexStream) {
            assert_eq!(s.roundtrip_latency_frames(), 0);
            assert!(s.is_disconnected());
        }
        let _ = check_output as fn(&PulseOutputStream);
        let _ = check_input as fn(&PulseInputStream);
        let _ = check_duplex as fn(&PulseDuplexStream);
    }
}
