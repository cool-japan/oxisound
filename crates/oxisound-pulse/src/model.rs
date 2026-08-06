//! Platform-independent device model, buffer arithmetic and `DeviceInfo` mapping.
//!
//! The Linux adapter converts `pulseaudio::protocol::{SinkInfo, SourceInfo, ServerInfo}`
//! into the plain structs defined here and then never touches protocol types again.
//! Everything below is ordinary Rust with no PulseAudio dependency, so all of the
//! mapping rules, the role invariant and the buffer-attribute arithmetic are unit-tested
//! on every host — including the macOS development machine, where no PulseAudio server
//! exists.

use oxisound_core::{
    DeviceCapabilities, DeviceInfo, NegotiatedConfig, OxiSoundError, StreamConfig,
};

use crate::format::{PulseSampleFormat, negotiate_format};

/// Default period size in frames when [`StreamConfig::buffer_size`] is `None`.
///
/// 1024 frames is ~21 ms at 48 kHz — the same order as PulseAudio's own default
/// `minreq` for a general-purpose (non-low-latency) stream.
pub const DEFAULT_PERIOD_FRAMES: u32 = 1024;

/// Smallest period this backend will ask for, in frames.
pub const MIN_PERIOD_FRAMES: u32 = 32;

/// Largest period this backend will ask for, in frames.
///
/// Bounds `tlength`/`maxlength` so a pathological `buffer_size` cannot ask the server
/// for a multi-second buffer.
pub const MAX_PERIOD_FRAMES: u32 = 65_536;

/// Number of periods the server-side playback buffer targets (`tlength`).
pub const PLAYBACK_TARGET_PERIODS: u32 = 4;

/// Number of periods the server-side playback buffer may grow to (`maxlength`).
pub const PLAYBACK_MAX_PERIODS: u32 = 8;

/// Default client-side ring capacity in seconds when
/// [`StreamConfig::buffer_capacity_secs`] is `None`.
///
/// Matches the `oxisound-cpal` ring default so both backends behave alike.
pub const DEFAULT_RING_CAPACITY_SECS: f32 = 2.0;

/// Maximum number of channels a PulseAudio stream may carry (`PA_CHANNELS_MAX`).
///
/// Cross-checked against `pulseaudio::protocol::MAX_CHANNELS` by a Linux-only test.
pub const PULSE_MAX_CHANNELS: u16 = 32;

/// Maximum sample rate a PulseAudio stream may use, in Hz (`PA_RATE_MAX`).
///
/// Cross-checked against `pulseaudio::protocol::MAX_RATE` by a Linux-only test.
pub const PULSE_MAX_SAMPLE_RATE: u32 = 48_000 * 16;

/// Whether a PulseAudio endpoint plays audio out (a sink) or captures it (a source).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PulseDeviceRole {
    /// A sink — an output endpoint.
    Sink,
    /// A source — an input endpoint. Monitor sources are also sources.
    Source,
}

impl PulseDeviceRole {
    /// `true` for [`PulseDeviceRole::Sink`].
    #[must_use]
    pub const fn is_output(self) -> bool {
        matches!(self, Self::Sink)
    }

    /// `true` for [`PulseDeviceRole::Source`].
    #[must_use]
    pub const fn is_input(self) -> bool {
        matches!(self, Self::Source)
    }
}

/// A PulseAudio sink or source, reduced to the fields OxiSound needs.
///
/// `name` is the server-side device name verbatim (for example
/// `alsa_output.pci-0000_00_1f.3.analog-stereo`), because that is the handle used to
/// open a stream on it. Monitor sources keep PulseAudio's own `.monitor` suffix and
/// additionally carry [`is_monitor`](Self::is_monitor).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PulseDeviceDescriptor {
    /// Server-side device name; the identifier used to open a stream.
    pub name: String,
    /// Human-readable description reported by the server, when it provides one.
    pub description: Option<String>,
    /// Server-internal index of the sink or source.
    pub index: u32,
    /// Whether this endpoint plays out or captures.
    pub role: PulseDeviceRole,
    /// `true` for a monitor source (the loopback of a sink's output).
    pub is_monitor: bool,
    /// The endpoint's native sample rate in Hz.
    pub sample_rate: u32,
    /// The endpoint's native channel count.
    pub channels: u8,
    /// The endpoint's native sample format, when this backend can encode it.
    pub format: Option<PulseSampleFormat>,
    /// `true` when the server reports this endpoint as the default sink/source.
    pub is_default: bool,
}

