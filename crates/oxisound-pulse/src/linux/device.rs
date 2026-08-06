//! `PulseDevice` — enumeration and stream opening over the native protocol.

use std::ffi::{CStr, CString};
use std::sync::Arc;

use oxisound_core::{
    AudioDevice, DeviceInfo, DuplexStream, HostApi, InputStream, NegotiatedConfig, OutputStream,
    OxiSoundError, StreamConfig,
};
use pulseaudio::{Client, protocol};

use crate::format::PulseSampleFormat;
use crate::linux::conn::{DEFAULT_CLIENT_NAME, PulseConnection, connect_client, map_client_error};
use crate::linux::stream::{PulseDuplexStream, PulseInputStream, PulseOutputStream};
use crate::model::{PulseDeviceDescriptor, PulseDeviceRole, map_descriptors, predict_negotiated};
use crate::timeout::{PULSE_OP_TIMEOUT, block_on_timeout};

/// A PulseAudio sink or source, reached over the native protocol.
///
/// Each `PulseDevice` owns a connection to the server (one socket and one reactor
/// thread). Streams opened from it share that connection, so a duplex pair costs a single
/// socket. Dropping the device closes the connection once every stream opened from it has
/// also been dropped.
pub struct PulseDevice {
    connection: Arc<PulseConnection>,
    descriptor: PulseDeviceDescriptor,
}

impl std::fmt::Debug for PulseDevice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PulseDevice")
            .field("name", &self.descriptor.name)
            .field("role", &self.descriptor.role)
            .field("is_monitor", &self.descriptor.is_monitor)
            .finish()
    }
}

impl PulseDevice {
    /// Opens the server's default sink or source.
    ///
    /// `client_name` is what the server shows in mixers such as `pavucontrol`; pass
    /// [`DEFAULT_CLIENT_NAME`] for the standard `oxisound` label.
    ///
    /// # Errors
    ///
    /// Propagates connection failures, and returns [`OxiSoundError::NoDevice`] when the
    /// server has no usable default endpoint for `role`.
    pub fn open_default(role: PulseDeviceRole, client_name: &str) -> Result<Self, OxiSoundError> {
        let connection = connect_client(client_name)?;
        let client = connection.client();
        let server = fetch_server_info(client)?;
        let descriptor = match role {
            PulseDeviceRole::Sink => {
                let info = fetch_sink(client, protocol::DEFAULT_SINK.to_owned())?;
                sink_descriptor(&info, server.default_sink_name.as_deref())
            }
            PulseDeviceRole::Source => {
                let info = fetch_source(client, protocol::DEFAULT_SOURCE.to_owned())?;
                source_descriptor(&info, server.default_source_name.as_deref())
            }
        };
        Self::from_parts(connection, descriptor)
    }

    /// Opens a specific sink or source by its server-side name.
    ///
    /// The name is the one reported by [`DeviceInfo::name`] — for example
    /// `alsa_output.pci-0000_00_1f.3.analog-stereo`, or a monitor source's
    /// `…analog-stereo.monitor`.
    ///
    /// # Errors
    ///
    /// Returns [`OxiSoundError::NoDevice`] when no endpoint of that name exists, plus the
    /// usual connection errors.
    pub fn open_named(
        name: &str,
        role: PulseDeviceRole,
        client_name: &str,
    ) -> Result<Self, OxiSoundError> {
        let target = CString::new(name).map_err(|_| {
            OxiSoundError::Device(format!(
                "device name {name:?} contains an interior NUL byte"
            ))
        })?;
        let connection = connect_client(client_name)?;
        let client = connection.client();
        let server = fetch_server_info(client)?;
        let descriptor = match role {
            PulseDeviceRole::Sink => {
                let info = fetch_sink(client, target)?;
                sink_descriptor(&info, server.default_sink_name.as_deref())
            }
            PulseDeviceRole::Source => {
                let info = fetch_source(client, target)?;
                source_descriptor(&info, server.default_source_name.as_deref())
            }
        };
        Self::from_parts(connection, descriptor)
    }

