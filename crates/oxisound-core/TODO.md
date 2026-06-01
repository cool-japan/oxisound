# oxisound-core TODO

## Status
Pure Rust traits, types, and error enum for the OxiSound audio device I/O workspace. Implements `DeviceInfo` (with builder, Display, serde), `StreamConfig` (with const presets, validation, Display), `HostApi` enum (7 variants), `OxiSoundError` (7 variants including Disconnected/Overrun/Underrun), traits (`AudioDevice`, `OutputStream`, `InputStream`, `DuplexStream`, `AsyncOutputStream`, `AsyncInputStream` behind tokio feature), device selectors (`DefaultSelector`, `LatencyOptimalSelector`, `NameMatchSelector`). M0-M5 complete. Approximately 592 SLOC including tests.

## Core Implementation

### Device Capabilities Query
- [x] Add `DeviceCapabilities` struct with detailed device info: min/max buffer sizes, supported sample formats (`Vec<SampleFormat>`), exclusive mode support flag, native endianness (~40 SLOC)
- [x] Add `DeviceInfo::capabilities: Option<DeviceCapabilities>` field for backends that expose detailed caps (~10 SLOC)
- [x] Add `SampleFormat` enum to oxisound-core: `F32`, `I16`, `I32`, `U8`, `F64` matching oxiaudio-core's SampleFormat for cross-crate consistency (~15 SLOC)
- [x] Add `StreamConfig::sample_format: Option<SampleFormat>` field for format negotiation (None = let backend choose best) (~10 SLOC)

### Sample Format Negotiation
- [x] Add `StreamConfig::preferred_formats: Vec<SampleFormat>` for ranked format preference (e.g., [F32, I32, I16]) (~10 SLOC)
- [x] Add `NegotiatedConfig` struct returned by `open_output`/`open_input` containing the actual format/rate/channels used by the backend (~20 SLOC)
- [x] Add `AudioDevice::negotiate_output(config: StreamConfig) -> Result<NegotiatedConfig, OxiSoundError>` for pre-flight validation without opening a stream (~15 SLOC)

### Channel Routing
- [x] Add `ChannelRouting` struct: mapping from logical channels (FrontLeft, FrontRight, ...) to physical device channels by index (~30 SLOC)
- [x] Add `StreamConfig::channel_routing: Option<ChannelRouting>` for explicit channel assignment on multi-channel interfaces (~10 SLOC)
- [x] Add predefined routing maps for stereo, 5.1, 7.1 standard speaker configurations (~20 SLOC)

### Device Hot-Plug Detection
- [x] Define `DeviceEvent` enum: `DeviceAdded(DeviceInfo)`, `DeviceRemoved(String)`, `DefaultChanged(DeviceInfo)` (~15 SLOC)
- [x] Define `DeviceWatcher` trait: `fn events(&mut self) -> impl Stream<Item = DeviceEvent> + '_` (behind tokio feature) (~15 SLOC)
- [x] Define synchronous variant: `DeviceNotificationCallback` trait with `fn on_device_change(&self, event: DeviceEvent)` (~10 SLOC)
- [x] Add `OxiSoundError::HotPlugError(String)` variant for hot-plug notification failures (~5 SLOC)

### Audio Session Management
- [x] Define `AudioSession` trait for platform-specific audio session control (iOS/macOS/Android): category (playback, record, play_and_record), routing override (speaker, receiver), interruption handling (~40 SLOC)
- [x] Add `SessionCategory` enum: `Playback`, `Record`, `PlayAndRecord`, `Ambient`, `SoloAmbient` (~10 SLOC)
- [x] Add `SessionInterruptionEvent` enum: `Began`, `Ended { should_resume: bool }` (~10 SLOC)
- [x] Add `AudioSession::set_preferred_sample_rate(rate: u32)` and `set_preferred_buffer_duration(secs: f64)` for iOS/macOS (~10 SLOC)

### MIDI Device Support
- [x] Define `MidiDeviceInfo` struct: name, is_input, is_output, port_count (~15 SLOC)
- [x] Define `MidiInput` trait: `fn receive(&mut self) -> Result<Option<MidiMessage>, OxiSoundError>` (~10 SLOC)
- [x] Define `MidiOutput` trait: `fn send(&mut self, msg: &MidiMessage) -> Result<(), OxiSoundError>` (~10 SLOC)
- [x] Define `MidiMessage` struct: status byte, data bytes, timestamp (~20 SLOC)
- [x] Add `MidiDevice` trait: `fn enumerate_midi() -> Result<Vec<MidiDeviceInfo>, OxiSoundError>`, `fn open_midi_input(port: usize) -> Result<Box<dyn MidiInput>, OxiSoundError>` (~15 SLOC)

### Stream Monitoring
- [x] Define `StreamStats` struct: `frames_processed: u64`, `underruns: u64`, `overruns: u64`, `latency_frames: u32`, `cpu_load_percent: f32` (~15 SLOC)
- [x] Add `OutputStream::stats(&self) -> StreamStats` and `InputStream::stats(&self) -> StreamStats` to trait definitions (~10 SLOC)
- [x] Add `StreamConfig::callback_priority: CallbackPriority` enum: `Normal`, `Realtime` for thread priority hints (~10 SLOC)

