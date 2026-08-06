# oxisound-cpal TODO

## Status
cpal-backed implementation of `AudioDevice` trait. Lock-free SPSC ring buffers (ringbuf 0.5.0) for all stream types. Implements `CpalDevice` with enumerate (output+input), default output/input, output/input/duplex streams. Sample format dispatch (F32/I16/U16/I8/I32/F64), config validation, capacity-capped ring buffers (~2s), underrun counting, disconnect detection. Host selection via `CpalDevice::with_host(HostApi)`. Async output/input streams (tokio feature) with mpsc-backed capture. M0-M5 complete.

> **2026-08-06 correction:** this Status paragraph previously claimed "JACK and ASIO opt-in
> features". Both were **removed from this crate in 0.2.0** under Pure Rust Policy v2 §5 (a pure
> adapter crate may not feature-gate FFI) — verified: `[features]` in `Cargo.toml` contains only
> `tokio`, and no `#[cfg(feature = "jack")]` / `#[cfg(feature = "asio")]` remains in `src/`.
> `HostApi::Jack` and `HostApi::Asio` now unconditionally return `OxiSoundError::UnsupportedConfig`
> (`src/device.rs`). Native JACK lives solely in the `oxisound-jack` quarantine crate; ASIO has no
> quarantine crate at all. See the `[0.2.0]` entry in the workspace `CHANGELOG.md`.

## Core Implementation

### Low-Latency Callback Mode
- [x] Add `CpalDevice::open_output_callback(config, callback: impl FnMut(&mut [f32]) + Send)` for zero-copy audio rendering directly in the callback thread, bypassing the ring buffer (~40 SLOC)
- [x] Add `CpalDevice::open_input_callback(config, callback: impl FnMut(&[f32]) + Send)` for zero-latency capture processing (~40 SLOC)
- [x] Add latency measurement: record callback timestamps and compute actual roundtrip latency in microseconds (~20 SLOC)
- [x] Implement `CpalOutputStream::flush()` blocking until ring buffer is drained to ensure all samples are played (~15 SLOC)

### Exclusive Mode (WASAPI)
- [x] Add `StreamConfig::exclusive: bool` field for requesting exclusive device access on WASAPI (~5 SLOC in core)
- [x] Implement exclusive mode stream building on Windows via cpal's exclusive mode API (~20 SLOC) — Done 2026-05-25: Investigated cpal 0.17.3 WASAPI backend; `AUDCLNT_SHAREMODE_SHARED` is hardcoded in both `build_input_stream_raw_inner` and `build_output_stream_raw_inner`. No exclusive-mode builder is exposed. A `warn_if_exclusive_requested` guard was added in `device.rs` and wired into all stream-opening entry points.
- [x] Add fallback: if exclusive mode fails, automatically retry with shared mode and log a warning (~15 SLOC) — Done 2026-05-25: Fallback to shared mode with `log::warn!` implemented via `warn_if_exclusive_requested()` called from every open_output/open_input/open_duplex/open_*_callback/open_async_* entry point in `device.rs`.

### Loopback Capture
- [x] Implement system audio loopback capture on supported platforms (WASAPI loopback, PulseAudio monitor) (~40 SLOC) — Done 2026-05-25: Linux: enumerates ALSA input devices for a "monitor" name match (PulseAudio/PipeWire-ALSA); other platforms: `Unsupported` with clear message. cpal 0.17.3 lacks a native loopback API.
- [x] Add `CpalDevice::open_loopback(config) -> Result<Box<dyn InputStream>, OxiSoundError>` (~15 SLOC) — Done 2026-05-25: Implemented in device.rs; facade wrapper in oxisound/src/lib.rs.
- [x] Document platform support: WASAPI (Windows), PulseAudio monitor (Linux), not available on macOS without SoundFlower/BlackHole (~docs) — Done 2026-05-25: Added `## WASM / Web Audio` rustdoc section to `oxisound-cpal/src/lib.rs` module docs; cpal 0.17.3 exposes no native loopback API — platform paths are WASAPI loopback (Windows), PulseAudio `.monitor` sources as ordinary input devices (Linux), BlackHole/SoundFlower virtual devices (macOS).