impl PulseDeviceDescriptor {
    /// Converts the descriptor to an [`oxisound_core::DeviceInfo`].
    ///
    /// Returns `None` for a degenerate endpoint — zero channels or a zero sample rate,
    /// which PulseAudio reports for the placeholder "dummy" sink it exposes when no
    /// hardware is present and for endpoints whose card profile is off. Such an entry
    /// cannot be opened in either direction, so emitting it would break the workspace
    /// invariant that every enumerated device has at least one role.
    ///
    /// # Mapping rules
    ///
    /// - Sinks set `is_output`; sources (including monitors) set `is_input`. Exactly
    ///   one role is set, never neither.
    /// - `sample_rates` and `channel_counts` carry the endpoint's **native**
    ///   sample-spec values only. PulseAudio resamples, remixes and reformats
    ///   server-side, so configurations outside these values also open successfully;
    ///   [`DeviceInfo::supports_config`] therefore under-reports on purpose rather than
    ///   advertising capabilities the hardware does not actually have.
    /// - `capabilities.supported_formats` carries the native format when this backend
    ///   can encode it.
    ///
    /// # Examples
    /// ```
    /// use oxisound_pulse::{PulseDeviceDescriptor, PulseDeviceRole, PulseSampleFormat};
    /// let sink = PulseDeviceDescriptor {
    ///     name: "alsa_output.analog-stereo".into(),
    ///     description: Some("Built-in Audio".into()),
    ///     index: 0,
    ///     role: PulseDeviceRole::Sink,
    ///     is_monitor: false,
    ///     sample_rate: 48_000,
    ///     channels: 2,
    ///     format: Some(PulseSampleFormat::S16Le),
    ///     is_default: true,
    /// };
    /// let info = sink.to_device_info().expect("usable sink");
    /// assert!(info.is_output && !info.is_input && info.is_default);
    /// assert_eq!(info.sample_rates, vec![48_000]);
    /// ```
    #[must_use]
    pub fn to_device_info(&self) -> Option<DeviceInfo> {
        if self.channels == 0 || self.sample_rate == 0 {
            log::debug!(
                "skipping degenerate PulseAudio {:?} \"{}\" ({} ch @ {} Hz): it cannot be \
                 opened in either direction",
                self.role,
                self.name,
                self.channels,
                self.sample_rate
            );
            return None;
        }

        let capabilities = DeviceCapabilities {
            min_buffer_size: Some(MIN_PERIOD_FRAMES),
            max_buffer_size: Some(MAX_PERIOD_FRAMES),
            supported_formats: self.format.map(|f| vec![f.to_core()]).unwrap_or_default(),
            // PulseAudio is a mixing server: there is no exclusive-mode equivalent.
            exclusive_mode: false,
        };

        let mut builder = DeviceInfo::builder(self.name.clone())
            .sample_rates(vec![self.sample_rate])
            .channel_counts(vec![u16::from(self.channels)])
            .capabilities(capabilities);
        builder = match self.role {
            PulseDeviceRole::Sink => builder.output(true),
            PulseDeviceRole::Source => builder.input(true),
        };
        if self.is_default {
            builder = builder.default_device();
        }
        Some(builder.build())
    }
}

