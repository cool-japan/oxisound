# OxiSound TODO

Workspace-wide task list. Individual sub-crate TODOs live under `crates/<crate>/TODO.md`.

## Release Status

**v0.2.0 — Released 2026-06-22**

- **Breaking (facade):** Removed `jack`, `jack-native`, and `asio` Cargo features from `oxisound` facade and `oxisound-cpal`. JACK is now opt-in via `oxisound-jack` quarantine crate only. ASIO: no quarantine crate yet (future work). Enforces COOLJAPAN Pure Rust Policy v2 §5.
- **Tests:** 194 passing (8 skipped, platform-conditional); `oxisound-jack` excluded (no libjack2 on macOS).

**v0.1.3 — Released 2026-06-19**

**v0.1.2 — Released 2026-06-10**

**v0.1.1 — Released 2026-06-04**

- **oxisound-core**: M0-M5 complete. DeviceInfo (builder, serde), StreamConfig (const presets, validation, low-latency), HostApi (7 variants), OxiSoundError (7 variants), AudioDevice/OutputStream/InputStream/DuplexStream traits, AsyncOutputStream/AsyncInputStream (tokio), DeviceSelector (Default/LatencyOptimal/NameMatch). `no_std` support. oxiaudio type bridge (optional).
- **oxisound-cpal**: M0-M5 complete. CpalDevice with output/input/duplex streams, lock-free SPSC ring buffers (ringbuf 0.5.0), sample format dispatch (F32/I16/U16/I8/I32/F64), config validation, capacity cap (~2s), underrun counting, disconnect detection, JACK/ASIO feature gates, host selection, async output/input (tokio). Adaptive buffer sizing, auto-reconnect, loopback capture, WASM support.
- **oxisound** (facade): M0-M5 complete. Convenience API (default_output/input, enumerate, open_output/input, select_device, duplex_stream, jack/asio helpers), sine_test_tone, format_devices, async_output/capture_stream, 4 examples. Callback API, monitor_stream, open_loopback.
- **oxisound-midi**: Complete. MidiHost, MidiInputImpl, MidiOutputImpl, virtual port creation, SysEx, MidiClock.
- **oxisound-smf**: Complete. SMF format 0/1 parser, TempoMap, SmfPlayer, SMF writer, serde support.
- **oxisound-jack**: Complete (pure-Rust stub + jack-backend feature). JACK streams, MIDI ports, transport, observability metrics.
- **oxisound-osc**: Complete. OSC encode/decode, all type tags, bundle support, UDP transport.
- **Total workspace SLoC**: ~10,179 Rust (production + tests), 194 tests passing (0.2.0; oxisound-jack excluded on macOS).

## Publish Prerequisites

- `oxisound-osc` — independently publishable (no oxisound-core dep)
- All other crates — blocked on `oxiaudio-core` being published to crates.io first (oxisound-core optional dep via `oxiaudio` feature)

## Known Blocked Items (post-0.1.0)

## Workspace-Wide Priorities

### Device Capabilities and Negotiation
- [x] Add `DeviceCapabilities` to oxisound-core: buffer size ranges, supported sample formats, exclusive mode flag
- [x] Add `SampleFormat` enum to oxisound-core for format negotiation
- [x] Add `NegotiatedConfig` returned from stream opening with actual format/rate/channels used
- [x] Implement format preference ranking in stream builders
  - **Goal:** Wire preferred_formats Vec<SampleFormat> and channel routing into cpal build path; planned 2026-05-25.

### Channel Routing
- [x] Add `ChannelRouting` type for mapping logical channels to physical device channels
- [x] Predefined routing maps for stereo, 5.1, 7.1 speaker configurations
- [x] Coordinate with oxiaudio-core's `ChannelMap` for unified channel management — Done 2026-05-25: Full bidirectional bridge in `oxisound-core` behind `oxiaudio` feature: `From<&ChannelRouting> for ChannelMap` (total), `TryFrom<&ChannelMap> for ChannelRouting` (fallible for Top* height channels), inherent `to_oxiaudio_channel_map()`.