    fn from_parts(
        connection: Arc<PulseConnection>,
        descriptor: PulseDeviceDescriptor,
    ) -> Result<Self, OxiSoundError> {
        if descriptor.channels == 0 || descriptor.sample_rate == 0 {
            return Err(OxiSoundError::NoDevice);
        }
        Ok(Self {
            connection,
            descriptor,
        })
    }

    /// Enumerates every sink and source, with the PulseAudio-specific detail that
    /// [`DeviceInfo`] has no room for.
    ///
    /// Unlike [`AudioDevice::enumerate`], degenerate endpoints are **not** filtered out
    /// here: this is the raw server view. Monitor sources are flagged by
    /// [`PulseDeviceDescriptor::is_monitor`].
    ///
    /// # Errors
    ///
    /// Propagates connection and round-trip failures.
    pub fn enumerate_details(
        client_name: &str,
    ) -> Result<Vec<PulseDeviceDescriptor>, OxiSoundError> {
        let connection = connect_client(client_name)?;
        let client = connection.client();
        let server = fetch_server_info(client)?;
        let sinks: Vec<protocol::SinkInfo> =
            block_on_timeout(client.list_sinks(), PULSE_OP_TIMEOUT, "GET_SINK_INFO_LIST")?
                .map_err(map_client_error)?;
        let sources: Vec<protocol::SourceInfo> = block_on_timeout(
            client.list_sources(),
            PULSE_OP_TIMEOUT,
            "GET_SOURCE_INFO_LIST",
        )?
        .map_err(map_client_error)?;

        let mut out = Vec::with_capacity(sinks.len() + sources.len());
        for sink in &sinks {
            out.push(sink_descriptor(sink, server.default_sink_name.as_deref()));
        }
        for source in &sources {
            out.push(source_descriptor(
                source,
                server.default_source_name.as_deref(),
            ));
        }
        Ok(out)
    }

    /// Enumerates every sink and source as [`DeviceInfo`].
    ///
    /// # Invariant
    ///
    /// Every returned entry has `is_input == true`, `is_output == true`, or both.
    /// PulseAudio's placeholder endpoints (zero channels or a zero sample rate — the
    /// `auto_null` dummy sink, or a card whose profile is off) are skipped with a
    /// `log::debug!` note, because they cannot be opened in either direction.
    ///
    /// # Errors
    ///
    /// Propagates connection and round-trip failures.
    pub fn enumerate_all_as(client_name: &str) -> Result<Vec<DeviceInfo>, OxiSoundError> {
        Ok(map_descriptors(&Self::enumerate_details(client_name)?))
    }

    /// Enumerates sinks only (`is_output == true`).
    ///
    /// # Errors
    ///
    /// Propagates connection and round-trip failures.
    pub fn enumerate_output() -> Result<Vec<DeviceInfo>, OxiSoundError> {
        Ok(Self::enumerate_all_as(DEFAULT_CLIENT_NAME)?
            .into_iter()
            .filter(|d| d.is_output)
            .collect())
    }

    /// Enumerates sources only (`is_input == true`), monitor sources included.
    ///
    /// # Errors
    ///
    /// Propagates connection and round-trip failures.
    pub fn enumerate_input() -> Result<Vec<DeviceInfo>, OxiSoundError> {
        Ok(Self::enumerate_all_as(DEFAULT_CLIENT_NAME)?
            .into_iter()
            .filter(|d| d.is_input)
            .collect())
    }

    /// Returns the descriptor of the endpoint this device is bound to.
    #[must_use]
    pub fn descriptor(&self) -> &PulseDeviceDescriptor {
        &self.descriptor
    }

    /// Returns the endpoint as a [`DeviceInfo`].
    ///
    /// Always `Some` for a live `PulseDevice`: degenerate endpoints are rejected at open
    /// time.
    #[must_use]
    pub fn device_info(&self) -> Option<DeviceInfo> {
        self.descriptor.to_device_info()
    }

    /// Returns [`HostApi::PulseAudio`].
    ///
    /// Provided for parity with `CpalDevice::host_api()`. Note that
    /// `CpalDevice::with_host(HostApi::PulseAudio)` still returns an error by design —
    /// cpal has no PulseAudio host, which is the reason this crate exists.
    #[must_use]
    pub fn host_api(&self) -> HostApi {
        HostApi::PulseAudio
    }