/// Maps a descriptor list to [`DeviceInfo`]s, dropping degenerate endpoints.
///
/// # Invariant
///
/// Every returned entry satisfies `is_input || is_output`.
///
/// # Examples
/// ```
/// use oxisound_pulse::{map_descriptors, PulseDeviceDescriptor, PulseDeviceRole};
/// let broken = PulseDeviceDescriptor {
///     name: "auto_null".into(),
///     description: None,
///     index: 0,
///     role: PulseDeviceRole::Sink,
///     is_monitor: false,
///     sample_rate: 0,
///     channels: 0,
///     format: None,
///     is_default: false,
/// };
/// assert!(map_descriptors(&[broken]).is_empty());
/// ```
#[must_use]
pub fn map_descriptors(descriptors: &[PulseDeviceDescriptor]) -> Vec<DeviceInfo> {
    descriptors
        .iter()
        .filter_map(PulseDeviceDescriptor::to_device_info)
        .collect()
}

/// Server-side buffer attributes for a stream, in bytes.
///
/// Mirrors `pulseaudio::protocol::BufferAttr`; kept separate so the arithmetic is
/// testable without the protocol crate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PulseBufferAttrs {
    /// Maximum size of the server-side buffer.
    pub max_length: u32,
    /// Target fill level of the server-side playback buffer (playback only).
    pub target_length: u32,
    /// Bytes that must be buffered before playback starts (playback only).
    pub pre_buffering: u32,
    /// Smallest chunk the server will request (playback only).
    pub minimum_request_length: u32,
    /// Chunk size the server delivers captured audio in (record only).
    pub fragment_size: u32,
}

/// Resolves the period size in frames for a stream configuration.
///
/// Uses [`StreamConfig::buffer_size`] when set, otherwise [`DEFAULT_PERIOD_FRAMES`],
/// clamped to `[MIN_PERIOD_FRAMES, MAX_PERIOD_FRAMES]`.
///
/// # Examples
/// ```
/// use oxisound_core::StreamConfig;
/// use oxisound_pulse::{period_frames, DEFAULT_PERIOD_FRAMES, MIN_PERIOD_FRAMES};
/// assert_eq!(period_frames(&StreamConfig::stereo_48k()), DEFAULT_PERIOD_FRAMES);
/// let tiny = StreamConfig::builder().buffer_size(1).build();
/// assert_eq!(period_frames(&tiny), MIN_PERIOD_FRAMES);
/// ```
#[must_use]
pub fn period_frames(config: &StreamConfig) -> u32 {
    config
        .buffer_size
        .unwrap_or(DEFAULT_PERIOD_FRAMES)
        .clamp(MIN_PERIOD_FRAMES, MAX_PERIOD_FRAMES)
}

/// Computes the playback buffer attributes for a period size.
///
/// The server-side buffer targets [`PLAYBACK_TARGET_PERIODS`] periods and is capped at
/// [`PLAYBACK_MAX_PERIODS`]. Bounding `tlength` matters: it is the upper bound on how
/// much audio the server buffers ahead, hence on playback latency, and PulseAudio's own
/// default is around two seconds.
///
/// `pre_buffering` is one period, so playback starts as soon as a single period has been
/// written rather than waiting for the whole target buffer.
///
/// # Examples
/// ```
/// use oxisound_pulse::{playback_buffer_attrs, PulseSampleFormat};
/// // 512 frames × 2 ch × 4 bytes = 4096 bytes per period.
/// let attrs = playback_buffer_attrs(512, 2, PulseSampleFormat::Float32Le);
/// assert_eq!(attrs.minimum_request_length, 4096);
/// assert_eq!(attrs.target_length, 4 * 4096);
/// assert_eq!(attrs.max_length, 8 * 4096);
/// ```
#[must_use]
pub fn playback_buffer_attrs(
    period_frames: u32,
    channels: u16,
    format: PulseSampleFormat,
) -> PulseBufferAttrs {
    let period_bytes = period_bytes(period_frames, channels, format);
    PulseBufferAttrs {
        max_length: period_bytes.saturating_mul(PLAYBACK_MAX_PERIODS),
        target_length: period_bytes.saturating_mul(PLAYBACK_TARGET_PERIODS),
        pre_buffering: period_bytes,
        minimum_request_length: period_bytes,
        // Not meaningful for playback; the server ignores it.
        fragment_size: u32::MAX,
    }
}