### Device Hot-Plug Detection
- [x] Define `DeviceEvent` enum: Added/Removed/DefaultChanged
- [x] Implement polling-based hot-plug detection in oxisound-cpal
- [x] Add async `DeviceWatcher` (tokio feature) for device change notifications
- [x] Implement automatic stream recovery on device disconnect with reconnection to new default

### Callback-Based API
- [x] Add zero-copy callback mode bypassing ring buffer for lowest-latency audio rendering
- [x] Add duplex callback combining input and output in a single callback invocation
- [x] Expose callback-based API through the oxisound facade

### Low-Latency and Exclusive Mode
- [x] WASAPI exclusive mode support on Windows — Done 2026-05-25: cpal 0.17.3 does not expose an exclusive-mode builder (hardcoded AUDCLNT_SHAREMODE_SHARED). Implemented `warn_if_exclusive_requested()` guard wired into all stream-opening entry points in oxisound-cpal; falls back to shared mode with a `log::warn!` on both Windows and other platforms.
- [x] Adaptive buffer sizing: grow on underrun, shrink after stable period
- [x] Latency measurement instrumentation in callbacks — Done: callback_duration_ns/buffer_period_ns AtomicU64 pairs in CpalOutputStream/CpalInputStream; cpu_load_percent computed in stats() on 2026-05-25.

### Loopback Capture
- [x] System audio loopback capture on Windows (WASAPI loopback) and Linux (PulseAudio monitor) — Done 2026-05-25: Linux path implemented (monitor device enumeration via `cpal::default_host().input_devices()`); Windows/macOS return `Unsupported` (cpal 0.17.3 has no native loopback API).
- [x] Expose via `open_loopback()` in facade — Done 2026-05-25: `oxisound::open_loopback(config)` added to facade crate.

### Stream Monitoring
- [x] `StreamStats` struct with frames_processed, underruns, overruns, latency, CPU load
- [x] Periodic health reporting for production monitoring

### Platform-Specific Features
- [x] iOS/macOS audio session management (category, routing, interruption handling) — Done 2026-06-03: New `oxisound-session` subcrate with `AVAudioSession.setCategory:error:` via `objc2-avf-audio` behind `avf-audio` feature. `oxisound` facade `macos-session` feature delegates `configure_session()` to `oxisound_session::configure_session()`. Default macOS (CoreAudio desktop, no `avf-audio`): `Ok(())` with debug log. iOS without feature: `UnsupportedConfig`. Other platforms: `UnsupportedConfig`. Routing override and interruption handling are tracked in `AudioSession` trait (oxisound-core); full implementation requires iOS run-loop integration (deferred).
- [x] Microphone permission request for iOS/macOS/Android — Done 2026-06-03: `oxisound-session::request_microphone_permission()` uses `AVAudioApplication.requestRecordPermissionWithCompletionHandler:` (modern API replacing deprecated `AVAudioSession.requestRecordPermission:`). Checks `recordPermission` first; if undetermined, sends block callback and spin-waits up to 30 s. `oxisound` facade `macos-session` feature delegates to this. Android: `PermissionDenied` (hardware-gated, no AAudio permission prompt API).
- [ ] PipeWire backend investigation and opt-in feature — **Blocked upstream:** cpal 0.17.3 has no native PipeWire feature; PipeWire users rely on ALSA compat layer. Revisit when cpal adds PipeWire support. See oxisound-cpal TODO for per-item detail.

### Error Recovery and Resilience
- [x] Automatic stream restart on non-fatal errors with exponential backoff
- [x] Stream health indicator: Healthy/Degraded/Disconnected
- [x] Replace `eprintln!` with `log` crate for production-grade error reporting

### MIDI Device Support
- [x] Define MIDI types in oxisound-core: MidiDeviceInfo, MidiInput, MidiOutput, MidiMessage
- [x] Plan oxisound-midi subcrate for MIDI I/O implementation
  - **Goal:** Implemented oxisound-midi crate with midir backend (CoreMIDI/WinMM/ALSA) on 2026-05-25.
