//! OxiSound public facade for COOLJAPAN audio device I/O.
//!
//! # Quick Start
//!
//! ```no_run
//! use oxisound::StreamConfig;
//!
//! // Play a sine tone
//! let mut stream = oxisound::open_output(StreamConfig::stereo_48k()).expect("no output device");
//! let buf = oxisound::sine_test_tone(440.0, 1.0, StreamConfig::stereo_48k());
//! stream.write(&buf).expect("write failed");
//! ```
//!
//! # Feature Flags
//!
//! - `pure` (default) — enables the cpal backend ([`CpalDevice`] etc.)
//! - `tokio` — enables async I/O (`async_output`, `capture_stream`)
//! - `pulse` (opt-in, **not** in `default`) — enables the Pure-Rust PulseAudio
//!   native-protocol backend ([`PulseDevice`], [`pulse_output`], [`pulse_input`],
//!   [`pulse_enumerate_devices`]). On Linux it is the only backend with no C library in
//!   the audio path; it also serves PipeWire through `pipewire-pulse`. On non-Linux
//!   targets the backend compiles as a Pure-Rust stub returning
//!   [`OxiSoundError::Unsupported`], so enabling the feature never breaks a build.
//! - JACK / ASIO are NOT facade features: native JACK lives in the `oxisound-jack` quarantine crate (depend on it directly); ASIO would require its own `oxisound-*-asio` quarantine crate.
//!
//! ## Platform Support
//!
//! | Feature | macOS | Linux | Windows | iOS | Android |
//! |---------|:-----:|:-----:|:-------:|:---:|:-------:|
//! | Output stream | ✓ | ✓ | ✓ | ✓ | — |
//! | Input stream | ✓ | ✓ | ✓ | ✓ | — |
//! | Callback mode | ✓ | ✓ | ✓ | ✓ | — |
//! | Device hot-plug | ✓ | ✓ | ✓ | — | — |
//! | Auto-reconnect | ✓ | ✓ | ✓ | — | — |
//! | PulseAudio / PipeWire (`pulse`) | — | ✓ | — | — | — |
//! | JACK (via `oxisound-jack`, not this facade) | ✓ | ✓ | — | — | — |
//! | ASIO (no quarantine crate exists yet) | — | — | — | — | — |
//! | Exclusive mode | — | — | planned | — | — |
//! | Loopback capture | — | ✓ (Pulse/PipeWire monitor source) | planned | — | — |
//!
//! # Test Tone Generators
//!
//! These pure-computation functions require no hardware:
//! - [`sine_test_tone`] — sine wave at a fixed frequency
//! - [`white_noise_test`] — deterministic white noise
//! - [`chirp_test_tone`] — linear frequency sweep
//! - [`silence`] — zero-filled buffer
//! - [`click_track`] — metronome-style click track
#![forbid(unsafe_code)]

pub use oxisound_core::{
    AudioDevice, CallbackPriority, Channel, ChannelRouting, DefaultSelector, DeviceCapabilities,
    DeviceEvent, DeviceInfo, DeviceNotificationCallback, DeviceSelector, DuplexStream, HostApi,
    InputStream, LatencyOptimalSelector, MidiDevice, MidiDeviceInfo, MidiInput, MidiMessage,
    MidiOutput, NameMatchSelector, NegotiatedConfig, OutputStream, OxiSoundError, SampleFormat,
    SessionCategory, SessionInterruptionEvent, StreamConfig, StreamStats,
};

// ── MIDI convenience functions (requires `midi` feature) ─────────────────────

/// List all available MIDI devices.
///
/// Requires the `midi` feature. Returns an empty list on platforms with no MIDI devices.
///
/// # Examples
///
/// ```no_run
/// #[cfg(feature = "midi")]
/// {
///     let devices = oxisound::enumerate_midi_devices().expect("MIDI enumeration failed");
///     for d in &devices {
///         println!("{}: in={} out={}", d.name, d.is_input, d.is_output);
///     }
/// }
/// ```
#[cfg(feature = "midi")]
#[must_use = "returns device list; ignoring it means you did the work for nothing"]
pub fn enumerate_midi_devices() -> Result<Vec<MidiDeviceInfo>, OxiSoundError> {
    oxisound_midi::MidiDeviceImpl::enumerate_midi()
}

/// Open a MIDI input connection to the device at the given port index.
///
/// Requires the `midi` feature. Use [`enumerate_midi_devices`] to discover port indices.
///
/// # Examples
///
/// ```no_run
/// #[cfg(feature = "midi")]
/// {
///     let mut input = oxisound::open_midi_input(0).expect("no MIDI input");
///     if let Ok(Some(msg)) = input.receive() {
///         println!("MIDI: status={:#04x} data={:?}", msg.status, msg.data);
///     }
/// }
/// ```
#[cfg(feature = "midi")]
pub fn open_midi_input(port: usize) -> Result<Box<dyn MidiInput>, OxiSoundError> {
    <oxisound_midi::MidiDeviceImpl as oxisound_core::MidiDevice>::open_midi_input(port)
}

/// Open a MIDI output connection to the device at the given port index.
///
/// Requires the `midi` feature. Use [`enumerate_midi_devices`] to discover port indices.
///
/// # Examples
///
/// ```no_run
/// #[cfg(feature = "midi")]
/// {
///     let mut output = oxisound::open_midi_output(0).expect("no MIDI output");
///     let msg = oxisound::MidiMessage { status: 0x90, data: vec![60, 100], timestamp_micros: 0 };
///     output.send(&msg).expect("send failed");
/// }
/// ```
#[cfg(feature = "midi")]
pub fn open_midi_output(port: usize) -> Result<Box<dyn MidiOutput>, OxiSoundError> {
    <oxisound_midi::MidiDeviceImpl as oxisound_core::MidiDevice>::open_midi_output(port)
}

#[cfg(all(feature = "pure", not(target_arch = "wasm32")))]
pub use oxisound_cpal::DeviceChangeGuard;
#[cfg(feature = "pure")]
pub use oxisound_cpal::{
    AdaptiveBufferSizer, CpalCallbackInputStream, CpalCallbackOutputStream, CpalDevice,
    CpalDeviceWatcher, CpalOutputStream, StreamHealth,
};

#[cfg(feature = "tokio")]
pub use oxisound_cpal::{CpalAsyncInputStream, CpalAsyncOutputStream};

#[cfg(feature = "tokio")]
pub use oxisound_core::{AsyncInputStream, AsyncOutputStream};

/// Returns the default audio output device.
///
/// # Examples
///
/// ```no_run
/// let device = oxisound::default_output().expect("no output device");
/// println!("Got a device");
/// let _ = device;
/// ```
#[must_use = "handle or discard the returned device"]
#[cfg(feature = "pure")]
pub fn default_output() -> Result<CpalDevice, OxiSoundError> {
    CpalDevice::default_output()
}

