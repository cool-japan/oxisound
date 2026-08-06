# Changelog

All notable changes to the OxiSound workspace are documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
OxiSound adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.2] - Unreleased

### Added

### Changed

### Fixed

## [0.2.1] - 2026-08-06

### Security

#### `oxisound-osc`
- **`read_str` out-of-bounds read (fixed):** `crates/oxisound-osc/src/decode.rs` — `read_str` sliced `data[start..]` without first checking `start <= data.len()`. A prior string/type-tag's 4-byte alignment padding could advance the read cursor past the end of the buffer even though the unpadded content ended exactly at the buffer boundary; the next `read_str` call would then panic on an out-of-range slice index instead of returning an error. `read_str` now guards `start > data.len()` explicitly before slicing and returns a descriptive `OscError`. Regression test: `decode_alignment_padding_past_end_returns_error`.
- **Unbounded bundle/array nesting → stack overflow (fixed):** `crates/oxisound-osc/src/decode.rs` — neither `read_typed_args` (`[`/`]` array-nesting type tags) nor `decode_bundle` (`#bundle`-within-`#bundle` recursion) capped how deeply an `OscPacket`/`OscArg` tree could nest. `OscReceiver::recv` (`crates/oxisound-osc/src/server.rs`) hands `decode` up to 65536 raw bytes from an untrusted UDP socket, so a single crafted datagram (e.g. 16384 `[` followed by 16384 `]`, or ~2000 nested `#bundle` wrappers) could build a tree tens of thousands of levels deep; every subsequent recursive traversal of that tree — `Drop` glue, `Debug`, `PartialEq`, or `encode` — would overflow the stack (a denial-of-service abort/SIGSEGV in any process with an OSC receiver bound). Fixed by threading an explicit depth counter through `decode_with_depth`/`decode_bundle` (bundle nesting) and rejecting further `[` nesting once `read_typed_args`'s explicit stack reaches `MAX_NESTING_DEPTH` (32) — both caps are enforced at construction time, before a tree deep enough to matter can ever exist, rather than deferred to a later traversal. Regression tests: `decode_array_nesting_exceeds_max_depth_returns_error`, `decode_16384_open_brackets_raw_udp_payload_rejected_fast`, `decode_array_nesting_within_max_depth_succeeds`, `decode_bundle_nesting_exceeds_max_depth_returns_error`, `decode_2000_nested_bundles_raw_payload_rejected_fast`, `decode_bundle_nesting_within_max_depth_succeeds`.
- **`decode_bundle` element-size addition could wrap on a 32-bit target (fixed):** `crates/oxisound-osc/src/decode.rs` — `decode_bundle` computed a bundle element's end offset as a plain `pos + size`, where `size` is an attacker-controlled 4-byte big-endian length prefix that can be as large as `u32::MAX`. On a 32-bit target `usize` is also 32 bits wide, so `u32::MAX` *is* `usize::MAX`: the addition could wrap around to a small value that would slip past the `> data.len()` bounds check and then panic when slicing `data[pos..end]` with a start greater than its end (unreachable on 64-bit, where the same bytes merely produce a huge-but-non-overflowing value that still safely fails the ordinary bounds check). Extracted the computation into `element_end`, which uses `checked_add` and returns a typed `OscError` on overflow instead. `read_exact`'s analogous `*pos + N` got the same treatment for defense in depth (that branch is unreachable via the current call graph once `decode_bundle` is guarded, since nothing else can drive `pos` near `usize::MAX`). Regression tests: `element_end_overflow_returns_typed_error_not_panic` and `read_exact_pos_overflow_returns_typed_error_not_panic` (direct, width-independent proofs of the `checked_add` branch) plus `decode_bundle_element_size_near_usize_max_returns_error_not_panic` (end-to-end, hand-crafted raw OSC bundle bytes through the public `decode` entry point).

### Added

#### `oxisound-pulse` (new crate — first release)

