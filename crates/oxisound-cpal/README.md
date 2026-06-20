# oxisound-cpal — CPAL-backed audio device implementation for OxiSound

[![Crates.io](https://img.shields.io/crates/v/oxisound-cpal.svg)](https://crates.io/crates/oxisound-cpal)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

`oxisound-cpal` is the [CPAL](https://crates.io/crates/cpal)-backed implementation of the device and stream traits defined in [`oxisound-core`](../oxisound-core). It provides the `CpalDevice` type (implementing `AudioDevice`) plus a family of ring-buffer and zero-copy stream wrappers, adaptive buffer sizing, device hot-plug detection, automatic stream recovery, and optional async stream adapters.

> **Pure-Rust status: platform backend (NOT Pure Rust).** CPAL binds the host operating system's native audio APIs through C-FFI: **ALSA** + `libc` on Linux/BSD, **CoreAudio** (`coreaudio-rs`, `objc2-core-foundation`) on macOS/iOS, and the **WASAPI** path of the `windows` crate on Windows. These bindings are auto-selected by `cfg(target_os)` — there is no ALSA/CoreAudio/WASAPI feature to toggle. Under the COOLJAPAN Pure Rust Policy, CPAL is treated as a permitted OS-boundary backend (the same rationale as the JACK and Web Audio backends): it provides hardware audio access that no pure-Rust crate can replicate. This crate's own code is `#![forbid(unsafe_code)]`; the `unsafe` lives inside CPAL and the OS bindings.

## Installation

```toml
[dependencies]
oxisound-cpal = "0.1.3"
```

```toml
# async output/input stream wrappers (requires a tokio runtime; sync + rt features)
oxisound-cpal = { version = "0.1.3", features = ["tokio"] }

# WebAudio backend for wasm32-unknown-unknown
oxisound-cpal = { version = "0.1.3", features = ["wasm"] }
```

## Quick Start

Open the default output device and play one second of a 440 Hz sine tone:

```rust,no_run
use oxisound_core::{AudioDevice, OutputStream, StreamConfig};
use oxisound_cpal::CpalDevice;

let device = CpalDevice::default_output()?;
let mut out = device.open_output(StreamConfig::stereo_48k())?;

let mut buf = vec![0.0f32; 48_000 * 2]; // 1 s stereo @ 48 kHz
for (i, frame) in buf.chunks_mut(2).enumerate() {
    let t = i as f32 / 48_000.0;
    let s = (2.0 * std::f32::consts::PI * 440.0 * t).sin() * 0.2;
    frame[0] = s;
    frame[1] = s;
}
out.write(&buf)?;
# Ok::<(), oxisound_core::OxiSoundError>(())
```

Enumerate devices:

```rust,no_run
use oxisound_core::AudioDevice;
use oxisound_cpal::CpalDevice;

for d in CpalDevice::enumerate_all()? {
    println!("{d}");
}
# Ok::<(), oxisound_core::OxiSoundError>(())
```

## API Overview

### `CpalDevice`

Implements `oxisound_core::AudioDevice` and adds a large set of inherent methods for enumeration, host selection, concrete-typed stream opening, and hot-plug handling.

| Method | Description |
|--------|-------------|
| `default_output()` / `default_input()` | Open the system default output / input device (`AudioDevice`) |
| `enumerate()` | List output devices (`AudioDevice`) |
| `enumerate_input()` | List input devices |
| `enumerate_all()` | List every device (input + output) in one call |
| `with_host(HostApi)` | Open the default output device of a specific host API; `Err` on wasm32 |
| `select_output(fragment)` / `select_input(fragment)` | Find a device whose name contains a fragment (case-insensitive) |
| `host_api()` | Return the `HostApi` this device belongs to |
| `name()` | Device name |
| `default_output_latency_ms()` | Device-level latency estimate (ms) |
| `optimal_buffer_size()` | Recommended output buffer size (falls back to 512) |
| `open_output(config)` / `open_input(config)` / `open_duplex(config)` | `Box<dyn …>` streams (`AudioDevice`) |
| `open_output_concrete(config)` | Concrete `CpalOutputStream` (needed for `enable_auto_reconnect`) |
| `open_output_with_capacity(config, secs)` | Output stream with a custom ring-buffer capacity (clamped 0.1–30 s) |
| `open_output_with_retry(config)` | Output with exponential backoff (10→50→200→1000 ms); not on wasm32 |
| `open_output_callback(config, FnMut(&mut [f32]))` | Zero-copy `CpalCallbackOutputStream` |
| `open_input_callback(config, FnMut(&[f32]))` | Zero-copy `CpalCallbackInputStream` |
| `open_loopback(config)` | System loopback capture (Linux `.monitor` sources; `Unsupported` elsewhere) |
| `negotiate_output(config)` | Resolve a `NegotiatedConfig`, honouring `preferred_formats` ranking |
| `open_async_output(config)` / `open_async_input(config)` | Async stream wrappers (`tokio`) |
| `watch_devices()` | Start a `CpalDeviceWatcher` hot-plug poller; not on wasm32 |
| `on_device_change(Fn(DeviceEvent))` | Register a sync hot-plug callback → `DeviceChangeGuard`; not on wasm32 |
| `subscribe_device_events()` | `(CpalDeviceWatcher, broadcast::Receiver<DeviceEvent>)` (`tokio`, not wasm32) |

### `CpalOutputStream`

Ring-buffer-backed output stream (implements `OutputStream`). The application pushes samples via `write`; a lock-free SPSC ring buffer feeds the real-time audio callback.

| Method | Description |
|--------|-------------|
| `write(&[f32])` | Push interleaved samples (`OutputStream`); `Overrun` if the ring is full |
| `stats()` | `StreamStats` snapshot (frames, underruns, latency, CPU load) |
| `latency_frames()` | Frames currently buffered (output latency) |
| `ring_capacity()` | Maximum ring-buffer samples |
| `underrun_count()` | Underruns since open |
| `is_disconnected()` | Whether the device reported disconnection |
| `stream_time()` | Seconds since the stream opened (not on wasm32) |
| `pause()` / `resume()` | Pause / resume rendering (ring preserved) |
| `flush()` | Block until the ring drains (2 s timeout); immediate `Ok` on wasm32 |
| `health()` | `StreamHealth` (`Healthy` / `Degraded` / `Disconnected`) |
| `tick_adaptive()` | Advance the adaptive sizer from observed underruns; returns recommended size |
| `adaptive_sizer()` | Copy of the current `AdaptiveBufferSizer` state |
| `set_buffer_size(frames)` / `desired_buffer_size()` | Store / read a desired size for a caller-driven rebuild |
| `enable_auto_reconnect(config)` | Start background recovery, returns `RecoveryHandle` (not on wasm32) |

### `CpalInputStream`

Ring-buffer-backed input stream (implements `InputStream`). The audio callback pushes captured samples; the application pops them via `read`.

| Method | Description |
|--------|-------------|
| `read(&mut [f32])` | Pop captured samples, returns count (`InputStream`) |
| `stats()` | `StreamStats` snapshot |
| `latency_frames()` | Frames currently available in the capture ring |
| `capacity()` | Maximum capture-ring samples |
| `is_disconnected()` | Whether the device reported disconnection |
| `frames_processed()` | Frames captured since open |
| `pause()` / `resume()` | Pause / resume capture |

### `CpalDuplexStream`

Paired input + output ring buffers (implements `DuplexStream`), with clock-drift estimation and a built-in linear-interpolation resampler.

| Method | Description |
|--------|-------------|
| `write(&[f32])` / `read(&mut [f32])` | Output / input (`DuplexStream`) |
| `stats()` | `StreamStats` (output progress + roundtrip latency) |
| `input_latency_frames()` / `output_latency_frames()` | Per-direction buffered frames |
| `roundtrip_latency_frames()` | Sum of capture + playback latency |
| `input_frames_processed()` / `output_frames_processed()` | Per-direction processed-frame counters |
| `drift_ratio()` | Raw `output/input` frame ratio (`None` until input runs) |
| `drift_ema()` | EMA-smoothed drift ratio (converges to 1.0 with matched clocks) |
| `pump_resampled()` | Pump input → output with drift-corrected resampling; returns frames written |

### Callback (zero-copy) streams

`open_output_callback` / `open_input_callback` invoke your closure directly on the real-time audio thread, bypassing the ring buffer for minimal latency.

| Type | Methods |
|------|---------|
| `CpalCallbackOutputStream` | `last_callback_duration()`, `is_disconnected()`, `pause()`, `resume()` |
| `CpalCallbackInputStream` | `is_disconnected()`, `pause()`, `resume()` |

### Async streams (`tokio` feature)

| Type | Description |
|------|-------------|
| `CpalAsyncOutputStream` | Implements `oxisound_core::AsyncOutputStream`; `write` is non-blocking (ring push) |
| `CpalAsyncInputStream` | Implements `oxisound_core::AsyncInputStream` **and** `futures_core::Stream<Item = Vec<f32>>` |

### Adaptive buffer sizing

`AdaptiveBufferSizer` is a pure, allocation-free state machine: it doubles the buffer on an underrun (capped at `max`) and shrinks back toward the initial size after a configurable number of stable periods. It only *recommends* a size — applying it requires a caller-driven stream rebuild.

| Method | Description |
|--------|-------------|
| `new(initial, min, max, stable_threshold)` | Construct with normalised bounds (`min ≤ initial ≤ max`) |
| `current_size()` | Recommended buffer size in frames |
| `size_changed()` | Whether the last `record_*` call changed the size |
| `record_underrun()` | Grow (double, capped); returns new size |
| `record_stable_period()` | Count a stable period; shrink once the threshold is hit |
| `reset()` | Return to the initial size and clear counters |

### Hot-plug & recovery

| Type | Description |
|------|-------------|
| `CpalDeviceWatcher` | Polls the device list every ~500 ms. `start()`, `try_recv()`, `subscribe()` (`tokio`). Stops on drop. Not on wasm32 |
| `DeviceChangeGuard` | RAII guard returned by `on_device_change`; stops the listener thread on drop. Not on wasm32 |
| `RecoveryHandle` | RAII guard returned by `enable_auto_reconnect`; stops the recovery thread on drop. Not on wasm32 |
| `StreamHealth` | `Healthy`, `Degraded(f32)`, `Disconnected` |

## Feature Flags

| Feature | Description |
|---------|-------------|
| `jack` | Enables CPAL's JACK host (`cpal/jack`) on Linux/macOS — links libjack |
| `asio` | Enables CPAL's ASIO host (`cpal/asio`) on Windows — requires the ASIO SDK |
| `wasm` | Enables CPAL's WebAudio backend (`cpal/wasm-bindgen`) for `wasm32-unknown-unknown` |
| `tokio` | Async output/input stream wrappers; pulls in `tokio` (sync + rt) and `futures-core`; implies `oxisound-core/tokio` |

> **wasm32 caveats.** The `tokio` feature is incompatible with wasm32 (enabling both is a compile error — use `wasm` instead). On wasm32 there are no OS threads, so automatic recovery and hot-plug detection (`enable_auto_reconnect`, `watch_devices`, `on_device_change`) are not compiled in, and `std::time::Instant::now()` panics without a `web-time` shim.
>
> **Native backend selection is automatic.** ALSA / CoreAudio / WASAPI are chosen by `cfg(target_os)` and have **no** feature flag. PipeWire is not a CPAL feature in 0.17.3; on Linux it is reached through the ALSA or JACK compatibility layer.
>
> **Exclusive (low-latency) mode.** CPAL 0.17.3 hardcodes WASAPI shared mode, so `StreamConfig::exclusive` currently logs a warning and falls back to shared mode.

## Errors

All fallible methods return `oxisound_core::OxiSoundError`. CPAL build/play/enumeration errors are mapped onto its variants — most notably `DeviceNotAvailable` → `Disconnected`, format issues → `FormatMismatch`, and a full ring buffer → `Overrun`. See the [`oxisound-core` error table](../oxisound-core#oxisounderror-variants).

## Cross-references

- **Traits & types:** [`oxisound-core`](../oxisound-core) — `AudioDevice`, `OutputStream`, `InputStream`, `DuplexStream`, `StreamConfig`, `DeviceInfo`, `HostApi`.
- **Other backends:** [`oxisound-jack`](../oxisound-jack) (direct JACK client), [`oxisound-midi`](../oxisound-midi) (MIDI I/O).
- **Facade:** [`oxisound`](../oxisound).

## License

Apache-2.0 — COOLJAPAN OU (Team Kitasan)
