//! Pure-Rust traits, types, and error enum for the OxiSound audio device I/O workspace.
//!
//! # Feature flags
//!
//! - `std` (default) — links the Rust standard library; enables `OxiSoundError::Io`.
//! - `tokio` — enables `AsyncOutputStream` and `AsyncInputStream` traits backed by
//!   `futures_core::Stream`. Does NOT pull in the `tokio` runtime itself; callers choose
//!   their executor. Implies `std`.
//! - `serde` — enables `serde::Serialize` / `serde::Deserialize` derives on
//!   [`DeviceInfo`], [`StreamConfig`], and [`HostApi`].
//!
//! # `no_std` support
//!
//! Build with `--no-default-features` (or any feature set that omits `std`) to get a
//! `no_std` build backed by `alloc` + `core`.  `OxiSoundError::Io` is only available
//! when the `std` feature is enabled.  thiserror 2.x auto-selects `core::error::Error`
//! on MSRV ≥ 1.81 builds without `std`.
#![forbid(unsafe_code)]
#![no_std]
extern crate alloc;
#[cfg(feature = "std")]
extern crate std;

use alloc::boxed::Box;
use alloc::collections::VecDeque;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use core::fmt;

#[cfg(feature = "oxiaudio")]
mod oxiaudio_bridge;

/// Native sample format for audio streams.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum SampleFormat {
    /// 32-bit IEEE float, range [-1.0, 1.0].
    F32,
    /// Signed 16-bit integer.
    I16,
    /// 24-bit signed integer (stored as 3 bytes, but cpal uses i32 internally).
    I24,
    /// Signed 32-bit integer.
    I32,
    /// Unsigned 8-bit integer.
    U8,
    /// 64-bit IEEE float.
    F64,
}

impl fmt::Display for SampleFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            SampleFormat::F32 => "f32",
            SampleFormat::I16 => "i16",
            SampleFormat::I24 => "I24",
            SampleFormat::I32 => "i32",
            SampleFormat::U8 => "u8",
            SampleFormat::F64 => "f64",
        })
    }
}

impl SampleFormat {
    /// Size of one sample in bytes.
    ///
    /// # Examples
    /// ```
    /// use oxisound_core::SampleFormat;
    /// assert_eq!(SampleFormat::F32.byte_size(), 4);
    /// assert_eq!(SampleFormat::U8.byte_size(), 1);
    /// assert_eq!(SampleFormat::F64.byte_size(), 8);
    /// ```
    #[must_use]
    pub const fn byte_size(self) -> usize {
        match self {
            SampleFormat::U8 => 1,
            SampleFormat::I16 => 2,
            SampleFormat::I24 => 3,
            SampleFormat::F32 | SampleFormat::I32 => 4,
            SampleFormat::F64 => 8,
        }
    }

    /// Returns `true` if this is a floating-point format (`F32` or `F64`).
    ///
    /// # Examples
    /// ```
    /// use oxisound_core::SampleFormat;
    /// assert!(SampleFormat::F32.is_float());
    /// assert!(!SampleFormat::I16.is_float());
    /// ```
    #[must_use]
    pub const fn is_float(self) -> bool {
        matches!(self, SampleFormat::F32 | SampleFormat::F64)
    }
}

/// Picks the best-supported format from a ranked preference list.
///
/// Walks `preferred` in order and returns the first format that also appears in
/// `supported`. If no preferred format is supported (or `preferred` is empty),
/// falls back to the first entry of `supported`. Returns `None` only when
/// `supported` is empty.
///
/// # Examples
/// ```
/// use oxisound_core::{pick_preferred_format, SampleFormat};
/// let supported = [SampleFormat::I16, SampleFormat::F32];
/// let preferred = [SampleFormat::F32, SampleFormat::I32];
/// assert_eq!(
///     pick_preferred_format(&preferred, &supported),
///     Some(SampleFormat::F32),
/// );
/// // No preference → first supported.
/// assert_eq!(pick_preferred_format(&[], &supported), Some(SampleFormat::I16));
/// // No supported formats → None.
/// assert_eq!(pick_preferred_format(&preferred, &[]), None);
/// ```
#[must_use]
pub fn pick_preferred_format(
    preferred: &[SampleFormat],
    supported: &[SampleFormat],
) -> Option<SampleFormat> {
    for &fmt in preferred {
        if supported.contains(&fmt) {
            return Some(fmt);
        }
    }
    supported.first().copied()
}

/// Detailed device capability information.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DeviceCapabilities {
    /// Minimum supported buffer size in frames.
    pub min_buffer_size: Option<u32>,
    /// Maximum supported buffer size in frames.
    pub max_buffer_size: Option<u32>,
    /// Supported native sample formats.
    pub supported_formats: Vec<SampleFormat>,
    /// Whether exclusive (low-latency) mode is available.
    pub exclusive_mode: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DeviceInfo {
    pub name: String,
    pub is_default: bool,
    pub sample_rates: Vec<u32>,
    pub channel_counts: Vec<u16>,
    pub is_input: bool,
    pub is_output: bool,
    pub capabilities: Option<DeviceCapabilities>,
}

impl fmt::Display for DeviceInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let default_marker = if self.is_default { " [default]" } else { "" };
        let io = match (self.is_input, self.is_output) {
            (true, true) => " [in+out]",
            (true, false) => " [in]",
            (false, true) => " [out]",
            (false, false) => "",
        };
        write!(f, "{}{}{}", self.name, default_marker, io)
    }
}

impl DeviceInfo {
    /// Returns true if this device can support the given stream configuration.
    ///
    /// # Examples
    ///
    /// ```
    /// use oxisound_core::{DeviceInfo, StreamConfig};
    /// let mut info = DeviceInfo::builder("Speaker".to_string()).build();
    /// info.sample_rates = vec![44_100, 48_000];
    /// info.is_output = true;
    /// let config = StreamConfig::builder().sample_rate(44_100).channels(2).build();
    /// assert!(info.supports_config(&config));
    /// ```
    #[must_use]
    pub fn supports_config(&self, config: &StreamConfig) -> bool {
        if !self.channel_counts.is_empty() && !self.channel_counts.contains(&config.channels) {
            return false;
        }
        if !self.sample_rates.is_empty() {
            let min = self.sample_rates.iter().copied().min().unwrap_or(0);
            let max = self.sample_rates.iter().copied().max().unwrap_or(u32::MAX);
            if config.sample_rate < min || config.sample_rate > max {
                return false;
            }
        }
        true
    }
}

/// Builder for [`DeviceInfo`].
///
/// Prefer this over struct literal construction to avoid compile errors when new
/// fields are added in future versions.
///
/// # Examples
///
/// ```
/// use oxisound_core::DeviceInfo;
/// let info = DeviceInfo::builder("Built-in Audio")
///     .default_device()
///     .output(true)
///     .sample_rates(vec![44_100, 48_000])
///     .channel_counts(vec![1, 2])
///     .build();
/// assert_eq!(info.name, "Built-in Audio");
/// assert!(info.is_default);
/// ```
pub struct DeviceInfoBuilder {
    inner: DeviceInfo,
}