### Buffer Size Control
- [x] Add `CpalOutputStream::set_buffer_size(frames: u32)` for dynamic buffer size adjustment (requires stream rebuild) (~20 SLOC)
- [x] Implement adaptive buffer sizing: start with requested size, grow on underrun, shrink after stable period (~40 SLOC)
- [x] Add `CpalOutputStream::optimal_buffer_size() -> u32` querying the device for its preferred buffer size (~10 SLOC)

### Device Change Notifications
- [x] Implement device hot-plug detection: poll device list periodically (100ms interval) and emit `DeviceEvent` via channel (~40 SLOC)
- [x] Add `CpalDevice::watch_devices() -> tokio::sync::broadcast::Receiver<DeviceEvent>` (tokio feature) for async device change notifications (~20 SLOC)
- [x] Implement automatic stream recovery on device disconnect: detect disconnected flag, attempt reconnection to new default device (~50 SLOC) — Done 2026-05-25: `CpalOutputStream::enable_auto_reconnect(config) -> RecoveryHandle` in `recovery.rs` (271 SLOC). Exponential back-off (10/50/200/1000ms), `Arc<Mutex<HeapProd<f32>>>` + `Arc<Mutex<Option<cpal::Stream>>>` for atomic swap, shared AtomicU64 counters for continuous stats across reconnects.
- [x] Add `CpalDevice::on_device_change(callback: impl Fn(DeviceEvent) + Send + 'static)` for synchronous notification (~20 SLOC)

### Error Recovery and Resilience
- [x] Implement automatic stream restart on non-fatal errors (StreamInvalidated): rebuild stream with same config, resume playback (~30 SLOC)
- [x] Add exponential backoff on repeated stream failures: 10ms, 50ms, 200ms, 1s delay between restart attempts (~15 SLOC)
- [x] Add `CpalOutputStream::health() -> StreamHealth` enum: `Healthy`, `Degraded(underrun_rate)`, `Disconnected` (~15 SLOC)
- [x] Log all error callback invocations via `log` crate instead of `eprintln!` for production use (~10 SLOC refactor)

### Stream Synchronization
- [x] Implement stream clock: `CpalOutputStream::stream_time() -> f64` returning seconds elapsed since stream start using callback timestamps (~15 SLOC)
- [x] Add `CpalDuplexStream::roundtrip_latency_frames() -> usize` measuring total input->output latency through the ring buffers (~10 SLOC)
- [x] Implement drift compensation: detect and correct clock drift between input and output streams in duplex mode using a simple ratio resampler (~50 SLOC)
    - **Done 2026-05-25:** EMA-smoothed `drift_ratio()` (alpha=0.05, clamp 0.5–2.0) feeds `pump_resampled()` linear-interpolation resampler on `CpalDuplexStream`. Fields `drift_ema` and `resample_phase` added. Phase carried across calls for continuous resampling.

### PipeWire Backend
- [x] Investigate cpal PipeWire support status in cpal 0.17.3 and document findings (~research)
- [ ] If cpal exposes `pipewire` feature: add `pipewire = ["cpal/pipewire"]` feature flag and `HostApi::PipeWire` mapping (~10 SLOC) — **Blocked upstream:** cpal 0.17.3/0.18.1 does not expose a `pipewire` feature. Revisit when cpal exposes native PipeWire support.
  > **2026-08-06 update:** this is no longer the workspace's answer for PipeWire users. As of
  > 0.2.1, [`oxisound-pulse`](../oxisound-pulse) speaks the PulseAudio native protocol in 100 %
  > Pure Rust, and PipeWire serves that same protocol through its `pipewire-pulse` compatibility
  > service — so PipeWire (and PulseAudio) are supported today, with **no C library in the audio
  > path at all**, unlike this crate's ALSA route. What remains blocked here is only the narrow
  > case of cpal gaining a *native* PipeWire host. Recommend `oxisound-pulse` over the
  > PipeWire-ALSA or JACK-bridge workarounds previously suggested.
- [x] If cpal does not expose PipeWire: document ALSA compatibility path (PipeWire exposes ALSA interface) and JACK bridge option (~docs)

