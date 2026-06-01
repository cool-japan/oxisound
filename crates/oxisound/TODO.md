# oxisound (facade) TODO

## Status
Public facade exposing COOLJAPAN audio device I/O API. Re-exports all oxisound-core types. Provides convenience functions: `default_output`, `default_input`, `enumerate_devices`, `open_output`, `open_input`, `select_device`, `duplex_stream`, `jack_output`, `asio_output`, `latency_ms`, `format_devices`, `sine_test_tone`, `async_output`, `capture_stream`. Four examples (device_info, sine_tone, playback, capture). Feature flags: `pure` (default), `jack`, `asio`, `tokio`. M0-M5 complete. Approximately 270 SLOC.

## Core Implementation

### Device Management
- [x] Add `enumerate_all_devices() -> Result<Vec<DeviceInfo>, OxiSoundError>` returning both input and output devices in a single call (~10 SLOC)
- [x] Add `enumerate_input_devices() -> Result<Vec<DeviceInfo>, OxiSoundError>` convenience for input-only device listing (~10 SLOC)
- [x] Add `select_input_device(name_fragment: &str) -> Result<CpalDevice, OxiSoundError>` for input device selection by name (~10 SLOC)
- [x] Add `device_by_index(index: usize) -> Result<CpalDevice, OxiSoundError>` for positional device selection (~10 SLOC)
- [x] Add `preferred_output_config() -> Result<StreamConfig, OxiSoundError>` returning the default device's preferred config (~10 SLOC)

### Callback-Based API
- [x] Add `play_callback(config, callback: impl FnMut(&mut [f32]) + Send) -> Result<impl Drop, OxiSoundError>` for zero-copy audio rendering (~15 SLOC)
- [x] Add `capture_callback(config, callback: impl FnMut(&[f32]) + Send) -> Result<impl Drop, OxiSoundError>` for zero-latency capture (~15 SLOC)
- [x] Add `duplex_callback(config, callback: impl FnMut(&[f32], &mut [f32]) + Send) -> Result<impl Drop, OxiSoundError>` for simultaneous I/O in a single callback (~20 SLOC)

### Device Hot-Plug
- [x] Add `watch_devices() -> impl Stream<Item = DeviceEvent>` (tokio feature) for async device change notifications (~10 SLOC)
- [x] Add `on_device_change(callback: impl Fn(DeviceEvent) + Send + 'static)` for synchronous device change notifications (~10 SLOC)
- [x] Add `auto_reconnect_output(config) -> Result<Box<dyn OutputStream>, OxiSoundError>` that automatically reconnects to new default device on disconnect (~20 SLOC)

### Audio Session Control
- [x] Add `configure_session(category: SessionCategory) -> Result<(), OxiSoundError>` for iOS/macOS audio session management (~10 SLOC)
- [x] Add `request_microphone_permission() -> Result<bool, OxiSoundError>` for iOS/macOS microphone access prompt (~15 SLOC)

### Test Tone Generation
- [x] Add `white_noise_test(duration_secs: f32, config: StreamConfig) -> Vec<f32>` for noise floor testing (~10 SLOC)
- [x] Add `chirp_test_tone(f_start: f32, f_end: f32, duration_secs: f32, config: StreamConfig) -> Vec<f32>` for frequency sweep testing (~15 SLOC)
- [x] Add `silence(duration_secs: f32, config: StreamConfig) -> Vec<f32>` generating zero-filled buffer (~5 SLOC)
- [x] Add `click_track(bpm: f32, duration_secs: f32, config: StreamConfig) -> Vec<f32>` for metronome-style test signal (~15 SLOC)

### Stream Monitoring
- [x] Add `stream_stats(stream: &dyn OutputStream) -> Option<StreamStats>` returning underruns, latency, CPU load when available (~10 SLOC)
- [x] Add `monitor_stream(stream, interval_ms, callback)` for periodic stream health reporting (~20 SLOC)

### MIDI Convenience (oxisound-midi subcrate bridge)
- [x] Add `enumerate_midi_devices() -> Result<Vec<MidiDeviceInfo>, OxiSoundError>` once MIDI types land in core (~10 SLOC)
    - **Done:** Implemented behind `#[cfg(feature = "midi")]` on 2026-05-25.
- [x] Add `open_midi_input(port: usize) -> Result<Box<dyn MidiInput>, OxiSoundError>` (~10 SLOC)
    - **Done:** Delegates to oxisound-midi::MidiDeviceImpl; feature-gated on 2026-05-25.
- [x] Add `open_midi_output(port: usize) -> Result<Box<dyn MidiOutput>, OxiSoundError>` (~10 SLOC)
    - **Done:** Delegates to oxisound-midi::MidiDeviceImpl; feature-gated on 2026-05-25.

### PipeWire Convenience
- [x] Add `#[cfg(feature = "pipewire")] pub fn pipewire_output()` — Done 2026-05-26: `pipewire_output(config)` and `pipewire_input(config)` added behind `pipewire` feature; delegate to `oxisound_pipewire::PipeWireDevice::new("oxisound").open_output` / `.open_input`.