- [x] Coordinate with oxiaudio-decode MIDI file parser for synthesis pipeline — Done 2026-05-26: Integration contract documented in oxisound-core MidiInput/MidiOutput traits. Usage: parse SMF events externally (e.g., via oxiaudio-decode when available), then route each MidiMessage to MidiOutput::send(). Timeline events are caller-driven (sleep/schedule per event delta time). No coupling to oxiaudio internals required.

### New Subcrate Ideas

#### oxisound-smf (SMF Parser)
- [x] Parse Standard MIDI Files (SMF format 0 and format 1) — Done 2026-05-26: `oxisound-smf` crate with `parse(&[u8])`, `SmfFile`, `SmfTrack`, `SmfEvent`.
- [x] Tempo map: tick→seconds conversion with mid-track tempo changes — Done 2026-05-26: `TempoMap::from_file` + `tick_to_secs`.
- [x] Playback iterator: timed MIDI events across all tracks — Done 2026-05-26: `SmfPlayer::midi_events()`.
- [x] Blocking playback to MidiOutput — Done 2026-05-26: `SmfPlayer::play(output)` with `thread::sleep` between events.
- [x] SMF writer: encode SmfFile to .mid bytes — Done 2026-05-26: `write_smf(&SmfFile)` in `oxisound-smf`; VLQ encoding, EndOfTrack auto-appended, byte-exact round-trip test; 4 writer tests.
- [x] Serde support for SmfFile types — Done 2026-05-26: `serde` feature adds `Serialize`/`Deserialize` to `SmfFile`, `SmfTrack`, `TrackEvent`, `SmfEvent`, `SmfFormat`, `Division`, `SmfError`; chains `oxisound-core/serde` for `MidiMessage`.

#### oxisound-midi
- [x] MIDI device enumeration on macOS (CoreMIDI), Windows (WinMM), Linux (ALSA sequencer)
  - **Done:** Implemented in oxisound-midi via midir 0.11.0 on 2026-05-25.
- [x] MIDI input with timestamped message reception
  - **Done:** MidiInputImpl with mpsc channel-based polling on 2026-05-25.
- [x] MIDI output with message scheduling
  - **Done:** MidiOutputImpl wrapping MidiOutputConnection on 2026-05-25.
- [x] SysEx message support (variable-length) — Done: SysEx framing (F0..F7) handled in oxisound-midi callback; MidiMessage::new_sysex/is_sysex/to_bytes helpers in core on 2026-05-25.
- [x] MIDI clock synchronization — Done: MidiClock in oxisound-core with tick(), bpm() (24-tick EMA window), handle_message() dispatching FA/FB/FC/F8 on 2026-05-25.
- [x] Virtual MIDI port creation — Done: MidiHost::create_virtual_input/output in oxisound-midi; returns Unsupported on Windows at runtime on 2026-05-25.