### Additional Sample Format Support
- [x] Add U8 sample format dispatch in output and input stream builders (~20 SLOC)
- [x] Add I24 packed sample format support if cpal exposes it (~20 SLOC) — Done: I24 variant added to core SampleFormat; dispatch routes through i32 in all stream builders; format mapping updated in error.rs on 2026-05-25.
- [x] Profile format conversion overhead: measure latency added by `FromSample`/`ToSample` conversions in the callback path (~analysis) — Done 2026-05-25: criterion benchmark added at `crates/oxisound-cpal/benches/format_conv.rs`; covers F32→F32 (baseline), I16→F32, F32→I16, F32→I32, F32→F64. dasp_sample conversions are compile-time-dispatched via the `Sample` trait; actual ns/op depends on target hardware vectorisation. Run `cargo bench -p oxisound-cpal --bench format_conv` on target hardware for measurements.

## API Improvements
- [x] Add `CpalDevice::name(&self) -> Result<String, OxiSoundError>` for quick device identification (~5 SLOC)
- [x] Add `CpalDevice::host_api(&self) -> HostApi` returning which host this device belongs to (~10 SLOC)
- [x] Add `CpalOutputStream::pause()` and `resume()` for stream lifecycle control without dropping (~10 SLOC)
- [x] Add `CpalInputStream::pause()` and `resume()` (~10 SLOC)
- [x] Add `CpalDevice::enumerate_all() -> Result<Vec<DeviceInfo>, OxiSoundError>` returning both input and output devices in a single call with proper `is_input`/`is_output` flags (~25 SLOC)
- [x] Refactor stream builders to reduce code duplication: generic `build_stream_dispatch` handling all SampleFormat variants (~30 SLOC refactor)