### Error Improvements
- [x] Add `OxiSoundError::PermissionDenied(String)` variant for microphone/audio access permission failures (iOS/macOS/Android) (~5 SLOC)
- [x] Add `OxiSoundError::Timeout(String)` variant for stream operations that exceed deadline (~5 SLOC)
- [x] Add `OxiSoundError::FormatMismatch(String)` variant for sample format negotiation failures (~5 SLOC)
- [x] Implement `std::error::Error::source()` chain for wrapped platform errors (~10 SLOC) — handled by thiserror `#[from]` on `OxiSoundError::Io(#[from] std::io::Error)`

## API Improvements
- [x] Add `StreamConfig::builder()` pattern: `StreamConfig::builder().sample_rate(48000).channels(2).buffer_size(256).build()` (~25 SLOC)
- [x] Add `#[must_use]` on all Result-returning public methods (~5 SLOC)
- [x] Add `DeviceInfo::supports_config(&self, config: &StreamConfig) -> bool` convenience (~5 SLOC)
- [x] Add `HostApi::is_available() -> bool` runtime check per platform (~15 SLOC)
- [x] Add `Display` for `OxiSoundError` variants with more structured formatting (~10 SLOC)
- [x] Consider `no_std` path: gate `std::error::Error` behind `std` feature once MSRV >= 1.81 (core::error::Error) (~audit task) — Done 2026-05-25: MSRV bumped to 1.89; `#![no_std]` added to oxisound-core with `extern crate alloc`; `std` feature (default on) gates `OxiSoundError::Io(std::io::Error)` and std imports; all other variants use `alloc::string::String`; thiserror 2.x auto-selects `core::error::Error` on no_std builds. All `std::fmt::*` → `core::fmt`, `std::collections::VecDeque` → `alloc::collections::VecDeque`, `format!`/`vec!`/`Vec`/`String`/`Box`/`ToString` from `alloc`. `oxiaudio_bridge.rs` also updated with `alloc` imports. Verified: `cargo build -p oxisound-core --no-default-features` clean; 81/81 tests pass with `--all-features`; zero clippy warnings in both modes.

## Testing
- [x] Add property-based tests (proptest): random StreamConfig values validated against random DeviceInfo (~25 SLOC)
- [x] Test all DeviceSelector implementations with edge cases: empty device list, all defaults, no defaults, duplicate names (~20 SLOC)
- [x] Test StreamConfig::validate with boundary values: min/max sample rates, 0 channels, u32::MAX buffer size (~20 SLOC)
- [x] Test DeviceInfo builder: verify all combinations of optional fields (~15 SLOC)
- [x] Test HostApi Display + serde roundtrip for all 7 variants (~10 SLOC)
- [x] Test async trait implementations with mock structures under tokio feature (~20 SLOC)
- [x] Test DeviceEvent serialization and Display once hot-plug types are added (~15 SLOC)

## Performance
- [x] Evaluate whether `DeviceInfo` should use `SmallVec<[u32; 4]>` instead of `Vec<u32>` for sample_rates to avoid heap allocation for common cases (~analysis) — Done 2026-05-25: Analysis complete. Device enumeration is not on the hot audio callback path; allocation is one-time per device object. Adding `smallvec` as a workspace dep for this single field is not justified. `Vec<u32>` retained.
- [x] Profile `StreamConfig::validate` hot path: ensure no unnecessary allocations in error message formatting (~5 SLOC) — Done 2026-05-25: Verified at lib.rs:379-401 — allocations only inside `Err` arms; success path is allocation-free. No change needed.

## Integration
- [x] Coordinate `SampleFormat` enum with oxiaudio-core's `SampleFormat` for cross-crate type consistency — Done 2026-05-25: Bijective bridge in `oxiaudio_bridge.rs` (gated behind `oxiaudio` feature): `impl From<oxiaudio_core::SampleFormat> for SampleFormat` + inherent `SampleFormat::to_oxiaudio(self)`. Both directions exhaustive (identical 6-variant sets). 9 bridge tests including roundtrip for all 6 variants.
- [x] Coordinate `ChannelRouting` with oxiaudio-core's `ChannelMap` for unified channel assignment — Done 2026-05-25: Full bridge in `oxiaudio_bridge.rs`: `From<Channel> for ChannelId` (total, 8→8), `TryFrom<ChannelId> for Channel` (fallible: Top* height channels → `FormatMismatch`), `From<&ChannelRouting> for ChannelMap` (sorts by physical index), `TryFrom<&ChannelMap> for ChannelRouting` (fallible), inherent `to_oxiaudio_channel_map()`. Tests: stereo/5.1/7.1 roundtrips, Top* rejection.
- [x] Ensure `DeviceEvent` types are compatible with oxisound facade re-exports (~re-export audit) — Done 2026-05-25: DeviceEvent is defined in oxisound-core and re-exported from the facade via `pub use oxisound_core::DeviceEvent`; inherently compatible.
- [x] Define integration contract for MIDI types with potential oximidi crate (~API design) — Done 2026-05-25: Integration contract documented in rustdoc on `MidiInput`/`MidiOutput` traits: timestamp merging, clock sync via `MidiClock::handle_message`, sequencer BPM bridge. See `lib.rs`.
- [x] Provide `AudioSession` platform-specific implementations in oxisound-cpal (~implementation guide) — Done 2026-05-25: Implementation guide documented in rustdoc on `AudioSession` trait: AVAudioSession bridge steps, Android AAudio/oboe path, feature-gating requirements, status note.