impl DeviceInfo {
    /// Creates a builder for constructing a [`DeviceInfo`].
    ///
    /// # Examples
    ///
    /// ```
    /// use oxisound_core::DeviceInfo;
    /// let info = DeviceInfo::builder("Built-in Microphone".to_string())
    ///     .input(true)
    ///     .sample_rates(vec![44_100, 48_000])
    ///     .build();
    /// assert_eq!(info.name, "Built-in Microphone");
    /// assert!(info.is_input);
    /// ```
    pub fn builder(name: impl Into<String>) -> DeviceInfoBuilder {
        DeviceInfoBuilder {
            inner: DeviceInfo {
                name: name.into(),
                ..Default::default()
            },
        }
    }
}

impl DeviceInfoBuilder {
    /// Marks this device as the system default.
    ///
    /// # Examples
    ///
    /// ```
    /// use oxisound_core::DeviceInfo;
    /// let info = DeviceInfo::builder("Speakers").default_device().output(true).build();
    /// assert!(info.is_default);
    /// ```
    pub fn default_device(mut self) -> Self {
        self.inner.is_default = true;
        self
    }

    /// Sets whether this device supports audio input.
    ///
    /// # Examples
    ///
    /// ```
    /// use oxisound_core::DeviceInfo;
    /// let info = DeviceInfo::builder("Microphone").input(true).build();
    /// assert!(info.is_input);
    /// assert!(!info.is_output);
    /// ```
    pub fn input(mut self, v: bool) -> Self {
        self.inner.is_input = v;
        self
    }

    /// Sets whether this device supports audio output.
    ///
    /// # Examples
    ///
    /// ```
    /// use oxisound_core::DeviceInfo;
    /// let info = DeviceInfo::builder("Speakers").output(true).build();
    /// assert!(info.is_output);
    /// assert!(!info.is_input);
    /// ```
    pub fn output(mut self, v: bool) -> Self {
        self.inner.is_output = v;
        self
    }

    /// Sets the supported sample rates (min and max endpoints).
    ///
    /// # Examples
    ///
    /// ```
    /// use oxisound_core::DeviceInfo;
    /// let info = DeviceInfo::builder("Audio Interface")
    ///     .sample_rates(vec![44_100, 48_000, 96_000])
    ///     .build();
    /// assert!(info.sample_rates.contains(&48_000));
    /// ```
    pub fn sample_rates(mut self, rates: Vec<u32>) -> Self {
        self.inner.sample_rates = rates;
        self
    }

    /// Sets the supported channel counts.
    ///
    /// # Examples
    ///
    /// ```
    /// use oxisound_core::DeviceInfo;
    /// let info = DeviceInfo::builder("Audio Interface")
    ///     .channel_counts(vec![1, 2, 8])
    ///     .build();
    /// assert!(info.channel_counts.contains(&2));
    /// ```
    pub fn channel_counts(mut self, counts: Vec<u16>) -> Self {
        self.inner.channel_counts = counts;
        self
    }

    /// Sets the device capabilities.
    ///
    /// # Examples
    ///
    /// ```
    /// use oxisound_core::{DeviceCapabilities, DeviceInfo, SampleFormat};
    /// let caps = DeviceCapabilities {
    ///     min_buffer_size: Some(64),
    ///     max_buffer_size: Some(4096),
    ///     supported_formats: vec![SampleFormat::F32, SampleFormat::I16],
    ///     exclusive_mode: false,
    /// };
    /// let info = DeviceInfo::builder("Pro Audio").capabilities(caps).build();
    /// assert!(info.capabilities.is_some());
    /// ```
    pub fn capabilities(mut self, caps: DeviceCapabilities) -> Self {
        self.inner.capabilities = Some(caps);
        self
    }

    /// Builds the [`DeviceInfo`].
    ///
    /// # Examples
    ///
    /// ```
    /// use oxisound_core::DeviceInfo;
    /// let info = DeviceInfo::builder("Built-in Output")
    ///     .output(true)
    ///     .default_device()
    ///     .build();
    /// assert_eq!(info.name, "Built-in Output");
    /// ```
    pub fn build(self) -> DeviceInfo {
        self.inner
    }
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct StreamConfig {
    pub sample_rate: u32,
    pub channels: u16,
    pub buffer_size: Option<u32>,
    /// Native sample format; `None` means let the backend choose.
    pub sample_format: Option<SampleFormat>,
    /// `true` = exclusive (low-latency) mode; `false` = shared mode.
    pub exclusive: bool,
    /// Ranked list of preferred sample formats; empty means use backend default.
    #[cfg_attr(feature = "serde", serde(default))]
    pub preferred_formats: Vec<SampleFormat>,
    /// Optional channel routing for multi-channel devices.
    #[cfg_attr(feature = "serde", serde(default))]
    pub channel_routing: Option<ChannelRouting>,
    /// Ring buffer capacity in seconds. `None` uses the backend default (~2 seconds).
    #[cfg_attr(feature = "serde", serde(default))]
    pub buffer_capacity_secs: Option<f32>,
}

impl StreamConfig {
    /// 48 kHz stereo, no buffer size constraint.
    pub const STEREO_48K: StreamConfig = StreamConfig {
        sample_rate: 48_000,
        channels: 2,
        buffer_size: None,
        sample_format: None,
        exclusive: false,
        preferred_formats: Vec::new(),
        channel_routing: None,
        buffer_capacity_secs: None,
    };

    /// 44.1 kHz stereo, no buffer size constraint.
    pub const STEREO_44K: StreamConfig = StreamConfig {
        sample_rate: 44_100,
        channels: 2,
        buffer_size: None,
        sample_format: None,
        exclusive: false,
        preferred_formats: Vec::new(),
        channel_routing: None,
        buffer_capacity_secs: None,
    };

    /// 16 kHz mono, no buffer size constraint.
    pub const MONO_16K: StreamConfig = StreamConfig {
        sample_rate: 16_000,
        channels: 1,
        buffer_size: None,
        sample_format: None,
        exclusive: false,
        preferred_formats: Vec::new(),
        channel_routing: None,
        buffer_capacity_secs: None,
    };

    /// Returns a 48 kHz stereo config with automatic buffer sizing.
    ///
    /// # Examples
    ///
    /// ```
    /// use oxisound_core::StreamConfig;
    /// let config = StreamConfig::stereo_48k();
    /// assert_eq!(config.sample_rate, 48_000);
    /// assert_eq!(config.channels, 2);
    /// ```
    pub fn stereo_48k() -> Self {
        Self {
            sample_rate: 48_000,
            channels: 2,
            buffer_size: None,
            sample_format: None,
            exclusive: false,
            preferred_formats: Vec::new(),
            channel_routing: None,
            buffer_capacity_secs: None,
        }
    }