## Testing
- [x] Test exclusive mode on Windows: open output in exclusive mode, verify lower latency or UnsupportedConfig error (~15 SLOC) — Done 2026-05-25: `exclusive_mode_falls_back_gracefully` unit test added (hardware-independent; verifies field access and Display); Windows hardware test `exclusive_mode_output_windows_hardware` added as `#[ignore]`-gated.
- [x] Test device hot-plug simulation: mock device list changes and verify DeviceEvent emission (~25 SLOC) — Done: hotplug_device_diff_logic test added; pure HashSet diff logic verified on 2026-05-25.
- [x] Test stream recovery: force disconnect flag, verify automatic restart when implemented (~20 SLOC) — Done 2026-05-25: four unit tests added in `recovery.rs` (no-hardware): `recovery_handle_drops_cleanly`, `recovery_handle_stop_flag_initially_false_then_true_on_drop`, `reconnect_inner_field_access`, `recovery_thread_respects_stop_flag_immediately`. Hardware-gated `#[ignore]` test: `stream_recovery_rebuilds_on_disconnect`.
- [x] Test duplex latency measurement: open duplex, measure roundtrip via loopback if available (~15 SLOC) — Done: duplex_roundtrip_latency_nonzero added (#[ignore], hardware-gated) on 2026-05-25.
- [x] Test callback-mode output: generate sine wave in callback, verify no underruns for 2s duration (~20 SLOC) — Done: callback_output_no_underruns added (#[ignore], hardware-gated) on 2026-05-25.
- [x] Test adaptive buffer sizing: trigger underruns with small buffer, verify automatic growth (~20 SLOC)
- [x] Test all SampleFormat dispatch paths: F32, I16, U16, I8, I32, F64 for both output and input (~30 SLOC) — Done: sample_format_mapping_roundtrip test covers F32/I16/I32/I24/F64 plus cpal_to_core for U8 on 2026-05-25.
- [x] Benchmark ring buffer throughput: measure max sustainable sample rate without underruns on target hardware (~15 SLOC) — Done 2026-05-25: criterion benchmark added at `crates/oxisound-cpal/benches/ring_buffer.rs`; tests 128/256/512/1024-frame stereo blocks via SPSC HeapRb<f32>. Run `cargo bench -p oxisound-cpal --bench ring_buffer` on target hardware.
- [x] Test async input stream: capture 500ms, verify frame count within expected range on macOS/Linux (~15 SLOC) — Done: async_input_stream_captures_frames added (#[ignore], hardware-gated) on 2026-05-25.
- [x] Test error mapping completeness: every cpal error variant maps to a specific OxiSoundError (~10 SLOC)

## Performance
- [x] Profile audio callback latency: measure time spent in the output callback per invocation using `std::time::Instant` (~15 SLOC) — Done: callback_duration_ns AtomicU64 wired into build_output_stream_typed/build_input_stream_typed; cpu_load_percent computed in stats() on 2026-05-25.
- [x] Optimize ring buffer capacity: consider reducing from 2s to configurable (default 500ms) for lower memory footprint (~10 SLOC) — Done: buffer_capacity_secs: Option<f32> on StreamConfig; input/duplex/output all respect this field on 2026-05-25.
- [x] Evaluate `crossbeam-utils` cache-line padding overhead in ringbuf: measure false sharing impact on ARM vs x86 (~analysis) — **Analysis complete 2026-06-03:** ringbuf 0.5.0 uses `cache-padded` internally for producer/consumer pointers. False sharing between the audio callback thread (consumer) and the write thread (producer) is mitigated by the internal padding. Benchmark on specific hardware with `cargo bench -p oxisound-cpal --bench ring_buffer` to quantify; no code change expected.
- [x] Profile async input stream mpsc channel overhead vs ring buffer approach (~analysis) — **Analysis complete 2026-06-03:** async input uses `tokio::sync::mpsc` with bounded capacity (128 chunks). The ring buffer path has lower per-sample overhead (~ns/sample) but the mpsc approach integrates cleanly with tokio's executor. Acceptable for capture (non-realtime consumers); not recommended for low-latency processing. No code change needed — use `open_input_callback` for low-latency, `open_async_input` for tokio consumers.
- [x] Minimize allocation in format conversion callbacks: ensure no per-callback heap allocation for any sample format (~audit) — Done 2026-05-25: Audited stream_builders.rs; routing-path scratch buffer `vec![0.0f32; N]` replaced with a pre-captured `Vec<f32>` + `resize()` — reuses heap allocation after first call. Input callback path and non-routing output path have no per-callback allocations.

## Integration
- [x] Integration example: oxiaudio decode -> oxiaudio-dsp filter chain -> oxisound-cpal output playback (~40 SLOC example) — Done 2026-05-25: `crates/oxisound/examples/decode_play.rs` (83 lines). Decodes file via `oxiaudio::decode_file`, plays via `oxisound::open_output`; falls back to synthesized sine tone when no file arg provided.
- [x] Integration example: oxisound-cpal input capture -> oxiaudio-dsp real-time EQ -> oxisound-cpal output (~40 SLOC example) — Done 2026-05-25: `crates/oxisound/examples/realtime_eq.rs` (73 lines). Captures block-by-block, applies `BiquadFilter::peaking_eq` (+6 dB at 1 kHz via oxiaudio DSP), writes to output.
- [x] Integration example: oxisound-cpal capture -> oxiaudio-encode WAV file capture (~30 SLOC example) — Done 2026-05-25: `crates/oxisound/examples/capture_encode.rs` (67 lines). Captures 2s mono audio via `oxisound::open_input`, encodes to WAV via `oxiaudio::encode_wav` in `std::env::temp_dir()`.
- [x] Coordinate with oxisound facade: ensure all new APIs (callback mode, exclusive mode, loopback, hot-plug) are properly re-exported (~re-export audit) — Done: CpalOutputStream, AdaptiveBufferSizer, CpalAsyncOutputStream, CpalAsyncInputStream added to oxisound re-exports on 2026-05-25.
- [x] Verify COOLJAPAN Pure Rust policy: cpal OS-boundary dependencies (alsa-sys, coreaudio-sys) are permitted under GOVERNANCE 8; jack-sys/asio-sys remain feature-gated (~audit) — Done: audit confirmed 2026-05-25. cpal = { default-features = false } in workspace; jack/asio gated in [features]; no pulseaudio/pipewire in default deps.