/// Returns the default audio input device.
///
/// # Examples
///
/// ```no_run
/// let device = oxisound::default_input().expect("no input device");
/// println!("Got an input device");
/// let _ = device;
/// ```
#[must_use = "handle or discard the returned device"]
#[cfg(feature = "pure")]
pub fn default_input() -> Result<CpalDevice, OxiSoundError> {
    CpalDevice::default_input()
}

/// Enumerates all available audio output devices.
///
/// Returns an empty `Vec` on headless systems or when no devices are present.
///
/// # Examples
///
/// ```no_run
/// let devices = oxisound::enumerate_devices().expect("enumeration failed");
/// for d in &devices {
///     println!("{}: output={} input={}", d.name, d.is_output, d.is_input);
/// }
/// ```
#[must_use = "handle or discard the returned device list"]
#[cfg(feature = "pure")]
pub fn enumerate_devices() -> Result<Vec<DeviceInfo>, OxiSoundError> {
    CpalDevice::enumerate()
}

/// Enumerates available audio input devices.
///
/// Returns a list of [`DeviceInfo`] for all input devices on the default host.
/// Returns an empty `Vec` on headless systems or when no input devices are present.
///
/// # Examples
///
/// ```no_run
/// let devices = oxisound::enumerate_input_devices().expect("enumeration failed");
/// for d in &devices {
///     println!("{}: {}", d.name, if d.is_default { "default" } else { "" });
/// }
/// ```
#[must_use = "handle or discard the returned device list"]
#[cfg(feature = "pure")]
pub fn enumerate_input_devices() -> Result<Vec<DeviceInfo>, OxiSoundError> {
    CpalDevice::enumerate_input()
}

/// Opens the default audio output device with the given stream configuration.
///
/// Combines [`default_output`] and [`AudioDevice::open_output`] into a single call.
/// Returns a boxed [`OutputStream`] ready for writing interleaved `f32` samples.
///
/// # Examples
///
/// ```no_run
/// use oxisound::StreamConfig;
/// let mut stream = oxisound::open_output(StreamConfig::stereo_48k()).expect("open failed");
/// // Write 100 ms of silence at 48 kHz stereo (48_000 * 2 * 0.1 = 9_600 samples).
/// let silence = vec![0.0f32; 9_600];
/// stream.write(&silence).expect("write failed");
/// ```
#[must_use = "handle or discard the returned stream"]
#[cfg(feature = "pure")]
pub fn open_output(config: StreamConfig) -> Result<Box<dyn OutputStream>, OxiSoundError> {
    CpalDevice::default_output()?.open_output(config)
}

/// Opens the default audio input device with the given stream configuration.
///
/// Combines [`default_input`] and [`AudioDevice::open_input`] into a single call.
///
/// # Examples
///
/// ```no_run
/// use oxisound::StreamConfig;
/// let mut stream = oxisound::open_input(StreamConfig::mono_16k()).expect("open failed");
/// let mut buf = vec![0.0f32; 1600];
/// let n = stream.read(&mut buf).expect("read failed");
/// println!("Read {n} samples");
/// ```
#[must_use = "handle or discard the returned stream"]
#[cfg(feature = "pure")]
pub fn open_input(config: StreamConfig) -> Result<Box<dyn InputStream>, OxiSoundError> {
    CpalDevice::default_input()?.open_input(config)
}

/// Opens a system audio loopback stream using the default output device's host.
///
/// On Linux with PulseAudio or PipeWire-ALSA, captures system audio output via a `.monitor`
/// source exposed as a regular ALSA input device. On other platforms, returns
/// [`OxiSoundError::Unsupported`] — see [`CpalDevice::open_loopback`] for per-platform details.
///
/// # Examples
///
/// ```no_run
/// use oxisound::StreamConfig;
/// match oxisound::open_loopback(StreamConfig::stereo_48k()) {
///     Ok(mut stream) => {
///         let mut buf = vec![0.0f32; 4800];
///         let n = stream.read(&mut buf).expect("read failed");
///         println!("Captured {n} loopback samples");
///     }
///     Err(e) => println!("Loopback not available: {e}"),
/// }
/// ```
#[must_use = "handle or discard the returned stream"]
#[cfg(feature = "pure")]
pub fn open_loopback(config: StreamConfig) -> Result<Box<dyn InputStream>, OxiSoundError> {
    CpalDevice::default_output()?.open_loopback(config)
}

/// Finds the first output device whose name contains `name_fragment` (case-insensitive).
///
/// Returns [`OxiSoundError::NoDevice`] if no match is found.
///
/// # Examples
///
/// ```no_run
/// match oxisound::select_device("built-in") {
///     Ok(device) => println!("Found device"),
///     Err(e) => println!("Not found: {e}"),
/// }
/// ```
#[must_use = "handle or discard the returned device"]
#[cfg(feature = "pure")]
pub fn select_device(name_fragment: &str) -> Result<CpalDevice, OxiSoundError> {
    CpalDevice::select_output(name_fragment)
}

/// Finds the first input device whose name contains `name_fragment` (case-insensitive).
///
/// Returns [`OxiSoundError::NoDevice`] if no match is found.
///
/// # Examples
///
/// ```no_run
/// match oxisound::select_input_device("built-in") {
///     Ok(device) => println!("Found input device"),
///     Err(e) => println!("Not found: {e}"),
/// }
/// ```
#[must_use = "handle or discard the returned device"]
#[cfg(feature = "pure")]
pub fn select_input_device(name_fragment: &str) -> Result<CpalDevice, OxiSoundError> {
    CpalDevice::select_input(name_fragment)
}

/// Opens the default output device in duplex mode using the given config.
///
/// Convenience combining `default_output()` + `device.open_duplex(config)`.
///
/// # Examples
///
/// ```no_run
/// use oxisound::StreamConfig;
/// let mut duplex = oxisound::duplex_stream(StreamConfig::stereo_48k()).expect("open failed");
/// ```
#[must_use = "handle or discard the returned stream"]
#[cfg(feature = "pure")]
pub fn duplex_stream(config: StreamConfig) -> Result<Box<dyn DuplexStream>, OxiSoundError> {
    CpalDevice::default_output()?.open_duplex(config)
}

// ---------------------------------------------------------------------------
// Native JACK (oxisound-jack quarantine crate) — Pure Rust Policy v2 §5
//
// JACK (libjack2 C-FFI) is NOT re-exported from this pure facade. Applications
// that need it depend on the `oxisound-jack` crate directly:
//     oxisound-jack = "0.2.2"
// and call e.g. `oxisound_jack::JackDevice::new(...)`.
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// PulseAudio native-protocol backend (`pulse` feature, opt-in)
//
// A Pure-Rust alternative to the default cpal/ALSA path on Linux: it speaks the
// PulseAudio native IPC protocol over a unix socket, so no C library sits in the
// audio path. The same socket is served by PipeWire's `pipewire-pulse` shim.
//
// NOTE: `CpalDevice::with_host(HostApi::PulseAudio)` still returns an error and will
// keep doing so — cpal has no PulseAudio host to dispatch to, which is exactly why
// `oxisound-pulse` exists. Use the functions below (or `PulseDevice` directly).
// ---------------------------------------------------------------------------