    /// Returns a 44.1 kHz stereo config (CD quality) with automatic buffer sizing.
    ///
    /// # Examples
    ///
    /// ```
    /// use oxisound_core::StreamConfig;
    /// let config = StreamConfig::stereo_44k();
    /// assert_eq!(config.sample_rate, 44_100);
    /// assert_eq!(config.channels, 2);
    /// ```
    pub fn stereo_44k() -> Self {
        Self {
            sample_rate: 44_100,
            channels: 2,
            buffer_size: None,
            sample_format: None,
            exclusive: false,
            preferred_formats: Vec::new(),
            channel_routing: None,
            buffer_capacity_secs: None,
        }
    }

    /// Returns a 16 kHz mono config suitable for voice/speech capture.
    ///
    /// # Examples
    ///
    /// ```
    /// use oxisound_core::StreamConfig;
    /// let config = StreamConfig::mono_16k();
    /// assert_eq!(config.sample_rate, 16_000);
    /// assert_eq!(config.channels, 1);
    /// ```
    pub fn mono_16k() -> Self {
        Self {
            sample_rate: 16_000,
            channels: 1,
            buffer_size: None,
            sample_format: None,
            exclusive: false,
            preferred_formats: Vec::new(),
            channel_routing: None,
            buffer_capacity_secs: None,
        }
    }

    /// Returns a stereo 48 kHz config with a 256-frame buffer for low-latency use (~5.3 ms at 48 kHz).
    ///
    /// # Examples
    ///
    /// ```
    /// use oxisound_core::StreamConfig;
    /// let config = StreamConfig::low_latency_stereo_48k();
    /// assert_eq!(config.buffer_size, Some(256));
    /// assert_eq!(config.sample_rate, 48_000);
    /// ```
    pub fn low_latency_stereo_48k() -> Self {
        Self {
            sample_rate: 48_000,
            channels: 2,
            buffer_size: Some(256),
            sample_format: None,
            exclusive: false,
            preferred_formats: Vec::new(),
            channel_routing: None,
            buffer_capacity_secs: None,
        }
    }

    /// Creates a builder for constructing a [`StreamConfig`].
    ///
    /// # Examples
    ///
    /// ```
    /// use oxisound_core::{StreamConfig, SampleFormat};
    /// let config = StreamConfig::builder()
    ///     .sample_rate(44_100)
    ///     .channels(2)
    ///     .buffer_size(256)
    ///     .sample_format(SampleFormat::F32)
    ///     .build();
    /// assert_eq!(config.sample_rate, 44_100);
    /// ```
    pub fn builder() -> StreamConfigBuilder {
        StreamConfigBuilder {
            inner: StreamConfig {
                sample_rate: 44_100,
                channels: 2,
                buffer_size: None,
                sample_format: None,
                exclusive: false,
                preferred_formats: Vec::new(),
                channel_routing: None,
                buffer_capacity_secs: None,
            },
        }
    }

    /// Validates this config against a device's known capabilities.
    ///
    /// Returns `Ok(())` if the config is compatible, or `Err(UnsupportedConfig)` with a
    /// descriptive message when channel count or sample rate is out of range.
    /// Unknown capabilities (empty `channel_counts` / `sample_rates`) are treated as "all valid".
    ///
    /// # Examples
    ///
    /// ```
    /// use oxisound_core::{DeviceInfo, StreamConfig};
    /// let mut info = DeviceInfo::builder("Speaker".to_string())
    ///     .sample_rates(vec![44_100, 48_000])
    ///     .channel_counts(vec![1, 2])
    ///     .output(true)
    ///     .build();
    ///
    /// // Compatible config passes.
    /// let ok_config = StreamConfig::stereo_48k();
    /// assert!(ok_config.validate(&info).is_ok());
    ///
    /// // Unsupported channel count fails.
    /// let bad_config = StreamConfig::builder().channels(8).sample_rate(48_000).build();
    /// assert!(bad_config.validate(&info).is_err());
    /// ```
    pub fn validate(&self, info: &DeviceInfo) -> Result<(), OxiSoundError> {
        // Channel check: skip if channel_counts is empty (unknown = pass)
        if !info.channel_counts.is_empty() && !info.channel_counts.contains(&self.channels) {
            return Err(OxiSoundError::UnsupportedConfig(format!(
                "channel count {} not supported; device supports: {:?}",
                self.channels, info.channel_counts
            )));
        }
        // Sample-rate check: skip if sample_rates is empty (unknown = pass)
        // sample_rates holds the distinct {min, max} endpoint values; validate that
        // min(sample_rates) <= self.sample_rate <= max(sample_rates)
        if !info.sample_rates.is_empty() {
            let min_rate = info.sample_rates.iter().copied().min().unwrap_or(0);
            let max_rate = info.sample_rates.iter().copied().max().unwrap_or(u32::MAX);
            if self.sample_rate < min_rate || self.sample_rate > max_rate {
                return Err(OxiSoundError::UnsupportedConfig(format!(
                    "sample rate {} Hz not supported; device range: {}–{} Hz",
                    self.sample_rate, min_rate, max_rate
                )));
            }
        }
        Ok(())
    }
}

impl fmt::Display for StreamConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let buf = match self.buffer_size {
            Some(n) => format!("{n} frames"),
            None => "auto".to_string(),
        };
        write!(f, "{}Hz {}ch buf={}", self.sample_rate, self.channels, buf)?;
        if let Some(fmt) = self.sample_format {
            write!(f, " fmt={fmt}")?;
        }
        if self.exclusive {
            f.write_str(" excl")?;
        }
        if let Some(cap) = self.buffer_capacity_secs {
            write!(f, ", capacity={}s", cap)?;
        }
        Ok(())
    }
}

/// Builder for [`StreamConfig`] with fluent API.
pub struct StreamConfigBuilder {
    inner: StreamConfig,
}

impl StreamConfigBuilder {
    /// Sets the sample rate in Hz.
    ///
    /// # Examples
    ///
    /// ```
    /// use oxisound_core::StreamConfig;
    /// let config = StreamConfig::builder().sample_rate(96_000).channels(2).build();
    /// assert_eq!(config.sample_rate, 96_000);
    /// ```
    pub fn sample_rate(mut self, rate: u32) -> Self {
        self.inner.sample_rate = rate;
        self
    }

    /// Sets the number of interleaved channels.
    ///
    /// # Examples
    ///
    /// ```
    /// use oxisound_core::StreamConfig;
    /// let config = StreamConfig::builder().channels(1).build();
    /// assert_eq!(config.channels, 1);
    /// ```
    pub fn channels(mut self, ch: u16) -> Self {
        self.inner.channels = ch;
        self
    }

    /// Sets the preferred hardware buffer size in frames.
    ///
    /// `None` (the default) lets the backend choose an appropriate size.
    ///
    /// # Examples
    ///
    /// ```
    /// use oxisound_core::StreamConfig;
    /// let config = StreamConfig::builder().buffer_size(512).build();
    /// assert_eq!(config.buffer_size, Some(512));
    /// ```
    pub fn buffer_size(mut self, size: u32) -> Self {
        self.inner.buffer_size = Some(size);
        self
    }

