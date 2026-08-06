# oxisound-smf — Standard MIDI File (SMF) reader, writer, and player

[![Crates.io](https://img.shields.io/crates/v/oxisound-smf.svg)](https://crates.io/crates/oxisound-smf)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

`oxisound-smf` is a pure-Rust reader, writer, and player for Standard MIDI Files (`.mid`). It parses the full SMF chunk structure — the `MThd` header followed by one or more `MTrk` tracks — handling variable-length quantities, running status, channel messages, SysEx, and meta events (tempo, time/key signature, track name, end-of-track). It recognizes all three SMF format variants: format 0 (single track), format 1 (multi-track, shared timeline), and format 2 (multi-song). Unknown meta events are preserved verbatim so files survive a parse → write round-trip byte-for-byte.

Within the OxiSound ecosystem this crate is the offline song-data layer. It complements live MIDI device I/O ([`oxisound-midi`](../oxisound-midi)) and live network control ([`oxisound-osc`](../oxisound-osc)) by turning stored `.mid` files into a timed, chronologically merged event stream that can be played out through any [`oxisound_core::MidiOutput`]. A `TempoMap` converts tick positions into wall-clock seconds, correctly following mid-track tempo changes. The parser/writer core is `#![no_std]` (needs only `alloc`); the optional `std` feature adds real-time blocking playback via `SmfPlayer::play`. The crate is `#![forbid(unsafe_code)]` and depends only on `oxisound-core` and `log` — it is 100% Pure Rust.

## Installation

```toml
[dependencies]
oxisound-smf = "0.2.2"
```

Feature variants:

```toml
# no_std (parser/writer only — no real-time playback)
oxisound-smf = { version = "0.2.2", default-features = false }

# Enable serde derives on all data types
oxisound-smf = { version = "0.2.2", features = ["serde"] }
```

## Quick Start

Parse a minimal Format-0 SMF (one track, 480 ticks/beat, a single Note-On):

```rust
use oxisound_smf::{parse, SmfFormat};

let bytes: Vec<u8> = vec![
    0x4D, 0x54, 0x68, 0x64, 0x00, 0x00, 0x00, 0x06, // "MThd", len 6
    0x00, 0x00, 0x00, 0x01, 0x01, 0xE0,             // fmt 0, 1 track, 480 tpb
    0x4D, 0x54, 0x72, 0x6B, 0x00, 0x00, 0x00, 0x08, // "MTrk", len 8
    0x00, 0x90, 0x3C, 0x40,                         // delta 0, NoteOn C4 v64
    0x00, 0xFF, 0x2F, 0x00,                         // delta 0, EndOfTrack
];

let file = parse(&bytes)?;
assert_eq!(file.format, SmfFormat::SingleTrack);
assert_eq!(file.tracks.len(), 1);
# Ok::<(), oxisound_smf::SmfError>(())
```

### Round-trip: read, modify, write

```rust,no_run
use oxisound_smf::{parse, write_smf};

let data = std::fs::read("input.mid")?;
let smf = parse(&data)?;
// ... inspect or transform `smf.tracks` ...
let out = write_smf(&smf).expect("encode failed");
std::fs::write("output.mid", &out)?;
# Ok::<(), std::io::Error>(())
```

### Build a timed event stream and play it (`std` feature)

```rust,no_run
use oxisound_smf::{parse, SmfPlayer};

let data = std::fs::read("song.mid")?;
let smf = parse(&data).expect("parse failed");
let player = SmfPlayer::new(smf);

// Inspect every event with its wall-clock start time (seconds from the start).
for (secs, event) in player.events() {
    println!("{secs:>8.3}s  {event:?}");
}

// Or send each MIDI message to an output port in real time:
// let mut port = /* impl oxisound_core::MidiOutput */;
// player.play(&mut port)?;
# Ok::<(), std::io::Error>(())
```

## API Overview

### Free functions

| Function | Signature | Description |
|----------|-----------|-------------|
| `parse` | `fn(&[u8]) -> Result<SmfFile, SmfError>` | Parse a `.mid` byte stream into an `SmfFile` |
| `write_smf` | `fn(&SmfFile) -> Result<Vec<u8>, SmfError>` | Encode an `SmfFile` back to SMF bytes (auto-appends `EndOfTrack` if missing) |
| `ticks_to_seconds` | `fn(u64, u32, u16) -> f64` | Convert a tick duration to seconds, given `tempo_us` and `ticks_per_beat` |

### Data types

| Type | Kind | Description |
|------|------|-------------|
| `SmfFile` | struct | `format: SmfFormat`, `division: Division`, `tracks: Vec<SmfTrack>` |
| `SmfTrack` | struct | `name: Option<String>`, `events: Vec<TrackEvent>` |
| `TrackEvent` | struct | `delta_ticks: u32`, `event: SmfEvent` |
| `SmfFormat` | enum | `SingleTrack = 0`, `MultiTrack = 1`, `MultiSong = 2` |
| `Division` | enum | `TicksPerBeat(u16)` or `Smpte { fps: u8, subframes: u8 }` |
| `SmfEvent` | enum | One event kind in a track (see below) |
| `SmfError` | struct | Tuple error `SmfError(pub String)`; implements `Display` (+ `std::error::Error` under `std`) |

`SmfFormat` and `Division` derive `Debug, Clone, Copy, PartialEq, Eq`. `SmfFile`, `SmfTrack`, `TrackEvent`, and `SmfEvent` derive `Debug, Clone`. All seven data types gain `Serialize`/`Deserialize` under the `serde` feature.

### `SmfEvent` variants

| Variant | Payload | Description |
|---------|---------|-------------|
| `Midi(oxisound_core::MidiMessage)` | channel message | Note/CC/etc.; the channel nibble lives in `MidiMessage::status` |
| `Tempo(u32)` | microseconds/quarter note | Set-tempo meta event (`FF 51`) |
| `TimeSignature { numerator, denominator_pow2, clocks_per_click, notated_32nds_per_beat }` | meta | `denominator_pow2` is log₂ of the denominator (`2` → /4) |
| `KeySignature { sharps_flats, is_minor }` | meta | `sharps_flats`: negative = flats, positive = sharps |
| `TrackName(String)` | meta | Track/sequence name (`FF 03`) |
| `EndOfTrack` | — | Mandatory track terminator (`FF 2F 00`) |
| `UnknownMeta { meta_type, data }` | raw | Any other meta event, preserved for byte-exact round-trips |

### `SmfPlayer`

Merges every track into one chronologically sorted stream and assigns each event a wall-clock start time (in seconds) via an internally built `TempoMap`.

| Method | Description |
|--------|-------------|
| `SmfPlayer::new(file: SmfFile)` | Build a player; computes absolute seconds for every event and sorts by time |
| `events(&self)` | Iterate all `(f64, SmfEvent)` pairs in chronological order |
| `midi_events(&self)` | Iterate only MIDI channel/SysEx events as `(f64, &MidiMessage)`, skipping meta events |
| `play(&mut dyn MidiOutput)` *(std only)* | Blocking real-time playback: sleeps between events per their timestamps and sends each MIDI message |

### `TempoMap`

| Item | Description |
|------|-------------|
| `TempoMap::from_file(&SmfFile)` | Scan all tracks for `Tempo` events; defaults to 120 BPM (500 000 µs/beat) when none is present |
| `tick_to_secs(&self, tick: u64) -> f64` | Convert an absolute tick to wall-clock seconds, following every mid-track tempo change |
| `TempoEntry { tick: u64, tempo_us: u32 }` | One recorded tempo change (field of the map) |

For SMPTE-divided files the tempo map is inert and `tick_to_secs` returns `0.0` (SMPTE uses a fixed frame rate rather than beat-relative tempo).

## Feature Flags

| Feature | Default | Description |
|---------|:-------:|-------------|
| `std` | ✓ | Enables `std`; adds `SmfPlayer::play` (real-time blocking playback via `thread::sleep`) |
| `serde` | — | Derives `Serialize`/`Deserialize` on all data types (also enables `oxisound-core/serde`) |

## Errors

All fallible operations return `SmfError(pub String)`. Representative parse-time conditions:

| Condition | Cause |
|-----------|-------|
| Missing `MThd` magic | The stream does not begin with a valid SMF header chunk |
| Bad header length | `MThd` chunk length is not the required 6 |
| Unknown format word | Header format is not 0, 1, or 2 |
| Truncated chunk | Fewer bytes remain than a chunk or event declares |
| Empty MIDI bytes (write) | A `MidiMessage` serialized to zero bytes during `write_smf` |

Under the `std` feature, `SmfError` also implements `std::error::Error`.

## Related crates

- [`oxisound-midi`](../oxisound-midi) — live MIDI device I/O; the playback target for `SmfPlayer::play`
- [`oxisound-osc`](../oxisound-osc) — Open Sound Control codec and UDP transport (live network control)
- [`oxisound-core`](../oxisound-core) — shared types, including the `MidiMessage` and `MidiOutput` used here
- [`oxisound`](../oxisound) — the OxiSound facade; re-exports these SMF types under its `smf` feature and adds `load_smf` / `play_smf` helpers

## License

Apache-2.0 — COOLJAPAN OU (Team Kitasan)
