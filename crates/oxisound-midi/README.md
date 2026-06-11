# oxisound-midi — MIDI device I/O for OxiSound

[![Crates.io](https://img.shields.io/crates/v/oxisound-midi.svg)](https://crates.io/crates/oxisound-midi)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

`oxisound-midi` implements the `MidiDevice`, `MidiInput`, and `MidiOutput` traits from [`oxisound-core`](../oxisound-core) on top of the [`midir`](https://crates.io/crates/midir) crate. It enumerates MIDI ports, opens input/output connections, and (on supported platforms) creates virtual MIDI ports. Platform selection is automatic: **CoreMIDI** on macOS/iOS, **WinMM/WinRT** on Windows, and the **ALSA sequencer** on Linux.

> **Pure-Rust status: platform backend (NOT Pure Rust).** `midir` binds the host OS's native MIDI subsystem through C-FFI: the **ALSA** sequencer + `libc` on Linux, **CoreMIDI** (`coremidi`) on macOS/iOS, and the **WinRT** path of the `windows` crate on Windows. These are auto-selected by `cfg(target_os)` and cannot be turned off — MIDI hardware access has no pure-Rust equivalent. Under the COOLJAPAN Pure Rust Policy this is a permitted OS-boundary backend, analogous to the CPAL and JACK audio backends. (Note: `midir`'s own optional `jack`/`jack-sys` feature is **not** enabled here, so libjack is never linked through this crate.) The crate's own code is `#![forbid(unsafe_code)]`.

## Installation

```toml
[dependencies]
oxisound-midi = "0.1.2"
```

This crate has no Cargo features; the platform MIDI backend is selected automatically at compile time.

## Quick Start

List MIDI ports and send a Note On to the first output port:

```rust,no_run
use oxisound_core::{MidiDevice, MidiMessage, MidiOutput};
use oxisound_midi::MidiDeviceImpl;

// Enumerate every MIDI device (input + output).
for d in MidiDeviceImpl::enumerate_midi()? {
    println!("{}: in={} out={}", d.name, d.is_input, d.is_output);
}

// Open output port 0 and play middle C.
let mut out = MidiDeviceImpl::open_midi_output(0)?;
out.send(&MidiMessage { status: 0x90, data: vec![60, 100], timestamp_micros: 0 })?;
# Ok::<(), oxisound_core::OxiSoundError>(())
```

Poll an input port for incoming messages:

```rust,no_run
use oxisound_core::{MidiDevice, MidiInput};
use oxisound_midi::MidiDeviceImpl;

let mut input = MidiDeviceImpl::open_midi_input(0)?;
while let Some(msg) = input.receive()? {
    println!("status=0x{:02X} data={:?}", msg.status, msg.data);
}
# Ok::<(), oxisound_core::OxiSoundError>(())
```

## API Overview

### `MidiDeviceImpl`

A zero-sized type implementing `oxisound_core::MidiDevice`. Each call creates a fresh `midir` client internally; for repeated port queries prefer [`MidiHost`](#midihost), which amortises client creation.

| Method | Description |
|--------|-------------|
| `enumerate_midi()` | List all MIDI devices as `Vec<MidiDeviceInfo>`. Input/output ports sharing a name are merged into a single in+out entry. Returns `Ok([])` on headless systems rather than failing |
| `open_midi_input(port)` | Open input port by index → `Box<dyn MidiInput>`; out-of-range index → `Device` error |
| `open_midi_output(port)` | Open output port by index → `Box<dyn MidiOutput>`; out-of-range index → `Device` error |

The returned `MidiInput` is non-blocking: `receive()` returns `Ok(None)` when no message is queued and `Err(Disconnected)` once the callback channel closes. The returned `MidiOutput` serialises each `MidiMessage` via `MidiMessage::to_bytes()` (adding F0/F7 framing for SysEx).

### `MidiHost`

Holds `midir` input and output clients for its lifetime, reducing per-call initialization overhead. Construct with `MidiHost::new()`.

| Method | Description |
|--------|-------------|
| `new()` | Initialize input + output clients; `Err(Device)` only if **both** are unavailable |
| `default()` | Infallible constructor (empty host on failure) |
| `input_port_names()` | All MIDI input port names (empty `Vec` if none) |
| `output_port_names()` | All MIDI output port names (empty `Vec` if none) |
| `create_virtual_input(name, FnMut(MidiMessage))` | Create a virtual input port (macOS / Linux); returns `Unsupported` on Windows |
| `create_virtual_output(name)` | Create a virtual output port (macOS / Linux); returns `Unsupported` on Windows |

Virtual ports are supported on macOS (CoreMIDI) and Linux (ALSA sequencer); WinMM does not support them, so both virtual-port methods return `OxiSoundError::Unsupported` on Windows.

### MIDI types (from `oxisound-core`)

These are re-used from [`oxisound-core`](../oxisound-core), not redefined here:

- `MidiDeviceInfo { name, is_input, is_output, port_count }`
- `MidiMessage { status, data, timestamp_micros }` — with `is_sysex()`, `sysex_payload()`, `new_sysex()`, `to_bytes()`
- `MidiInput::receive()`, `MidiOutput::send()`, `MidiDevice` (the trait `MidiDeviceImpl` implements)

SysEx framing is handled transparently: inbound raw bytes are parsed so that a `0xF0 … 0xF7` stream becomes a `MidiMessage` carrying only the payload (no framing bytes), and `to_bytes()` re-adds the framing on send.

## Errors

All fallible methods return `oxisound_core::OxiSoundError`. Notable mappings: an out-of-range or unopenable port → `Device`; a dropped input callback channel → `Disconnected`; virtual ports on Windows → `Unsupported`. See the [`oxisound-core` error table](../oxisound-core#oxisounderror-variants).

## Cross-references

- **Traits & types:** [`oxisound-core`](../oxisound-core) — `MidiDevice`, `MidiInput`, `MidiOutput`, `MidiMessage`, `MidiDeviceInfo`, `MidiClock`.
- **JACK MIDI:** [`oxisound-jack`](../oxisound-jack) exposes `JackMidiInput` / `JackMidiOutput` for low-latency MIDI over the JACK transport.
- **Facade:** [`oxisound`](../oxisound). Sibling crate `oxisound-smf` handles Standard MIDI File parsing.

## License

Apache-2.0 — COOLJAPAN OU (Team Kitasan)
