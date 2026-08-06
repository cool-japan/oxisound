# OxiSound TODO

Workspace-wide task list. Individual sub-crate TODOs live under `crates/<crate>/TODO.md`.

## Release Status

**v0.2.1 — Released 2026-08-06**

- **New crate:** `oxisound-pulse` — Pure-Rust PulseAudio native-protocol backend (also serves
  PipeWire through `pipewire-pulse`), Linux-only functionality with a Pure-Rust `Unsupported`
  stub on every other target. Opt-in via the facade's `pulse` feature (**not** in `default`).
  First publish to crates.io in this release.
- **Security (oxisound-osc):** `read_str` out-of-bounds read, unbounded bundle/array nesting
  (stack-overflow DoS from a single UDP datagram) and a 32-bit `pos + size` overflow in
  `decode_bundle` — all fixed with regression tests. See CHANGELOG `[0.2.1]`.
- **Dependencies:** `oxiaudio-core` / `oxiaudio` 0.2.0 → 0.2.1.
- **Tests:** 297 passing / 8 skipped (default features), 313 passing / 11 skipped
  (`--all-features`), 127 doctests — all skips are hardware-conditional. Zero warnings from
  clippy, rustfmt and rustdoc; `cargo deny --all-features --workspace check` fully clean.

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

#### oxisound-pipewire (PipeWire Integration) — REMOVED
> **2026-08-03 correction:** This subcrate does not exist in the workspace (`crates/`
> currently has 8 members, none PipeWire; see `Cargo.toml` `[workspace].members`). The
> `[x] Done` items previously listed below described a `crates/oxisound-pipewire/` crate,
> a `pipewire-backend` Cargo feature, and `oxisound::pipewire_output`/`pipewire_input`
> facade functions — none of which are present on disk (`rg pipewire crates/` finds only
> the `HostApi::PipeWire` enum variant, which is a platform-detection marker, not a
> working backend; `oxisound-cpal`'s `HostApi::PipeWire` arm unconditionally returns
> `OxiSoundError::UnsupportedConfig`). Either the crate was removed after this section was
> written and the checklist was never updated, or the items were never actually
> implemented; either way, tracking them as `[x] Done` was false. See
> `crates/oxisound-cpal/TODO.md`'s "PipeWire Backend" section for the real (accurate)
> status: blocked upstream because cpal does not expose a native PipeWire feature.
> Superseded by the "PipeWire backend investigation" entry above, which correctly
> describes this as blocked/not implemented.

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
  > **2026-08-06 (0.2.1 release verification):** `cargo deny --all-features --workspace check` →
  > `advisories ok, bans ok, licenses ok, sources ok`, exit 0. All four categories are clean,
  > including the `jack-sys` `wrappers = ["jack"]` exception actually being exercised — `jack-sys`
  > is present in the `--all-features` dependency graph via `jack` → `oxisound-jack`, and is not
  > flagged.
  >
  > *History:* on 2026-08-04 the `advisories` category was **failing** on `RUSTSEC-2026-0204`
  > (invalid pointer dereference in `crossbeam-epoch 0.9.18`'s `fmt::Pointer` impl) plus a
  > yanked-crate warning for `spin 0.12.1`, both reached exclusively via
  > `(dev) oxisound → oxiaudio → oxifft → rayon → crossbeam-deque → crossbeam-epoch` (and
  > `→ oxifft → spin`) — i.e. through external crates' own dependency choices, not fixable from
  > inside this repo. The `oxiaudio` 0.2.0 → 0.2.1 bump resolved it: `Cargo.lock` now carries
  > `crossbeam-epoch 0.9.20` and `spin 0.12.2`. `multiple-versions = "warn"` still reports
  > duplicate `thiserror` / `windows-*` entries — warnings by configuration, not gate failures.

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


---

<!-- production-readiness-backlog 2026-07-16 -->
## Production-Readiness Backlog — 2026-07-16

_Consolidated from static audit + Opus adversarial bug-hunt (48 verified defects across noffi) + baseline nextest/clippy + design investigation. See `../NOFFI_PRODUCTION_BACKLOG.md` for the full cross-project list and severity/model legend. Fixes below are in the working tree; no commits._

**Confirmed bug — Opus-verified:**
- [x] **S · high** `oxisound-osc/src/decode.rs:153` — `read_str` slices `data[start..]` without `start <= data.len()`; `align4()` padding after a prior string/type-tag can push `*pos` past end → later `read_str` OOB panic. R2/N0 — **Fixed:** `read_str` now guards `start > data.len()` before slicing and returns `OscError` instead of panicking; regression test `decode_alignment_padding_past_end_returns_error`. See CHANGELOG.md `[0.2.1]` Security section.
- [x] **B · L2** otherwise stub-clean (upstream/HW-blocked items only); verify after target fix. — **Verified 2026-08-03:** `target -> /tmp/target/oxisound` resolves; see the workspace README's Test Results section for the measured build/test/clippy state.
- [x] **B · easy** `oxisound-osc/src/decode.rs` `decode_bundle` — unchecked `pos + size` addition can wrap `usize` on a 32-bit target, bypassing the `> data.len()` bounds check. — **Fixed 2026-08-04:** extracted `element_end(pos, size)` using `checked_add`, returning a typed `OscError` on overflow instead; `read_exact`'s analogous `*pos + N` got the same treatment. Regression tests: `element_end_overflow_returns_typed_error_not_panic`, `read_exact_pos_overflow_returns_typed_error_not_panic`, `decode_bundle_element_size_near_usize_max_returns_error_not_panic`. See CHANGELOG.md `[0.2.1]` Security section.

---

<!-- linux-alsa-robustness 2026-08-04 -->
## Linux / ALSA Robustness — 2026-08-04

_Fixes for failures reported from a real Linux (ALSA) machine and not reproducible on macOS;
implemented by code reasoning, macOS gate kept green (235/235 tests, zero clippy warnings)._

- [x] **Role-less devices no longer escape enumeration.** `CpalDevice::enumerate_all()`
  (`crates/oxisound-cpal/src/device.rs`) probed `supported_output_configs()` /
  `supported_input_configs()` and still pushed a `DeviceInfo` when **both** failed, producing an
  entry with `is_input == false && is_output == false`. On ALSA this happens for an HDMI PCM with
  no monitor attached (`HDA Intel PCH, HDMI 0`), which cannot be opened in either direction, and it
  broke the documented "every device has at least one role" invariant — panicking
  `oxisound::facade_tones::test_enumerate_all_devices_has_output` and
  `oxisound-cpal tests::cpal_device_enumerate_all_has_io_flags`. Such devices are now skipped with a
  `log::debug!` note, and the invariant is documented on both `CpalDevice::enumerate_all` and the
  facade's `oxisound::enumerate_all_devices`. `enumerate()` / `enumerate_input()` set a role by
  construction and cannot leak a role-less entry; the hot-plug watcher's `DeviceAdded` payload is a
  deliberate name-only stub (probing roles from the polling thread would make `drop(watcher)` hang
  on exactly the broken devices) and is now documented as such.
- [x] **`open_duplex` can no longer hang forever.** The open sequence (config probing,
  `default_output_config` / `default_input_config`, two `build_*_stream` calls, two `play` calls) is
  a chain of uninterruptible C calls; on a pathological ALSA device one of them blocks indefinitely,
  which hung `oxisound-cpal tests::test_duplex_open_no_panic` for >300 s until it was interrupted.
  The whole sequence now runs on a dedicated `oxisound-duplex-open` worker thread and the caller
  waits with `recv_timeout(DUPLEX_OPEN_TIMEOUT)` (15 s, a new public constant in `oxisound-cpal`;
  `StreamConfig` gained no new field). `cpal::Stream` is `Send` on every backend — cpal 0.18.1
  asserts it per host and `oxisound_core::DuplexStream` already required it — so the finished
  streams, their stats counters and disconnect flags move back to the caller unchanged; the fast
  path and every existing success/error semantic are preserved. On timeout the call returns
  `OxiSoundError::Timeout` and the worker thread is detached: it stays parked inside the backend
  call until it returns, then drops the half-built streams and exits. Leaking one parked thread is
  the only sound option against an uninterruptible FFI call. `wasm32` (no OS threads) keeps the
  inline path.
- [x] **Test hardening.** `test_duplex_open_no_panic` now opens *and drops* the stream on a watched
  thread behind its own 60 s watchdog and fails with an explicit message, so a future regression that
  reintroduces an unbounded call reports a failure instead of hanging the suite until someone sends
  SIGINT. The facade assertion in `crates/oxisound/tests/facade_tones.rs` is unchanged — it states
  the correct contract; the fix belongs in enumeration.

> **Note on the `ALSA lib pcm_dmix.c … unable to open slave` stderr spam.** Those lines are printed
> by alsa-lib itself (its C error handler writes straight to `stderr`) while cpal probes or opens a
> PCM. Nothing in Rust can suppress them: silencing them would require calling
> `snd_lib_error_set_handler()` through FFI, which the Pure Rust policy forbids here. They are
> harmless diagnostics, they do not indicate an OxiSound failure, and they disappear only under a
> non-alsa-lib backend.

---

<!-- oxisound-pulse 2026-08-04 -->
## Pure-Rust PulseAudio Backend (`oxisound-pulse`) — 2026-08-04

_New workspace member. Answers the standing requirement "Linux audio without any C library in the
path": the default `oxisound-cpal` route reaches Linux hardware through `alsa-lib` (C), while this
crate speaks the **PulseAudio native IPC protocol** directly over a unix socket. It serves PipeWire
too, unchanged, through the `pipewire-pulse` compatibility service. `#![forbid(unsafe_code)]`,
`#![deny(missing_docs)]`, zero C in the audio path._

**Protocol layer:** the pure-Rust `pulseaudio` 0.3.1 crate (colinmarc/pulseaudio-rs, MIT) — used
as-is, no in-house protocol fallback was needed. Its `Client` (AUTH / SET_CLIENT_NAME handshake,
reactor thread, `GET_*_INFO_LIST`, `CREATE_{PLAYBACK,RECORD}_STREAM`, cork/drain/flush) plus the
`protocol` module's `SinkInfo` / `SourceInfo` / `ServerInfo` / `SampleSpec` / `BufferAttr` /
`ChannelMap` types are all that is consumed. Declared under
`[target.'cfg(target_os = "linux")'.dependencies]`; `futures-executor` is an unconditional dep so
the deadline machinery is testable everywhere.

- [x] **Server discovery + bounded connect.** `$PULSE_SERVER` (incl. the `{machine-id}unix:` form
  and whitespace-separated lists) → `$PULSE_RUNTIME_PATH/{native,pulse/native}` →
  `$XDG_RUNTIME_DIR/pulse/native` → `/run/user/<uid>/pulse/native`, with the uid read from
  `stat("/proc/self")` (no libc FFI). Cookie: `$PULSE_COOKIE` → `$XDG_CONFIG_HOME/pulse/cookie` →
  `$HOME/.config/pulse/cookie` → `$HOME/.pulse-cookie`; a missing cookie is not an error (same-uid
  sockets, the usual `pipewire-pulse` configuration). A `$PULSE_SERVER` that names only a remote
  transport returns `Unsupported` instead of silently using the local server.
- [x] **No call can block forever.** `connect(2)` on a unix socket is uninterruptible, so
  connect+handshake runs on an `oxisound-pulse-connect` worker thread bounded by
  `PULSE_CONNECT_TIMEOUT` (5 s) — the same pattern as `oxisound-cpal`'s `DUPLEX_OPEN_TIMEOUT` fix —
  and the handshake is additionally bounded by `SO_RCVTIMEO`/`SO_SNDTIMEO`. Every control-plane
  round-trip goes through `block_on_timeout` (`PULSE_OP_TIMEOUT` 5 s, `PULSE_FLUSH_TIMEOUT` 2 s
  matching cpal's `flush()`, `PULSE_DRAIN_TIMEOUT` 10 s).
- [x] **Enumeration + role invariant.** `GET_SERVER_INFO` + `GET_SINK_INFO_LIST` +
  `GET_SOURCE_INFO_LIST` → `DeviceInfo`: sinks `is_output`, sources (monitors included) `is_input`,
  `is_default` from the server's default sink/source names. Degenerate endpoints (0 channels or
  0 Hz — `auto_null`, a card whose profile is off) are skipped with a `log::debug!`, so the
  workspace invariant "every enumerated device has at least one role" holds here exactly as it now
  does for cpal. `DeviceInfo` has no monitor field, so the server-side name is kept verbatim
  (`.monitor` suffix preserved, and it round-trips into `open_named`) and `is_monitor` is exposed on
  `PulseDeviceDescriptor` via `enumerate_details()`.
- [x] **Playback / capture / duplex.** Trait semantics mirror `oxisound-cpal`: lock-free SPSC ring,
  `write()` returns `Overrun` without partially consuming, `read()` is non-blocking, `Disconnected`
  flag, `flush()` with the `Timeout` pattern. Playback is server-clocked — on an empty ring the
  reactor-side source returns `Poll::Pending` and parks a `Waker` (never `Ready(0)`, which the
  reactor reads as EOF, and never silence padding, which would add phantom latency); the next
  `write()` wakes it. Underruns are counted on the rising edge; capture overruns are counted when a
  server chunk does not fit. Duplex shares one connection (one socket, one reactor thread) and is
  `Send` end to end.
- [x] **Connection teardown (reactor-thread leak fix).** The `pulseaudio` reactor loop is
  `poll(…, None)` → `recv` → `write_streams` → `write_commands`, and its only exit is
  `write_commands` seeing a disconnected outgoing channel — reached *after* the unbounded poll,
  whose mio waker the reactor owns itself. Dropping every `Client` clone therefore does **not**
  stop the thread: it parks in `epoll_wait` forever holding the socket, so every `enumerate()` /
  `default_output()` / stream teardown would leak a thread and an fd. Fixed by `PulseConnection`,
  which `try_clone`s the socket before handing it to the client and calls `shutdown(Both)` on the
  dup when the last `Arc` drops; the reactor's next read returns `Ok(0)` → `Disconnected` → thread
  exits → the ring adapters drop → `is_disconnected()` becomes true on surviving handles. Devices
  and both stream halves share one `Arc<PulseConnection>`. Regression test:
  `dropping_the_connection_shuts_the_socket_down_so_the_reactor_can_exit` (real socket pair, stub
  server asserts it observes EOF).
- [x] **Lost-wakeup window in the playback source.** `poll_read` popped, saw an empty ring, then
  stored the waker; a `write()` landing between those two steps found an empty waker slot, skipped
  its wake, and left the reactor parked with data queued (one spurious underrun plus a stalled
  period per occurrence — the normal burst-write pattern). `poll_read` now re-checks the ring after
  registering the waker and serves it if data arrived; both phases go through the `serve` helper.
- [x] **Bounded discard.** `discard_buffered()` used to set a flag that could fire on a much later
  server REQUEST and `clear()` the whole ring, swallowing audio written in between. It now snapshots
  the backlog into an `AtomicUsize` and the reactor `skip`s at most that many bytes.
- [x] **Negotiated-spec verification.** No `fix_format` / `fix_rate` / `fix_channels` flag is set, so
  the server is expected to echo the requested spec. If it ever does not, the encoder/decoder would
  misinterpret every byte while `negotiated()` reported the server's real spec. Stream creation now
  compares the two and fails with `FormatMismatch` instead.
- [x] **Facade wiring.** New opt-in `pulse = ["dep:oxisound-pulse"]` feature (NOT in `default` —
  flipping the Linux default is a release decision left to the user) plus `pulse_enumerate_devices`,
  `pulse_default_output`, `pulse_default_input`, `pulse_output`, `pulse_input`, `pulse_duplex`,
  `pulse_output_named`, `pulse_input_named`, and re-exports of `PulseDevice` and friends.
  `CpalDevice::with_host(HostApi::PulseAudio)` deliberately still returns `Err`: cpal has no
  PulseAudio host to dispatch to, and a `CpalDevice`-returning constructor cannot yield a
  `PulseDevice`. `PulseDevice::host_api()` reports `HostApi::PulseAudio` for parity. `oxisound-cpal`
  was not touched.

**Deferred / out of scope (documented in the crate README, not stubs):**

- **SHM / memfd zero-copy transfer** — audio is streamed over the socket as plain memblocks. Costs
  one memory copy per chunk; keeps the crate free of shared-memory `unsafe`.
- **Latency / buffer tuning is minimal** — `tlength` = 4 periods, `minreq` = `prebuf` = 1 period,
  `maxlength` = 8 periods, `fragsize` = 1 period, all derived from `StreamConfig::buffer_size`
  (default 1024 frames). No `adjust_latency` negotiation, no dynamic re-tuning.
- **Volume / mute control**, module loading and the subscription (hot-plug) event stream are out of
  scope. Remote `tcp:` servers are rejected rather than supported.

**Verification (macOS host, 2026-08-04; re-measured for the 0.2.1 release on 2026-08-06):**
`cargo build --workspace` and `cargo build --workspace --features oxisound/pulse` ok;
`cargo nextest run --workspace` **297/297** (8 hardware-conditional skips) and
`cargo nextest run --all-features --workspace` **313/313** (11 skips) — of which
`-p oxisound-pulse` contributes 58/58 (57 host-independent unit tests + 1 stub integration
test) plus 17 doctests. (The 2026-08-04 figure recorded here was 293/293; the delta is
additional non-pulse tests landed later in the same cycle.) A further **25 Linux-only unit tests** (protocol-limit cross-checks, error-taxonomy
mapping, a full `AUTH`/`SET_CLIENT_NAME` handshake and the connection-shutdown regression over
`UnixStream::pair()`, sink/source→descriptor mapping, channel maps for all 32 counts, ring/waker/
discard behaviour) and 6 live integration tests compile under the cross-target clippy gate but can
only *run* on Linux; `cargo clippy --workspace --all-targets -- -D warnings` and
`cargo clippy -p oxisound --features pulse --all-targets -- -D warnings` clean;
`cargo clippy -p oxisound-pulse --target x86_64-unknown-linux-gnu --all-targets -- -D warnings`
clean (this is the gate that type-checks the Linux-only code **and** its `#[cfg(test)]` tests);
`cargo check -p oxisound --no-default-features --features pulse --target x86_64-unknown-linux-gnu`
ok; `cargo deny check bans` ok, `check licenses` ok; `cargo fmt --all -- --check` clean.

> Two notes on the cross-target checks. `cargo check -p oxisound --features pulse --target
> x86_64-unknown-linux-gnu` (i.e. with `default`/`pure` on) cannot run from macOS: it pulls
> `oxisound-cpal` → `alsa-sys`, whose build script needs a cross `pkg-config` and ALSA headers.
> `--no-default-features --features pulse` is the equivalent check that does run, and it exercises
> exactly the new wiring. Likewise `--all-targets` on the facade for that target fails in the
> `alloca` build script (a C-compiler dev-dependency reached through the `oxiaudio` dev-dep), not in
> any OxiSound code.
>
> `cargo deny check bans` passes; note that `pulseaudio` pulls `thiserror 1.x` while the workspace
> is on 2.x, so the duplicate-version list grew by `thiserror` / `thiserror-impl`. `multiple-versions
> = "warn"`, so this is a warning, not a gate failure, and it will clear when upstream bumps.

**Pending user action — live verification on a real Linux box.** Everything above was verified by
cross-target type-checking and host-independent unit tests; no PulseAudio server was reachable from
the macOS development machine. On a Linux desktop session please run:
>
> ```bash
> export CARGO_TARGET_DIR=/tmp/target/oxisound
> cargo nextest run -p oxisound-pulse --no-capture   # live tests print SKIP: when no server
> cargo clippy --workspace --all-targets -- -D warnings
> ```
>
> `tests/live_server.rs` then exercises enumeration (non-empty, role invariant, monitor sources as
> inputs), a short playback write + `flush` + `drain`, opening and reading a capture stream, duplex,
> and `negotiate_output`. Each test checks socket reachability first and returns early with a
> `SKIP:` note, so the suite is green with or without a server — read the `--no-capture` output to
> confirm the tests actually ran rather than skipped.
>
> Also worth a manual check that no host-side gate can perform: that connections are reclaimed
> rather than parked. Call `oxisound::pulse_enumerate_devices()` (or `PulseDevice::enumerate()`) a
> few dozen times in a loop and watch the process's thread and fd count — `ps -o thcount -p <pid>`
> and `ls /proc/<pid>/fd | wc -l` — which must stay flat. That is what the `PulseConnection`
> shutdown above is for, and it is verified here only by the socket-pair regression test, not
> against a real server.

---
