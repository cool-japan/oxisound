# OxiSound

**OxiSound is the COOLJAPAN Pure-Rust audio device I/O layer.**

Version: **0.2.0** — 2026-06-22

[![Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)
[![Rust 1.89+](https://img.shields.io/badge/rustc-1.89%2B-orange.svg)](https://releases.rs/docs/1.89.0/)

## Overview

OxiSound provides a cross-platform audio device I/O API for Rust applications. It wraps
[cpal](https://crates.io/crates/cpal) (Pure Rust, OS-boundary) and adds MIDI, SMF, OSC, JACK,
and PipeWire support via optional subcrates.

## Crate Layout

```
oxisound/
├── crates/
│   ├── oxisound-core/     # Core traits: AudioDevice, OutputStream, InputStream; types; no_std support
│   ├── oxisound-cpal/     # cpal-backed implementation (ALSA/CoreAudio/WASAPI auto-selected by OS)
│   ├── oxisound-midi/     # MIDI I/O via midir (CoreMIDI/WinMM/ALSA sequencer)
│   ├── oxisound-smf/      # Standard MIDI File (SMF) parser + writer + playback iterator
│   ├── oxisound-jack/     # JACK audio server client (opt-in: jack-backend feature)
│   ├── oxisound-osc/      # Open Sound Control (OSC) encode/decode + UDP transport
│   ├── oxisound-session/  # iOS/macOS audio session management (AVAudioSession; opt-in: avf-audio)
│   └── oxisound/          # Public facade (default = ["pure"])
├── deny.toml
├── Dockerfile.ffi-audit
└── scripts/ffi-audit.sh
```

## What OxiSound Wraps

OxiSound's OS-backend crates are accepted under COOLJAPAN GOVERNANCE §8 (OS-boundary exemptions):

| Backend                   | Classification | Notes                                    |
|---------------------------|----------------|------------------------------------------|
| `alsa-sys` (Linux)        | OS_BOUNDARY    | Auto-selected via cpal; no feature flag  |
| `coreaudio-sys` (macOS)   | OS_BOUNDARY    | Auto-selected via cpal; no feature flag  |
| `wasapi` (Windows)        | OS_BOUNDARY    | Auto-selected via cpal; no feature flag  |
| `jack-sys` (Linux opt)    | OS_BOUNDARY    | `jack-backend` feature; default = stub   |
| `pipewire-sys` (Linux opt)| OS_BOUNDARY    | `pipewire-backend` feature; default = stub |

**IMPORTANT:** cpal backends (ALSA/CoreAudio/WASAPI) are auto-selected by `cfg(target_os)`.
There are NO alsa/coreaudio/wasapi Cargo features — do NOT add them.

## Quick Start

```toml
[dependencies]
oxisound = "0.2.0"
```

```rust
// Play 2 seconds of silence through the default output device
let stream = oxisound::open_output(oxisound::StreamConfig::stereo_48k())?;
std::thread::sleep(std::time::Duration::from_secs(2));
stream.stop()?;
```

## Feature Flags

| Feature          | Description                                                       | Default |
|------------------|-------------------------------------------------------------------|---------|
| `pure`           | cpal backend (ALSA/CoreAudio/WASAPI)                              | ✅      |
| `tokio`          | Async output/input streams, device event subscriptions            |         |
| `midi`           | MIDI I/O via `oxisound-midi`                                      |         |
| `smf`            | SMF parser/writer/player via `oxisound-smf`                       |         |
| `osc`            | OSC encode/decode/UDP via `oxisound-osc`                          |         |
| `session`        | Audio session management via `oxisound-session` (stub on non-Apple)|         |
| `macos-session`  | `session` + AVFoundation backend (`avf-audio`) on macOS/iOS       |         |
| `wasm`           | WebAudio backend for `wasm32-unknown-unknown`                     |         |
| `oxiaudio`       | Type bridge with `oxiaudio-core` (`SampleFormat` etc.)            |         |

> **JACK / ASIO (0.2.0 change):** The `jack`, `jack-native`, and `asio` features have been removed from the `oxisound` facade to enforce COOLJAPAN Pure Rust Policy v2 §5. Applications requiring native JACK must depend on `oxisound-jack` directly (with the `jack-backend` feature). ASIO support requires a future dedicated quarantine crate.

## API Surface (oxisound facade)

### Device Enumeration

```rust
let devices = oxisound::enumerate_all_devices()?;
let out = oxisound::default_output()?;
let inp = oxisound::default_input()?;
let dev = oxisound::device_by_index(2)?;
let config = oxisound::preferred_output_config(&out)?;
```

### Stream Control

```rust
// Simple blocking write
let mut stream = oxisound::open_output(config)?;
stream.write(&samples)?;
stream.stop()?;

// Duplex (simultaneous input/output)
let duplex = oxisound::duplex_stream(config)?;
```

### Callback API (lowest latency)

```rust
oxisound::play_callback(config, |buf: &mut [f32]| {
    // fill buf in realtime — no allocations
})?;

oxisound::duplex_callback(config, |in_buf: &[f32], out_buf: &mut [f32]| {
    // process in realtime
})?;
```

### Test Tones

```rust
oxisound::sine_test_tone(440.0, 2.0)?;     // 440 Hz, 2 seconds
oxisound::white_noise_test(1.0)?;
oxisound::chirp_test_tone(100.0, 8000.0, 2.0)?;
oxisound::silence(1.0)?;
```

### Monitoring

```rust
let guard = oxisound::monitor_stream(&stream, std::time::Duration::from_secs(1))?;
// guard logs health (Healthy/Degraded/Disconnected) every second
drop(guard);  // stops monitoring
```

## oxisound-core (no_std)

```rust
#![no_std]
extern crate alloc;
use oxisound_core::{StreamConfig, OxiSoundError, SampleFormat};

let config = StreamConfig::stereo_48k();
assert!(config.validate(None).is_ok());
```

## oxisound-midi

```rust
let host = oxisound_midi::MidiHost::new()?;
let ports = host.input_port_names()?;
let mut input = host.open_input(0, |ts, msg| println!("{ts}: {msg:?}"))?;
```

## oxisound-smf

```rust
let smf = oxisound_smf::parse(include_bytes!("song.mid"))?;
let map = oxisound_smf::TempoMap::from_file(&smf);
let player = oxisound_smf::SmfPlayer::new(smf);
// player.play(&mut midi_output)?;
for (secs, msg) in player.midi_events() {
    println!("{secs:.3}s: {msg:?}");
}
```

## oxisound-osc

```rust
use oxisound_osc::{OscMessage, OscArg, encode, decode};

let msg = OscMessage {
    address: "/synth/freq".into(),
    args: vec![OscArg::Float(440.0)],
};
let bytes = encode::message(&msg)?;
let decoded = decode::packet(&bytes)?;

// UDP transport
let sender = oxisound_osc::OscSender::connect("127.0.0.1:9000")?;
sender.send_message("/synth/note", vec![OscArg::Int(60)])?;
```

## oxisound-jack (quarantine crate — C-FFI boundary)

`oxisound-jack` is the sole COOLJAPAN quarantine crate for the libjack2 C-FFI. It is not exposed through the `oxisound` facade. Applications requiring native JACK depend on `oxisound-jack` directly:

```toml
[dependencies]
oxisound-jack = { version = "0.2.0", features = ["jack-backend"] }
```

```rust
let dev = oxisound_jack::JackDevice::new("my_app")?;
let stream = dev.open_output(config)?;
dev.auto_connect_output(&stream)?;
println!("CPU load: {:.1}%", stream.cpu_load() * 100.0);
```

## Milestones

- **M0** — workspace skeleton, core traits, deny gates ✅ (2026-05-25)
- **M1** — cpal output/input, device enumeration, silence playback ✅ (2026-05-25)
- **M2** — input capture, full format dispatch, adaptive buffer sizing ✅ (2026-05-25)
- **M3** — duplex, JACK/ASIO opt-in, callback API ✅ (2026-05-25)
- **M4** — async streams (tokio), device hot-plug, auto-reconnect ✅ (2026-05-25)
- **M5** — MIDI, SMF, OSC, JACK MIDI, PipeWire, docs+examples ✅ (2026-05-26)

### Known Blocked Items

| Item | Reason |
|------|--------|
| PipeWire backend (non-Linux) | PipeWire is Linux-only by design |
| Android device testing | Requires NDK cross-compilation + hardware |
| JACK freewheel mode | `set_freewheel` not yet implemented in `jack 0.13.5` |
| iOS interruption handling | Full AVAudioSession interruption run-loop requires iOS runtime integration |

## Test Results

**194 tests passed** (as of 2026-06-22, version 0.2.0; 8 skipped — platform-conditional)

`oxisound-jack` was excluded from this run (no libjack2 available on macOS; `jack-backend` requires a Linux/macOS JACK daemon). All other crates pass.

## License

Apache-2.0 — © COOLJAPAN OU (Team Kitasan)

## Related

- [`oxiaudio`](https://github.com/cool-japan/oxiaudio) — Audio processing, DSP, codecs
- [`../00_MASTER_ROADMAP.md`](../00_MASTER_ROADMAP.md) — noffi ecosystem roadmap
