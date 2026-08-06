# oxisound-pulse — Pure-Rust PulseAudio native-protocol backend for OxiSound

[![Crates.io](https://img.shields.io/crates/v/oxisound-pulse.svg)](https://crates.io/crates/oxisound-pulse)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

`oxisound-pulse` speaks the **PulseAudio native IPC protocol** directly over a unix socket. It is
**100 % Pure Rust**: no `libpulse`, no `alsa-lib`, no C, and `#![forbid(unsafe_code)]` throughout.
Framing and tagstruct serialisation come from the pure-Rust
[`pulseaudio`](https://crates.io/crates/pulseaudio) crate (0.3, MIT).

This is the crate that answers *"Linux audio with no C library in the path"* for the OxiSound
workspace — the default [`oxisound-cpal`](../oxisound-cpal) backend reaches Linux hardware through
`alsa-lib`, which is a C library at the OS boundary.

It works unchanged against both Linux sound servers:

| Server | How |
|--------|-----|
| **PulseAudio** | the native socket, normally `$XDG_RUNTIME_DIR/pulse/native` |
| **PipeWire** | the `pipewire-pulse` compatibility service, serving the same protocol on the same socket — no PipeWire-specific code |

## Installation

```toml
[dependencies]
oxisound-pulse = "0.2.1"
```

Or through the facade's opt-in feature (**not** in `default`):

```toml
[dependencies]
oxisound = { version = "0.2.1", features = ["pulse"] }
```

## Quick Start

```rust,no_run
use oxisound_core::{AudioDevice, OutputStream, StreamConfig};
use oxisound_pulse::PulseDevice;

let device = PulseDevice::default_output()?;
let mut stream = device.open_output_concrete(StreamConfig::stereo_48k())?;

// 100 ms of silence at 48 kHz stereo.
stream.write(&vec![0.0f32; 4_800 * 2])?;
stream.drain()?;
# Ok::<(), oxisound_core::OxiSoundError>(())
```

Capture what is currently playing, via a sink's monitor source:

```rust,no_run
use oxisound_core::{InputStream, StreamConfig};
use oxisound_pulse::{DEFAULT_CLIENT_NAME, PulseDevice, PulseDeviceRole};

let device = PulseDevice::open_named(
    "alsa_output.pci-0000_00_1f.3.analog-stereo.monitor",
    PulseDeviceRole::Source,
    DEFAULT_CLIENT_NAME,
)?;
let mut stream = device.open_input_concrete(StreamConfig::stereo_48k())?;

let mut buf = vec![0.0f32; 4_800 * 2];
let n = stream.read(&mut buf)?;
println!("captured {n} samples");
# Ok::<(), oxisound_core::OxiSoundError>(())
```

## API Overview

### `PulseDevice`

Implements [`oxisound_core::AudioDevice`](../oxisound-core). Each device owns one connection
(one socket, one reactor thread); streams opened from it share that connection.

| Method | Description |
|--------|-------------|
| `enumerate()` | Every usable sink and source as `DeviceInfo` |
| `enumerate_output()` / `enumerate_input()` | Filtered by role |
| `enumerate_details(client_name)` | Raw `PulseDeviceDescriptor`s, including the monitor flag and description |
| `default_output()` / `default_input()` | The server's default sink / source |
| `open_default(role, client_name)` | Default endpoint with a custom client name |
| `open_named(name, role, client_name)` | A specific sink or source by server-side name |
| `open_output_concrete` / `open_input_concrete` / `open_duplex_concrete` | Concrete stream types |
| `open_output` / `open_input` / `open_duplex` | `Box<dyn …>` trait objects (`AudioDevice`) |
| `negotiate_output(config)` | Predicts the negotiated config without a round-trip |
| `descriptor()` / `device_info()` / `server_info()` / `host_api()` | Introspection |

### Streams

`PulseOutputStream` implements `OutputStream`, `PulseInputStream` implements `InputStream`, and
`PulseDuplexStream` implements `DuplexStream`. All three are `Send`.

| Method | Output | Input | Duplex |
|--------|:------:|:-----:|:------:|
| `write(&[f32])` / `read(&mut [f32])` | `write` | `read` | both |
| `stats()` | yes | yes | yes |
| `negotiated()` / `wire_format()` / `requested_buffer_attrs()` | yes | yes | via halves |
| `latency_frames()` / `ring_capacity_bytes()` | yes | yes | `roundtrip_latency_frames()` |
| `underrun_count()` | yes | — | via `output()` |
| `overrun_count()` | — | yes | via `input()` |
| `is_disconnected()` | yes | yes | yes |
| `flush()` / `drain()` | yes | — | via `output()` |
| `discard_buffered()` | yes | yes | via halves |
| `pause()` / `resume()` (cork/uncork) | yes | yes | via halves |

### Platform-independent helpers

These compile and are unit-tested on **every** host, not just Linux:

| Item | Description |
|------|-------------|
| `resolve_server_socket(env)` | `$PULSE_SERVER` → `$PULSE_RUNTIME_PATH` → `$XDG_RUNTIME_DIR` → `/run/user/<uid>` |
| `resolve_cookie_path(env)` / `load_cookie(path)` | `$PULSE_COOKIE` → `$XDG_CONFIG_HOME/pulse/cookie` → `$HOME/.config/pulse/cookie` → `$HOME/.pulse-cookie` |
| `EnvProvider` / `ProcessEnv` / `MapEnv` | Injectable environment, so tests never mutate process-global state |
| `PulseSampleFormat` + `negotiate_format` | `u8` / `s16le` / `s24le` / `s32le` / `f32le` and the `f32` ⇄ bytes conversion |
| `PulseDeviceDescriptor` / `map_descriptors` | Device model and the `DeviceInfo` mapping |
| `playback_buffer_attrs` / `record_buffer_attrs` / `ring_capacity_bytes` / `period_frames` | Buffer arithmetic |
| `validate_stream_config` | Protocol limits (`PULSE_MAX_CHANNELS`, `PULSE_MAX_SAMPLE_RATE`) |
| `block_on_timeout` / `WithDeadline` + the `PULSE_*_TIMEOUT` constants | Deadlines for every blocking call |

## Architecture

```text
  caller thread                         reactor thread (pulseaudio crate)
  ─────────────                         ─────────────────────────────────
  write(&[f32])
    └─ encode → HeapProd<u8> ──ring──▶ poll_read(&mut [u8]) ──▶ socket
       └─ Waker::wake() ─────────────▶ mio::Waker (re-enters write_streams)

                                        socket ──▶ RecordSink::write(&[u8])
  read(&mut [f32]) ◀──ring── HeapCons<u8> ◀───────────────┘
```

- **Server-clocked playback.** PulseAudio asks for bytes with a `REQUEST` command. When the
  client-side ring is empty the reactor-side source returns `Poll::Pending` and parks a
  `std::task::Waker` instead of injecting silence; the next `write()` wakes it. That is real
  back-pressure: an idle stream adds no phantom latency, and the server-side buffer stays bounded
  by the requested `tlength` (4 periods).
- **Underruns** are counted on the rising edge — a stream that stays starved reports one underrun,
  not one per request. **Overruns** are counted on the capture side when a server chunk does not
  fit in the ring. `poll_read` re-checks the ring after parking the waker, closing the
  write-between-pop-and-register window that would otherwise leave the reactor asleep on
  queued audio.
- **Connections are reclaimed, not parked.** The `pulseaudio` reactor loop blocks in an unbounded
  `poll()` whose waker it owns itself, so dropping the client handles cannot stop it.
  `oxisound-pulse` keeps a `try_clone`'d socket handle and calls `shutdown(2)` on it when the last
  reference drops; the reactor then reads EOF, returns, and its thread exits. Devices and both
  duplex halves share one connection guard, so a duplex pair still costs a single socket.
- **Every blocking call has a deadline.** Connect + handshake runs on a worker thread bounded by
  `PULSE_CONNECT_TIMEOUT` (5 s, since `connect(2)` on a unix socket is uninterruptible), the
  handshake is additionally bounded by socket read/write timeouts, and every control-plane
  round-trip goes through `block_on_timeout`. No call can block forever.
- **Alignment.** The server delivers arbitrary byte counts, so the capture ring can hold a partial
  sample; only whole samples are popped, and the remainder joins the next chunk.
- **Layering.** Only a thin protocol adapter is Linux-gated. Socket discovery, format negotiation
  and conversion, device mapping and buffer arithmetic are ordinary Rust, so they are unit-tested
  on every host — including macOS, where no PulseAudio server exists.

## Enumeration semantics

- Sinks map to `is_output`, sources (**including monitor sources**) map to `is_input`. Every
  emitted `DeviceInfo` has at least one role — the workspace invariant. PulseAudio's placeholder
  endpoints (zero channels or a zero sample rate: the `auto_null` dummy sink, a card whose profile
  is off) are skipped with a `log::debug!` note, because they cannot be opened in either direction.
- `DeviceInfo::name` is the **server-side name verbatim**, so it round-trips into
  `PulseDevice::open_named`. Monitor sources keep PulseAudio's own `.monitor` suffix and are
  additionally flagged by `PulseDeviceDescriptor::is_monitor` (`DeviceInfo` has no field for it).
- `sample_rates` and `channel_counts` carry the endpoint's **native** sample-spec values only.
  PulseAudio resamples, remixes and reformats server-side, so configurations outside those values
  open successfully too; `DeviceInfo::supports_config` deliberately under-reports rather than
  advertising capabilities the hardware does not have.

## Non-Linux targets

The `pulseaudio` dependency is declared under `[target.'cfg(target_os = "linux")'.dependencies]`.
On every other target the crate still compiles — as a Pure-Rust stub where each constructor returns
`OxiSoundError::Unsupported` — so downstream crates can depend on it unconditionally without
`#[cfg]` boilerplate. This mirrors the [`oxisound-jack`](../oxisound-jack) stub pattern.

## Relationship to `HostApi::PulseAudio`

`CpalDevice::with_host(HostApi::PulseAudio)` returns an error and will keep doing so: cpal has no
PulseAudio host to dispatch to, which is precisely why this crate exists. Use `PulseDevice`
directly, or the `oxisound` facade's `pulse_*` functions. `PulseDevice::host_api()` reports
`HostApi::PulseAudio` for parity.

## Scope

**Implemented:** server discovery and cookie authentication (including cookie-less same-uid
sockets, the usual `pipewire-pulse` setup); enumeration (`GET_SERVER_INFO`, `GET_SINK_INFO_LIST`,
`GET_SOURCE_INFO_LIST`) with monitor tagging and default detection; playback; capture; duplex on a
single connection; cork/uncork; flush/drain/discard; underrun and overrun counters.

**Not implemented (deliberate):**

- **SHM / memfd zero-copy transfer** — audio is streamed over the socket as plain memblocks. This
  costs a memory copy per chunk but keeps the crate free of shared-memory `unsafe`.
- **Volume and mute control**, module loading, and the subscription (hot-plug) event stream.
- **Remote (`tcp:`) servers** — `$PULSE_SERVER` entries naming a remote transport return
  `OxiSoundError::Unsupported` rather than silently falling back to the local server.

## Testing

Unit tests (57) plus one stub integration test — **58 under `cargo nextest run -p
oxisound-pulse`**, measured 2026-08-06 on macOS/aarch64 — and 17 doctests run on any host, and
cover socket/cookie resolution precedence, format conversion and negotiation, `DeviceInfo`
mapping and the role invariant, buffer arithmetic, timeout behaviour and the non-Linux stub
(all green). Linux adds 25 more that need no server — protocol-limit
cross-checks against the `pulseaudio` crate's own constants, error-taxonomy mapping, sink/source →
descriptor mapping, channel maps for all 32 supported counts, ring/waker/discard behaviour, and a
full `AUTH` / `SET_CLIENT_NAME` handshake plus the connection-shutdown regression over a
`UnixStream::pair()`.

`tests/live_server.rs` holds integration tests that need a running server; each one checks socket
reachability first and returns early with a `SKIP:` note when there is none, so the suite stays
green on machines without audio.

```bash
cargo nextest run -p oxisound-pulse --no-capture   # run on a Linux desktop session
```

## Cross-references

- **Traits & types:** [`oxisound-core`](../oxisound-core) — `AudioDevice`, `OutputStream`,
  `InputStream`, `DuplexStream`, `StreamConfig`, `OxiSoundError`.
- **Other backends:** [`oxisound-cpal`](../oxisound-cpal) (ALSA / CoreAudio / WASAPI via cpal),
  [`oxisound-jack`](../oxisound-jack) (libjack2 quarantine crate).
- **Facade:** [`oxisound`](../oxisound) — enable the `pulse` feature.

## License

Apache-2.0 — COOLJAPAN OU (Team Kitasan)