    /// Sets the required native sample format.
    ///
    /// `None` (the default) lets the backend choose the most efficient format.
    ///
    /// # Examples
    ///
    /// ```
    /// use oxisound_core::{SampleFormat, StreamConfig};
    /// let config = StreamConfig::builder().sample_format(SampleFormat::I16).build();
    /// assert_eq!(config.sample_format, Some(SampleFormat::I16));
    /// ```
    pub fn sample_format(mut self, fmt: SampleFormat) -> Self {
        self.inner.sample_format = Some(fmt);
        self
    }

    /// Requests exclusive (low-latency) device access.
    ///
    /// When set, the backend will attempt to open the device in exclusive mode,
    /// bypassing the OS mixer for lowest possible latency. Falls back to shared
    /// mode if the device or platform does not support it.
    ///
    /// # Examples
    ///
    /// ```
    /// use oxisound_core::StreamConfig;
    /// let config = StreamConfig::builder().exclusive().build();
    /// assert!(config.exclusive);
    /// ```
    pub fn exclusive(mut self) -> Self {
        self.inner.exclusive = true;
        self
    }

    /// Sets the ranked list of preferred sample formats.
    ///
    /// The backend will try each format in order and open the stream with the first supported one.
    /// An empty list means "use backend default".
    ///
    /// # Examples
    ///
    /// ```
    /// use oxisound_core::{SampleFormat, StreamConfig};
    /// let config = StreamConfig::builder()
    ///     .preferred_formats(vec![SampleFormat::F32, SampleFormat::I16])
    ///     .build();
    /// assert_eq!(config.preferred_formats, vec![SampleFormat::F32, SampleFormat::I16]);
    /// ```
    pub fn preferred_formats(mut self, formats: Vec<SampleFormat>) -> Self {
        self.inner.preferred_formats = formats;
        self
    }

    /// Sets the channel routing map for multi-channel devices.
    ///
    /// # Examples
    ///
    /// ```
    /// use oxisound_core::{ChannelRouting, StreamConfig};
    /// let config = StreamConfig::builder()
    ///     .channels(2)
    ///     .channel_routing(ChannelRouting::stereo())
    ///     .build();
    /// assert!(config.channel_routing.is_some());
    /// ```
    pub fn channel_routing(mut self, routing: ChannelRouting) -> Self {
        self.inner.channel_routing = Some(routing);
        self
    }

    /// Sets the ring buffer capacity in seconds.
    ///
    /// Controls how many seconds of audio the internal ring buffer can hold.
    /// `None` (the default) uses the backend-chosen capacity (~2 seconds).
    ///
    /// # Examples
    ///
    /// ```
    /// use oxisound_core::StreamConfig;
    /// let config = StreamConfig::builder().buffer_capacity_secs(5.0).build();
    /// assert_eq!(config.buffer_capacity_secs, Some(5.0));
    /// ```
    pub fn buffer_capacity_secs(mut self, secs: f32) -> Self {
        self.inner.buffer_capacity_secs = Some(secs);
        self
    }

    /// Builds the final [`StreamConfig`].
    ///
    /// # Examples
    ///
    /// ```
    /// use oxisound_core::StreamConfig;
    /// let config = StreamConfig::builder()
    ///     .sample_rate(48_000)
    ///     .channels(2)
    ///     .build();
    /// assert_eq!(config.channels, 2);
    /// ```
    pub fn build(self) -> StreamConfig {
        self.inner
    }
}

/// The actual stream configuration negotiated with the hardware.
///
/// # Examples
///
/// ```
/// use oxisound_core::{NegotiatedConfig, SampleFormat};
/// let cfg = NegotiatedConfig {
///     sample_rate: 48_000,
///     channels: 2,
///     buffer_size: 256,
///     sample_format: SampleFormat::F32,
/// };
/// assert_eq!(cfg.sample_rate, 48_000);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct NegotiatedConfig {
    pub sample_rate: u32,
    pub channels: u16,
    pub buffer_size: u32,
    pub sample_format: SampleFormat,
}

/// Identifies the OS audio host/backend API.
///
/// Variants correspond to platform-native or opt-in audio backends.
/// `CoreAudio` is the default on macOS/iOS; `Wasapi` on Windows;
/// `Alsa` on Linux by default; `Jack`, `PipeWire`, `PulseAudio` require
/// opt-in Cargo features in the `oxisound-cpal` crate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum HostApi {
    /// macOS and iOS Core Audio (default on Apple platforms).
    CoreAudio,
    /// Windows Audio Session API (default on Windows).
    Wasapi,
    /// Steinberg ASIO low-latency driver (Windows, opt-in feature).
    Asio,
    /// Advanced Linux Sound Architecture (default on Linux).
    Alsa,
    /// JACK Audio Connection Kit (Linux/macOS, opt-in feature).
    Jack,
    /// PipeWire modern Linux audio server (Linux, opt-in feature).
    PipeWire,
    /// PulseAudio Linux sound server (Linux, opt-in feature).
    PulseAudio,
}

impl fmt::Display for HostApi {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = match self {
            HostApi::CoreAudio => "Core Audio",
            HostApi::Wasapi => "WASAPI",
            HostApi::Asio => "ASIO",
            HostApi::Alsa => "ALSA",
            HostApi::Jack => "JACK",
            HostApi::PipeWire => "PipeWire",
            HostApi::PulseAudio => "PulseAudio",
        };
        f.write_str(label)
    }
}

impl HostApi {
    /// Returns true if this host API is available on the current platform (compile-time check).
    ///
    /// # Examples
    ///
    /// ```
    /// use oxisound_core::HostApi;
    /// // On macOS: CoreAudio is available; WASAPI is not.
    /// let available = HostApi::CoreAudio.is_available();
    /// // `available` is true on macOS and iOS, false elsewhere.
    /// let _ = available;
    /// ```
    #[must_use]
    pub fn is_available(&self) -> bool {
        match self {
            Self::CoreAudio => cfg!(any(target_os = "macos", target_os = "ios")),
            Self::Wasapi => cfg!(target_os = "windows"),
            Self::Alsa => cfg!(target_os = "linux"),
            // Jack and Asio are feature-gated at the backend crate level (oxisound-cpal),
            // not in oxisound-core. Always report as unavailable here; backend crates
            // override or replace this via their own is_available logic.
            Self::Jack => false,
            Self::Asio => false,
            Self::PulseAudio => cfg!(target_os = "linux"),
            Self::PipeWire => cfg!(target_os = "linux"),
        }
    }
}

/// Runtime statistics for an audio stream.
#[derive(Debug, Clone, Copy, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct StreamStats {
    pub frames_processed: u64,
    pub underruns: u64,
    pub overruns: u64,
    pub latency_frames: u32,
    pub cpu_load_percent: f32,
}