    /// Fetches the server's own information (name, version, defaults).
    ///
    /// # Errors
    ///
    /// Propagates round-trip failures.
    pub fn server_info(&self) -> Result<protocol::ServerInfo, OxiSoundError> {
        fetch_server_info(self.connection.client())
    }

    /// Opens a playback stream, returning the concrete type.
    ///
    /// # Errors
    ///
    /// Returns [`OxiSoundError::UnsupportedConfig`] when this device is a source, or when
    /// the config exceeds the protocol's channel/rate limits, and propagates stream
    /// creation failures.
    pub fn open_output_concrete(
        &self,
        config: StreamConfig,
    ) -> Result<PulseOutputStream, OxiSoundError> {
        if !self.descriptor.role.is_output() {
            return Err(OxiSoundError::UnsupportedConfig(format!(
                "{} is a PulseAudio source and cannot be opened for playback",
                self.descriptor.name
            )));
        }
        PulseOutputStream::open(Arc::clone(&self.connection), &self.descriptor, config)
    }

    /// Opens a capture stream, returning the concrete type.
    ///
    /// # Errors
    ///
    /// Returns [`OxiSoundError::UnsupportedConfig`] when this device is a sink, and
    /// propagates stream creation failures.
    pub fn open_input_concrete(
        &self,
        config: StreamConfig,
    ) -> Result<PulseInputStream, OxiSoundError> {
        if !self.descriptor.role.is_input() {
            return Err(OxiSoundError::UnsupportedConfig(format!(
                "{} is a PulseAudio sink; capture from its monitor source \
                 \"{}.monitor\" instead",
                self.descriptor.name, self.descriptor.name
            )));
        }
        PulseInputStream::open(Arc::clone(&self.connection), &self.descriptor, config)
    }

    /// Opens a playback and a capture stream on one connection.
    ///
    /// The half this device does not itself provide is taken from the server default:
    /// opening duplex on a sink pairs it with the default source, and vice versa.
    ///
    /// # Errors
    ///
    /// Propagates the errors of both halves.
    pub fn open_duplex_concrete(
        &self,
        config: StreamConfig,
    ) -> Result<PulseDuplexStream, OxiSoundError> {
        let (sink, source) = match self.descriptor.role {
            PulseDeviceRole::Sink => {
                let source_info = fetch_source(
                    self.connection.client(),
                    protocol::DEFAULT_SOURCE.to_owned(),
                )?;
                (
                    self.descriptor.clone(),
                    source_descriptor(&source_info, None),
                )
            }
            PulseDeviceRole::Source => {
                let sink_info =
                    fetch_sink(self.connection.client(), protocol::DEFAULT_SINK.to_owned())?;
                (sink_descriptor(&sink_info, None), self.descriptor.clone())
            }
        };
        let output = PulseOutputStream::open(Arc::clone(&self.connection), &sink, config.clone())?;
        let input = PulseInputStream::open(Arc::clone(&self.connection), &source, config)?;
        Ok(PulseDuplexStream::new(output, input))
    }
}

impl AudioDevice for PulseDevice {
    /// Enumerates every usable sink and source.
    ///
    /// See [`PulseDevice::enumerate_all_as`] for the role invariant.
    fn enumerate() -> Result<Vec<DeviceInfo>, OxiSoundError> {
        Self::enumerate_all_as(DEFAULT_CLIENT_NAME)
    }

    fn default_output() -> Result<Self, OxiSoundError> {
        Self::open_default(PulseDeviceRole::Sink, DEFAULT_CLIENT_NAME)
    }

    fn default_input() -> Result<Self, OxiSoundError> {
        Self::open_default(PulseDeviceRole::Source, DEFAULT_CLIENT_NAME)
    }

    fn open_output(&self, config: StreamConfig) -> Result<Box<dyn OutputStream>, OxiSoundError> {
        Ok(Box::new(self.open_output_concrete(config)?))
    }

    fn open_input(&self, config: StreamConfig) -> Result<Box<dyn InputStream>, OxiSoundError> {
        Ok(Box::new(self.open_input_concrete(config)?))
    }

    fn open_duplex(&self, config: StreamConfig) -> Result<Box<dyn DuplexStream>, OxiSoundError> {
        Ok(Box::new(self.open_duplex_concrete(config)?))
    }