#[cfg(feature = "pulse")]
pub use oxisound_pulse::{
    PulseDevice, PulseDeviceDescriptor, PulseDeviceRole, PulseDuplexStream, PulseInputStream,
    PulseOutputStream, PulseSampleFormat,
};

/// Enumerates every PulseAudio sink and source.
///
/// Requires the `pulse` feature. Sinks are reported with `is_output`, sources
/// (including monitor sources) with `is_input`; every entry has at least one role.
/// Returns [`OxiSoundError::Unsupported`] on non-Linux targets.
///
/// # Errors
///
/// Propagates connection and round-trip failures from the PulseAudio server.
///
/// # Examples
///
/// ```no_run
/// # #[cfg(feature = "pulse")]
/// # {
/// let devices = oxisound::pulse_enumerate_devices().expect("enumeration failed");
/// for d in &devices {
///     assert!(d.is_input || d.is_output);
///     println!("{d}");
/// }
/// # }
/// ```
#[must_use = "handle or discard the returned device list"]
#[cfg(feature = "pulse")]
pub fn pulse_enumerate_devices() -> Result<Vec<DeviceInfo>, OxiSoundError> {
    <PulseDevice as AudioDevice>::enumerate()
}

/// Returns the PulseAudio server's default sink.
///
/// Requires the `pulse` feature.
///
/// # Errors
///
/// Propagates connection failures, or [`OxiSoundError::NoDevice`] when the server has no
/// usable default sink.
///
/// # Examples
///
/// ```no_run
/// # #[cfg(feature = "pulse")]
/// # {
/// let device = oxisound::pulse_default_output().expect("no PulseAudio sink");
/// println!("{:?}", device.descriptor());
/// # }
/// ```
#[must_use = "handle or discard the returned device"]
#[cfg(feature = "pulse")]
pub fn pulse_default_output() -> Result<PulseDevice, OxiSoundError> {
    <PulseDevice as AudioDevice>::default_output()
}

/// Returns the PulseAudio server's default source.
///
/// Requires the `pulse` feature.
///
/// # Errors
///
/// Propagates connection failures, or [`OxiSoundError::NoDevice`] when the server has no
/// usable default source.
///
/// # Examples
///
/// ```no_run
/// # #[cfg(feature = "pulse")]
/// # {
/// let device = oxisound::pulse_default_input().expect("no PulseAudio source");
/// # }
/// ```
#[must_use = "handle or discard the returned device"]
#[cfg(feature = "pulse")]
pub fn pulse_default_input() -> Result<PulseDevice, OxiSoundError> {
    <PulseDevice as AudioDevice>::default_input()
}

/// Opens a playback stream on the PulseAudio default sink.
///
/// The PulseAudio counterpart of `open_output` (the cpal-backed default). Requires the
/// `pulse` feature.
///
/// # Errors
///
/// Propagates connection and stream-creation failures.
///
/// # Examples
///
/// ```no_run
/// # #[cfg(feature = "pulse")]
/// # {
/// use oxisound::StreamConfig;
/// let mut stream = oxisound::pulse_output(StreamConfig::stereo_48k()).expect("open failed");
/// stream.write(&vec![0.0f32; 9_600]).expect("write failed");
/// # }
/// ```
#[must_use = "handle or discard the returned stream"]
#[cfg(feature = "pulse")]
pub fn pulse_output(config: StreamConfig) -> Result<Box<dyn OutputStream>, OxiSoundError> {
    <PulseDevice as AudioDevice>::default_output()?.open_output(config)
}

/// Opens a capture stream on the PulseAudio default source.
///
/// The PulseAudio counterpart of `open_input` (the cpal-backed default). Requires the
/// `pulse` feature.
///
/// # Errors
///
/// Propagates connection and stream-creation failures.
///
/// # Examples
///
/// ```no_run
/// # #[cfg(feature = "pulse")]
/// # {
/// use oxisound::StreamConfig;
/// let mut stream = oxisound::pulse_input(StreamConfig::mono_16k()).expect("open failed");
/// let mut buf = vec![0.0f32; 1_600];
/// let n = stream.read(&mut buf).expect("read failed");
/// println!("read {n} samples");
/// # }
/// ```
#[must_use = "handle or discard the returned stream"]
#[cfg(feature = "pulse")]
pub fn pulse_input(config: StreamConfig) -> Result<Box<dyn InputStream>, OxiSoundError> {
    <PulseDevice as AudioDevice>::default_input()?.open_input(config)
}

/// Opens a duplex stream (default sink + default source) on one PulseAudio connection.
///
/// The PulseAudio counterpart of `duplex_stream` (the cpal-backed default). Requires the
/// `pulse` feature.
///
/// # Errors
///
/// Propagates connection and stream-creation failures from either half.
///
/// # Examples
///
/// ```no_run
/// # #[cfg(feature = "pulse")]
/// # {
/// use oxisound::StreamConfig;
/// let mut duplex = oxisound::pulse_duplex(StreamConfig::stereo_48k()).expect("open failed");
/// # }
/// ```
#[must_use = "handle or discard the returned stream"]
#[cfg(feature = "pulse")]
pub fn pulse_duplex(config: StreamConfig) -> Result<Box<dyn DuplexStream>, OxiSoundError> {
    <PulseDevice as AudioDevice>::default_output()?.open_duplex(config)
}

/// Opens a playback stream on a named PulseAudio sink.
///
/// `name` is the server-side sink name from [`DeviceInfo::name`], for example
/// `alsa_output.pci-0000_00_1f.3.analog-stereo`. Requires the `pulse` feature.
///
/// # Errors
///
/// Returns [`OxiSoundError::NoDevice`] when no sink of that name exists, plus the usual
/// connection failures.
///
/// # Examples
///
/// ```no_run
/// # #[cfg(feature = "pulse")]
/// # {
/// use oxisound::StreamConfig;
/// let mut stream = oxisound::pulse_output_named(
///     "alsa_output.pci-0000_00_1f.3.analog-stereo",
///     StreamConfig::stereo_48k(),
/// )
/// .expect("open failed");
/// # }
/// ```
#[must_use = "handle or discard the returned stream"]
#[cfg(feature = "pulse")]
pub fn pulse_output_named(
    name: &str,
    config: StreamConfig,
) -> Result<Box<dyn OutputStream>, OxiSoundError> {
    PulseDevice::open_named(
        name,
        PulseDeviceRole::Sink,
        oxisound_pulse::DEFAULT_CLIENT_NAME,
    )?
    .open_output(config)
}

