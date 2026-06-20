# oxisound-session — Audio session management for OxiSound

[![Crates.io](https://img.shields.io/crates/v/oxisound-session.svg)](https://crates.io/crates/oxisound-session)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

`oxisound-session` provides platform audio session management for iOS and macOS. It wraps
`AVAudioSession` (via `objc2-avf-audio`) to set the session category and query microphone
recording permission. On all other platforms (Linux, Windows, Android) every function returns
a well-typed error (`UnsupportedConfig` or `PermissionDenied`) so downstream code can target
Apple and non-Apple platforms without `#[cfg]` guards.

> **Pure-Rust status (default features): 100% Pure Rust.** The `avf-audio` feature opt-in
> introduces Objective-C FFI via `objc2`, `objc2-avf-audio`, `objc2-foundation`, and `block2`.
> Under the COOLJAPAN Pure Rust Policy this is permitted as an OS-boundary backend gated
> behind a non-default feature. The crate's own code is `#![forbid(unsafe_code)]`.

## Installation

```toml
[dependencies]
# Pure-Rust stubs (macOS desktop returns Ok(()), other platforms return errors)
oxisound-session = "0.1.3"

# Enable real AVAudioSession calls on iOS / macOS (links objc2-avf-audio)
oxisound-session = { version = "0.1.3", features = ["avf-audio"] }
```

## Quick Start

```rust,no_run
use oxisound_session::{configure_session, request_microphone_permission, SessionCategory};

// Set the audio session category on iOS/macOS.
configure_session(SessionCategory::PlayAndRecord)?;

// Check (or request) microphone access.
let granted = request_microphone_permission()?;
if !granted {
    eprintln!("microphone access denied");
}
# Ok::<(), oxisound_core::OxiSoundError>(())
```

## API Overview

### Free functions

| Function | Description |
|----------|-------------|
| `configure_session(SessionCategory)` | Set the `AVAudioSession` category (iOS/macOS with `avf-audio`); no-op on macOS desktop without the feature; `UnsupportedConfig` on all other platforms |
| `request_microphone_permission()` | Query or request microphone recording permission. Uses the modern `AVAudioApplication` API (iOS 17+ / macOS 14+). Blocks the calling thread on iOS for up to 30 s, then returns `Timeout`. Reads TCC state on macOS without prompting |

### `SessionCategory`

Re-exported from [`oxisound-core`](../oxisound-core):

| Variant | AVAudioSession equivalent | Description |
|---------|--------------------------|-------------|
| `Playback` | `AVAudioSessionCategoryPlayback` | Output only; continues when the device is silenced |
| `Record` | `AVAudioSessionCategoryRecord` | Input only |
| `PlayAndRecord` | `AVAudioSessionCategoryPlayAndRecord` | Simultaneous input + output |
| `Ambient` | `AVAudioSessionCategoryAmbient` | Mixes with other apps; silences with the mute switch |
| `SoloAmbient` | `AVAudioSessionCategorySoloAmbient` | Exclusive; silences with the mute switch |

## Platform Behaviour

| Platform | `avf-audio` off | `avf-audio` on |
|----------|-----------------|----------------|
| macOS (CoreAudio desktop) | `Ok(())` with debug log | `Ok(())` with debug log |
| macOS / iOS (AVFoundation) | N/A | Real `AVAudioSession.setCategory:error:` call |
| iOS (no `avf-audio`) | `UnsupportedConfig` | N/A |
| Linux / Windows / Android | `UnsupportedConfig` | N/A |

## Feature Flags

| Feature | Default | Description |
|---------|:-------:|-------------|
| `avf-audio` | no | Enables the real `AVAudioSession` implementation via `objc2-avf-audio`. Links `objc2`, `objc2-foundation`, `objc2-avf-audio`, `block2`. Requires macOS 10.15+ or iOS 13+ SDK |

## Errors

All fallible functions return `oxisound_core::OxiSoundError`:

| Condition | Error variant |
|-----------|---------------|
| Not on Apple platform | `UnsupportedConfig` |
| `avf-audio` feature not enabled on iOS | `UnsupportedConfig` |
| AVAudioSession category set failure | `Device(String)` |
| Microphone permission denied | `PermissionDenied` |
| iOS permission dialog timed out (30 s) | `Timeout` |

## Cross-references

- **Facade:** [`oxisound`](../oxisound) — the `session` and `macos-session` features route `configure_session`/`request_microphone_permission` here
- **Traits & types:** [`oxisound-core`](../oxisound-core) — `SessionCategory`, `OxiSoundError`
- **Audio backends:** [`oxisound-cpal`](../oxisound-cpal) (CPAL), [`oxisound-jack`](../oxisound-jack) (JACK)

## License

Apache-2.0 — COOLJAPAN OU (Team Kitasan)
