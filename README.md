# OxiSound

**OxiSound is the COOLJAPAN Pure-Rust audio device I/O layer.**

Version: **0.2.1** — released 2026-08-06 (see [CHANGELOG.md](CHANGELOG.md))

[![Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)
[![Rust 1.89+](https://img.shields.io/badge/rustc-1.89%2B-orange.svg)](https://releases.rs/docs/1.89.0/)

## Overview

OxiSound provides a cross-platform audio device I/O API for Rust applications. It wraps
[cpal](https://crates.io/crates/cpal) (Pure Rust, OS-boundary) and adds MIDI, SMF, OSC, and
JACK support via optional subcrates.

On Linux, the new `oxisound-pulse` crate (0.2.1) additionally speaks the **PulseAudio native
IPC protocol** directly — 100% Pure Rust, no `libpulse`, no `alsa-lib`, no C in the audio
path. Because PipeWire serves the same protocol through its `pipewire-pulse` compatibility
service, that backend covers **PipeWire** as well, with no PipeWire-specific code. It is
opt-in behind the facade's `pulse` feature and is not part of `default`. A *native* (non-Pulse)
PipeWire protocol backend remains unimplemented; see
[Known Blocked Items](#known-blocked-items).

## Crate Layout

```
oxisound/
├── crates/
│   ├── oxisound-core/     # Core traits: AudioDevice, OutputStream, InputStream; types; no_std support
│   ├── oxisound-cpal/     # cpal-backed implementation (ALSA/CoreAudio/WASAPI auto-selected by OS)
│   ├── oxisound-pulse/    # Pure-Rust PulseAudio native-protocol backend (also PipeWire via pipewire-pulse; Linux; opt-in: pulse feature)
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
| `jack-sys` (Linux/macOS opt) | OS_BOUNDARY | `jack-backend` feature on `oxisound-jack`; default = stub |
| `oxisound-pulse` (Linux)  | **PURE RUST**  | No C at all — native PulseAudio IPC over a unix socket; also covers PipeWire via `pipewire-pulse`. Opt-in `pulse` feature on the facade. |

There is no `pipewire-sys` row: cpal has no native PipeWire feature to wrap (see
[Known Blocked Items](#known-blocked-items)), so no PipeWire C-FFI dependency is reachable
from this workspace at any feature combination. PipeWire is reached instead through
`oxisound-pulse`, which needs no C library of any kind.

**IMPORTANT:** cpal backends (ALSA/CoreAudio/WASAPI) are auto-selected by `cfg(target_os)`.
There are NO alsa/coreaudio/wasapi Cargo features — do NOT add them.

## Quick Start

```toml
[dependencies]
oxisound = "0.2.1"
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
| `pulse`          | Pure-Rust PulseAudio/PipeWire backend via `oxisound-pulse` (Linux; stub elsewhere) |         |
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
use oxisound_osc::{OscArg, OscMessage, OscPacket, encode, decode};

let packet = OscPacket::Message(OscMessage {
    address: "/synth/freq".to_string(),
    args: vec![OscArg::Float(440.0)],
});
let bytes = encode(&packet);
let decoded = decode(&bytes)?;
assert_eq!(decoded, packet);

// UDP transport
let sender = oxisound_osc::OscSender::connect("127.0.0.1:9000")?;
sender.send_message("/synth/note", vec![OscArg::Int(60)])?;
```

## oxisound-pulse (Pure-Rust PulseAudio / PipeWire backend — new in 0.2.1)

`oxisound-pulse` talks the **PulseAudio native IPC protocol** straight over a unix socket
(framing via the pure-Rust [`pulseaudio`](https://crates.io/crates/pulseaudio) crate). No
`libpulse`, no `alsa-lib`, no C, `#![forbid(unsafe_code)]`. PipeWire is covered for free
through its `pipewire-pulse` compatibility service — same protocol, same socket, no
PipeWire-specific code.

It is **opt-in** — not part of `default = ["pure"]`:

```toml
[dependencies]
oxisound = { version = "0.2.1", features = ["pulse"] }
```

```rust
// Enumerate PulseAudio/PipeWire endpoints (sinks -> is_output, sources -> is_input;
// a sink's monitor source is reported as an input named "<sink>.monitor").
let devices = oxisound::pulse_enumerate_devices()?;

// Playback / capture / duplex on the server default
let mut out    = oxisound::pulse_output(oxisound::StreamConfig::stereo_48k())?;
let mut inp    = oxisound::pulse_input(oxisound::StreamConfig::mono_16k())?;
let mut duplex = oxisound::pulse_duplex(oxisound::StreamConfig::stereo_48k())?;

// Named endpoints — capture system output via the monitor source
let mut monitor = oxisound::pulse_input_named(
    "alsa_output.pci-0000_00_1f.3.analog-stereo.monitor",
    oxisound::StreamConfig::stereo_48k(),
)?;
```

Or use the crate directly, without the facade:

```rust
use oxisound_core::{AudioDevice, OutputStream, StreamConfig};
use oxisound_pulse::PulseDevice;

let device = PulseDevice::default_output()?;
let mut stream = device.open_output_concrete(StreamConfig::stereo_48k())?;
stream.write(&vec![0.0f32; 1024 * 2])?;
stream.drain()?;
```

**Platform behaviour.** The `pulseaudio` dependency sits under
`[target.'cfg(target_os = "linux")'.dependencies]`. On every other target the crate still
compiles — as a Pure-Rust stub whose constructors return `OxiSoundError::Unsupported` —
so downstream crates may depend on it unconditionally, exactly like `oxisound-jack`. The
socket/cookie discovery (`env`), format negotiation (`format`), device mapping and buffer
arithmetic (`model`) and deadline machinery (`timeout`) modules are host-independent and
unit-tested on every platform.

**Implemented:** server discovery + cookie auth, enumeration (server info, sink list, source
list) with monitor tagging, playback, capture, duplex, cork/uncork (`pause()`/`resume()`),
`flush()`/`drain()`/`discard_buffered()`, underrun/overrun counters, and an explicit deadline
on every blocking control-plane round-trip.

**Not implemented (deliberate, tracked in `TODO.md`):** SHM/memfd zero-copy transfer (audio
goes over the socket as plain memblocks), volume and mute control, module loading, and the
subscription/hot-plug event stream.

> `CpalDevice::with_host(HostApi::PulseAudio)` still returns an error and will keep doing so —
> cpal has no PulseAudio host to dispatch to, which is precisely why this crate exists.
> `PulseDevice::host_api()` reports `HostApi::PulseAudio` for parity.

## oxisound-jack (quarantine crate — C-FFI boundary)

`oxisound-jack` is the sole COOLJAPAN quarantine crate for the libjack2 C-FFI. It is not exposed through the `oxisound` facade. Applications requiring native JACK depend on `oxisound-jack` directly:

```toml
[dependencies]
oxisound-jack = { version = "0.2.1", features = ["jack-backend"] }
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
- **M5** — MIDI, SMF, OSC, JACK MIDI, docs+examples ✅ (2026-05-26)
- **M6** — `oxisound-pulse`: Pure-Rust PulseAudio native-protocol backend (also serves PipeWire via `pipewire-pulse`), opt-in `pulse` facade feature ✅ (2026-08-04)

### Known Blocked Items

| Item | Reason |
|------|--------|
| Native PipeWire (non-Pulse) protocol backend | PipeWire itself **is** supported as of 0.2.1, through `oxisound-pulse` and PipeWire's `pipewire-pulse` compatibility service. What is still missing is a backend speaking PipeWire's *own* native protocol: cpal 0.17.3/0.18.1 exposes no native `pipewire` feature to wrap, and no Pure-Rust PipeWire protocol crate exists yet. Revisit if one appears — see `crates/oxisound-cpal/TODO.md`. |
| `oxisound-pulse` SHM / memfd zero-copy | Audio is currently streamed to the server as plain memblocks over the socket. SHM/memfd transfer, volume/mute control, module loading and the subscription/hot-plug event stream are deliberately out of scope for 0.2.1 — see the `oxisound-pulse` section of `TODO.md`. |
| `oxisound-pulse` on-hardware validation | The 58 unit tests and the stub integration test are host-independent and run everywhere; the live-server path (`crates/oxisound-pulse/tests/live_server.rs`) needs a real PulseAudio or `pipewire-pulse` socket, so it is only exercised on Linux. |
| Android device testing | Requires NDK cross-compilation + hardware |
| JACK freewheel mode | `set_freewheel` not yet implemented in `jack 0.13.5` |
| iOS interruption handling | Full AVAudioSession interruption run-loop requires iOS runtime integration |

## Test Results

Measured 2026-08-06 on macOS/aarch64 at version 0.2.1:

| Run | Result |
|-----|--------|
| `cargo nextest run --workspace` (default features) | **297 passed**, 0 failed, 8 skipped |
| `cargo nextest run --all-features --workspace`     | **313 passed**, 0 failed, 11 skipped |
| `cargo test --doc --all-features --workspace`      | **127 doctests passed**, 0 failed |

Per-crate breakdown (default features / `--all-features`):

| Crate | default | `--all-features` | doctests |
|-------|--------:|-----------------:|---------:|
| `oxisound-core`    | 63 | 81 | 46 |
| `oxisound-cpal`    | 57 | 60 |  2 |
| `oxisound-pulse`   | **58** | **58** | **17** |
| `oxisound-jack`    | 25 | 20 |  2 |
| `oxisound-osc`     | 25 | 25 |  3 |
| `oxisound-smf`     | 22 | 22 |  3 |
| `oxisound-midi`    | 15 | 15 |  2 |
| `oxisound-session` |  4 |  4 |  4 |
| `oxisound` facade  | 28 | 28 | 48 |
| **Total**          | **297** | **313** | **127** |

> `oxisound-jack`'s count *drops* from 25 to 20 under `--all-features`, which is expected rather
> than a regression: enabling `jack-backend` swaps the Pure-Rust stub for the real client, so the
> stub-specific unit tests are `cfg`'d out and 2 hardware-integration tests become `#[ignore]`d
> (they move into the skipped column, not the failed one). `oxisound-pulse` is identical in both
> columns because it has no Cargo features.

`cargo clippy --workspace --all-targets --all-features -- -D warnings`,
`cargo fmt --all -- --check` and `RUSTDOCFLAGS="-D warnings" cargo doc --workspace
--all-features --no-deps` are all clean (zero warnings), and
`cargo deny --all-features --workspace check` reports `advisories ok, bans ok, licenses ok,
sources ok`.

The totals are well above the 0.2.0-era figures (`235` default / `251` all-features) chiefly
because of the new `oxisound-pulse` crate, which contributes 58 tests and 17 doctests of its
own — all host-independent, exercising the socket/cookie discovery, format negotiation,
device-mapping and buffer-arithmetic layers plus the non-Linux `Unsupported` stub, so they run
identically on macOS and on Linux.

**Skipped tests are hardware-conditional, by design.** The 8 skipped under default features are
1 facade integration test (`integration_sine_to_output`) and 7 `oxisound-cpal` tests that need a
real output/input device. `--all-features` skips 3 more: `oxisound-cpal`'s
`async_input_stream_captures_frames` (enabled by `tokio`, still device-bound) and
`oxisound-jack`'s 2 dedicated hardware-integration stubs (`test_jack_midi_output_integration`,
`test_jack_midi_input_integration` in `crates/oxisound-jack/src/midi.rs`, `#[ignore]`d by design
with a comment on how to run them manually against a real daemon). `oxisound-jack` with
`jack-backend` is otherwise fully included in the 313 count — it compiles and its non-hardware
unit tests (metrics, MIDI utilities, transport state/position) pass without a JACK daemon
installed. All crates pass under both default and `--all-features`.

## License

Apache-2.0 — © COOLJAPAN OU (Team Kitasan)

## Related

- [`oxiaudio`](https://github.com/cool-japan/oxiaudio) — Audio processing, DSP, codecs
- [`../00_MASTER_ROADMAP.md`](../00_MASTER_ROADMAP.md) — noffi ecosystem roadmap