/// Computes the record buffer attributes for a period size.
///
/// `fragment_size` is one period, so the server delivers captured audio in chunks of the
/// requested size instead of its ~2 s default.
///
/// # Examples
/// ```
/// use oxisound_pulse::{record_buffer_attrs, PulseSampleFormat};
/// let attrs = record_buffer_attrs(256, 1, PulseSampleFormat::S16Le);
/// assert_eq!(attrs.fragment_size, 512);
/// ```
#[must_use]
pub fn record_buffer_attrs(
    period_frames: u32,
    channels: u16,
    format: PulseSampleFormat,
) -> PulseBufferAttrs {
    let period_bytes = period_bytes(period_frames, channels, format);
    PulseBufferAttrs {
        max_length: period_bytes.saturating_mul(PLAYBACK_MAX_PERIODS),
        // Playback-only fields; `u32::MAX` tells the server to pick its own value.
        target_length: u32::MAX,
        pre_buffering: u32::MAX,
        minimum_request_length: u32::MAX,
        fragment_size: period_bytes,
    }
}

fn period_bytes(period_frames: u32, channels: u16, format: PulseSampleFormat) -> u32 {
    let frame_size = format.frame_size(channels) as u32;
    period_frames.saturating_mul(frame_size).max(frame_size)
}

/// Capacity in bytes of the client-side ring buffer shared with the PulseAudio reactor.
///
/// Derived from [`StreamConfig::buffer_capacity_secs`] (default
/// [`DEFAULT_RING_CAPACITY_SECS`]) and floored at [`PLAYBACK_MAX_PERIODS`] periods so a
/// tiny or nonsensical request still leaves room for the server's largest chunk.
///
/// # Examples
/// ```
/// use oxisound_core::StreamConfig;
/// use oxisound_pulse::{ring_capacity_bytes, PulseSampleFormat};
/// let config = StreamConfig::builder()
///     .sample_rate(48_000)
///     .channels(2)
///     .buffer_capacity_secs(1.0)
///     .build();
/// // 1 s × 48 000 frames × 2 ch × 4 bytes.
/// assert_eq!(ring_capacity_bytes(&config, PulseSampleFormat::Float32Le), 384_000);
/// ```
#[must_use]
pub fn ring_capacity_bytes(config: &StreamConfig, format: PulseSampleFormat) -> usize {
    let secs = config
        .buffer_capacity_secs
        .filter(|s| s.is_finite() && *s > 0.0)
        .unwrap_or(DEFAULT_RING_CAPACITY_SECS);
    let frame_size = format.frame_size(config.channels);
    let frames = (f64::from(config.sample_rate) * f64::from(secs))
        .ceil()
        .max(0.0) as usize;
    let floor = period_frames(config) as usize * PLAYBACK_MAX_PERIODS as usize;
    frames.max(floor).saturating_mul(frame_size)
}

/// Rounds a byte count down to a whole number of samples of `format`.
///
/// Capture data arrives from the server in arbitrary byte counts, so the client-side
/// ring can hold a partial sample at its tail. Popping only whole samples keeps the
/// stream aligned; the leftover bytes stay in the ring and join the next sample.
///
/// # Examples
/// ```
/// use oxisound_pulse::{whole_samples, PulseSampleFormat};
/// assert_eq!(whole_samples(5, PulseSampleFormat::Float32Le), 4);
/// assert_eq!(whole_samples(3, PulseSampleFormat::Float32Le), 0);
/// ```
#[must_use]
pub fn whole_samples(bytes: usize, format: PulseSampleFormat) -> usize {
    let width = format.bytes_per_sample();
    bytes - (bytes % width)
}

