# oxisound-jack — Direct JACK Audio Server client for OxiSound

[![Crates.io](https://img.shields.io/crates/v/oxisound-jack.svg)](https://crates.io/crates/oxisound-jack)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

`oxisound-jack` is a direct [JACK Audio Connection Kit](https://jackaudio.org/) client for OxiSound, providing low-latency audio (and MIDI) through the JACK process callback. JACK is inherently callback-based: the server invokes your `process` callback once per JACK buffer period. This crate exposes both a ring-buffer model (`JackOutputStream` / `JackInputStream`, matching `oxisound-cpal`'s usage) and a zero-copy model (`JackCallbackOutputStream`, lowest latency).

> **Quarantine crate (since 0.2.0).** `oxisound-jack` is the sole COOLJAPAN quarantine crate for the libjack2 C-FFI under Pure Rust Policy v2 §5. The `oxisound` facade and `oxisound-cpal` no longer expose JACK features — applications requiring native JACK depend on this crate directly. The actual JACK client is gated behind the **`jack-backend`** feature, which enables the [`jack`](https://crates.io/crates/jack) 0.13.5 crate → `jack-sys` → **libjack2** (a C library that must be installed on the system). With default features, the crate compiles as a **Pure-Rust stub**: every `JackDevice` constructor returns `OxiSoundError::Unsupported`, so downstream crates can offer optional JACK support without `#[cfg]` boilerplate. This crate's own code is `#![forbid(unsafe_code)]`; the `jack` crate exposes a safe Rust API.

## Installation

```toml
[dependencies]
# Pure-Rust stub (default): all JackDevice constructors return Unsupported.
oxisound-jack = "0.2.2"
```

```toml
# Real JACK client — links libjack2 (must be installed: Linux/macOS).
oxisound-jack = { version = "0.2.2", features = ["jack-backend"] }
```

## Quick Start

Open a JACK output stream, auto-connect it to the system playback ports, and write samples (requires the `jack-backend` feature and a running JACK server):

```rust,no_run
use oxisound_core::{OutputStream, StreamConfig};
use oxisound_jack::JackDevice;

let device = JackDevice::new("my-oxisound-client")?;
let mut out = device.open_output(StreamConfig::stereo_48k())?;
out.auto_connect()?; // wire up to system:playback_1 / _2

let buf = vec![0.0f32; 1024 * 2];
out.write(&buf)?;
# Ok::<(), oxisound_core::OxiSoundError>(())
```

Zero-copy synthesis straight from the JACK process thread:

```rust,no_run
use oxisound_core::StreamConfig;
use oxisound_jack::JackDevice;

let device = JackDevice::new("synth")?;
let mut phase = 0.0f32;
let _stream = device.open_output_callback(StreamConfig::stereo_48k(), move |buf: &mut [f32]| {
    for frame in buf.chunks_mut(2) {
        let s = (phase * std::f32::consts::TAU).sin() * 0.2;
        frame[0] = s;
        frame[1] = s;
        phase = (phase + 440.0 / 48_000.0).fract();
    }
})?;
# Ok::<(), oxisound_core::OxiSoundError>(())
```

## API Overview

### Always available (no feature flag required)

These items compile and work even without `jack-backend`, so unit tests and downstream type plumbing never need libjack.

| Item | Description |
|------|-------------|
| `JackTransportState` | JACK transport rolling state: `Rolling`, `Stopped`, `Starting` |
| `JackTransportPosition` | `{ frame: u64, bpm: Option<f64> }` snapshot of the transport timeline |
| `JackMetrics` | Atomic observability counters shared with the realtime callback. Methods: `new()`, `record_sample_rate`, `record_xrun`, `record_buffer_size`, `record_latency`, `snapshot()` |
| `MetricsSnapshot` | `{ sample_rate: u32, xrun_count: u64, buffer_size: u32, latency_frames: u32 }` |
| `SysExReassembler` | Reassembles SysEx messages split across chunks; extracts interleaved realtime bytes. Methods: `new()`, `feed(&[u8]) -> Vec<SysExEvent>`, `in_sysex()`, `reset()` |
| `SysExEvent` | `Complete(Vec<u8>)` (full F0..F7 message) or `Realtime(u8)` |
| `midi_message_len(status) -> Option<usize>` | Expected byte length of a standard MIDI message (`None` for SysEx / unknown) |
| `is_realtime(byte) -> bool` | Whether a byte is a MIDI realtime status (0xF8–0xFF) |
| `is_status(byte) -> bool` | Whether a byte is any MIDI status byte (high bit set) |

### `JackDevice`

The JACK client handle. Without `jack-backend`, `new()` returns `OxiSoundError::Unsupported`; the methods below describe the behaviour when the feature is enabled.

| Method | Description |
|--------|-------------|
| `new(client_name)` | Open (register) a JACK client |
| `sample_rate()` | JACK server sample rate |
| `buffer_size()` | JACK server buffer size in frames |
| `open_output(config)` | Ring-buffer output stream → `JackOutputStream` |
| `open_input(config)` | Ring-buffer input stream → `JackInputStream` |
| `open_output_callback(config, FnMut(&mut [f32]))` | Zero-copy `JackCallbackOutputStream` |
| `open_midi_output(port_name)` | MIDI output port → `JackMidiOutput` (requires `jack-backend`) |
| `open_midi_input(port_name)` | MIDI input port → `JackMidiInput` (requires `jack-backend`) |
| `connect_ports(src, dst)` | Connect two named JACK ports |
| `auto_connect_output(port_name)` | Auto-wire an output port to system playback |
| `transport_state()` | Query the JACK transport clock state |
| `transport_position()` | Query the transport frame / BPM |
| `set_freewheel(enabled)` | Always returns `Unsupported` — `set_freewheel` is not in the `jack` 0.13.5 safe API (upstream TODO) |

### Stream types

`JackOutputStream` implements `oxisound_core::OutputStream`; `JackInputStream` implements `InputStream`. All three stream types share the same port-management and observability surface.

| Method | `JackOutputStream` | `JackInputStream` | `JackCallbackOutputStream` |
|--------|:------------------:|:-----------------:|:--------------------------:|
| `write(&[f32])` / `read(&mut [f32])` | `write` (trait) | `read` (trait) | — (callback-driven) |
| `stats()` | trait | trait | inherent |
| `connect_ports(src, dst)` | yes | yes | yes |
| `auto_connect()` | yes | yes | yes |
| `cpu_load()` | yes | yes | yes |
| `list_ports()` | yes | yes | yes |
| `list_input_ports(Option<&str>)` | yes | yes | yes |
| `list_output_ports(Option<&str>)` | yes | yes | yes |
| `current_sample_rate()` | yes | yes | yes |
| `xrun_count()` | yes | yes | yes |
| `current_buffer_size()` | yes | yes | yes |

### MIDI (requires `jack-backend`)

| Item | Description |
|------|-------------|
| `MIDI_ENTRY_MAX` | `usize` constant (16): max bytes per realtime-safe MIDI ring entry |
| `MidiEntry` | `{ time: u32, len: u8, data: [u8; MIDI_ENTRY_MAX] }` — a fixed-size, `Copy` MIDI entry |
| `JackMidiOutput` | Ring-buffer MIDI output port. `send_raw(time, &[u8])` (auto-chunks SysEx), `frames_processed()`; also implements `oxisound_core::MidiOutput` |
| `JackMidiInput` | MIDI input port. `try_recv() -> Option<MidiEntry>`, `recv_with_sysex() -> (Vec<MidiEntry>, Vec<SysExEvent>)`, `frames_processed()` |

## Feature Flags

| Feature | Default | Description |
|---------|---------|-------------|
| `jack-backend` | no | Links the `jack` crate → `jack-sys` → **libjack2** (C-FFI). Without it, the crate is a Pure-Rust stub where every constructor returns `OxiSoundError::Unsupported`. Requires libjack2 installed (Linux/macOS) |

## Errors

All fallible methods return `oxisound_core::OxiSoundError`. Notable mappings: a full MIDI output ring → `Underrun`; missing `jack-backend` or unimplemented `set_freewheel` → `Unsupported`. See the [`oxisound-core` error table](../oxisound-core#oxisounderror-variants).

## Architecture notes

- **Two output models.** `JackOutputStream` is ring-buffer-backed (you call `write`, the JACK callback drains it). `JackCallbackOutputStream` runs your `FnMut(&mut [f32])` directly in the JACK process thread for the lowest possible latency.
- **Transport.** `transport_state()` and `transport_position()` read the JACK transport clock and BBT data. `set_freewheel` is intentionally `Unsupported` pending upstream `jack` support.
- **MIDI ring safety.** `MidiEntry` is fixed-size and `Copy` so it can cross the SPSC ring buffer without allocation; SysEx longer than `MIDI_ENTRY_MAX` is chunked and reassembled by the process callback.

## Cross-references

- **Traits & types:** [`oxisound-core`](../oxisound-core) — `OutputStream`, `InputStream`, `MidiOutput`, `MidiMessage`, `StreamConfig`.
- **Other backends:** [`oxisound-cpal`](../oxisound-cpal) (CPAL / native APIs), [`oxisound-midi`](../oxisound-midi) (cross-platform MIDI via midir).
- **Facade:** [`oxisound`](../oxisound).

## License

Apache-2.0 — COOLJAPAN OU (Team Kitasan)