/// Thread priority hint for audio callbacks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum CallbackPriority {
    #[default]
    Normal,
    Realtime,
}

pub trait AudioDevice: Sized {
    fn enumerate() -> Result<Vec<DeviceInfo>, OxiSoundError>;
    fn default_output() -> Result<Self, OxiSoundError>;
    fn default_input() -> Result<Self, OxiSoundError>;
    fn open_output(&self, config: StreamConfig) -> Result<Box<dyn OutputStream>, OxiSoundError>;
    fn open_input(&self, config: StreamConfig) -> Result<Box<dyn InputStream>, OxiSoundError>;
    fn open_duplex(&self, config: StreamConfig) -> Result<Box<dyn DuplexStream>, OxiSoundError>;
    /// Pre-flight negotiation: returns the actual config the backend would use for this request.
    /// Returns `Err(UnsupportedConfig)` if the config cannot be satisfied.
    fn negotiate_output(&self, _config: StreamConfig) -> Result<NegotiatedConfig, OxiSoundError> {
        Err(OxiSoundError::UnsupportedConfig(
            "negotiate_output not implemented for this backend".into(),
        ))
    }
}

pub trait OutputStream: Send {
    fn write(&mut self, samples: &[f32]) -> Result<(), OxiSoundError>;
    fn stats(&self) -> StreamStats {
        StreamStats::default()
    }
}

pub trait InputStream: Send {
    fn read(&mut self, samples: &mut [f32]) -> Result<usize, OxiSoundError>;
    fn stats(&self) -> StreamStats {
        StreamStats::default()
    }
}

/// A combined input + output stream for duplex audio I/O.
/// Bound `Send` to allow passing to worker threads.
pub trait DuplexStream: Send {
    fn write(&mut self, out: &[f32]) -> Result<(), OxiSoundError>;
    fn read(&mut self, inp: &mut [f32]) -> Result<usize, OxiSoundError>;
    fn stats(&self) -> StreamStats {
        StreamStats::default()
    }
}

/// Async write-only audio output stream.
///
/// Implemented by async-capable output backends. NOT object-safe (uses RPITIT).
/// Use `impl AsyncOutputStream` in return positions rather than `Box<dyn AsyncOutputStream>`.
///
/// # Auto-trait bounds
///
/// Native `async fn` in traits (RPITIT) does not propagate `Send` on the returned future.
/// Callers that require `Send` futures should constrain the impl: `T: AsyncOutputStream` where
/// `for<'a> <T as AsyncOutputStream>::write(…): Send`.
#[cfg(feature = "tokio")]
#[expect(
    async_fn_in_trait,
    reason = "DeviceWatcher is not used as a trait object; the async fn in trait limitation is acknowledged"
)]
pub trait AsyncOutputStream: Send {
    /// Writes interleaved `f32` samples to the output stream asynchronously.
    async fn write(&mut self, samples: &[f32]) -> Result<(), OxiSoundError>;
}

/// Async read-only audio input stream that produces frames via a [`futures_core::Stream`].
///
/// NOT object-safe. Use `impl AsyncInputStream` in return positions.
#[cfg(feature = "tokio")]
pub trait AsyncInputStream: Send {
    /// Returns a [`futures_core::Stream`] of captured sample frames.
    fn stream(&mut self) -> impl futures_core::Stream<Item = Vec<f32>> + '_;
}

/// Pluggable strategy for selecting a device from a list.
pub trait DeviceSelector {
    /// Returns the index of the selected device, or `None` if no device matches.
    fn select(&self, devices: &[DeviceInfo]) -> Option<usize>;
}

/// Selects the first device marked `is_default`. Falls back to index 0 if no default is set.
pub struct DefaultSelector;

impl DeviceSelector for DefaultSelector {
    fn select(&self, devices: &[DeviceInfo]) -> Option<usize> {
        if devices.is_empty() {
            return None;
        }
        devices.iter().position(|d| d.is_default).or(Some(0))
    }
}

/// Placeholder: selects the first available device. Latency-optimal selection
/// requires runtime buffer-size data not yet surfaced in `DeviceInfo`.
pub struct LatencyOptimalSelector;

impl DeviceSelector for LatencyOptimalSelector {
    fn select(&self, devices: &[DeviceInfo]) -> Option<usize> {
        if devices.is_empty() { None } else { Some(0) }
    }
}

/// Selects the first device whose name contains `fragment` (case-insensitive).
pub struct NameMatchSelector(pub String);

impl DeviceSelector for NameMatchSelector {
    fn select(&self, devices: &[DeviceInfo]) -> Option<usize> {
        let fragment = self.0.to_lowercase();
        devices
            .iter()
            .position(|d| d.name.to_lowercase().contains(&fragment))
    }
}

/// Logical audio channel identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Channel {
    FrontLeft,
    FrontRight,
    Center,
    Lfe,
    SurroundLeft,
    SurroundRight,
    BackLeft,
    BackRight,
}

impl Channel {
    /// Returns the conventional physical channel index for this logical channel
    /// in the ITU-R BS.775 / Microsoft surround layout (FL=0, FR=1, C=2, LFE=3,
    /// SL=4, SR=5, BL=6, BR=7).
    ///
    /// This is a hint only; actual routing is controlled by `ChannelRouting`.
    ///
    /// # Examples
    ///
    /// ```
    /// use oxisound_core::Channel;
    /// assert_eq!(Channel::FrontLeft.standard_index(), 0);
    /// assert_eq!(Channel::FrontRight.standard_index(), 1);
    /// assert_eq!(Channel::Lfe.standard_index(), 3);
    /// ```
    #[must_use]
    pub fn standard_index(self) -> usize {
        match self {
            Channel::FrontLeft => 0,
            Channel::FrontRight => 1,
            Channel::Center => 2,
            Channel::Lfe => 3,
            Channel::SurroundLeft => 4,
            Channel::SurroundRight => 5,
            Channel::BackLeft => 6,
            Channel::BackRight => 7,
        }
    }
}

impl fmt::Display for Channel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Channel::FrontLeft => "FL",
            Channel::FrontRight => "FR",
            Channel::Center => "C",
            Channel::Lfe => "LFE",
            Channel::SurroundLeft => "SL",
            Channel::SurroundRight => "SR",
            Channel::BackLeft => "BL",
            Channel::BackRight => "BR",
        })
    }
}

/// Maps logical channels to physical device channel indices.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ChannelRouting(pub Vec<(Channel, usize)>);

impl ChannelRouting {
    /// Stereo: FrontLeft→0, FrontRight→1.
    ///
    /// # Examples
    ///
    /// ```
    /// use oxisound_core::ChannelRouting;
    /// let routing = ChannelRouting::stereo();
    /// assert_eq!(routing.channel_count(), 2);
    /// ```
    pub fn stereo() -> Self {
        Self(vec![(Channel::FrontLeft, 0), (Channel::FrontRight, 1)])
    }

