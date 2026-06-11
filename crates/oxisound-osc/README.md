# oxisound-osc — Open Sound Control (OSC) codec and UDP transport

[![Crates.io](https://img.shields.io/crates/v/oxisound-osc.svg)](https://crates.io/crates/oxisound-osc)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

`oxisound-osc` is a pure-Rust implementation of the Open Sound Control protocol (OSC 1.0/1.1). It encodes and decodes OSC **messages** (an address pattern plus a list of typed arguments) and **bundles** (a time-tagged collection of nested packets), covering every standard OSC type tag — including the OSC 1.1 additions `T`/`F`/`N`/`I` and array `[ ]` grouping. Multi-byte values are big-endian and all strings/blobs are 4-byte aligned, exactly as the specification requires.

Within the OxiSound ecosystem this crate is the network-control layer: it lets audio applications send and receive control data (synth parameters, transport commands, sequencer triggers) to and from tools such as SuperCollider, Max/MSP, Pure Data, and TouchOSC. The codec core is `#![no_std]` (it only needs `alloc`), and the optional `std` feature adds a lightweight blocking UDP `OscSender`/`OscReceiver`. The whole crate is `#![forbid(unsafe_code)]` and has **zero non-`log` dependencies** — it is 100% Pure Rust.

## Installation

```toml
[dependencies]
oxisound-osc = "0.1.2"
```

The default `std` feature enables the UDP transport. For embedded or `no_std` targets, disable it and use the encode/decode functions directly:

```toml
[dependencies]
oxisound-osc = { version = "0.1.2", default-features = false }
```

## Quick Start

Build a message, encode it to bytes, and decode it back:

```rust
use oxisound_osc::{OscArg, OscMessage, OscPacket, encode, decode};

let msg = OscPacket::Message(OscMessage {
    address: "/synth/freq".to_string(),
    args: vec![OscArg::Float(440.0)],
});

let bytes = encode(&msg);
let decoded = decode(&bytes)?;
assert_eq!(decoded, msg);
# Ok::<(), oxisound_osc::OscError>(())
```

### Sending and receiving over UDP (`std` feature)

```rust,no_run
use oxisound_osc::{OscArg, OscReceiver, OscSender};

// Sender — binds an ephemeral local port, targets a fixed address.
let sender = OscSender::connect("127.0.0.1:57120")?;
sender.send_message("/synth/freq", vec![OscArg::Float(440.0)])?;

// Receiver — binds a socket and blocks for the next packet.
let receiver = OscReceiver::bind("0.0.0.0:57120")?;
let packet = receiver.recv()?;
println!("received: {packet:?}");
# Ok::<(), oxisound_osc::OscError>(())
```

### Time-tagged bundles

```rust
use oxisound_osc::{OscArg, OscBundle, OscMessage, OscPacket, OscTimeTag, encode, decode};

let bundle = OscPacket::Bundle(OscBundle {
    time: OscTimeTag::IMMEDIATE,
    elements: vec![
        OscPacket::Message(OscMessage { address: "/a".into(), args: vec![OscArg::Int(1)] }),
        OscPacket::Message(OscMessage { address: "/b".into(), args: vec![OscArg::Float(2.0)] }),
    ],
});

let bytes = encode(&bundle);
assert_eq!(decode(&bytes)?, bundle);
# Ok::<(), oxisound_osc::OscError>(())
```

## API Overview

### Free functions

| Function | Signature | Description |
|----------|-----------|-------------|
| `encode` | `fn(&OscPacket) -> Vec<u8>` | Serialize a packet (message or bundle) into a byte buffer. Infallible. |
| `decode` | `fn(&[u8]) -> Result<OscPacket, OscError>` | Parse a packet from a byte slice; recursively decodes nested bundle elements. |

### Core types

| Type | Kind | Description |
|------|------|-------------|
| `OscPacket` | enum | Top-level unit: `Message(OscMessage)` or `Bundle(OscBundle)` |
| `OscMessage` | struct | `address: String`, `args: Vec<OscArg>` |
| `OscBundle` | struct | `time: OscTimeTag`, `elements: Vec<OscPacket>` |
| `OscArg` | enum | A single typed argument value (see below) |
| `OscTimeTag` | struct | NTP-format time tag: `seconds: u32`, `fractional: u32` |
| `OscError` | struct | Tuple error `OscError(pub String)`; implements `Display` (+ `std::error::Error` under `std`) |

`OscPacket`, `OscMessage`, `OscBundle`, and `OscArg` derive `Debug`, `Clone`, and `PartialEq`. `OscTimeTag` additionally derives `Copy` and `Eq`.

### `OscArg` variants and type tags

`OscArg::type_tag(&self) -> char` returns the OSC type-tag character for each variant.

| Variant | Rust payload | Tag | Notes |
|---------|--------------|-----|-------|
| `Int(i32)` | `i32` | `i` | 32-bit big-endian integer |
| `Float(f32)` | `f32` | `f` | 32-bit big-endian float |
| `String(String)` | `String` | `s` | NUL-terminated, 4-byte padded |
| `Blob(Vec<u8>)` | `Vec<u8>` | `b` | Length-prefixed, 4-byte padded |
| `Long(i64)` | `i64` | `h` | 64-bit big-endian integer |
| `Double(f64)` | `f64` | `d` | 64-bit big-endian float |
| `TimeTag(OscTimeTag)` | `OscTimeTag` | `t` | NTP time tag |
| `Char(char)` | `char` | `c` | 4-byte big-endian code point |
| `Color(u8, u8, u8, u8)` | RGBA bytes | `r` | 32-bit RGBA color |
| `Midi([u8; 4])` | 4 raw bytes | `m` | MIDI message (port id, status, data1, data2) |
| `Bool(true)` | — | `T` | OSC 1.1; carries no payload bytes |
| `Bool(false)` | — | `F` | OSC 1.1; carries no payload bytes |
| `Nil` | — | `N` | OSC 1.1; carries no payload bytes |
| `Impulse` | — | `I` | OSC 1.1 "bang"; carries no payload bytes |
| `Array(Vec<OscArg>)` | nested args | `[` | Grouped via `[`…`]` in the type-tag string |

### `OscTimeTag`

| Item | Description |
|------|-------------|
| `OscTimeTag { seconds, fractional }` | NTP timestamp: seconds since 1900-01-01 plus a 32-bit fractional part |
| `OscTimeTag::IMMEDIATE` | The "execute immediately" sentinel (`seconds = 0`, `fractional = 1`) from OSC 1.0 §5.1 |

### UDP transport (requires `std`)

#### `OscSender`

| Method | Description |
|--------|-------------|
| `OscSender::connect(target: &str)` | Bind an ephemeral local UDP port and target `target` (e.g. `"127.0.0.1:57120"`) |
| `send(&self, packet: &OscPacket)` | Encode and transmit any packet |
| `send_message(&self, address: &str, args: Vec<OscArg>)` | Convenience: build and send a single message |

#### `OscReceiver`

| Method | Description |
|--------|-------------|
| `OscReceiver::bind(addr: &str)` | Bind a UDP socket to `addr` (e.g. `"0.0.0.0:57120"`) |
| `recv(&self)` | Block until a packet arrives, then decode it (64 KiB receive buffer) |
| `set_timeout(&self, timeout: Option<Duration>)` | Set the read timeout for `recv`; `None` blocks indefinitely |

## Feature Flags

| Feature | Default | Description |
|---------|:-------:|-------------|
| `std` | ✓ | Enables `std`; adds the `OscSender` and `OscReceiver` UDP transport types. Disable for `no_std` (codec still works via `alloc`). |

## Errors

The crate uses a single error type, `OscError(pub String)`, returned by `decode`, `OscSender`, and `OscReceiver`. Common decode-time conditions include:

| Condition | Cause |
|-----------|-------|
| Bad leading bytes | Packet does not begin with `'/'` (message) or `"#bundle\0"` (bundle) |
| Truncated data | Fewer bytes remain than a fixed-width argument requires |
| Unterminated string | No NUL terminator found in an OSC string |
| Invalid UTF-8 | An OSC string is not valid UTF-8 |
| Blob length overflow / overrun | A declared blob length exceeds the remaining buffer |
| Unmatched `[` / `]` | Malformed array grouping in the type-tag string |
| Unknown type tag | An unrecognized type-tag character |
| Invalid char code | A `c` argument is not a valid Unicode scalar |

Under the `std` feature, `OscError` also implements `std::error::Error`. UDP errors (bind/send/recv failures, address parse errors) are wrapped into `OscError` with a descriptive message.

## Related crates

- [`oxisound`](../oxisound) — the OxiSound facade; re-exports this crate's OSC types under its `osc` feature (e.g. `encode_osc`, `decode_osc`)
- [`oxisound-smf`](../oxisound-smf) — Standard MIDI File read/write, the file-based counterpart to OSC's live network control
- [`oxisound-midi`](../oxisound-midi) — live MIDI device I/O
- [`oxisound-core`](../oxisound-core) — shared device/stream traits and types

## License

Apache-2.0 — COOLJAPAN OU (Team Kitasan)