/// Checks a stream configuration against the PulseAudio protocol's hard limits.
///
/// The protocol encodes the channel count in a single byte and caps it at
/// [`PULSE_MAX_CHANNELS`]; the sample rate must be in `1..=`[`PULSE_MAX_SAMPLE_RATE`].
/// Values outside those ranges cannot be put on the wire at all, so they are rejected
/// before a socket is touched rather than producing a server-side protocol error (and,
/// for the channel count, a panic inside `ChannelMap::push`).
///
/// # Errors
///
/// Returns [`OxiSoundError::UnsupportedConfig`] describing the offending field.
///
/// # Examples
/// ```
/// use oxisound_core::StreamConfig;
/// use oxisound_pulse::validate_stream_config;
/// assert!(validate_stream_config(&StreamConfig::stereo_48k()).is_ok());
///
/// let too_many = StreamConfig::builder().channels(64).build();
/// assert!(validate_stream_config(&too_many).is_err());
/// ```
pub fn validate_stream_config(config: &StreamConfig) -> Result<(), OxiSoundError> {
    if config.channels == 0 || config.channels > PULSE_MAX_CHANNELS {
        return Err(OxiSoundError::UnsupportedConfig(format!(
            "channel count {} is outside the PulseAudio range 1..={PULSE_MAX_CHANNELS}",
            config.channels
        )));
    }
    if config.sample_rate == 0 || config.sample_rate > PULSE_MAX_SAMPLE_RATE {
        return Err(OxiSoundError::UnsupportedConfig(format!(
            "sample rate {} Hz is outside the PulseAudio range 1..={PULSE_MAX_SAMPLE_RATE} Hz",
            config.sample_rate
        )));
    }
    Ok(())
}