#### oxisound-jack (JACK Audio Server)
- [x] Direct JACK client API (bypass cpal for lowest latency): `jack_client_open`, port registration, process callback — Done 2026-05-26: `oxisound-jack` crate with `JackDevice::new`, `open_output`, `open_input`, `open_output_callback`; ring-buffer backed OutputStream/InputStream + zero-copy callback mode.
- [x] JACK transport integration: position query, tempo sync — Done 2026-05-26: `JackDevice::transport_state()`, `transport_position()` (frame + BPM via BBT data).
- [x] JACK port connection management: auto-connect to system ports — Done 2026-05-26: `connect_ports()` + `auto_connect_output()`.
- [x] JACK cpu_load and port listing — Done 2026-05-26: `JackOutputStream::cpu_load()`, `list_ports()`, `list_input_ports()`, `list_output_ports()` via `jack::AsyncClient::as_client()`; same API on `JackInputStream` and `JackCallbackOutputStream`; stub equivalents in non-`jack-backend` build return 0.0/empty-vec.
- [ ] JACK freewheel mode for offline rendering — **Blocked upstream:** `set_freewheel` is commented out as TODO in `jack` 0.13.5 safe API. `JackDevice::set_freewheel(bool)` is wired and returns `OxiSoundError::Unsupported` until upstream implements it. Track: https://github.com/RustAudio/rust-jack
- [x] JACK MIDI ports: frame-accurate input/output (planned 2026-05-26) — Done 2026-05-26: `midi_util.rs` (SysExReassembler, MIDI framing helpers, 11 unit tests, no libjack dep); `midi.rs` (JackMidiOutput/JackMidiInput with ringbuf SPSC, JackMidiOutputHandler/JackMidiInputHandler implementing ProcessHandler, optional MidiOutput trait impl); facade passthroughs `jack_midi_output`/`jack_midi_input`.
  - **Goal:** Realtime-safe JACK MIDI in/out — the one transport the JACK subcrate lacks. Sub-buffer frame-accurate timing via jack's `RawMidi { time, bytes }`.
  - **Design:** Pure always-compiled `midi_util.rs`: `SysExReassembler` (accumulates F0..F7 across reads, handles interleaved realtime bytes) + MIDI framing helpers. Feature-gated (`jack-backend`) `midi.rs`: `JackMidiOutput` / `JackMidiInput` with ringbuf SPSC ring buffer carrying `(time: u32, len: u8, [u8; N])` entries. `JackMidiOutputHandler: ProcessHandler` drains ring in `process()`, writes via `port.writer(ps).write(&RawMidi { time, bytes })`. `JackMidiInputHandler: ProcessHandler` iterates `port.iter(ps)`, pushes into ring for main-thread drain. Optional `impl oxisound_core::MidiOutput for JackMidiOutput` (uses time=0). Facade stubs.
  - **Files:** `crates/oxisound-jack/src/midi_util.rs` (new, pure), `crates/oxisound-jack/src/midi.rs` (new, feature-gated), `crates/oxisound-jack/src/lib.rs`, `crates/oxisound/src/lib.rs`.
  - **Tests:** Unit tests on `SysExReassembler` (split SysEx, interleaved realtime, back-to-back); `#[ignore]` hardware integration tests.
- [x] JACK observability: sample-rate / xrun / buffer-size / latency atomics (planned 2026-05-26) — Done 2026-05-26: `metrics.rs` (JackMetrics with Arc<Atomic*>, MetricsSnapshot, 6 unit tests, no libjack dep); JackNotifier implementing NotificationHandler (sample_rate + xrun); buffer_size override on all ProcessHandler impls; per-cycle get_latency_range → metrics; AsyncClient<(),H> → AsyncClient<JackNotifier,H>; current_sample_rate/xrun_count/current_buffer_size accessors; stats() reads real latency_frames; lib.rs wired.
  - **Goal:** Replace hardcoded `latency_frames: 0` in `stats()` with real values; expose sample-rate, xrun count, buffer size — bringing JACK to parity with cpal `StreamStats`.
  - **Design:** Pure always-compiled `metrics.rs`: `struct JackMetrics` with `Arc<AtomicU32/U64>` fields for sample_rate, xrun_count, buffer_size, latency_frames; `record_*` / `snapshot` methods. Feature-gated `struct JackNotifier: NotificationHandler + Send + Sync`: `sample_rate()` stores atomic; `xrun()` fetch_add. Existing `ProcessHandler` impls gain `buffer_size()` override; inside `process()` call `port.get_latency_range(LatencyType::Playback/Capture).1` and store. Type change `AsyncClient<(), H>` → `AsyncClient<JackNotifier, H>` across three stream types. Stream structs gain `current_sample_rate()`, `xrun_count()`, `current_buffer_size()` accessors.
  - **Files:** `crates/oxisound-jack/src/metrics.rs` (new, pure), `crates/oxisound-jack/src/client.rs`, `crates/oxisound-jack/src/lib.rs`.
  - **Tests:** Unit tests on `JackMetrics` atomics; `#[ignore]` hardware integration tests.
- [x] Note: requires C FFI (libjack), must be feature-gated per COOLJAPAN policy — Done: `jack-backend` feature gates libjack2; default build is Pure Rust stub.