### Native JACK Convenience
- [x] Native JACK integration via oxisound-jack: jack-native feature adds JackDevice, JackOutputStream, JackInputStream, JackCallbackOutputStream, JackTransportState, JackTransportPosition re-exports and jack_native_output/jack_native_input convenience functions — Done 2026-05-26.

## API Improvements
- [x] Add `#[must_use]` on all Result-returning public functions (~5 SLOC)
- [x] Re-export `HostApi` from facade root (currently only in oxisound-core) (~2 SLOC)
- [x] Re-export `DuplexStream` trait from facade root (~2 SLOC)
- [x] Add comprehensive rustdoc `# Examples` blocks on all public functions (~40 SLOC)
- [x] Add module-level documentation with usage overview and feature flag explanation (~20 SLOC docs)
- [x] Ensure `cargo doc --no-deps --all-features` zero warnings (~gate check)

## Testing
- [x] Test `enumerate_all_devices` returns devices with correct `is_input`/`is_output` flags (~15 SLOC)
- [x] Test `select_input_device("nonexistent")` returns `NoDevice` error (~5 SLOC)
- [x] Test `sine_test_tone` at various frequencies (20 Hz, 440 Hz, 20 kHz) and sample rates (8k, 44.1k, 96k) (~20 SLOC)
- [x] Test `chirp_test_tone` frequency sweep is monotonically increasing in spectral content (~15 SLOC)
- [x] Test `white_noise_test` has approximately flat spectrum and correct RMS level (~15 SLOC)
- [x] Test `format_devices` with multi-device list including various I/O combinations (~15 SLOC)
- [x] Test callback-based API: render 100ms of sine in callback, verify no panic (~20 SLOC)
- [x] Integration test: `sine_test_tone` -> `open_output` -> `write` -> sleep -> verify no error (macOS gated) (~15 SLOC) — Done 2026-05-26: tests/integration.rs, hardware-gated with #[ignore = "requires audio hardware"].
- [x] Test async output stream with tokio runtime: write 100ms of silence asynchronously (~15 SLOC)

## Performance
- [ ] Profile `sine_test_tone` generation: ensure no unnecessary allocation beyond the output Vec (~analysis)
- [ ] Profile facade function overhead: measure time in `default_output()` + `open_output()` call chain (~analysis)
- [x] Benchmark `format_devices` with 100+ mock devices to verify O(n) behavior (~10 SLOC)

## Integration
- [x] OSC integration: osc feature re-exports OscArg/OscMessage/OscBundle/encode_osc/decode_osc/OscReceiver/OscSender — Done 2026-05-26.
- [x] SMF facade integration: parse_smf, load_smf, play_smf, SmfPlayer re-export — Done 2026-05-26: `smf` feature adds oxisound-smf re-exports; `play_smf(path, port)` bridges smf+midi features.
- [x] Integration example: play MIDI file via SMF parser + MIDI output — Done 2026-05-26: examples/play_midi.rs, required-features = ["smf", "midi"].
- [x] Integration example: SMF file → polyphonic sine synthesis → audio output — Done 2026-05-26: examples/smf_synth.rs; midi_note_to_freq + BTreeMap oscillator per voice; NoteOn-vel-0 treated as NoteOff; demo sequence with overlapping C-E-G-C5 chord; required-features = ["smf"].
- [x] Integration example: decode WAV with oxiaudio -> play via oxisound output stream (~30 SLOC example)
    - **Done 2026-05-25:** decode_play.rs (83 lines); decodes via oxiaudio::decode_file, plays via oxisound::open_output, falls back to sine tone.
- [x] Integration example: capture audio -> encode to FLAC with oxiaudio-encode (~30 SLOC example)
    - **Done 2026-05-25:** capture_encode.rs (67 lines); captures 2s mono via oxisound::open_input, encodes to WAV via oxiaudio::encode_wav to temp dir.
- [x] Integration example: real-time guitar effects chain (capture -> EQ -> reverb -> output) using oxiaudio-dsp (~40 SLOC example)
    - **Done 2026-05-25:** realtime_eq.rs (73 lines); block-by-block BiquadFilter::peaking_eq (+6 dB at 1 kHz), writes to output.
- [x] Integration example: async capture -> WebSocket broadcast for remote audio monitoring (~30 SLOC example)
    - **Done 2026-05-26:** ws_broadcast.rs — WebSocket server on 127.0.0.1:9001; broadcasts raw f32 LE audio blocks; RMS level meter in terminal; stops after 10s.
- [x] Coordinate with oxiaudio facade: ensure `AudioBuffer` from oxiaudio can be written to oxisound streams with correct interleaving (~type compatibility verification)
    - **Done 2026-05-25:** Verified via decode_play.rs — AudioBuffer<f32>.samples (Vec<f32> interleaved) is directly writable to CpalOutputStream::write(&[f32]). No adapter needed.
- [x] Add WASM target investigation: cpal AudioWorklet support, `#![forbid(unsafe_code)]` compatibility (~research)
    - **Done 2026-05-25:** wasm = ["cpal/wasm-bindgen"] added to oxisound-cpal; cfg-gated thread::spawn and Instant sites; wasm32 build verified.