    /// Predicts the negotiated configuration without contacting the server.
    ///
    /// See [`crate::predict_negotiated`] for why this is a prediction rather than a
    /// round-trip.
    fn negotiate_output(&self, config: StreamConfig) -> Result<NegotiatedConfig, OxiSoundError> {
        crate::model::validate_stream_config(&config)?;
        predict_negotiated(&config, self.descriptor.format)
    }
}

// ---------------------------------------------------------------------------
// Protocol → model conversion
// ---------------------------------------------------------------------------

fn fetch_server_info(client: &Client) -> Result<protocol::ServerInfo, OxiSoundError> {
    block_on_timeout(client.server_info(), PULSE_OP_TIMEOUT, "GET_SERVER_INFO")?
        .map_err(map_client_error)
}

fn fetch_sink(client: &Client, name: CString) -> Result<protocol::SinkInfo, OxiSoundError> {
    block_on_timeout(
        client.sink_info_by_name(name),
        PULSE_OP_TIMEOUT,
        "GET_SINK_INFO",
    )?
    .map_err(map_client_error)
}

fn fetch_source(client: &Client, name: CString) -> Result<protocol::SourceInfo, OxiSoundError> {
    block_on_timeout(
        client.source_info_by_name(name),
        PULSE_OP_TIMEOUT,
        "GET_SOURCE_INFO",
    )?
    .map_err(map_client_error)
}

/// Maps a PulseAudio wire sample format onto the subset this backend can convert.
///
/// Returns `None` for formats with no `f32` conversion here (A-law, µ-law, the
/// big-endian spellings and the 24-in-32 packings). The stream is then created with
/// [`PulseSampleFormat::Float32Le`] and PulseAudio converts server-side.
pub(crate) fn map_wire_format(format: protocol::SampleFormat) -> Option<PulseSampleFormat> {
    match format {
        protocol::SampleFormat::U8 => Some(PulseSampleFormat::U8),
        protocol::SampleFormat::S16Le => Some(PulseSampleFormat::S16Le),
        protocol::SampleFormat::S24Le => Some(PulseSampleFormat::S24Le),
        protocol::SampleFormat::S32Le => Some(PulseSampleFormat::S32Le),
        protocol::SampleFormat::Float32Le => Some(PulseSampleFormat::Float32Le),
        _ => None,
    }
}

/// The wire format for a negotiated [`PulseSampleFormat`].
pub(crate) fn to_wire_format(format: PulseSampleFormat) -> protocol::SampleFormat {
    match format {
        PulseSampleFormat::U8 => protocol::SampleFormat::U8,
        PulseSampleFormat::S16Le => protocol::SampleFormat::S16Le,
        PulseSampleFormat::S24Le => protocol::SampleFormat::S24Le,
        PulseSampleFormat::S32Le => protocol::SampleFormat::S32Le,
        PulseSampleFormat::Float32Le => protocol::SampleFormat::Float32Le,
    }
}

fn cstr_to_string(value: &CStr) -> String {
    value.to_string_lossy().into_owned()
}

fn channels_of(spec: &protocol::SampleSpec, map: &protocol::ChannelMap) -> u8 {
    if spec.channels > 0 {
        spec.channels
    } else {
        map.num_channels()
    }
}

fn sink_descriptor(
    info: &protocol::SinkInfo,
    default_sink: Option<&CStr>,
) -> PulseDeviceDescriptor {
    PulseDeviceDescriptor {
        name: cstr_to_string(&info.name),
        description: info.description.as_deref().map(cstr_to_string),
        index: info.index,
        role: PulseDeviceRole::Sink,
        is_monitor: false,
        sample_rate: info.sample_spec.sample_rate,
        channels: channels_of(&info.sample_spec, &info.channel_map),
        format: map_wire_format(info.sample_spec.format),
        is_default: default_sink == Some(info.name.as_c_str()),
    }
}

