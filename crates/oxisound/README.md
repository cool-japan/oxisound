# oxisound — The COOLJAPAN audio device I/O facade

[![Crates.io](https://img.shields.io/crates/v/oxisound.svg)](https://crates.io/crates/oxisound)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

`oxisound` is the top-level façade crate for the OxiSound ecosystem. It gives you one flat, ergonomic API — `open_output`, `open_input`, `play_callback`, `enumerate_devices`, and friends — that hides which backend is doing the work. Cargo feature flags select the backend: the default `pure` feature uses the cpal backend ([`oxisound-cpal`](../oxisound-cpal)), while optional features pull in native JACK ([`oxisound-jack`](../oxisound-jack)), MIDI device I/O ([`oxisound-midi`](../oxisound-midi)), Standard MIDI File support ([`oxisound-smf`](../oxisound-smf)), or Open Sound Control ([`oxisound-osc`](../oxisound-osc)). All device traits and shared types come from [`oxisound-core`](../oxisound-core) and are re-exported at the crate root, so most programs only ever `use oxisound::…`.

The crate is `#![forbid(unsafe_code)]` at the facade layer. **Pure-Rust status depends on the features you enable.** The default `pure` (cpal), `tokio`, `midi`, `smf`, `osc`, and `pulse` paths are Pure Rust — and `pulse` (new in 0.2.1) is the only Linux backend with no C library anywhere in the audio path, since cpal reaches Linux hardware through `alsa-lib`.

> **0.2.0 change:** The `jack`, `jack-native`, and `asio` features have been removed from this facade (COOLJAPAN Pure Rust Policy v2 §5 — FFI quarantine enforcement). Applications requiring native JACK must depend on `oxisound-jack` directly with its `jack-backend` feature. ASIO support requires a future dedicated quarantine crate.

## Installation

```toml
[dependencies]
# Default: Pure-Rust cpal backend
oxisound = "0.2.1"

# Async streaming (tokio) + Pure-Rust playback
oxisound = { version = "0.2.1", features = ["tokio"] }

# Add live MIDI + Standard MIDI File playback
oxisound = { version = "0.2.1", features = ["midi", "smf"] }

# Add Open Sound Control
oxisound = { version = "0.2.1", features = ["osc"] }

# Pure-Rust PulseAudio / PipeWire backend on Linux (new in 0.2.1; opt-in, not in default)
oxisound = { version = "0.2.1", features = ["pulse"] }
```

## Quick Start

```rust,no_run
use oxisound::StreamConfig;

// Open the default output device and play a 1-second sine tone.
let mut stream = oxisound::open_output(StreamConfig::stereo_48k())
    .expect("no output device");
let buf = oxisound::sine_test_tone(440.0, 1.0, StreamConfig::stereo_48k());
stream.write(&buf).expect("write failed");
```

### Zero-copy real-time callback

```rust,no_run
use oxisound::StreamConfig;

// The closure runs on the real-time audio thread; drop the guard to stop.
let _guard = oxisound::play_callback(StreamConfig::stereo_48k(), |buf| {
    for s in buf.iter_mut() {
        *s = 0.0; // fill with your own DSP
    }
}).expect("no output device");
```

### Enumerate devices

```rust,no_run
let devices = oxisound::enumerate_devices().expect("enumeration failed");
print!("{}", oxisound::format_devices(&devices));
```

## Feature Flags

| Feature | Default | Pure Rust | Pulls in | Enables |
|---------|:-------:|:---------:|----------|---------|
| `pure` | ✓ | ✓ | `oxisound-cpal` | cpal backend: `CpalDevice`, blocking + callback streams, device watch/hot-plug, auto-reconnect |
| `pulse` | — | ✓ | `oxisound-pulse` | Pure-Rust PulseAudio native-protocol backend (also PipeWire via `pipewire-pulse`): `PulseDevice`, `pulse_output`/`pulse_input`/`pulse_duplex`, `pulse_*_named`, `pulse_enumerate_devices`. Linux only; a Pure-Rust `Unsupported` stub elsewhere |
| `tokio` | — | ✓ | `oxisound-cpal/tokio`, `tokio-stream`, `futures-core` | Async I/O: `async_output`, `capture_stream`, `watch_devices`, `AsyncInputStream`/`AsyncOutputStream` |
| `midi` | — | ✓ | `oxisound-midi` | Live MIDI device I/O: `enumerate_midi_devices`, `open_midi_input`, `open_midi_output` |
| `smf` | — | ✓ | `oxisound-smf` | Standard MIDI File read/play: `load_smf`, SMF re-exports (`SmfFile`, `SmfPlayer`, …) |
| `osc` | — | ✓ | `oxisound-osc` | Open Sound Control: `encode_osc`/`decode_osc`, `OscSender`, `OscReceiver`, OSC types |
| `session` | — | ✓ | `oxisound-session` | Audio session management (`configure_session`, `request_microphone_permission`); real impl on Apple, stubs elsewhere |
| `macos-session` | — | ✗ | `oxisound-session/avf-audio` | `session` + AVFoundation Obj-C FFI on iOS/macOS |
| `oxiaudio` | — | ✓ | `oxisound-core/oxiaudio` | OxiAudio integration in `oxisound-core` (implies `pure`) |
| `wasm` | — | ✓ | `oxisound-cpal/wasm` | WebAudio backend for `wasm32` targets (implies `pure`) |

> **JACK / ASIO removed in 0.2.0.** The `jack`, `jack-native`, and `asio` features were removed from this crate. Use `oxisound-jack` with `jack-backend` directly for JACK support.

> The `smf` + `midi` combination additionally unlocks `play_smf` (parse and play a `.mid` straight to a MIDI port).

## API Overview

### Test-tone generators (no hardware, always available)

Pure-computation helpers that return interleaved `Vec<f32>` buffers — handy for tests, demos, and DSP scaffolding.

| Function | Description |
|----------|-------------|
| `sine_test_tone(freq_hz, duration_secs, config)` | Pure sine wave at a fixed frequency |
| `white_noise_test(duration_secs, config)` | Deterministic white noise (seeded xorshift32) |
| `chirp_test_tone(f_start, f_end, duration_secs, config)` | Linear frequency sweep with correct phase integration |
| `silence(duration_secs, config)` | Zero-filled buffer |
| `click_track(bpm, duration_secs, config)` | Metronome-style click track (1 ms attack, 5 ms decay) |
| `format_devices(&[DeviceInfo]) -> String` | Human-readable multi-line device listing for debugging |

### Device & stream entry points (`pure` feature)

| Function | Returns | Description |
|----------|---------|-------------|
| `default_output()` / `default_input()` | `CpalDevice` | The system default output / input device |
| `enumerate_devices()` | `Vec<DeviceInfo>` | All output devices on the default host |
| `enumerate_input_devices()` | `Vec<DeviceInfo>` | All input devices |
| `enumerate_all_devices()` | `Vec<DeviceInfo>` | All devices (input and output) in one call |
| `device_by_index(index)` | `CpalDevice` | Output device by zero-based enumeration index |
| `select_device(name_fragment)` | `CpalDevice` | First output device whose name contains the fragment (case-insensitive) |
| `select_input_device(name_fragment)` | `CpalDevice` | First matching input device |
| `open_output(config)` | `Box<dyn OutputStream>` | Default output device, opened for writing |
| `open_input(config)` | `Box<dyn InputStream>` | Default input device, opened for reading |
| `open_loopback(config)` | `Box<dyn InputStream>` | System-audio loopback capture (Linux PulseAudio/PipeWire; `Unsupported` elsewhere) |
| `duplex_stream(config)` | `Box<dyn DuplexStream>` | Default device opened in duplex mode |
| `preferred_output_config()` | `StreamConfig` | Sensible stereo 48 kHz config using the device's optimal buffer |
| `latency_ms(&CpalDevice)` | `f32` | Device-level output latency estimate in milliseconds |

### Callback streams (`pure` feature)

| Function | Returns | Description |
|----------|---------|-------------|
| `play_callback(config, FnMut(&mut [f32]))` | `CpalCallbackOutputStream` | Output with a zero-copy RT callback |
| `capture_callback(config, FnMut(&[f32]))` | `CpalCallbackInputStream` | Input with a zero-copy RT callback |
| `duplex_callback(config, out_cb, in_cb)` | `DuplexCallbackGuard` | Simultaneous in+out callbacks; drop the guard to stop both |

### Monitoring, hot-plug & resilience

| Item | Feature | Description |
|------|---------|-------------|
| `stream_stats(&dyn OutputStream) -> Option<StreamStats>` | always | Snapshot of stream stats (always `Some`; `stats()` is infallible — all-zero fields mean an idle/new stream, not "unavailable") |
| `monitor_stream(stats_fn, interval_ms, callback) -> MonitorGuard` | non-wasm | Periodically sample stats from a background thread |
| `on_device_change(FnMut(DeviceEvent)) -> DeviceChangeGuard` | `pure`, non-wasm | Synchronous callback on device add/remove/default-change (500 ms polling) |
| `auto_reconnect_output(config) -> AutoReconnectGuard` | `pure`, non-wasm | Output stream that auto-reconnects on disconnect (exponential backoff 10→50→200→1000 ms); implements `OutputStream`, with `is_connected()` / `config()` |
| `watch_devices() -> DeviceEventStream` | `tokio` | Async `Stream` of `DeviceEvent`s |

### Async streaming (`tokio` feature)

| Function | Returns | Description |
|----------|---------|-------------|
| `async_output(config)` | `impl AsyncOutputStream` | Async output stream on the default device (not object-safe) |
| `capture_stream(config)` | `impl Stream<Item = Vec<f32>>` | Async stream of captured audio frames |

### MIDI device I/O (`midi` feature)

| Function | Returns | Description |
|----------|---------|-------------|
| `enumerate_midi_devices()` | `Vec<MidiDeviceInfo>` | All available MIDI devices |
| `open_midi_input(port)` | `Box<dyn MidiInput>` | Open a MIDI input by port index |
| `open_midi_output(port)` | `Box<dyn MidiOutput>` | Open a MIDI output by port index |

### Standard MIDI File (`smf` feature)

| Item | Feature | Description |
|------|---------|-------------|
| `load_smf(&[u8]) -> Result<SmfFile, SmfError>` | `smf` | Parse a `.mid` byte buffer |
| `play_smf(&Path, midi_port)` | `smf` + `midi` | Parse a file and play it straight to a MIDI port (blocking) |
| re-exports | `smf` | `SmfFile`, `SmfTrack`, `TrackEvent`, `SmfEvent`, `SmfFormat`, `Division`, `SmfPlayer`, `TempoMap`, `SmfError`, `parse_smf` |

### PulseAudio / PipeWire (`pulse` feature, Linux)

Pure Rust end to end — no `libpulse`, no `alsa-lib`, no C in the audio path. PipeWire is
covered through its `pipewire-pulse` compatibility service. On non-Linux targets these
functions compile but return `OxiSoundError::Unsupported`.

| Function | Returns | Description |
|----------|---------|-------------|
| `pulse_enumerate_devices()` | `Vec<DeviceInfo>` | Every usable sink and source; sinks → `is_output`, sources (incl. `.monitor`) → `is_input` |
| `pulse_default_output()` / `pulse_default_input()` | `PulseDevice` | The server's default sink / source |
| `pulse_output(config)` | `Box<dyn OutputStream>` | Playback on the default sink |
| `pulse_input(config)` | `Box<dyn InputStream>` | Capture on the default source |
| `pulse_duplex(config)` | `Box<dyn DuplexStream>` | Duplex over a single connection |
| `pulse_output_named(name, config)` | `Box<dyn OutputStream>` | Playback on a named sink |
| `pulse_input_named(name, config)` | `Box<dyn InputStream>` | Capture on a named source — pass `"<sink>.monitor"` to record system output |

Re-exported under the feature: `PulseDevice`, `PulseOutputStream`, `PulseInputStream`,
`PulseDuplexStream`. See [`oxisound-pulse`](../oxisound-pulse) for the full API, the
architecture notes, and the explicit list of what is deliberately not implemented (SHM/memfd
zero-copy, volume/mute, module loading, hot-plug subscriptions).

> `CpalDevice::with_host(HostApi::PulseAudio)` still returns an error and will keep doing so —
> cpal has no PulseAudio host to dispatch to, which is exactly why `oxisound-pulse` exists.

### Open Sound Control (`osc` feature)

Re-exports from [`oxisound-osc`](../oxisound-osc): `OscArg`, `OscMessage`, `OscBundle`, `OscPacket`, `OscTimeTag`, `OscError`, `OscSender`, `OscReceiver`, plus `encode_osc` / `decode_osc` (the crate's `encode` / `decode` functions, renamed at re-export).

### Session & permissions

| Function | Description |
|----------|-------------|
| `configure_session(SessionCategory)` | Configure the audio session category (iOS/macOS; `UnsupportedConfig` elsewhere) |
| `request_microphone_permission() -> bool` | Request mic access (iOS/macOS; `PermissionDenied` elsewhere) |

### Re-exported core types (always at crate root)

From [`oxisound-core`](../oxisound-core): `AudioDevice`, `InputStream`, `OutputStream`, `DuplexStream`, `StreamConfig`, `StreamStats`, `SampleFormat`, `NegotiatedConfig`, `Channel`, `ChannelRouting`, `HostApi`, `DeviceInfo`, `DeviceCapabilities`, `DeviceEvent`, `DeviceSelector`, `DefaultSelector`, `NameMatchSelector`, `LatencyOptimalSelector`, `DeviceNotificationCallback`, `CallbackPriority`, `MidiDevice`, `MidiInput`, `MidiOutput`, `MidiDeviceInfo`, `MidiMessage`, `SessionCategory`, `SessionInterruptionEvent`, and the crate error type `OxiSoundError`.

Backend-specific re-exports appear under their features: `CpalDevice`, `CpalOutputStream`, `CpalCallbackInputStream`, `CpalCallbackOutputStream`, `CpalDeviceWatcher`, `DeviceChangeGuard`, `AdaptiveBufferSizer`, `StreamHealth` (`pure`); `CpalAsyncInputStream`, `CpalAsyncOutputStream`, `AsyncInputStream`, `AsyncOutputStream` (`tokio`).

> `oxisound-jack` types (`JackDevice`, `JackOutputStream`, etc.) are no longer re-exported here since 0.2.0. Depend on `oxisound-jack` directly.

## Platform Support

| Capability | macOS | Linux | Windows | iOS | Android |
|------------|:-----:|:-----:|:-------:|:---:|:-------:|
| Output stream | ✓ | ✓ | ✓ | ✓ | — |
| Input stream | ✓ | ✓ | ✓ | ✓ | — |
| Callback mode | ✓ | ✓ | ✓ | ✓ | — |
| Device hot-plug | ✓ | ✓ | ✓ | — | — |
| Auto-reconnect | ✓ | ✓ | ✓ | — | — |
| PulseAudio / PipeWire (`pulse`) | — | ✓ | — | — | — |
| JACK (via `oxisound-jack`, not this facade) | ✓ | ✓ | — | — | — |
| ASIO | — | — | — | — | — |
| Loopback capture | — | ✓ (Pulse/PipeWire) | planned | — | — |

> **JACK** is reached by depending on [`oxisound-jack`](../oxisound-jack) directly with its
> `jack-backend` feature — it is not exposed through this facade (see the 0.2.0 note above).
> **ASIO** is not implemented anywhere in the workspace: it would need its own
> `oxisound-*-asio` quarantine crate, which does not exist yet.

## Related crates

- [`oxisound-core`](../oxisound-core) — device/stream traits and shared types (re-exported here)
- [`oxisound-cpal`](../oxisound-cpal) — the default Pure-Rust cpal backend (`pure`, `tokio`, `wasm`)
- [`oxisound-pulse`](../oxisound-pulse) — Pure-Rust PulseAudio native-protocol backend, also covering PipeWire via `pipewire-pulse` (`pulse`; Linux, stub elsewhere)
- [`oxisound-jack`](../oxisound-jack) — native JACK client quarantine crate (C-FFI, use directly with `jack-backend` feature; not exposed through this facade since 0.2.0)
- [`oxisound-midi`](../oxisound-midi) — live MIDI device I/O (`midi`)
- [`oxisound-smf`](../oxisound-smf) — Standard MIDI File reader/writer/player (`smf`)
- [`oxisound-osc`](../oxisound-osc) — Open Sound Control codec and UDP transport (`osc`)
- [`oxisound-session`](../oxisound-session) — iOS/macOS audio session management (`session`, `macos-session`)

## License

Apache-2.0 — COOLJAPAN OU (Team Kitasan)