#### oxisound-pipewire (PipeWire Integration)
- [x] Native PipeWire client API via `pipewire-rs` (Pure Rust bindings) — Done 2026-05-26: `PipeWireDevice::new`, `open_output`, `open_input` via `pipewire 0.10.0` (`MainLoopRc`/`ContextRc`/`CoreRc`/`StreamBox`).
- [x] Stream creation with format negotiation — Done 2026-05-26: SPA `AudioInfoRaw` F32LE format pod built with `PodSerializer`; passed to `stream.connect`.
- [x] Device/node enumeration via PipeWire registry — Done 2026-05-26: `PipeWireDevice::enumerate_devices` via `RegistryRc` `GlobalObject` events + `Core::sync` round-trip.
- [x] Volume/mute control via PipeWire node properties — Done 2026-05-26: Software-level volume/mute via Arc<AtomicU32>/Arc<AtomicBool>; applied in process callback before writing to PipeWire buffer.
- [x] enumerate_devices with 2s timeout — Done 2026-05-26: Timer thread fires `mainloop.quit()` after 2s to prevent hang when no PipeWire daemon is running.
- [x] Session management: route audio to specific sinks (planned 2026-05-26) — Done 2026-05-26: `open_output_to_target`/`open_input_to_target` added to `PipeWireDevice`; `build_stream_properties` helper uses raw string `"target.object"` (avoids `v0_3_44` feature gate); target threaded through `run_output_loop`/`run_input_loop`; facade passthroughs `pipewire_output_to_target`/`pipewire_input_to_target` added.
  - **Goal:** Open a PipeWire output/input bound to a specific node by name via `target.object`.
  - **Design:** Add `PipeWireDevice::open_output_to_target(&self, config, target: &str)` and `open_input_to_target`; refactor private `open_output_impl(config, target: Option<String>)` delegated by both. Thread target into `run_output_loop` / `run_input_loop`. Factor `build_stream_properties(name, category, role, target: Option<&str>) -> Properties` that runs the existing `properties!{...}` then conditionally `props.insert("target.object", t)` (raw string avoids `v0_3_44` feature gate). Facade passthroughs `pipewire_output_to_target` / `pipewire_input_to_target` under pipewire feature; stub Unsupported otherwise.
  - **Files:** `crates/oxisound-pipewire/src/device.rs`, `crates/oxisound-pipewire/src/lib.rs`, `crates/oxisound/src/lib.rs`.
  - **Tests:** Pure unit test on `build_stream_properties` (feature + linux gated, no daemon); `#[ignore]` integration test.
- [x] Note: evaluate `pipewire-rs` Pure Rust status before adoption — Done 2026-05-26: `pipewire 0.10.0` safe public API is compatible with `#![forbid(unsafe_code)]`; C FFI is internal to `pipewire-sys`. Linux-only. Feature-gated as `pipewire-backend`; default build is 100% Pure Rust stub.

#### oxisound-osc (Open Sound Control)
- [x] OSC message encoding (address + typed args to bytes) — Done 2026-05-26: `oxisound_osc::encode`.
- [x] OSC message decoding (bytes to OscMessage/OscBundle) — Done 2026-05-26: `oxisound_osc::decode`.
- [x] OSC bundle support (time-tagged collections of messages) — Done 2026-05-26: `OscBundle` + `OscPacket`.
- [x] All OSC type tags: i f s b h d t c r m T F N I [ ] — Done 2026-05-26: `OscArg` enum.
- [x] UDP receiver (OscReceiver::bind + recv) — Done 2026-05-26: std-feature-gated.
- [x] UDP sender (OscSender::connect + send + send_message) — Done 2026-05-26: std-feature-gated.

### Quality and Documentation
- [x] Comprehensive rustdoc with examples for every public function across all crates — Done 2026-05-25: Added `# Examples` sections to all public functions that lacked them in the `oxisound` facade and `oxisound-core`. Facade additions: `AutoReconnectGuard::is_connected` and `AutoReconnectGuard::config` (hardware-dependent, `no_run`). Core additions: `StreamConfig::stereo_48k/stereo_44k/mono_16k/low_latency_stereo_48k/validate`, all `StreamConfigBuilder` methods, `Channel::standard_index`, `ChannelRouting::surround_5_1/surround_7_1/channel_count/apply_interleaved`, `MidiMessage::is_sysex/sysex_payload/new_sysex/to_bytes`, `MidiClock::new/tick/bpm/is_running/handle_message`, and all `DeviceInfoBuilder` methods. Pure-computation functions use runnable examples; hardware-dependent ones use `no_run`. Doc-tests: oxisound 38 passed, oxisound-core 46 passed. Zero doc/clippy warnings.
- [x] Platform support matrix: which features work on which OS
- [x] `cargo doc --workspace --no-deps --all-features` zero warnings
- [x] Integration examples: decode+play, capture+encode, real-time effects, async monitoring — Done 2026-05-25: Four examples in `crates/oxisound/examples/`: `decode_play.rs` (oxiaudio::decode_file → playback), `realtime_eq.rs` (BiquadFilter::peaking_eq), `capture_encode.rs` (capture → oxiaudio::encode_wav to temp dir), `async_monitor.rs` (tokio Stream RMS level meter).
- [x] CHANGELOG.md in Keep-a-Changelog format
- [x] `cargo deny check` clean across all features