fn source_descriptor(
    info: &protocol::SourceInfo,
    default_source: Option<&CStr>,
) -> PulseDeviceDescriptor {
    PulseDeviceDescriptor {
        name: cstr_to_string(&info.name),
        description: info.description.as_deref().map(cstr_to_string),
        index: info.index,
        role: PulseDeviceRole::Source,
        is_monitor: info.monitor_of_sink_index.is_some() || info.monitor_of_sink_name.is_some(),
        sample_rate: info.sample_spec.sample_rate,
        channels: channels_of(&info.sample_spec, &info.channel_map),
        format: map_wire_format(info.sample_spec.format),
        is_default: default_source == Some(info.name.as_c_str()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(format: protocol::SampleFormat, channels: u8, rate: u32) -> protocol::SampleSpec {
        protocol::SampleSpec {
            format,
            channels,
            sample_rate: rate,
        }
    }

    #[test]
    fn wire_format_mapping_round_trips() {
        for fmt in [
            PulseSampleFormat::U8,
            PulseSampleFormat::S16Le,
            PulseSampleFormat::S24Le,
            PulseSampleFormat::S32Le,
            PulseSampleFormat::Float32Le,
        ] {
            assert_eq!(map_wire_format(to_wire_format(fmt)), Some(fmt));
        }
    }

    #[test]
    fn unconvertible_wire_formats_map_to_none() {
        for fmt in [
            protocol::SampleFormat::Alaw,
            protocol::SampleFormat::Ulaw,
            protocol::SampleFormat::S16Be,
            protocol::SampleFormat::Float32Be,
            protocol::SampleFormat::S24In32Le,
            protocol::SampleFormat::Invalid,
        ] {
            assert_eq!(map_wire_format(fmt), None, "{fmt:?} must not be claimed");
        }
    }

    #[test]
    fn sink_info_maps_to_an_output_descriptor() {
        let info = protocol::SinkInfo {
            index: 3,
            name: CString::new("alsa_output.analog-stereo").expect("name"),
            description: Some(CString::new("Built-in Audio").expect("desc")),
            sample_spec: spec(protocol::SampleFormat::S16Le, 2, 48_000),
            ..Default::default()
        };
        let default = CString::new("alsa_output.analog-stereo").expect("default");
        let desc = sink_descriptor(&info, Some(default.as_c_str()));
        assert_eq!(desc.role, PulseDeviceRole::Sink);
        assert!(!desc.is_monitor);
        assert!(desc.is_default);
        assert_eq!(desc.channels, 2);
        assert_eq!(desc.sample_rate, 48_000);
        assert_eq!(desc.format, Some(PulseSampleFormat::S16Le));
        assert_eq!(desc.description.as_deref(), Some("Built-in Audio"));

        let mapped = desc.to_device_info().expect("usable");
        assert!(mapped.is_output && !mapped.is_input);
    }

    #[test]
    fn monitor_source_is_flagged_and_stays_an_input() {
        let info = protocol::SourceInfo {
            index: 4,
            name: CString::new("alsa_output.analog-stereo.monitor").expect("name"),
            monitor_of_sink_index: Some(3),
            sample_spec: spec(protocol::SampleFormat::Float32Le, 2, 48_000),
            ..Default::default()
        };
        let desc = source_descriptor(&info, None);
        assert!(desc.is_monitor);
        assert!(!desc.is_default);
        let mapped = desc.to_device_info().expect("usable");
        assert!(mapped.is_input && !mapped.is_output);
        assert_eq!(mapped.name, "alsa_output.analog-stereo.monitor");
    }

    #[test]
    fn channel_count_falls_back_to_the_channel_map() {
        let mut map = protocol::ChannelMap::empty();
        map.push(protocol::ChannelPosition::FrontLeft);
        map.push(protocol::ChannelPosition::FrontRight);
        let info = protocol::SourceInfo {
            index: 5,
            name: CString::new("weird").expect("name"),
            sample_spec: spec(protocol::SampleFormat::S16Le, 0, 44_100),
            channel_map: map,
            ..Default::default()
        };
        assert_eq!(source_descriptor(&info, None).channels, 2);
    }

    #[test]
    fn dummy_sink_is_filtered_out_of_device_info() {
        let info = protocol::SinkInfo {
            index: 0,
            name: CString::new("auto_null").expect("name"),
            sample_spec: spec(protocol::SampleFormat::Invalid, 0, 0),
            ..Default::default()
        };
        assert!(sink_descriptor(&info, None).to_device_info().is_none());
    }
}