    /// 5.1 surround: FL→0, FR→1, C→2, LFE→3, SL→4, SR→5.
    ///
    /// # Examples
    ///
    /// ```
    /// use oxisound_core::ChannelRouting;
    /// let routing = ChannelRouting::surround_5_1();
    /// assert_eq!(routing.channel_count(), 6);
    /// ```
    pub fn surround_5_1() -> Self {
        Self(vec![
            (Channel::FrontLeft, 0),
            (Channel::FrontRight, 1),
            (Channel::Center, 2),
            (Channel::Lfe, 3),
            (Channel::SurroundLeft, 4),
            (Channel::SurroundRight, 5),
        ])
    }

    /// 7.1 surround: FL→0, FR→1, C→2, LFE→3, SL→4, SR→5, BL→6, BR→7.
    ///
    /// # Examples
    ///
    /// ```
    /// use oxisound_core::ChannelRouting;
    /// let routing = ChannelRouting::surround_7_1();
    /// assert_eq!(routing.channel_count(), 8);
    /// ```
    pub fn surround_7_1() -> Self {
        Self(vec![
            (Channel::FrontLeft, 0),
            (Channel::FrontRight, 1),
            (Channel::Center, 2),
            (Channel::Lfe, 3),
            (Channel::SurroundLeft, 4),
            (Channel::SurroundRight, 5),
            (Channel::BackLeft, 6),
            (Channel::BackRight, 7),
        ])
    }

    /// Number of channels in this routing.
    ///
    /// # Examples
    ///
    /// ```
    /// use oxisound_core::ChannelRouting;
    /// assert_eq!(ChannelRouting::stereo().channel_count(), 2);
    /// assert_eq!(ChannelRouting::surround_5_1().channel_count(), 6);
    /// ```
    #[must_use]
    pub fn channel_count(&self) -> usize {
        self.0.len()
    }

    /// Applies the channel routing to an interleaved f32 buffer in-place.
    ///
    /// Each frame has `channels` samples. For each mapping `(logical, physical)`,
    /// sample at logical position `logical.standard_index()` (clamped to the frame)
    /// is written to the frame slot for `physical` index (clamped to `channels - 1`).
    ///
    /// Mappings referencing physical indices >= `channels` are silently skipped.
    /// Frames are processed independently; unrouted output slots are zero-filled.
    ///
    /// # Arguments
    ///
    /// * `buf` – interleaved f32 sample buffer (length must be a multiple of `channels`)
    /// * `channels` – number of channels per frame
    ///
    /// # Examples
    ///
    /// ```
    /// use oxisound_core::ChannelRouting;
    /// // Two stereo frames: [L, R, L, R]
    /// let mut buf = vec![1.0f32, 2.0, 3.0, 4.0];
    /// ChannelRouting::stereo().apply_interleaved(&mut buf, 2);
    /// // FrontLeft→0, FrontRight→1: order unchanged for standard stereo.
    /// assert_eq!(buf[0], 1.0);
    /// assert_eq!(buf[1], 2.0);
    /// ```
    pub fn apply_interleaved(&self, buf: &mut [f32], channels: usize) {
        if channels == 0 || self.0.is_empty() {
            return;
        }
        let frame_count = buf.len() / channels;
        // Scratch buffer for one frame to avoid aliasing within the frame.
        let mut scratch = vec![0.0f32; channels];
        for frame_idx in 0..frame_count {
            let base = frame_idx * channels;
            let frame = &buf[base..base + channels];
            // Copy frame into scratch as-is first (identity pass for unrouted slots).
            scratch[..channels].copy_from_slice(frame);
            // Apply explicit routing: move logical channel to physical slot.
            for &(ref logical, physical) in &self.0 {
                let src = logical.standard_index();
                if src < channels && physical < channels {
                    scratch[physical] = frame[src];
                }
            }
            buf[base..base + channels].copy_from_slice(&scratch[..channels]);
        }
    }
}

impl fmt::Display for ChannelRouting {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s: Vec<String> = self
            .0
            .iter()
            .map(|(ch, idx)| format!("{ch}:{idx}"))
            .collect();
        f.write_str(&s.join(" "))
    }
}

/// An event emitted when the set of available audio devices changes.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum DeviceEvent {
    DeviceAdded(DeviceInfo),
    DeviceRemoved(String),
    DefaultChanged(DeviceInfo),
}

impl fmt::Display for DeviceEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DeviceEvent::DeviceAdded(d) => write!(f, "device added: {}", d.name),
            DeviceEvent::DeviceRemoved(name) => write!(f, "device removed: {name}"),
            DeviceEvent::DefaultChanged(d) => write!(f, "default changed: {}", d.name),
        }
    }
}

/// Synchronous device change notification callback.
pub trait DeviceNotificationCallback: Send {
    fn on_device_change(&self, event: DeviceEvent);
}

/// Async device watcher producing a stream of device change events.
#[cfg(feature = "tokio")]
pub trait DeviceWatcher: Send {
    fn events(&mut self) -> impl futures_core::Stream<Item = DeviceEvent> + '_;
}

/// Audio session category, primarily for iOS/macOS/Android session configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum SessionCategory {
    Playback,
    Record,
    PlayAndRecord,
    Ambient,
    SoloAmbient,
}

/// Audio session interruption event.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum SessionInterruptionEvent {
    Began,
    Ended { should_resume: bool },
}

/// Platform-specific audio session management (iOS, macOS, Android).
///
/// ## Implementation guide for oxisound-cpal
///
/// To wire up `AudioSession` on Apple platforms, an implementor in `oxisound-cpal` would:
///
/// 1. Call `AVAudioSession.sharedInstance()` via a thin `unsafe` Objective-C bridge crate
///    (e.g. `objc2-av-foundation`) — this is **C-FFI** and must be feature-gated per the
///    COOLJAPAN Pure Rust Policy (default features must remain 100% Pure Rust).
/// 2. Map [`SessionCategory`] variants to the corresponding `AVAudioSessionCategory` strings:
///    `Playback → "AVAudioSessionCategoryPlayback"`, `Record → "AVAudioSessionCategoryRecord"`, etc.
/// 3. Register an `AVAudioSessionInterruptionNotification` observer and translate Begin/End
///    notifications to [`SessionInterruptionEvent`] callbacks.
/// 4. On Android, use `AAudioStream_requestStart/Stop` via the `oboe` or `ndk` crate (also
///    feature-gated as C-FFI).
///
/// Until platform-specific implementations exist, this trait is a specification. Callers on
/// desktop platforms (Linux, Windows, macOS without the `avfoundation` feature) should expect
/// `OxiSoundError::Unsupported` from any implementor.
///
/// **Status:** Planned; awaiting `objc2-av-foundation` or `core-foundation` Pure Rust bindings.
pub trait AudioSession: Send {
    fn set_category(&self, category: SessionCategory) -> Result<(), OxiSoundError>;
    fn set_preferred_sample_rate(&self, rate: u32) -> Result<(), OxiSoundError>;
    fn set_preferred_buffer_duration(&self, secs: f64) -> Result<(), OxiSoundError>;
    fn on_interruption(&self, event: SessionInterruptionEvent);
}