/// Opens a capture stream on a named PulseAudio source.
///
/// `name` is the server-side source name from [`DeviceInfo::name`]. A sink's monitor
/// source — the way to capture system output on Linux — is that sink's name with a
/// `.monitor` suffix. Requires the `pulse` feature.
///
/// # Errors
///
/// Returns [`OxiSoundError::NoDevice`] when no source of that name exists, plus the usual
/// connection failures.
///
/// # Examples
///
/// ```no_run
/// # #[cfg(feature = "pulse")]
/// # {
/// use oxisound::StreamConfig;
/// // Capture what is currently playing.
/// let mut stream = oxisound::pulse_input_named(
///     "alsa_output.pci-0000_00_1f.3.analog-stereo.monitor",
///     StreamConfig::stereo_48k(),
/// )
/// .expect("open failed");
/// # }
/// ```
#[must_use = "handle or discard the returned stream"]
#[cfg(feature = "pulse")]
pub fn pulse_input_named(
    name: &str,
    config: StreamConfig,
) -> Result<Box<dyn InputStream>, OxiSoundError> {
    PulseDevice::open_named(
        name,
        PulseDeviceRole::Source,
        oxisound_pulse::DEFAULT_CLIENT_NAME,
    )?
    .open_input(config)
}

// ---------------------------------------------------------------------------
// OSC (Open Sound Control) re-exports
// ---------------------------------------------------------------------------

#[cfg(feature = "osc")]
pub use oxisound_osc::{
    OscArg, OscBundle, OscError, OscMessage, OscPacket, OscReceiver, OscSender, OscTimeTag,
    decode as decode_osc, encode as encode_osc,
};

/// Returns a latency estimate in milliseconds for the default output device.
///
/// This is a device-level estimate based on the default config buffer size;
/// use `CpalOutputStream::latency_frames()` for live stream buffering.
/// Returns `Ok(0.0)` when the device reports an unknown buffer size.
///
/// # Examples
///
/// ```no_run
/// if let Ok(device) = oxisound::default_output() {
///     let ms = oxisound::latency_ms(&device).unwrap_or(0.0);
///     println!("Estimated latency: {ms:.1} ms");
/// }
/// ```
#[must_use = "handle or discard the returned latency value"]
#[cfg(feature = "pure")]
pub fn latency_ms(device: &CpalDevice) -> Result<f32, OxiSoundError> {
    device.default_output_latency_ms()
}

/// Returns a human-readable multi-line device listing for debugging.
///
/// An empty slice returns an empty string. Each line shows whether the device is
/// marked as default (`*`), its I/O direction, channel counts, and sample-rate range.
///
/// # Examples
///
/// ```no_run
/// let devices = oxisound::enumerate_devices().expect("enumeration failed");
/// print!("{}", oxisound::format_devices(&devices));
/// ```
pub fn format_devices(devices: &[DeviceInfo]) -> String {
    if devices.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    for d in devices {
        let marker = if d.is_default { '*' } else { ' ' };
        let io = match (d.is_input, d.is_output) {
            (true, true) => "in+out",
            (true, false) => "in    ",
            (false, true) => "out   ",
            (false, false) => "      ",
        };
        let ch_str = if d.channel_counts.is_empty() {
            "ch:?".to_string()
        } else {
            format!(
                "ch:[{}]",
                d.channel_counts
                    .iter()
                    .map(|c| c.to_string())
                    .collect::<Vec<_>>()
                    .join(" ")
            )
        };
        let rate_str = if d.sample_rates.is_empty() {
            "rates:?".to_string()
        } else {
            let min = d.sample_rates.iter().copied().min().unwrap_or(0);
            let max = d.sample_rates.iter().copied().max().unwrap_or(0);
            if min == max {
                format!("rates:[{}]", min)
            } else {
                format!("rates:[{}-{}]", min, max)
            }
        };
        out.push_str(&format!(
            "{} [{io}] {ch_str} {rate_str}  {name}\n",
            marker,
            name = d.name
        ));
    }
    out
}

/// Generates a pure sine wave as an interleaved `Vec<f32>`.
///
/// # Arguments
/// * `freq_hz` — frequency in Hz (e.g. 440.0 for A4)
/// * `duration_secs` — duration in seconds
/// * `config` — sample rate and channel count (buffer_size is ignored)
///
/// # Returns
/// Interleaved samples: length = `(sample_rate * channels * duration_secs) as usize`.
/// Each sample is in `[-1.0, 1.0]`. Does NOT play audio — the caller writes the buffer
/// to an [`OutputStream`].
///
/// # Examples
/// ```
/// use oxisound::{sine_test_tone, StreamConfig};
/// let buf = sine_test_tone(440.0, 1.0, StreamConfig::stereo_48k());
/// assert!(!buf.is_empty());
/// ```
#[must_use]
pub fn sine_test_tone(freq_hz: f32, duration_secs: f32, config: StreamConfig) -> Vec<f32> {
    let sample_rate = config.sample_rate as f32;
    let channels = config.channels as usize;
    let total_frames = (sample_rate * duration_secs) as usize;
    let total_samples = total_frames * channels;
    let mut buf = Vec::with_capacity(total_samples);
    for frame in 0..total_frames {
        let t = frame as f32;
        let sample = (2.0 * std::f32::consts::PI * freq_hz * t / sample_rate).sin();
        for _ in 0..channels {
            buf.push(sample);
        }
    }
    buf
}

/// Generates white noise as an interleaved `Vec<f32>`.
///
/// Uses a deterministic xorshift32 PRNG seeded by frame index for reproducibility.
/// Values are in `[-1.0, 1.0]`. Does NOT play audio.
///
/// # Examples
/// ```
/// use oxisound::{white_noise_test, StreamConfig};
/// let buf = white_noise_test(0.1, StreamConfig::stereo_48k());
/// assert!(!buf.is_empty());
/// ```
#[must_use]
pub fn white_noise_test(duration_secs: f32, config: StreamConfig) -> Vec<f32> {
    let sample_rate = config.sample_rate as usize;
    let channels = config.channels as usize;
    let total_frames = (sample_rate as f32 * duration_secs) as usize;
    let total_samples = total_frames * channels;
    let mut buf = Vec::with_capacity(total_samples);
    let mut state: u32 = 2_463_534_242; // xorshift32 seed
    for _ in 0..total_frames {
        // xorshift32 step
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        // Map to [-1.0, 1.0]
        let sample = (state as f32 / u32::MAX as f32) * 2.0 - 1.0;
        for _ in 0..channels {
            buf.push(sample);
        }
    }
    buf
}

/// Generates a linear frequency sweep (chirp) as an interleaved `Vec<f32>`.
///
/// Frequency sweeps from `f_start` to `f_end` Hz over `duration_secs`.
/// Uses correct phase integration: `φ(t) = 2π * (f_start * t + (f_end - f_start) * t² / (2 * dur))`.
/// Values are in `[-1.0, 1.0]`. Does NOT play audio.
///
/// # Examples
/// ```
/// use oxisound::{chirp_test_tone, StreamConfig};
/// let buf = chirp_test_tone(20.0, 20_000.0, 1.0, StreamConfig::stereo_48k());
/// assert!(!buf.is_empty());
/// ```
#[must_use]
pub fn chirp_test_tone(
    f_start: f32,
    f_end: f32,
    duration_secs: f32,
    config: StreamConfig,
) -> Vec<f32> {
    let sample_rate = config.sample_rate as f32;
    let channels = config.channels as usize;
    let total_frames = (sample_rate * duration_secs) as usize;
    let total_samples = total_frames * channels;
    let mut buf = Vec::with_capacity(total_samples);
    for frame in 0..total_frames {
        let t = frame as f32 / sample_rate;
        // Phase integral of instantaneous frequency f(t) = f_start + (f_end - f_start) * t / dur
        let phase = 2.0
            * std::f32::consts::PI
            * (f_start * t + (f_end - f_start) * t * t / (2.0 * duration_secs));
        let sample = phase.sin();
        for _ in 0..channels {
            buf.push(sample);
        }
    }
    buf
}

