# oxisound-core — Core traits and types for OxiSound

[![Crates.io](https://img.shields.io/crates/v/oxisound-core.svg)](https://crates.io/crates/oxisound-core)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

`oxisound-core` defines the device, stream, and MIDI abstractions that every OxiSound backend implements. It contains **no platform code** — concrete backends live in `oxisound-cpal` (CPAL / native audio APIs), `oxisound-jack` (JACK Audio Server), and `oxisound-midi` (MIDI I/O via midir).

The crate is **Pure Rust** and `no_std`-capable. Its only required dependency is `thiserror`; `futures-core`, `serde`, and `oxiaudio-core` are optional and feature-gated. Build it with `--no-default-features` for a `core` + `alloc` build (the `OxiSoundError::Io` variant is gated behind `std`). The whole crate is `#![forbid(unsafe_code)]`.

## Installation

```toml
[dependencies]
oxisound-core = "0.2.2"
```

```toml
# no_std build (core + alloc), no std::io::Error variant
oxisound-core = { version = "0.2.2", default-features = false }

# async stream traits + serde derives
oxisound-core = { version = "0.2.2", features = ["tokio", "serde"] }
```

## Quick Start

```rust
use oxisound_core::{StreamConfig, SampleFormat, DeviceInfo};

// Build a stream configuration with the fluent builder.
let config = StreamConfig::builder()
    .sample_rate(48_000)
    .channels(2)
    .buffer_size(256)
    .sample_format(SampleFormat::F32)
    .build();
assert_eq!(config.sample_rate, 48_000);

// Validate it against a device's advertised capabilities.
let device = DeviceInfo::builder("Built-in Output")
    .output(true)
    .sample_rates(vec![44_100, 48_000])
    .channel_counts(vec![1, 2])
    .build();
config.validate(&device)?;
# Ok::<(), oxisound_core::OxiSoundError>(())
```

### Preset configurations

```rust
use oxisound_core::StreamConfig;

let cd       = StreamConfig::stereo_44k();          // 44.1 kHz stereo
let standard = StreamConfig::stereo_48k();          // 48 kHz stereo
let voice    = StreamConfig::mono_16k();            // 16 kHz mono
let low_lat  = StreamConfig::low_latency_stereo_48k(); // 48 kHz stereo, 256-frame buffer
assert_eq!(low_lat.buffer_size, Some(256));
```

## API Overview

### Device & stream traits

A backend's device type implements `AudioDevice`; the streams it opens implement `OutputStream`, `InputStream`, or `DuplexStream`. Stream traits are `Send` so they can be moved onto worker threads.

| Trait | Method | Description |
|-------|--------|-------------|
| `AudioDevice` (`Sized`) | `enumerate()` | List available devices as `Vec<DeviceInfo>` |
| | `default_output()` | Open the system default output device |
| | `default_input()` | Open the system default input device |
| | `open_output(config)` | Open an output stream → `Box<dyn OutputStream>` |
| | `open_input(config)` | Open an input stream → `Box<dyn InputStream>` |
| | `open_duplex(config)` | Open a duplex stream → `Box<dyn DuplexStream>` |
| | `negotiate_output(config)` | Pre-flight: resolve the actual `NegotiatedConfig` (default impl returns `UnsupportedConfig`) |
| `OutputStream` (`Send`) | `write(&[f32])` | Write interleaved `f32` samples |
| | `stats()` | Return `StreamStats` (default = zeroed) |
| `InputStream` (`Send`) | `read(&mut [f32])` | Read captured samples, returns count |
| | `stats()` | Return `StreamStats` (default = zeroed) |
| `DuplexStream` (`Send`) | `write(&[f32])` / `read(&mut [f32])` | Combined output + input |
| | `stats()` | Return `StreamStats` (default = zeroed) |

### Async stream traits (`tokio` feature)

These use native `async fn` in traits (RPITIT) and are therefore **not object-safe** — use `impl AsyncOutputStream` / `impl AsyncInputStream` in return positions rather than `Box<dyn …>`. `async fn` in traits does not propagate `Send` on the returned future; callers needing `Send` futures should constrain the impl accordingly.

| Trait | Method | Description |
|-------|--------|-------------|
| `AsyncOutputStream` (`Send`) | `async write(&[f32])` | Asynchronously write interleaved samples |
| `AsyncInputStream` (`Send`) | `stream()` | Borrow a `futures_core::Stream<Item = Vec<f32>>` of captured frames |
| `DeviceWatcher` (`Send`) | `events()` | Borrow a `futures_core::Stream<Item = DeviceEvent>` |

### Configuration types

| Type | Key fields / methods |
|------|----------------------|
| `StreamConfig` | `sample_rate: u32`, `channels: u16`, `buffer_size: Option<u32>`, `sample_format: Option<SampleFormat>`, `exclusive: bool`, `preferred_formats: Vec<SampleFormat>`, `channel_routing: Option<ChannelRouting>`, `buffer_capacity_secs: Option<f32>` |
| `StreamConfig` consts | `STEREO_48K`, `STEREO_44K`, `MONO_16K` |
| `StreamConfig` presets | `stereo_48k()`, `stereo_44k()`, `mono_16k()`, `low_latency_stereo_48k()` |
| `StreamConfig` methods | `builder()`, `validate(&DeviceInfo)` |
| `StreamConfigBuilder` | `sample_rate`, `channels`, `buffer_size`, `sample_format`, `exclusive`, `preferred_formats`, `channel_routing`, `buffer_capacity_secs`, `build` |
| `NegotiatedConfig` | `sample_rate: u32`, `channels: u16`, `buffer_size: u32`, `sample_format: SampleFormat` (the config actually granted by hardware) |

### `SampleFormat` enum

| Variant | Bytes | Float? | Notes |
|---------|-------|--------|-------|
| `F32` | 4 | yes | 32-bit IEEE float, `[-1.0, 1.0]` |
| `I16` | 2 | no | signed 16-bit |
| `I24` | 3 | no | 24-bit signed (CPAL stores in `i32`) |
| `I32` | 4 | no | signed 32-bit |
| `U8` | 1 | no | unsigned 8-bit |
| `F64` | 8 | yes | 64-bit IEEE float |

Methods: `byte_size() -> usize`, `is_float() -> bool`, `Display`. The free function `pick_preferred_format(preferred, supported) -> Option<SampleFormat>` walks a ranked preference list and falls back to the first supported format.

### Device description types

| Type | Key fields / methods |
|------|----------------------|
| `DeviceInfo` | `name`, `is_default`, `sample_rates: Vec<u32>`, `channel_counts: Vec<u16>`, `is_input`, `is_output`, `capabilities: Option<DeviceCapabilities>` |
| `DeviceInfo` methods | `builder(name)`, `supports_config(&StreamConfig)`, `Display` |
| `DeviceInfoBuilder` | `default_device`, `input`, `output`, `sample_rates`, `channel_counts`, `capabilities`, `build` |
| `DeviceCapabilities` | `min_buffer_size`, `max_buffer_size`, `supported_formats: Vec<SampleFormat>`, `exclusive_mode` |
| `StreamStats` | `frames_processed: u64`, `underruns: u64`, `overruns: u64`, `latency_frames: u32`, `cpu_load_percent: f32` |
| `CallbackPriority` | `Normal` (default), `Realtime` |

### `HostApi` enum

Identifies the OS audio host/backend API. `CoreAudio` (macOS/iOS), `Wasapi` (Windows), and `Alsa` (Linux) are platform defaults; `Jack`, `PipeWire`, `PulseAudio`, and `Asio` are opt-in at the `oxisound-cpal` level.

| Variant | Platform | Availability |
|---------|----------|--------------|
| `CoreAudio` | macOS / iOS | default on Apple |
| `Wasapi` | Windows | default on Windows |
| `Asio` | Windows | opt-in (`asio` feature) |
| `Alsa` | Linux / BSD | default on Linux |
| `Jack` | Linux / macOS | opt-in |
| `PipeWire` | Linux | opt-in |
| `PulseAudio` | Linux | opt-in |

Methods: `is_available() -> bool` (compile-time platform check; `Jack`/`Asio` always report `false` here and are overridden by backend crates), `Display`.

### Channel routing

| Type | Description |
|------|-------------|
| `Channel` | Logical channel: `FrontLeft`, `FrontRight`, `Center`, `Lfe`, `SurroundLeft`, `SurroundRight`, `BackLeft`, `BackRight`. Methods: `standard_index()` (ITU-R BS.775 / Microsoft index), `Display` |
| `ChannelRouting(Vec<(Channel, usize)>)` | Maps logical channels to physical indices. Constructors: `stereo()`, `surround_5_1()`, `surround_7_1()`. Methods: `channel_count()`, `apply_interleaved(&mut [f32], channels)`, `Display` |

### Device selection

| Type | Strategy |
|------|----------|
| `DeviceSelector` (trait) | `select(&[DeviceInfo]) -> Option<usize>` |
| `DefaultSelector` | First device with `is_default`; falls back to index 0 |
| `LatencyOptimalSelector` | First available device (latency-aware selection is a planned refinement) |
| `NameMatchSelector(String)` | First device whose name contains the fragment (case-insensitive) |

### Device change notifications

| Type | Description |
|------|-------------|
| `DeviceEvent` | `DeviceAdded(DeviceInfo)`, `DeviceRemoved(String)`, `DefaultChanged(DeviceInfo)`; implements `Display` |
| `DeviceNotificationCallback` (trait) | `on_device_change(&self, DeviceEvent)` — synchronous callback |
| `DeviceWatcher` (trait, `tokio`) | `events()` — async stream of `DeviceEvent` |

### Audio session (platform spec, planned)

`AudioSession` is a specification for iOS/macOS/Android session management (`AVAudioSession`, `AAudio`). Implementing it requires C-FFI bindings that must be feature-gated under the COOLJAPAN Pure Rust Policy; until those exist, desktop implementors should return `OxiSoundError::Unsupported`.

| Type | Description |
|------|-------------|
| `SessionCategory` | `Playback`, `Record`, `PlayAndRecord`, `Ambient`, `SoloAmbient` |
| `SessionInterruptionEvent` | `Began`, `Ended { should_resume: bool }` |
| `AudioSession` (trait) | `set_category`, `set_preferred_sample_rate`, `set_preferred_buffer_duration`, `on_interruption` |

### MIDI types & traits

| Type | Key fields / methods |
|------|----------------------|
| `MidiDeviceInfo` | `name`, `is_input`, `is_output`, `port_count` |
| `MidiMessage` | `status: u8`, `data: Vec<u8>`, `timestamp_micros: u64`. Methods: `is_sysex()`, `sysex_payload()`, `new_sysex(payload)`, `to_bytes()` |
| `MidiInput` (trait, `Send`) | `receive() -> Result<Option<MidiMessage>, _>` |
| `MidiOutput` (trait, `Send`) | `send(&MidiMessage)` |
| `MidiDevice` (trait, `Sized`) | `enumerate_midi()`, `open_midi_input(port)`, `open_midi_output(port)` |

### `MidiClock`

A tempo tracker that computes BPM from MIDI timing ticks (`0xF8`) over a sliding 24-tick window.

| Method | Description |
|--------|-------------|
| `MidiClock::new()` / `default()` | Create a stopped clock |
| `tick(timestamp_micros)` | Record one timing tick |
| `bpm() -> Option<f64>` | Current tempo (`None` until ≥ 2 ticks) |
| `is_running() -> bool` | Whether Start/Continue was seen without a Stop |
| `handle_message(&MidiMessage)` | Dispatch on status byte: Clock/Start/Continue/Stop |

MIDI realtime status constants: `MIDI_CLOCK` (`0xF8`), `MIDI_START` (`0xFA`), `MIDI_CONTINUE` (`0xFB`), `MIDI_STOP` (`0xFC`).

## Feature Flags

| Feature | Default | Description |
|---------|---------|-------------|
| `std` | yes | Links the standard library; enables the `OxiSoundError::Io` variant |
| `tokio` | no | Enables `AsyncOutputStream`, `AsyncInputStream`, and `DeviceWatcher` (backed by `futures_core::Stream`). Does **not** pull in the tokio runtime — callers choose their executor. Implies `std` |
| `serde` | no | Derives `Serialize` / `Deserialize` on `DeviceInfo`, `StreamConfig`, `HostApi`, and most value types |
| `oxiaudio` | no | Enables an optional bridge to `oxiaudio-core` |

## `OxiSoundError` variants

| Variant | `kind()` tag | Description |
|---------|--------------|-------------|
| `NoDevice` | `no-device` | No audio device available |
| `Device(String)` | `device` | Device-level error |
| `Stream(String)` | `stream` | Stream error |
| `UnsupportedConfig(String)` | `unsupported-config` | Requested config not supported |
| `Disconnected(String)` | `disconnected` | Device disconnected |
| `Overrun(String)` | `overrun` | Buffer overrun |
| `Underrun(String)` | `underrun` | Buffer underrun |
| `HotPlugError(String)` | `hot-plug-error` | Hot-plug detection failure |
| `PermissionDenied(String)` | `permission-denied` | Microphone/device permission denied |
| `Timeout(String)` | `timeout` | Operation timed out |
| `FormatMismatch(String)` | `format-mismatch` | Sample-format negotiation failed |
| `Io(std::io::Error)` | `io` | Platform I/O error (`std` only; preserves source via `#[from]`) |
| `Unsupported(String)` | `unsupported` | Feature unsupported on this platform/configuration |

`kind()` returns a stable, lowercase kebab-case identifier suitable as a metrics tag or structured-log key.

## Cross-references

- **Backends:** [`oxisound-cpal`](../oxisound-cpal) (CPAL / native audio APIs), [`oxisound-jack`](../oxisound-jack) (JACK Audio Server), [`oxisound-midi`](../oxisound-midi) (MIDI via midir).
- **Facade:** [`oxisound`](../oxisound) re-exports the traits and types defined here.
- **Sibling crates:** `oxisound-smf` (Standard MIDI File), `oxisound-osc` (Open Sound Control).

## License

Apache-2.0 — COOLJAPAN OU (Team Kitasan)