/// Information about a MIDI device port.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MidiDeviceInfo {
    pub name: String,
    pub is_input: bool,
    pub is_output: bool,
    pub port_count: usize,
}

/// A single MIDI message with timestamp.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MidiMessage {
    /// Status byte (channel + message type).
    pub status: u8,
    /// Data bytes (up to 2 for most messages; variable for SysEx).
    pub data: Vec<u8>,
    /// Timestamp in microseconds since stream start.
    pub timestamp_micros: u64,
}

impl MidiMessage {
    /// Returns true if this is a System Exclusive message (status 0xF0).
    ///
    /// # Examples
    ///
    /// ```
    /// use oxisound_core::MidiMessage;
    /// let sysex = MidiMessage::new_sysex(&[0x41, 0x10]);
    /// assert!(sysex.is_sysex());
    /// let note_on = MidiMessage { status: 0x90, data: vec![60, 100], timestamp_micros: 0 };
    /// assert!(!note_on.is_sysex());
    /// ```
    pub fn is_sysex(&self) -> bool {
        self.status == 0xF0
    }

    /// Returns the SysEx payload bytes (excluding F0/F7 framing), or None if not a SysEx message.
    ///
    /// # Examples
    ///
    /// ```
    /// use oxisound_core::MidiMessage;
    /// let payload = &[0x41u8, 0x10, 0x42];
    /// let sysex = MidiMessage::new_sysex(payload);
    /// assert_eq!(sysex.sysex_payload(), Some(payload.as_ref()));
    ///
    /// let note = MidiMessage { status: 0x90, data: vec![60, 100], timestamp_micros: 0 };
    /// assert_eq!(note.sysex_payload(), None);
    /// ```
    pub fn sysex_payload(&self) -> Option<&[u8]> {
        if self.is_sysex() {
            Some(&self.data)
        } else {
            None
        }
    }

    /// Constructs a SysEx MidiMessage from a payload slice (no F0/F7 framing bytes in payload).
    ///
    /// # Examples
    ///
    /// ```
    /// use oxisound_core::MidiMessage;
    /// let msg = MidiMessage::new_sysex(&[0x41, 0x10, 0x42]);
    /// assert!(msg.is_sysex());
    /// assert_eq!(msg.data, &[0x41, 0x10, 0x42]);
    /// ```
    pub fn new_sysex(payload: &[u8]) -> Self {
        Self {
            status: 0xF0,
            data: payload.to_vec(),
            timestamp_micros: 0,
        }
    }

    /// Serializes this message to raw MIDI bytes.
    ///
    /// For SysEx: `[0xF0, ...payload..., 0xF7]`.
    /// For other messages: `[status, ...data...]`.
    ///
    /// # Examples
    ///
    /// ```
    /// use oxisound_core::MidiMessage;
    /// // Note On, channel 0, middle C, velocity 100.
    /// let note_on = MidiMessage { status: 0x90, data: vec![60, 100], timestamp_micros: 0 };
    /// assert_eq!(note_on.to_bytes(), vec![0x90, 60, 100]);
    ///
    /// // SysEx message with framing added.
    /// let sysex = MidiMessage::new_sysex(&[0x41, 0x10]);
    /// assert_eq!(sysex.to_bytes(), vec![0xF0, 0x41, 0x10, 0xF7]);
    /// ```
    pub fn to_bytes(&self) -> Vec<u8> {
        if self.is_sysex() {
            let mut bytes = Vec::with_capacity(self.data.len() + 2);
            bytes.push(0xF0);
            bytes.extend_from_slice(&self.data);
            bytes.push(0xF7);
            bytes
        } else {
            let mut bytes = Vec::with_capacity(self.data.len() + 1);
            bytes.push(self.status);
            bytes.extend_from_slice(&self.data);
            bytes
        }
    }
}

/// MIDI Clock timing tick (24 PPQN).
pub const MIDI_CLOCK: u8 = 0xF8;
/// MIDI Start message.
pub const MIDI_START: u8 = 0xFA;
/// MIDI Continue message.
pub const MIDI_CONTINUE: u8 = 0xFB;
/// MIDI Stop message.
pub const MIDI_STOP: u8 = 0xFC;

/// MIDI Clock tracker: computes BPM from timing ticks (0xF8) using a 24-tick sliding window.
///
/// Feed it messages via [`handle_message`](MidiClock::handle_message) or raw ticks via
/// [`tick`](MidiClock::tick). Call [`bpm`](MidiClock::bpm) to get the current tempo.
#[derive(Debug, Clone)]
pub struct MidiClock {
    tick_timestamps: VecDeque<u64>,
    running: bool,
}

impl MidiClock {
    /// Creates a new, stopped MidiClock.
    ///
    /// # Examples
    ///
    /// ```
    /// use oxisound_core::MidiClock;
    /// let clock = MidiClock::new();
    /// assert!(!clock.is_running());
    /// assert_eq!(clock.bpm(), None);
    /// ```
    pub fn new() -> Self {
        Self {
            tick_timestamps: VecDeque::with_capacity(25),
            running: false,
        }
    }

    /// Records a timing tick at the given timestamp (microseconds).
    ///
    /// Each MIDI Clock message (status 0xF8) represents one tick; 24 ticks = one beat.
    ///
    /// # Examples
    ///
    /// ```
    /// use oxisound_core::MidiClock;
    /// let mut clock = MidiClock::new();
    /// // Simulate 120 BPM: 24 ticks per beat, 500_000 µs per beat → 20_833 µs per tick.
    /// let tick_interval_us = 20_833u64;
    /// for i in 0..24 {
    ///     clock.tick(i * tick_interval_us);
    /// }
    /// assert!(clock.bpm().is_some());
    /// ```
    pub fn tick(&mut self, timestamp_micros: u64) {
        self.running = true;
        self.tick_timestamps.push_back(timestamp_micros);
        if self.tick_timestamps.len() > 24 {
            self.tick_timestamps.pop_front();
        }
    }

    /// Returns the current BPM, or None if fewer than 2 ticks have been received.
    ///
    /// Uses the average interval across the sliding 24-tick window: `BPM = 60_000_000 / (avg_interval_us * 24)`.
    ///
    /// # Examples
    ///
    /// ```
    /// use oxisound_core::MidiClock;
    /// let mut clock = MidiClock::new();
    /// assert_eq!(clock.bpm(), None); // not enough ticks yet
    ///
    /// // 120 BPM = 500_000 µs/beat ÷ 24 ticks/beat ≈ 20_833 µs/tick
    /// for i in 0..24u64 {
    ///     clock.tick(i * 20_833);
    /// }
    /// let bpm = clock.bpm().unwrap();
    /// assert!((bpm - 120.0).abs() < 1.0, "BPM should be ~120, got {bpm}");
    /// ```
    pub fn bpm(&self) -> Option<f64> {
        if self.tick_timestamps.len() < 2 {
            return None;
        }
        let first = *self.tick_timestamps.front()?;
        let last = *self.tick_timestamps.back()?;
        let span_us = last.saturating_sub(first);
        if span_us == 0 {
            return None;
        }
        let n_intervals = (self.tick_timestamps.len() - 1) as f64;
        let avg_interval_us = span_us as f64 / n_intervals;
        Some(60_000_000.0 / (avg_interval_us * 24.0))
    }