- **New workspace member `crates/oxisound-pulse`: a 100% Pure-Rust PulseAudio native-protocol backend.** It speaks the PulseAudio native IPC protocol directly over a unix socket (framing and tagstruct serialisation via the pure-Rust [`pulseaudio`](https://crates.io/crates/pulseaudio) 0.3 crate), so nothing in its audio path links a C library — on Linux this is the only OxiSound backend with no C in the path, where the default `oxisound-cpal` route goes through `alsa-lib`. The same socket is served unchanged by **PipeWire**'s `pipewire-pulse` compatibility service, so PipeWire is supported with no PipeWire-specific code. `#![forbid(unsafe_code)]` and `#![deny(missing_docs)]` are enforced crate-wide.
- **Public API.** `PulseDevice` (implements `oxisound_core::AudioDevice`) with `open_default`, `open_named`, `enumerate_output` / `enumerate_input` / `enumerate_all_as` / `enumerate_details`, `server_info`, `descriptor`, `device_info`, `host_api`, and the concrete constructors `open_output_concrete` / `open_input_concrete` / `open_duplex_concrete`; the stream types `PulseOutputStream`, `PulseInputStream`, `PulseDuplexStream` (implementing `OutputStream` / `InputStream` / `DuplexStream`) with `flush()`, `drain()`, `discard_buffered()`, `pause()` / `resume()` (PulseAudio cork/uncork), `negotiated()`, `wire_format()`, `requested_buffer_attrs()`, `ring_capacity_bytes()`, `latency_frames()`, `roundtrip_latency_frames()`, `underrun_count()` / `overrun_count()` and `is_disconnected()`; plus the `DEFAULT_CLIENT_NAME` constant (`"oxisound"`).
- **Host-independent modules, compiled and unit-tested on every target** (only the thin protocol adapter is Linux-gated): `env` — socket and cookie discovery (`resolve_server_socket`, `resolve_cookie_path`, `load_cookie`, `load_cookie_from_env`, `unix_path_from_server_entry`, the injectable `EnvProvider` / `ProcessEnv` / `MapEnv`); `format` — `PulseSampleFormat` and `negotiate_format`, `f32` ⇄ wire-byte conversion; `model` — `PulseDeviceDescriptor`, `PulseDeviceRole`, `PulseBufferAttrs`, `map_descriptors`, `predict_negotiated`, `validate_stream_config`, `period_frames`, `playback_buffer_attrs`, `record_buffer_attrs`, `ring_capacity_bytes`, `whole_samples`; `timeout` — `block_on_timeout`, `WithDeadline` and the `PULSE_CONNECT_TIMEOUT` / `PULSE_OP_TIMEOUT` / `PULSE_FLUSH_TIMEOUT` / `PULSE_DRAIN_TIMEOUT` deadlines that bound every blocking control-plane round-trip.
- **Non-Linux targets compile a Pure-Rust stub, not a build failure.** The `pulseaudio` dependency is declared under `[target.'cfg(target_os = "linux")'.dependencies]`; everywhere else `src/stub.rs` provides the same types with constructors returning `OxiSoundError::Unsupported`, so downstream crates can depend on `oxisound-pulse` unconditionally — the same arrangement `oxisound-jack` uses.
- **Threading model.** The `pulseaudio` client owns one reactor thread per connection and is the only thread touching the socket; OxiSound stream handles exchange bytes with it over lock-free SPSC rings and never block on it. Playback is server-clocked: an empty ring parks a `std::task::Waker` on the reactor side rather than injecting silence, and the next `write()` wakes it.
- **Facade wiring — opt-in `pulse` feature, deliberately NOT in `default = ["pure"]`** (making it the Linux default is a release decision left to the user): `oxisound = { version = "0.2.1", features = ["pulse"] }` enables `pulse_enumerate_devices`, `pulse_default_output`, `pulse_default_input`, `pulse_output`, `pulse_input`, `pulse_duplex`, `pulse_output_named`, `pulse_input_named`, plus re-exports of `PulseDevice` and the stream types. `CpalDevice::with_host(HostApi::PulseAudio)` deliberately continues to return an error — cpal has no PulseAudio host to dispatch to, which is exactly why this crate exists — while `PulseDevice::host_api()` reports `HostApi::PulseAudio` for parity.
- **Scope, stated explicitly.** Implemented: server discovery and cookie authentication, enumeration (server info, sink list, source list) with monitor tagging, playback, capture, duplex, cork/uncork, flush/drain/discard, and underrun/overrun counters. Deliberately *not* implemented in this release (tracked in `TODO.md`): SHM/memfd zero-copy transfer (audio is streamed over the socket as plain memblocks), volume and mute control, module loading, and the subscription/hot-plug event stream.
- Ships with 58 host-independent unit/integration tests and 17 doctests, all passing on non-Linux hosts against the stub and the shared discovery/format/buffer-arithmetic layers.

#### `oxisound-osc`
- `proptest`-based round-trip test (`decode::tests::proptest_roundtrip`): generates arbitrary well-formed `OscMessage` values (all leaf argument types plus bounded-depth nested arrays) and asserts `encode(decode(encode(msg))?) == encode(msg)` — general-form coverage of the same "encode ∘ decode is stable" property `roundtrip_all_types` checks with one hand-picked case, aimed at the boundary-arithmetic class of bug this crate has now shipped twice.
- `fuzz/` workspace (own top-level `[workspace]`, not a member of the main workspace, mirroring the `oxitext`/`oxih5` pattern) with two `cargo-fuzz` targets: `osc_decode` (fuzzes `oxisound_osc::decode` against arbitrary bytes — the same untrusted-UDP-datagram path both Security fixes above harden) and `smf_parse` (fuzzes `oxisound_smf::parse` against arbitrary `.mid` bytes). Both targets build cleanly with `cargo +nightly fuzz build` and ran crash-free for 200k iterations each during this wave's verification (not a substitute for ongoing fuzzing in CI — this only proves the targets are wired correctly).
- `crates/oxisound/tests/stream_stats.rs`: regression coverage for the `stream_stats()` fix below (idle all-zero stream, active stream with non-zero counters).

### Changed

#### `oxisound` (facade)
- **`stream_stats()` no longer conflates "no stats" with "all-zero stats":** previously returned `None` unless at least one of `frames_processed`/`underruns`/`overruns`/`latency_frames` was non-zero, so a freshly opened, perfectly healthy stream that hadn't processed a frame yet was indistinguishable from "stats unavailable." Since the underlying `OutputStream::stats()` trait method is infallible (it defaults to `StreamStats::default()`), `stream_stats()` now always returns `Some`; callers must not treat an all-zero snapshot as an error condition. The `Option` return type is kept for API stability.

#### Workspace hygiene
- `Cargo.toml`: updated `oxiaudio-core` and `oxiaudio` upstream deps from `0.2.0` → `0.2.1`. This bump also cleared the workspace's outstanding `cargo deny check advisories` failure — the refreshed lockfile pulls `crossbeam-epoch 0.9.20` (fixing `RUSTSEC-2026-0204`) and the un-yanked `spin 0.12.2` through the `(dev) oxiaudio → oxifft → rayon` chain (see the `TODO.md` note under Docs below).
- `Cargo.toml`: other `[workspace.dependencies]` refreshes — `thiserror` 2.0.18 → 2.0.19, `ringbuf` 0.5.0 → 0.5.1, `tokio-tungstenite` 0.29.0 → 0.30.0.
- `Cargo.toml`: added `crates/oxisound-pulse` to `[workspace.members]` and `oxisound-pulse = { path = "crates/oxisound-pulse", version = "0.2.1" }` to `[workspace.dependencies]`, along with the `pulseaudio` 0.3.1 and `futures-executor` 0.3.33 entries that crate consumes.
- Added `rustfmt.toml` (pins `edition = "2024"` plus rustfmt's stable-channel defaults, verified to produce zero reformatting diff against the existing codebase) and `clippy.toml` (`msrv = "1.89"`, matching `[workspace.package].rust-version`) at the workspace root.
- `deny.toml`: the `jack-sys` ban now carries a `wrappers = ["jack"]` exception. `jack-sys` was unconditionally denied even though it is only ever reached through the safe `jack` binding crate (itself confined to the `oxisound-jack` quarantine crate per Pure Rust Policy v2 §5) — `cargo deny check bans --all-features --workspace` was failing outright on a dependency the architecture already quarantines correctly. The wrapper exception allows `jack-sys` specifically when `jack` is its sole direct dependent; any other future path to `jack-sys` remains denied.

### Fixed

#### `oxisound-osc` (documentation, not behavior)
- Documented the pre-existing (unchanged) precondition that `encode`/`write_str` never validates: `OscMessage::address` and `OscArg::String` values must not contain an embedded NUL (`'\0'`) byte, or the packet silently round-trips to a *truncated* string (the decoder's `read_str` stops at the first NUL) with the remaining bytes reinterpreted as unrelated packet data. Added doc comments on `encode`, `write_str`, `write_blob` (analogous `u32`-length-truncation precondition for blobs `>4 GiB`), `OscArg::String`, and `OscMessage::address`, plus a permanent regression test (`embedded_nul_in_string_arg_silently_truncates_on_roundtrip`) demonstrating the truncation so the hazard has an executable record, not just a comment.

### Docs

- `TODO.md` and `crates/oxisound/TODO.md`: corrected two sections that tracked functionality as `[x] Done` which does not exist on disk — an `oxisound-pipewire` subcrate (never present in `crates/`; no `pipewire-backend` feature; no `pipewire_output`/`pipewire_input` facade functions) and, on the facade, `jack_native_output`/`jack_native_input`/the `jack-native` feature (removed in 0.2.0 per this file's own `[0.2.0]` entry, but the facade TODO was never updated to match). Both sections now carry a dated correction note instead of silently continuing to claim shipped functionality.
- `README.md`: removed the `pipewire-sys` row from the FFI-classification table (there is no PipeWire C-FFI dependency reachable from this workspace at any feature combination — cpal has no native `pipewire` feature to wrap) and corrected the "PipeWire backend" Known Blocked Items entry, the Overview paragraph, and the M5 milestone line to stop claiming PipeWire support that was never implemented. Bumped the version header from 0.2.0 to 0.2.1 and repinned every `[dependencies]` snippet (`oxisound`, `oxisound-jack`) to `"0.2.1"`. Added the new `oxisound-pulse` crate to the crate-layout tree, the backend/FFI-classification table (as the one Linux backend with no C library in its audio path) and the facade feature-flag table, and rewrote the PipeWire "Known Blocked Item" — PipeWire is now genuinely supported through `oxisound-pulse` and `pipewire-pulse`; what remains blocked is only a *native* PipeWire (non-Pulse) protocol backend. Also fixed the `oxisound-osc` quick-reference snippet, which called a nonexistent `encode::message(&msg)`/`decode::packet(&bytes)` API (the real, doctested public API is the flat `oxisound_osc::encode(&packet)`/`decode(&bytes)` operating on `OscPacket`, not `OscMessage` directly) — the old snippet would not have compiled.
- `crates/oxisound/README.md`: updated the `stream_stats` table row to describe the always-`Some` behavior above.
- `README.md` Test Results: re-measured for the release (2026-08-06, macOS/aarch64) — **297 passed / 8 skipped** under default features and **313 passed / 11 skipped** with `--all-features`, plus **127 doctests**. The earlier figures in this section (`235` default / `251` all-features) predate `oxisound-pulse`, which contributes 58 tests and 17 doctests of its own. Also corrected the `oxisound-jack`/`jack-backend` paragraph, which claimed the feature was excluded from the count — it isn't: `jack-backend` compiles and its non-hardware unit tests pass without a JACK daemon, only its 2 `#[ignore]`d hardware-integration stubs are skipped.
- `TODO.md`: `- [x] cargo deny check clean across all features` is now accurate again. It had been corrected mid-cycle to record that `bans`/`licenses`/`sources` passed but `advisories` failed on `RUSTSEC-2026-0204` (`crossbeam-epoch 0.9.18`) and a yanked `spin 0.12.1`, both reached only through the external `(dev) oxiaudio → oxifft → rayon` chain. The `oxiaudio` 0.2.1 bump above pulled `crossbeam-epoch 0.9.20` and `spin 0.12.2` into `Cargo.lock`, and the full `cargo deny --all-features --workspace check` now reports `advisories ok, bans ok, licenses ok, sources ok` (exit 0). The note has been updated to reflect the resolution rather than the transient failure.

## [0.2.0] - 2026-06-22

### Changed

#### Pure Rust Policy v2 §5 — FFI quarantine enforcement (facade / adapter crates)

- **`oxisound` facade** (`crates/oxisound/Cargo.toml`, `src/lib.rs`): removed `jack`, `asio`, and `jack-native` Cargo features; these forwarded C-FFI dependencies (`cpal/jack` → libjack2, `cpal/asio` → Steinberg ASIO SDK) through the pure facade, violating Policy v2 §5. Applications requiring native JACK must now depend on the `oxisound-jack` quarantine crate directly (`oxisound-jack = "0.2"`). ASIO support will require a dedicated `oxisound-*-asio` quarantine crate when/if created.
- **`oxisound-cpal` adapter** (`crates/oxisound-cpal/Cargo.toml`, `src/device.rs`): removed `jack` (`cpal/jack`) and `asio` (`cpal/asio`) features for the same reason. `HostApi::Jack` and `HostApi::Asio` arms now unconditionally return `OxiSoundError::UnsupportedConfig` with a message directing users to the appropriate quarantine crate.
- **`oxisound` facade** (`src/lib.rs`): removed `jack_output()`, `asio_output()`, `jack_native_output()`, `jack_native_input()`, `jack_midi_output()`, `jack_midi_input()` public functions and all `#[cfg(feature = "jack-native")]` re-exports of `oxisound_jack` types; JACK is no longer surfaced through the pure facade.
- **Workspace** (`Cargo.toml`): bumped workspace version from `0.1.3` → `0.2.0`; updated all internal path-dependency version pins (`oxisound-core`, `oxisound-cpal`, `oxisound-midi`, `oxisound-jack`, `oxisound-smf`, `oxisound-osc`, `oxisound-session`) to `0.2.0`; updated `oxiaudio-core` and `oxiaudio` upstream deps from `0.1.4` → `0.2.0`; updated `[workspace.dependencies]` comment block to reflect the quarantine policy for the `jack` entry.

#### `oxisound-jack` quarantine crate (retained, not removed)

- `crates/oxisound-jack` is intentionally kept as the sole legitimate home for JACK/libjack2 C-FFI. The `jack-backend` feature still activates the binding. No functional changes in this release; doc comment updated to reflect `0.1.4` → correct version reference in the inline Cargo.toml example (minor doc-only fixup committed with the version bump).

## [0.1.3] - 2026-06-19

### Changed
- Workspace version bumped from 0.1.2 to 0.1.3; all subcrate dependency pins updated accordingly (`oxisound-core`, `oxisound-cpal`, `oxisound-midi`, `oxisound-jack`, `oxisound-smf`, `oxisound-osc`, `oxisound-session`)

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
[0.1.3]: https://github.com/cool-japan/oxisound/releases/tag/v0.1.3
[0.2.0]: https://github.com/cool-japan/oxisound/releases/tag/v0.2.0
[0.2.1]: https://github.com/cool-japan/oxisound/releases/tag/v0.2.1
[0.2.2]: https://github.com/cool-japan/oxisound/compare/v0.2.1...HEAD
