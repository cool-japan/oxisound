# Changelog

All notable changes to the OxiSound workspace are documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
OxiSound adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.2] - 2026-06-10

### Added

#### oxisound-session
- Added `README.md` with full API overview, platform behaviour matrix, and feature flag documentation

### Changed

#### Workspace / dependency hygiene
- Moved `jack`, `objc2`, `objc2-foundation`, `objc2-avf-audio`, and `block2` to workspace `[dependencies]` so all subcrates use a consistent pinned version
- `oxisound-jack`: migrated `jack` dep to `workspace = true` (was a direct version pin)
- `oxisound-session`: migrated `objc2`, `objc2-foundation`, `objc2-avf-audio`, `block2` deps to `workspace = true`

### Dependencies updated (workspace)
- `jack` 0.13.5, `objc2` 0.6.4, `objc2-foundation` 0.3.2, `objc2-avf-audio` 0.3.2, `block2` 0.6.2 pinned in `[workspace.dependencies]`

## [0.1.1] - 2026-06-04

### Added

#### oxisound-session (new crate)
- New `oxisound-session` crate providing platform audio session management for iOS and macOS
- `configure_session(category: SessionCategory) -> Result<(), OxiSoundError>`: sets the `AVAudioSession` category via Objective-C (`[AVAudioSession sharedInstance] setCategory:error:`) when the `avf-audio` feature is enabled; returns `Ok(())` on macOS CoreAudio desktop without the feature; returns `OxiSoundError::UnsupportedConfig` on all other platforms
- `request_microphone_permission() -> Result<bool, OxiSoundError>`: queries or requests microphone recording permission using the modern `AVAudioApplication` API (iOS 17+ / macOS 14+); blocks the calling thread on iOS until the user responds (up to 30 s), then returns `OxiSoundError::Timeout`; reads TCC permission state on macOS without prompting
- `avf-audio` feature: opt-in Objective-C FFI via `objc2`, `objc2-avf-audio`, `objc2-foundation`, and `block2`; default features remain 100% Pure Rust (no FFI)
- Full platform dispatch: iOS+macOS with `avf-audio`, macOS without `avf-audio` (CoreAudio desktop stub), iOS without `avf-audio` (returns `UnsupportedConfig`/`PermissionDenied`), all other platforms (returns errors)

#### oxisound (facade)
- `session` feature: routes `configure_session()` and `request_microphone_permission()` through the new `oxisound-session` crate instead of the previous "pending" stubs
- `macos-session` feature: convenience alias that enables `session` + `oxisound-session/avf-audio` in one flag
- Criterion benchmark suite `benches/facade.rs`: benchmarks `sine_test_tone` throughput (100 ms / 1 s / 2 s at 48 kHz stereo), frequency independence, `default_output()` enumeration latency, and `default_output()` + `open_output()` round-trip; skips device benchmarks gracefully in headless CI

### Changed
- `configure_session()` in the facade now delegates to `oxisound-session` when the `session` feature is active; replaces the prior `log::warn!("... pending")` stub with a real AVFoundation call on Apple platforms
- `request_microphone_permission()` in the facade now delegates to `oxisound-session` when the `session` feature is active; replaces the prior stub that always returned `Ok(true)` on Apple platforms with a real TCC/AVAudioApplication permission check

## [0.1.0] - 2026-06-01

### Added

#### oxisound-core
- `SampleFormat` enum (`F32`, `I16`, `I32`, `U8`, `F64`) with serde and Display
- `DeviceCapabilities` struct (buffer size ranges, supported formats, exclusive mode)
- `DeviceInfo::capabilities: Option<DeviceCapabilities>` field
- `StreamConfig::sample_format` and `StreamConfig::exclusive` fields
- `StreamConfigBuilder` fluent builder pattern
- `NegotiatedConfig` struct for pre-flight config negotiation
- `AudioDevice::negotiate_output()` default method
- `ChannelRouting` with predefined stereo, 5.1, 7.1 maps
- `DeviceEvent` enum (DeviceAdded, DeviceRemoved, DefaultChanged)
- `DeviceNotificationCallback` and `DeviceWatcher` traits
- `SessionCategory`, `SessionInterruptionEvent`, and `AudioSession` trait
- MIDI types: `MidiDeviceInfo`, `MidiMessage`, `MidiInput`, `MidiOutput`, `MidiDevice`
- `StreamStats` struct with default impls on all stream traits
- `CallbackPriority` enum (Normal, Realtime)
- Error variants: `HotPlugError`, `PermissionDenied`, `Timeout`, `FormatMismatch`
- `DeviceInfo::supports_config()` convenience method
- `HostApi::is_available()` compile-time platform detection
- `no_std`-compatible core (feature `std` defaults on; `core::error::Error` via thiserror 2.x)
- `oxiaudio` feature: bidirectional type bridge with `oxiaudio-core` (`SampleFormat`, `ChannelRouting ↔ ChannelMap`)
- `MidiClock` with 24-tick EMA BPM estimation and realtime-message dispatch
- `DeviceInfoBuilder` fluent constructor