/// Predicts the configuration a stream would be created with, without contacting the
/// server.
///
/// PulseAudio only reports the final sample spec in the `CreatePlaybackStream` reply, so
/// this is a prediction from the negotiation rules, not a server round-trip. The server
/// may still substitute values (it will, for example, honour a `fix_rate` request); the
/// actual figures are available after opening via
/// `PulseOutputStream::negotiated()`.
///
/// # Errors
///
/// Returns [`OxiSoundError::FormatMismatch`] when the config asks for a sample format the
/// PulseAudio protocol cannot express.
///
/// # Examples
/// ```
/// use oxisound_core::{SampleFormat, StreamConfig};
/// use oxisound_pulse::{predict_negotiated, PulseSampleFormat};
/// let cfg = StreamConfig::stereo_48k();
/// let out = predict_negotiated(&cfg, Some(PulseSampleFormat::S16Le)).unwrap();
/// assert_eq!(out.sample_rate, 48_000);
/// assert_eq!(out.sample_format, SampleFormat::I16);
/// ```
pub fn predict_negotiated(
    config: &StreamConfig,
    native: Option<PulseSampleFormat>,
) -> Result<NegotiatedConfig, OxiSoundError> {
    let format = negotiate_format(config.sample_format, &config.preferred_formats, native)?;
    Ok(NegotiatedConfig {
        sample_rate: config.sample_rate,
        channels: config.channels,
        buffer_size: period_frames(config),
        sample_format: format.to_core(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxisound_core::SampleFormat;

    fn sink(name: &str, rate: u32, channels: u8) -> PulseDeviceDescriptor {
        PulseDeviceDescriptor {
            name: name.into(),
            description: Some(format!("{name} description")),
            index: 1,
            role: PulseDeviceRole::Sink,
            is_monitor: false,
            sample_rate: rate,
            channels,
            format: Some(PulseSampleFormat::S16Le),
            is_default: false,
        }
    }

    fn source(name: &str, monitor: bool) -> PulseDeviceDescriptor {
        PulseDeviceDescriptor {
            name: name.into(),
            description: None,
            index: 2,
            role: PulseDeviceRole::Source,
            is_monitor: monitor,
            sample_rate: 44_100,
            channels: 1,
            format: Some(PulseSampleFormat::Float32Le),
            is_default: false,
        }
    }

    #[test]
    fn sink_maps_to_output_only() {
        let info = sink("out", 48_000, 2).to_device_info().expect("usable");
        assert!(info.is_output);
        assert!(!info.is_input);
        assert_eq!(info.name, "out");
        assert_eq!(info.sample_rates, vec![48_000]);
        assert_eq!(info.channel_counts, vec![2]);
        let caps = info.capabilities.expect("capabilities present");
        assert_eq!(caps.supported_formats, vec![SampleFormat::I16]);
        assert!(!caps.exclusive_mode);
    }

    #[test]
    fn source_maps_to_input_only() {
        let info = source("in", false).to_device_info().expect("usable");
        assert!(info.is_input);
        assert!(!info.is_output);
    }

    #[test]
    fn monitor_source_is_an_input_and_keeps_its_pulseaudio_name() {
        let desc = source("alsa_output.analog-stereo.monitor", true);
        let info = desc.to_device_info().expect("usable");
        assert!(info.is_input, "monitor sources are inputs");
        assert!(!info.is_output);
        assert_eq!(
            info.name, "alsa_output.analog-stereo.monitor",
            "the server-side name must round-trip so the device can be opened by name"
        );
        assert!(desc.is_monitor);
    }

    #[test]
    fn default_flag_propagates() {
        let mut desc = sink("out", 48_000, 2);
        desc.is_default = true;
        assert!(desc.to_device_info().expect("usable").is_default);
    }

    #[test]
    fn degenerate_endpoints_are_dropped() {
        assert!(sink("zero-ch", 48_000, 0).to_device_info().is_none());
        assert!(sink("zero-rate", 0, 2).to_device_info().is_none());
    }

    #[test]
    fn role_invariant_holds_across_a_mixed_list() {
        let descriptors = vec![
            sink("good-sink", 48_000, 2),
            sink("dummy", 0, 0),
            source("good-source", false),
            source("monitor", true),
        ];
        let infos = map_descriptors(&descriptors);
        assert_eq!(infos.len(), 3, "the degenerate sink must be dropped");
        for info in &infos {
            assert!(
                info.is_input || info.is_output,
                "device {} escaped enumeration with no role",
                info.name
            );
        }
    }

    #[test]
    fn missing_native_format_yields_empty_supported_formats() {
        let mut desc = sink("odd", 48_000, 2);
        desc.format = None;
        let caps = desc
            .to_device_info()
            .expect("usable")
            .capabilities
            .expect("capabilities present");
        assert!(caps.supported_formats.is_empty());
    }

    #[test]
    fn period_frames_clamps_both_ends() {
        let low = StreamConfig::builder().buffer_size(1).build();
        let high = StreamConfig::builder().buffer_size(1_000_000).build();
        assert_eq!(period_frames(&low), MIN_PERIOD_FRAMES);
        assert_eq!(period_frames(&high), MAX_PERIOD_FRAMES);
        assert_eq!(
            period_frames(&StreamConfig::STEREO_48K),
            DEFAULT_PERIOD_FRAMES
        );
    }

    #[test]
    fn playback_attrs_bound_the_server_side_latency() {
        let attrs = playback_buffer_attrs(1024, 2, PulseSampleFormat::Float32Le);
        let period_bytes = 1024 * 2 * 4;
        assert_eq!(attrs.minimum_request_length, period_bytes);
        assert_eq!(attrs.pre_buffering, period_bytes);
        assert_eq!(attrs.target_length, period_bytes * PLAYBACK_TARGET_PERIODS);
        assert_eq!(attrs.max_length, period_bytes * PLAYBACK_MAX_PERIODS);
        assert!(
            attrs.target_length < 48_000 * 2 * 4,
            "tlength must stay well under one second of audio, got {} bytes",
            attrs.target_length
        );
    }

    #[test]
    fn record_attrs_set_fragment_size_and_defer_playback_fields() {
        let attrs = record_buffer_attrs(512, 2, PulseSampleFormat::S16Le);
        assert_eq!(attrs.fragment_size, 512 * 2 * 2);
        assert_eq!(attrs.target_length, u32::MAX);
        assert_eq!(attrs.pre_buffering, u32::MAX);
        assert_eq!(attrs.minimum_request_length, u32::MAX);
    }

    #[test]
    fn buffer_attrs_never_produce_a_zero_length() {
        // Zero channels is clamped to one by `frame_size`, so no field can be zero.
        let attrs = playback_buffer_attrs(MIN_PERIOD_FRAMES, 0, PulseSampleFormat::U8);
        assert!(attrs.minimum_request_length > 0);
        assert!(attrs.target_length > 0);
        assert!(attrs.max_length > 0);
    }

    #[test]
    fn ring_capacity_uses_requested_seconds() {
        let config = StreamConfig::builder()
            .sample_rate(48_000)
            .channels(2)
            .buffer_capacity_secs(0.5)
            .build();
        assert_eq!(
            ring_capacity_bytes(&config, PulseSampleFormat::Float32Le),
            24_000 * 2 * 4
        );
    }

    #[test]
    fn ring_capacity_floors_at_the_server_chunk_size() {
        let config = StreamConfig::builder()
            .sample_rate(48_000)
            .channels(2)
            .buffer_size(4096)
            .buffer_capacity_secs(0.000_001)
            .build();
        let bytes = ring_capacity_bytes(&config, PulseSampleFormat::Float32Le);
        assert!(
            bytes >= 4096 * PLAYBACK_MAX_PERIODS as usize * 2 * 4,
            "ring must hold at least {PLAYBACK_MAX_PERIODS} periods, got {bytes} bytes"
        );
    }

    #[test]
    fn ring_capacity_rejects_nonsense_seconds() {
        for secs in [-1.0f32, 0.0, f32::NAN, f32::INFINITY] {
            let config = StreamConfig::builder()
                .sample_rate(48_000)
                .channels(2)
                .buffer_capacity_secs(secs)
                .build();
            let bytes = ring_capacity_bytes(&config, PulseSampleFormat::Float32Le);
            assert_eq!(
                bytes,
                (48_000.0 * DEFAULT_RING_CAPACITY_SECS) as usize * 2 * 4,
                "secs={secs} should fall back to the default capacity"
            );
        }
    }

    #[test]
    fn whole_samples_rounds_down_per_format() {
        assert_eq!(whole_samples(0, PulseSampleFormat::S16Le), 0);
        assert_eq!(whole_samples(1, PulseSampleFormat::S16Le), 0);
        assert_eq!(whole_samples(7, PulseSampleFormat::S24Le), 6);
        assert_eq!(whole_samples(9, PulseSampleFormat::S32Le), 8);
        assert_eq!(whole_samples(5, PulseSampleFormat::U8), 5);
    }

    #[test]
    fn validation_rejects_out_of_range_channels_and_rates() {
        assert!(validate_stream_config(&StreamConfig::STEREO_48K).is_ok());
        assert!(validate_stream_config(&StreamConfig::MONO_16K).is_ok());

        let max_ok = StreamConfig::builder()
            .channels(PULSE_MAX_CHANNELS)
            .sample_rate(PULSE_MAX_SAMPLE_RATE)
            .build();
        assert!(validate_stream_config(&max_ok).is_ok());

        for bad in [
            StreamConfig::builder().channels(0).build(),
            StreamConfig::builder()
                .channels(PULSE_MAX_CHANNELS + 1)
                .build(),
            StreamConfig::builder().sample_rate(0).build(),
            StreamConfig::builder()
                .sample_rate(PULSE_MAX_SAMPLE_RATE + 1)
                .build(),
        ] {
            let err = validate_stream_config(&bad).expect_err("must be rejected");
            assert_eq!(
                err.kind(),
                "unsupported-config",
                "config {bad} was accepted"
            );
        }
    }

    #[test]
    fn predicted_config_follows_the_negotiation_rules() {
        let config = StreamConfig::builder()
            .sample_rate(44_100)
            .channels(1)
            .buffer_size(256)
            .build();
        let out = predict_negotiated(&config, Some(PulseSampleFormat::S24Le)).expect("predicts");
        assert_eq!(out.sample_rate, 44_100);
        assert_eq!(out.channels, 1);
        assert_eq!(out.buffer_size, 256);
        assert_eq!(out.sample_format, SampleFormat::I24);
    }

    #[test]
    fn predicted_config_rejects_unrepresentable_formats() {
        let config = StreamConfig::builder()
            .sample_format(SampleFormat::F64)
            .build();
        let err = predict_negotiated(&config, None).expect_err("f64 is not on the wire");
        assert_eq!(err.kind(), "format-mismatch");
    }
}