/// Generates a zero-filled buffer of the requested duration.
///
/// # Examples
/// ```
/// use oxisound::{silence, StreamConfig};
/// let buf = silence(0.1, StreamConfig::stereo_48k());
/// assert!(buf.iter().all(|&s| s == 0.0));
/// ```
#[must_use]
pub fn silence(duration_secs: f32, config: StreamConfig) -> Vec<f32> {
    let total_frames = (config.sample_rate as f32 * duration_secs) as usize;
    vec![0.0f32; total_frames * config.channels as usize]
}

/// Generates a click track at the given BPM as an interleaved `Vec<f32>`.
///
/// Each click is a brief impulse: 1ms sharp attack, 5ms exponential decay.
/// Does NOT play audio.
///
/// # Examples
/// ```
/// use oxisound::{click_track, StreamConfig};
/// let buf = click_track(120.0, 2.0, StreamConfig::stereo_48k());
/// assert!(!buf.is_empty());
/// ```
#[must_use]
pub fn click_track(bpm: f32, duration_secs: f32, config: StreamConfig) -> Vec<f32> {
    let sample_rate = config.sample_rate as f32;
    let channels = config.channels as usize;
    let total_frames = (sample_rate * duration_secs) as usize;
    let total_samples = total_frames * channels;
    let mut buf = vec![0.0f32; total_samples];
    let beat_interval_frames = (sample_rate * 60.0 / bpm) as usize;
    let attack_frames = (sample_rate * 0.001) as usize; // 1ms
    let decay_frames = (sample_rate * 0.005) as usize; // 5ms

    let mut beat_frame = 0usize;
    while beat_frame < total_frames {
        // Attack phase
        for i in 0..attack_frames.min(total_frames.saturating_sub(beat_frame)) {
            let amp = i as f32 / attack_frames as f32;
            let base = (beat_frame + i) * channels;
            if base < total_samples {
                for ch in 0..channels {
                    buf[base + ch] = amp;
                }
            }
        }
        // Decay phase
        for i in 0..decay_frames.min(total_frames.saturating_sub(beat_frame + attack_frames)) {
            let amp = 1.0 - (i as f32 / decay_frames as f32);
            let base = (beat_frame + attack_frames + i) * channels;
            if base < total_samples {
                for ch in 0..channels {
                    buf[base + ch] = amp;
                }
            }
        }
        beat_frame += beat_interval_frames;
    }
    buf
}

// ---------------------------------------------------------------------------
// Callback-based facade wrappers
// ---------------------------------------------------------------------------

/// Opens the default output device with a zero-copy callback.
///
/// The `callback` is invoked on the real-time audio thread with a mutable slice of
/// interleaved `f32` samples. Bypasses the internal ring buffer for lowest latency.
///
/// # Examples
///
/// ```no_run
/// use oxisound::StreamConfig;
/// let _guard = oxisound::play_callback(StreamConfig::stereo_48k(), |buf| {
///     for s in buf.iter_mut() { *s = 0.0; }
/// }).expect("no output device");
/// ```
#[must_use = "drop the guard to stop the stream"]
#[cfg(feature = "pure")]
pub fn play_callback(
    config: StreamConfig,
    callback: impl FnMut(&mut [f32]) + Send + 'static,
) -> Result<CpalCallbackOutputStream, OxiSoundError> {
    CpalDevice::default_output()?.open_output_callback(config, callback)
}

/// Opens the default input device with a zero-copy callback.
///
/// The `callback` is invoked on the real-time audio thread with an immutable slice of
/// interleaved `f32` samples. Bypasses the internal ring buffer for lowest latency.
///
/// # Examples
///
/// ```no_run
/// use oxisound::StreamConfig;
/// let _guard = oxisound::capture_callback(StreamConfig::mono_16k(), |buf| {
///     let _ = buf.len();
/// }).expect("no input device");
/// ```
#[must_use = "drop the guard to stop the stream"]
#[cfg(feature = "pure")]
pub fn capture_callback(
    config: StreamConfig,
    callback: impl FnMut(&[f32]) + Send + 'static,
) -> Result<CpalCallbackInputStream, OxiSoundError> {
    CpalDevice::default_input()?.open_input_callback(config, callback)
}

/// Guard struct that keeps both callback streams alive.
///
/// Drop this value to stop both streams.
#[cfg(feature = "pure")]
pub struct DuplexCallbackGuard {
    _out: CpalCallbackOutputStream,
    _inp: CpalCallbackInputStream,
}

/// Opens the default device for simultaneous input and output via callbacks.
///
/// Returns a [`DuplexCallbackGuard`] that keeps both streams alive.
/// Drop the guard to stop both streams.
///
/// # Examples
///
/// ```no_run
/// use oxisound::StreamConfig;
/// let _guard = oxisound::duplex_callback(
///     StreamConfig::stereo_48k(),
///     |buf| { for s in buf.iter_mut() { *s = 0.0; } },
///     |_buf| {},
/// ).expect("no device");
/// ```
#[must_use = "drop the guard to stop both streams"]
#[cfg(feature = "pure")]
pub fn duplex_callback(
    config: StreamConfig,
    out_callback: impl FnMut(&mut [f32]) + Send + 'static,
    in_callback: impl FnMut(&[f32]) + Send + 'static,
) -> Result<DuplexCallbackGuard, OxiSoundError> {
    let _out = CpalDevice::default_output()?.open_output_callback(config.clone(), out_callback)?;
    let _inp = CpalDevice::default_input()?.open_input_callback(config, in_callback)?;
    Ok(DuplexCallbackGuard { _out, _inp })
}

// ---------------------------------------------------------------------------
// Device management additions
// ---------------------------------------------------------------------------

/// Selects an audio output device by zero-based index in the enumeration order.
///
/// Returns [`OxiSoundError::NoDevice`] if the index is out of range.
///
/// # Examples
///
/// ```no_run
/// match oxisound::device_by_index(0) {
///     Ok(device) => println!("Got device at index 0"),
///     Err(e) => println!("No device: {e}"),
/// }
/// ```
#[must_use = "handle or discard the returned device"]
#[cfg(feature = "pure")]
pub fn device_by_index(index: usize) -> Result<CpalDevice, OxiSoundError> {
    let devices = enumerate_devices()?;
    if index < devices.len() {
        CpalDevice::select_output(&devices[index].name)
    } else {
        Err(OxiSoundError::NoDevice)
    }
}