#### oxisound-cpal
- `CpalDevice`: output / input / duplex streams backed by lock-free SPSC ring buffers (`ringbuf 0.5.0`)
- Sample format dispatch: F32 / I16 / I32 / U16 / I8 / F64 / U8
- Callback-based zero-copy streams: `CpalCallbackOutputStream`, `CpalCallbackInputStream`
- `CpalDevice::open_output_callback()` and `open_input_callback()`
- `CpalDeviceWatcher` for polling-based device hot-plug detection
- `CpalDevice::watch_devices()` and `subscribe_device_events()` (tokio feature)
- `CpalDevice::on_device_change()` synchronous notification with `DeviceChangeGuard`
- `StreamHealth` enum (Healthy, Degraded, Disconnected) and `CpalOutputStream::health()`
- `CpalOutputStream::flush()`, `stream_time()`, `pause()`, `resume()`
- `CpalDevice::enumerate_all()` returning both input and output devices with I/O flags
- `CpalDevice::optimal_buffer_size()` and `open_output_with_retry()` with exponential backoff
- `CpalOutputStream::stats()` override with real underrun, CPU load, and frame tracking
- `CpalDuplexStream::roundtrip_latency_frames()`
- Adaptive buffer sizing: grow on underrun, shrink after stable window
- Automatic stream recovery on disconnect with configurable reconnect policy
- WASAPI exclusive-mode guard (falls back to shared with `log::warn!`)
- Loopback capture support (Linux PulseAudio monitor source; Windows/macOS: `Unsupported`)
- WASM target support via `cpal/wasm-bindgen` (feature `wasm`)
- ASIO opt-in (`asio` feature), JACK opt-in (`jack` feature)
- Replaced all `eprintln!` with `log` crate

#### oxisound (facade)
- `open_output()`, `open_input()`, `duplex_stream()` convenience functions
- `default_output()`, `default_input()`, `enumerate_all_devices()`, `device_by_index()`
- `preferred_output_config()`, `select_device()`
- Test tone generators: `white_noise_test`, `chirp_test_tone`, `silence`, `click_track`
- Callback wrappers: `play_callback`, `capture_callback`, `duplex_callback`
- `configure_session()` stub (iOS/macOS platform implementation pending)
- `request_microphone_permission()` stub
- `stream_stats()` convenience function
- `monitor_stream()` with `MonitorGuard` for periodic health reporting
- `open_loopback()` for system audio capture
- JACK helpers (`jack_output`, `jack_input`) under `jack-native` feature
- PipeWire helpers (`pipewire_output`, `pipewire_input`) under `pipewire-backend` feature
- OSC re-exports under `osc` feature
- MIDI helpers under `midi` feature
- SMF playback helpers under `smf` feature
- Four integration examples: `decode_play`, `realtime_eq`, `capture_encode`, `async_monitor`
- Re-exports for all core types

#### oxisound-midi
- MIDI device enumeration on macOS (CoreMIDI), Windows (WinMM), Linux (ALSA sequencer) via `midir 0.11.0`
- `MidiHostImpl` with input/output port listing
- `MidiInputImpl` with mpsc channel-based timestamped reception
- `MidiOutputImpl` wrapping `MidiOutputConnection`
- SysEx framing (F0..F7); `MidiMessage::new_sysex/is_sysex/to_bytes`
- Virtual MIDI port creation (unsupported on Windows at runtime)

#### oxisound-smf
- SMF format 0 and format 1 parser: `parse(&[u8])` → `SmfFile`
- `TempoMap::from_file` + `tick_to_secs` for tick→seconds conversion with mid-track tempo changes
- `SmfPlayer::midi_events()` playback iterator and `play(output)` blocking player
- SMF writer: `write_smf(&SmfFile)` producing byte-exact round-trip output
- Serde support behind `serde` feature

#### oxisound-jack
- `JackDevice::new`, `open_output`, `open_input`, `open_output_callback` (feature `jack-backend`)
- Ring-buffer backed `JackOutputStream`/`JackInputStream` + zero-copy callback mode
- JACK transport: `transport_state()`, `transport_position()` (frame + BPM via BBT)
- Port management: `connect_ports()`, `auto_connect_output()`, `list_ports()`
- CPU load, xrun count, sample rate, buffer size observability via `JackMetrics` atomics
- JACK MIDI ports: `JackMidiOutput`/`JackMidiInput` with frame-accurate ring-buffer delivery
- `SysExReassembler` for split/interleaved SysEx across JACK process callbacks
- Default build: 100% Pure Rust stubs (no libjack2 required without `jack-backend` feature)

#### oxisound-osc
- OSC message encoding and decoding: all type tags (i f s b h d t c r m T F N I [ ])
- Bundle support with time-tagged `OscPacket` / `OscBundle`
- UDP transport: `OscReceiver::bind + recv`, `OscSender::connect + send`

### Changed
- MSRV bumped to 1.89 (edition 2024)

### Notes
- **Publish prerequisite:** `oxisound-core`'s `oxiaudio` feature depends on `oxiaudio-core` (path dep); `oxiaudio-core` must be published to crates.io before `oxisound-core` and all downstream crates can be packaged. Only `oxisound-osc` is independently publishable today.
- **JACK freewheel:** `set_freewheel` is stubbed as `Unsupported` pending upstream `jack 0.13.5` implementation.
- **Platform gaps:** iOS/macOS audio session management and microphone permission APIs are stubs; PipeWire requires a running daemon (Linux only); Android testing requires NDK cross-compilation setup.

[0.1.0]: https://github.com/cool-japan/oxisound/releases/tag/v0.1.0
[0.1.1]: https://github.com/cool-japan/oxisound/releases/tag/v0.1.1
[0.1.2]: https://github.com/cool-japan/oxisound/releases/tag/v0.1.2
[Unreleased]: https://github.com/cool-japan/oxisound/compare/v0.1.2...HEAD