### WASM Target Investigation
- [x] Evaluate cpal AudioWorklet/WebAudio backend for browser-based audio — Done 2026-05-25: cpal 0.17.3 exposes `wasm-bindgen` feature for WebAudio backend on wasm32-unknown-unknown; gated as `wasm = ["cpal/wasm-bindgen"]` in oxisound-cpal.
- [x] Verify `#![forbid(unsafe_code)]` compatibility with WASM audio path — Done 2026-05-25: Verified clean — cpal's internal WebAudio host has unsafe code (dependency); oxisound-cpal's own `#![forbid(unsafe_code)]` is unaffected. Build verified with `cargo build -p oxisound-cpal --no-default-features --features wasm --target wasm32-unknown-unknown`.
- [x] Feature gate behind `wasm` feature flag if viable — Done 2026-05-25: `wasm = ["cpal/wasm-bindgen"]` added to oxisound-cpal; facade passthrough `wasm = ["pure", "oxisound-cpal/wasm"]`; `std::thread::spawn` and `Instant::now()` sites cfg-gated with `#[cfg(not(target_arch = "wasm32"))]`.
- [x] Document GOVERNANCE classification for Web Audio API boundary — Done 2026-05-25: GOVERNANCE doc added to `oxisound-cpal/src/lib.rs` module docs, classifying Web Audio as OS-boundary backend (same rationale as ALSA/CoreAudio/WASAPI) with wasm32 runtime caveats documented.

### Android Support
- [x] Investigate cpal's Oboe/AAudio support for Android — Done 2026-05-25: cpal 0.17.3 has a native **AAudio** backend at `src/host/aaudio/` (no Oboe layer). The host is auto-selected via `cfg(target_os = "android")` — no feature flag needed in oxisound-cpal. Backend dependencies are target-gated in cpal's `Cargo.toml`: `ndk 0.9` (features = `["audio", "api-level-26"]`), `jni 0.21`, `ndk-context 0.1`. The aaudio backend uses `unsafe impl Send/Sync for Stream` inside cpal (OS-boundary, same governance rationale as ALSA/CoreAudio/WASAPI); `oxisound-cpal`'s own `#![forbid(unsafe_code)]` is unaffected. oxisound-cpal requires no code changes for Android — cpal handles the backend transparently.
- [ ] Test on Android emulator and physical device — **Deferred (hardware-gated):** requires Android NDK toolchain, cross-compilation to `aarch64-linux-android`, and a connected device/emulator. Add `#[ignore]` hardware tests when CI environment is available.
- [x] Document minimum Android API level requirements — Done 2026-05-25: **Minimum API level 26 (Android 8.0 Oreo)**, enforced by the `api-level-26` feature on ndk 0.9 in cpal's target-gated dependencies. AAudio itself was introduced in Android API 26. The runtime also requires `ndk-context` to be initialised (app must be attached to a Java VM / Android Activity) for JNI-based device enumeration (`AudioManager`) to work.
- [x] Feature gate if additional dependencies are needed — Done 2026-05-25: No feature gating required. cpal's Android AAudio backend is fully auto-selected; all Android-specific cpal deps (`ndk`, `jni`, `ndk-context`) are gated by `[target.'cfg(target_os = "android")'.dependencies]` inside cpal itself and do not appear in the oxisound-cpal dependency graph on non-Android targets.