/// Returns the preferred [`StreamConfig`] for the default output device.
///
/// Queries the device's optimal buffer size and constructs a sensible stereo 48 kHz config.
///
/// # Examples
///
/// ```no_run
/// let config = oxisound::preferred_output_config().expect("no output device");
/// println!("Preferred config: {config}");
/// ```
#[must_use = "handle or discard the returned config"]
#[cfg(feature = "pure")]
pub fn preferred_output_config() -> Result<StreamConfig, OxiSoundError> {
    let device = CpalDevice::default_output()?;
    let buf_size = device.optimal_buffer_size().ok();
    Ok(StreamConfig {
        sample_rate: 48_000,
        channels: 2,
        buffer_size: buf_size,
        sample_format: None,
        exclusive: false,
        preferred_formats: Vec::new(),
        channel_routing: None,
        buffer_capacity_secs: None,
    })
}

/// Returns all available audio devices (both input and output) in a single call.
///
/// # Invariant
///
/// **Enumeration never yields a device with neither role**: every returned
/// [`DeviceInfo`] has `is_input == true`, `is_output == true`, or both.  Devices that
/// the backend advertises but cannot open in either direction (for example an HDMI sink
/// with no monitor attached on Linux/ALSA) are omitted from the list, because they can
/// never be opened as a stream.
///
/// # Examples
///
/// ```no_run
/// let devices = oxisound::enumerate_all_devices().expect("enumeration failed");
/// for d in &devices {
///     assert!(d.is_input || d.is_output);
///     println!("{}", d.name);
/// }
/// ```
#[must_use = "handle or discard the returned device list"]
#[cfg(feature = "pure")]
pub fn enumerate_all_devices() -> Result<Vec<DeviceInfo>, OxiSoundError> {
    CpalDevice::enumerate_all()
}

// ---------------------------------------------------------------------------
// Audio session and permission management
// ---------------------------------------------------------------------------

/// Configures the audio session category for the current process.
///
/// - On **iOS / macOS with `macos-session` or `session` feature** (i.e.
///   `oxisound-session/avf-audio`): calls `[AVAudioSession sharedInstance]
///   setCategory:error:` via Objective-C. Returns `Ok(())` on success,
///   [`OxiSoundError::FormatMismatch`] if the AVFoundation call fails.
/// - On **macOS without the `session` feature** (CoreAudio desktop):
///   logs a debug message and returns `Ok(())` — CoreAudio desktop apps do
///   not require explicit session management.
/// - On **all other platforms**: returns
///   [`OxiSoundError::UnsupportedConfig`].
///
/// # Examples
///
/// ```no_run
/// use oxisound::{configure_session, SessionCategory};
/// configure_session(SessionCategory::Playback).ok();
/// ```
#[must_use = "check whether the session was configured successfully"]
pub fn configure_session(category: SessionCategory) -> Result<(), OxiSoundError> {
    configure_session_impl(category)
}

// With the `session` feature: delegate to oxisound-session which has the real
// AVAudioSession implementation (and is allowed to use unsafe for ObjC calls).
#[cfg(feature = "session")]
fn configure_session_impl(category: SessionCategory) -> Result<(), OxiSoundError> {
    oxisound_session::configure_session(category)
}

// Without `session` feature on macOS: CoreAudio desktop doesn't need session management.
#[cfg(all(target_os = "macos", not(feature = "session")))]
fn configure_session_impl(category: SessionCategory) -> Result<(), OxiSoundError> {
    log::debug!(
        "configure_session({category:?}): macOS CoreAudio desktop — no AVAudioSession needed. \
         Enable the `macos-session` feature for AVFoundation session management."
    );
    Ok(())
}

// Without `session` feature on iOS: not supported without the feature.
#[cfg(all(target_os = "ios", not(feature = "session")))]
fn configure_session_impl(category: SessionCategory) -> Result<(), OxiSoundError> {
    let _ = category;
    Err(OxiSoundError::UnsupportedConfig(
        "configure_session on iOS requires the `macos-session` feature of oxisound".into(),
    ))
}

// Without `session` feature on all other platforms.
#[cfg(not(any(target_os = "macos", target_os = "ios", feature = "session")))]
fn configure_session_impl(category: SessionCategory) -> Result<(), OxiSoundError> {
    let _ = category;
    Err(OxiSoundError::UnsupportedConfig(
        "audio session management requires iOS/macOS".into(),
    ))
}

/// Requests microphone recording permission from the OS.
///
/// - On **iOS / macOS with `macos-session` or `session` feature**: uses
///   `AVAudioApplication.requestRecordPermissionWithCompletionHandler:`.
///   Returns `Ok(true)` if granted, `Ok(false)` if denied, or
///   [`OxiSoundError::Timeout`] if the user doesn't respond within 30 s.
/// - On **macOS without the `session` feature**: returns `Ok(true)` (assumed
///   granted; CoreAudio desktop doesn't require a permission prompt in most
///   configurations).
/// - On **all other platforms**: returns
///   `Err(`[`OxiSoundError::PermissionDenied`]`)`.
///
/// # Examples
///
/// ```no_run
/// let granted = oxisound::request_microphone_permission().unwrap_or(false);
/// ```
pub fn request_microphone_permission() -> Result<bool, OxiSoundError> {
    request_microphone_permission_impl()
}

// With the `session` feature: delegate to oxisound-session.
#[cfg(feature = "session")]
fn request_microphone_permission_impl() -> Result<bool, OxiSoundError> {
    oxisound_session::request_microphone_permission()
}

// Without `session` feature on macOS: assume granted for CoreAudio desktop.
#[cfg(all(target_os = "macos", not(feature = "session")))]
fn request_microphone_permission_impl() -> Result<bool, OxiSoundError> {
    log::debug!(
        "request_microphone_permission: macOS CoreAudio desktop — assumed granted. \
         Enable `macos-session` for real TCC permission check."
    );
    Ok(true)
}

// Without `session` feature on iOS: not supported.
#[cfg(all(target_os = "ios", not(feature = "session")))]
fn request_microphone_permission_impl() -> Result<bool, OxiSoundError> {
    Err(OxiSoundError::PermissionDenied(
        "request_microphone_permission on iOS requires the `macos-session` feature".into(),
    ))
}

// Without `session` feature on all other platforms.
#[cfg(not(any(target_os = "macos", target_os = "ios", feature = "session")))]
fn request_microphone_permission_impl() -> Result<bool, OxiSoundError> {
    Err(OxiSoundError::PermissionDenied(
        "microphone permission prompt is not available on this platform".into(),
    ))
}

// ---------------------------------------------------------------------------
// Stream statistics and monitoring
// ---------------------------------------------------------------------------