    /// Returns whether the clock has received a Start or Continue and not yet received a Stop.
    ///
    /// # Examples
    ///
    /// ```
    /// use oxisound_core::{MidiClock, MidiMessage, MIDI_START, MIDI_STOP};
    /// let mut clock = MidiClock::new();
    /// assert!(!clock.is_running());
    /// clock.handle_message(&MidiMessage { status: MIDI_START, data: vec![], timestamp_micros: 0 });
    /// assert!(clock.is_running());
    /// clock.handle_message(&MidiMessage { status: MIDI_STOP, data: vec![], timestamp_micros: 0 });
    /// assert!(!clock.is_running());
    /// ```
    pub fn is_running(&self) -> bool {
        self.running
    }

    /// Handles a MidiMessage, updating clock state based on its status byte.
    ///
    /// Dispatches on the status byte:
    /// - `0xF8` (Clock): records a tick timestamp.
    /// - `0xFA` (Start): clears tick history and sets running.
    /// - `0xFB` (Continue): sets running without clearing ticks.
    /// - `0xFC` (Stop): clears running flag.
    /// - Other: ignored.
    ///
    /// # Examples
    ///
    /// ```
    /// use oxisound_core::{MidiClock, MidiMessage, MIDI_CLOCK, MIDI_START};
    /// let mut clock = MidiClock::new();
    /// clock.handle_message(&MidiMessage { status: MIDI_START, data: vec![], timestamp_micros: 0 });
    /// assert!(clock.is_running());
    ///
    /// // Feed timing ticks at 120 BPM (≈20833 µs apart).
    /// for i in 0..24u64 {
    ///     clock.handle_message(&MidiMessage {
    ///         status: MIDI_CLOCK,
    ///         data: vec![],
    ///         timestamp_micros: i * 20_833,
    ///     });
    /// }
    /// assert!(clock.bpm().is_some());
    /// ```
    pub fn handle_message(&mut self, msg: &MidiMessage) {
        match msg.status {
            MIDI_CLOCK => self.tick(msg.timestamp_micros),
            MIDI_START => {
                self.tick_timestamps.clear();
                self.running = true;
            }
            MIDI_CONTINUE => self.running = true,
            MIDI_STOP => self.running = false,
            _ => {}
        }
    }
}

impl Default for MidiClock {
    fn default() -> Self {
        Self::new()
    }
}

/// Input side of the OxiSound MIDI device abstraction.
///
/// ## Integration with oxiaudio-decode MIDI parser (planned)
///
/// When `oxiaudio-decode` gains MIDI file parsing (a planned future crate), the
/// integration contract is:
///
/// - `oxiaudio-decode` produces a `MidiEventStream` of `(timestamp_secs: f64, MidiMessage)` pairs.
/// - Callers feed those messages to a synthesizer (e.g. `oxisound-midi`'s MIDI output port)
///   using [`MidiOutput::send`].
/// - Realtime hardware-received messages (from [`MidiInput::receive`]) can be interleaved with
///   file events by merging on the timestamp field.
/// - Clock synchronisation: drive [`MidiClock::handle_message`] with `F8` (timing clock) bytes
///   from the hardware port; the MIDI file sequencer should honour the resulting BPM.
///
/// Until that integration exists, use `oxisound-midi`'s `MidiHost` to enumerate ports and
/// `open_midi_input`/`open_midi_output` to connect to physical devices.
pub trait MidiInput: Send {
    fn receive(&mut self) -> Result<Option<MidiMessage>, OxiSoundError>;
}

/// Output side of the OxiSound MIDI device abstraction.
///
/// See [`MidiInput`] for the full integration contract with the future oxiaudio-decode parser.
pub trait MidiOutput: Send {
    fn send(&mut self, msg: &MidiMessage) -> Result<(), OxiSoundError>;
}

/// Trait for MIDI device enumeration and port opening.
pub trait MidiDevice: Sized {
    fn enumerate_midi() -> Result<Vec<MidiDeviceInfo>, OxiSoundError>;
    fn open_midi_input(port: usize) -> Result<Box<dyn MidiInput>, OxiSoundError>;
    fn open_midi_output(port: usize) -> Result<Box<dyn MidiOutput>, OxiSoundError>;
}

#[derive(Debug, thiserror::Error)]
pub enum OxiSoundError {
    #[error("no audio device available")]
    NoDevice,
    #[error("device error: {0}")]
    Device(String),
    #[error("stream error: {0}")]
    Stream(String),
    #[error("configuration not supported: {0}")]
    UnsupportedConfig(String),
    #[error("device disconnected: {0}")]
    Disconnected(String),
    #[error("buffer overrun: {0}")]
    Overrun(String),
    #[error("buffer underrun: {0}")]
    Underrun(String),
    #[error("hot-plug error: {0}")]
    HotPlugError(String),
    #[error("permission denied: {0}")]
    PermissionDenied(String),
    #[error("operation timed out: {0}")]
    Timeout(String),
    #[error("sample format mismatch: {0}")]
    FormatMismatch(String),
    /// I/O error from the platform layer; source chain is preserved via thiserror `#[from]`.
    /// Only available when the `std` feature is enabled.
    #[cfg(feature = "std")]
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    /// Feature not supported on this platform or configuration.
    #[error("Unsupported: {0}")]
    Unsupported(String),
}

impl OxiSoundError {
    /// Returns a short, stable identifier for this error variant.
    ///
    /// Suitable for use as a metrics tag, log field, or structured-logging key.
    /// The string is always lowercase kebab-case and does not change between releases.
    ///
    /// # Examples
    ///
    /// ```
    /// use oxisound_core::OxiSoundError;
    /// assert_eq!(OxiSoundError::NoDevice.kind(), "no-device");
    /// assert_eq!(OxiSoundError::Timeout("".into()).kind(), "timeout");
    /// ```
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            #[cfg(feature = "std")]
            Self::Io(_) => "io",
            Self::NoDevice => "no-device",
            Self::Device(_) => "device",
            Self::Stream(_) => "stream",
            Self::UnsupportedConfig(_) => "unsupported-config",
            Self::Disconnected(_) => "disconnected",
            Self::Overrun(_) => "overrun",
            Self::Underrun(_) => "underrun",
            Self::HotPlugError(_) => "hot-plug-error",
            Self::PermissionDenied(_) => "permission-denied",
            Self::Timeout(_) => "timeout",
            Self::FormatMismatch(_) => "format-mismatch",
            Self::Unsupported(_) => "unsupported",
        }
    }
}

#[cfg(test)]
mod tests;