/// Returns a snapshot of the stream's statistics.
///
/// `OutputStream::stats()` is infallible (it defaults to
/// [`StreamStats::default()`](oxisound_core::StreamStats) when a backend doesn't track a
/// particular field), so this always returns `Some`. A freshly opened, perfectly healthy stream
/// that hasn't processed a frame yet legitimately reports all-zero fields — that is not the same
/// thing as "stats are unavailable", so callers must not treat an all-zero snapshot as an error
/// condition. The `Option` wrapper is kept for API stability; it is never `None`.
///
/// # Examples
///
/// ```no_run
/// # use oxisound::{StreamConfig, StreamStats};
/// # let mut stream = oxisound::open_output(StreamConfig::STEREO_48K).unwrap();
/// if let Some(stats) = oxisound::stream_stats(stream.as_ref()) {
///     println!("underruns: {}", stats.underruns);
/// }
/// ```
#[must_use]
pub fn stream_stats(stream: &dyn oxisound_core::OutputStream) -> Option<StreamStats> {
    Some(stream.stats())
}

/// RAII guard returned by [`monitor_stream`]. Dropping it stops the monitoring thread.
///
/// Not available on `wasm32` (no OS threads).
#[cfg(not(target_arch = "wasm32"))]
pub struct MonitorGuard {
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

#[cfg(not(target_arch = "wasm32"))]
impl Drop for MonitorGuard {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Periodically samples stream statistics and calls `callback` from a background thread.
///
/// `stats_fn` is called every `interval_ms` milliseconds; its return value is forwarded to
/// `callback`. A common usage is to capture a closure over `stream.stats()`.
///
/// Drop the returned [`MonitorGuard`] to stop monitoring.
///
/// Not available on `wasm32`.
///
/// # Examples
///
/// ```no_run
/// let guard = oxisound::monitor_stream(
///     || oxisound_core::StreamStats::default(),
///     250,
///     |stats| eprintln!("underruns={} frames={}", stats.underruns, stats.frames_processed),
/// );
/// // guard dropped here → monitoring stops
/// ```
#[cfg(not(target_arch = "wasm32"))]
pub fn monitor_stream(
    stats_fn: impl Fn() -> StreamStats + Send + 'static,
    interval_ms: u64,
    callback: impl Fn(StreamStats) + Send + 'static,
) -> MonitorGuard {
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let stop_clone = std::sync::Arc::clone(&stop);
    std::thread::spawn(move || {
        while !stop_clone.load(std::sync::atomic::Ordering::Relaxed) {
            callback(stats_fn());
            std::thread::sleep(std::time::Duration::from_millis(interval_ms));
        }
    });
    MonitorGuard { stop }
}

// ---------------------------------------------------------------------------
// Async variants (tokio feature)
// ---------------------------------------------------------------------------

/// Opens an async output stream on the default output device.
///
/// Requires the `tokio` feature. Returns `impl AsyncOutputStream` — NOT object-safe.
///
/// # Examples
///
/// ```no_run
/// use oxisound::{AsyncOutputStream, StreamConfig};
///
/// # async fn example() {
/// let config = StreamConfig::stereo_48k();
/// let mut stream = oxisound::async_output(config.clone()).expect("no output device");
/// let buf = oxisound::sine_test_tone(440.0, 1.0, config);
/// stream.write(&buf).await.expect("write failed");
/// # }
/// ```
#[must_use = "handle or discard the returned stream"]
#[cfg(feature = "tokio")]
pub fn async_output(
    config: StreamConfig,
) -> Result<impl oxisound_core::AsyncOutputStream, OxiSoundError> {
    CpalDevice::default_output()?.open_async_output(config)
}

/// Returns a [`futures_core::Stream`] of captured audio frames on the default input device.
///
/// Each item is a `Vec<f32>` of interleaved samples. Requires the `tokio` feature.
///
/// # Examples
///
/// ```no_run
/// use futures_core::Stream;
/// use oxisound::StreamConfig;
///
/// let config = StreamConfig::mono_16k();
/// let _stream = oxisound::capture_stream(config).expect("no input device");
/// ```
#[must_use = "handle or discard the returned stream"]
#[cfg(feature = "tokio")]
pub fn capture_stream(
    config: StreamConfig,
) -> Result<impl futures_core::Stream<Item = Vec<f32>>, OxiSoundError> {
    CpalDevice::default_input()?.open_async_input(config)
}

// ---------------------------------------------------------------------------
// Device Hot-Plug
// ---------------------------------------------------------------------------

/// An async stream of device change events backed by a polling watcher.
///
/// Created by [`watch_devices`]. Hold this value to keep events flowing;
/// dropping it stops the background polling thread.
#[cfg(feature = "tokio")]
pub struct DeviceEventStream {
    inner: tokio_stream::wrappers::BroadcastStream<DeviceEvent>,
    _watcher: CpalDeviceWatcher,
}

#[cfg(feature = "tokio")]
impl futures_core::Stream for DeviceEventStream {
    type Item = DeviceEvent;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        use std::task::Poll;
        loop {
            match std::pin::Pin::new(&mut self.inner).poll_next(cx) {
                Poll::Ready(Some(Ok(event))) => return Poll::Ready(Some(event)),
                // Lagged: skip missed events and try again.
                Poll::Ready(Some(Err(_))) => continue,
                Poll::Ready(None) => return Poll::Ready(None),
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

/// Returns an async stream of device change events (device added, removed, or default changed).
///
/// Polls the system device list every 500 ms in a background thread.
/// Drop the returned [`DeviceEventStream`] to stop the polling thread.
///
/// Requires the `tokio` feature.
///
/// # Examples
///
/// ```no_run
/// # async fn example() {
/// use tokio_stream::StreamExt;
/// let mut events = oxisound::watch_devices().unwrap();
/// while let Some(event) = events.next().await {
///     println!("device event: {event:?}");
/// }
/// # }
/// ```
#[must_use = "drop the returned stream to stop watching"]
#[cfg(feature = "tokio")]
pub fn watch_devices() -> Result<DeviceEventStream, OxiSoundError> {
    let (watcher, rx) = CpalDevice::subscribe_device_events()?;
    Ok(DeviceEventStream {
        inner: tokio_stream::wrappers::BroadcastStream::new(rx),
        _watcher: watcher,
    })
}

/// Registers a synchronous callback for device change events.
///
/// Spawns a background polling thread (500 ms interval). The `callback` is called for each
/// [`DeviceEvent`]. Returns a [`DeviceChangeGuard`]; dropping it stops the thread.
///
/// Not available on `wasm32`.
///
/// # Examples
///
/// ```no_run
/// let _guard = oxisound::on_device_change(|event| {
///     println!("device event: {event:?}");
/// }).unwrap();
/// // _guard dropped here → polling stops
/// ```
#[must_use = "drop the guard to stop listening for device changes"]
#[cfg(all(feature = "pure", not(target_arch = "wasm32")))]
pub fn on_device_change(
    callback: impl Fn(DeviceEvent) + Send + 'static,
) -> Result<DeviceChangeGuard, OxiSoundError> {
    CpalDevice::on_device_change(callback)
}

// ---------------------------------------------------------------------------
// SMF (Standard MIDI File) re-exports
// ---------------------------------------------------------------------------

#[cfg(feature = "smf")]
pub use oxisound_smf::{
    Division, SmfError, SmfEvent, SmfFile, SmfFormat, SmfPlayer, SmfTrack, TempoMap, TrackEvent,
    parse as parse_smf,
};

/// Parse a Standard MIDI File from raw bytes.
///
/// Convenience wrapper around `oxisound_smf::parse`.
///
/// # Examples
///
/// ```no_run
/// let data = std::fs::read("song.mid").unwrap();
/// let smf = oxisound::load_smf(&data).expect("invalid SMF file");
/// println!("{} tracks, {:?}", smf.tracks.len(), smf.division);
/// ```
#[cfg(feature = "smf")]
pub fn load_smf(data: &[u8]) -> Result<SmfFile, SmfError> {
    oxisound_smf::parse(data)
}

/// Open and play an SMF file through a MIDI output port.
///
/// Blocks until playback is complete, sleeping between events according to the
/// file's embedded tempo map. Combines file I/O, SMF parsing, MIDI port open,
/// and `SmfPlayer::play` into a single call for convenience.
///
/// # Examples
///
/// ```no_run
/// oxisound::play_smf(std::path::Path::new("song.mid"), 0)
///     .expect("playback failed");
/// ```
#[cfg(all(feature = "smf", feature = "midi"))]
pub fn play_smf(path: &std::path::Path, midi_port: usize) -> Result<(), OxiSoundError> {
    let data = std::fs::read(path)?;
    let smf = oxisound_smf::parse(&data)
        .map_err(|e| OxiSoundError::Stream(format!("SMF parse error: {}", e.0)))?;
    let mut output = open_midi_output(midi_port)?;
    oxisound_smf::SmfPlayer::new(smf).play(output.as_mut())
}

// ---------------------------------------------------------------------------
// Auto-reconnect output
// ---------------------------------------------------------------------------

/// RAII guard for an auto-reconnecting output stream.
///
/// Implements [`oxisound_core::OutputStream`] — write samples directly to it.
/// A background thread monitors the stream health and reconnects to the default output
/// device if disconnection is detected, using exponential backoff (10 → 50 → 200 → 1000 ms).
///
/// Drop this guard to stop the background monitor thread.
///
/// Not available on `wasm32` (no OS threads).
#[cfg(all(feature = "pure", not(target_arch = "wasm32")))]
pub struct AutoReconnectGuard {
    inner: std::sync::Arc<std::sync::Mutex<Option<CpalOutputStream>>>,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    stream_config: StreamConfig,
    _monitor: std::thread::JoinHandle<()>,
}

#[cfg(all(feature = "pure", not(target_arch = "wasm32")))]
impl AutoReconnectGuard {
    /// Returns `true` if the stream is currently healthy.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let stream = oxisound::auto_reconnect_output(oxisound::StreamConfig::STEREO_48K)?;
    /// if stream.is_connected() {
    ///     println!("audio path is healthy");
    /// }
    /// #     Ok(())
    /// # }
    /// ```
    pub fn is_connected(&self) -> bool {
        self.inner
            .lock()
            .is_ok_and(|g| g.as_ref().is_some_and(|s| !s.is_disconnected()))
    }

    /// Returns the [`StreamConfig`] used when opening (and re-opening) this stream.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let stream = oxisound::auto_reconnect_output(oxisound::StreamConfig::STEREO_48K)?;
    /// assert_eq!(stream.config().sample_rate, 48_000);
    /// #     Ok(())
    /// # }
    /// ```
    pub fn config(&self) -> StreamConfig {
        self.stream_config.clone()
    }
}

#[cfg(all(feature = "pure", not(target_arch = "wasm32")))]
impl oxisound_core::OutputStream for AutoReconnectGuard {
    fn write(&mut self, samples: &[f32]) -> Result<(), OxiSoundError> {
        let mut guard = self
            .inner
            .lock()
            .map_err(|_| OxiSoundError::Device("mutex poisoned".into()))?;
        match guard.as_mut() {
            Some(stream) => stream.write(samples),
            None => Err(OxiSoundError::Disconnected(
                "reconnecting to audio device".into(),
            )),
        }
    }

    fn stats(&self) -> oxisound_core::StreamStats {
        self.inner
            .lock()
            .ok()
            .and_then(|g| g.as_ref().map(|s| s.stats()))
            .unwrap_or_default()
    }
}

#[cfg(all(feature = "pure", not(target_arch = "wasm32")))]
impl Drop for AutoReconnectGuard {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Opens an output stream that automatically reconnects to the system default device on disconnect.
///
/// A background monitor thread checks stream health every 100 ms; on disconnection it rebuilds
/// the stream with exponential backoff (10 → 50 → 200 → 1000 ms between attempts).
///
/// Not available on `wasm32`.
///
/// # Examples
///
/// ```no_run
/// use oxisound_core::OutputStream;
/// let mut stream = oxisound::auto_reconnect_output(oxisound::StreamConfig::STEREO_48K).unwrap();
/// let silence = vec![0.0f32; 480 * 2];
/// stream.write(&silence).ok();
/// ```
#[must_use = "drop the guard to stop the reconnect monitor"]
#[cfg(all(feature = "pure", not(target_arch = "wasm32")))]
pub fn auto_reconnect_output(config: StreamConfig) -> Result<AutoReconnectGuard, OxiSoundError> {
    let initial = CpalDevice::default_output()?.open_output_concrete(config.clone())?;
    let inner = std::sync::Arc::new(std::sync::Mutex::new(Some(initial)));
    let inner_clone = std::sync::Arc::clone(&inner);
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let stop_clone = std::sync::Arc::clone(&stop);
    let monitor_config = config.clone();

    let monitor = std::thread::spawn(move || {
        const BACKOFF_MS: [u64; 4] = [10, 50, 200, 1000];
        let mut backoff_idx = 0usize;
        while !stop_clone.load(std::sync::atomic::Ordering::Relaxed) {
            std::thread::sleep(std::time::Duration::from_millis(100));
            let is_disconnected = inner_clone
                .lock()
                .is_ok_and(|g| g.as_ref().is_some_and(|s| s.is_disconnected()));
            if is_disconnected {
                log::warn!(
                    "auto_reconnect_output: device disconnected, reconnecting (attempt {})",
                    backoff_idx + 1
                );
                let delay = BACKOFF_MS[backoff_idx.min(BACKOFF_MS.len() - 1)];
                std::thread::sleep(std::time::Duration::from_millis(delay));
                backoff_idx = (backoff_idx + 1).min(BACKOFF_MS.len() - 1);
                match CpalDevice::default_output()
                    .and_then(|dev| dev.open_output_concrete(monitor_config.clone()))
                {
                    Ok(new_stream) => {
                        if let Ok(mut guard) = inner_clone.lock() {
                            *guard = Some(new_stream);
                            backoff_idx = 0;
                            log::info!("auto_reconnect_output: reconnected successfully");
                        }
                    }
                    Err(e) => {
                        log::error!("auto_reconnect_output: reconnect attempt failed: {e}");
                    }
                }
            } else {
                backoff_idx = 0;
            }
        }
    });

    Ok(AutoReconnectGuard {
        inner,
        stop,
        stream_config: config,
        _monitor: monitor,
    })
}
