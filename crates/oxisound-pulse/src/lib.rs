//! Pure-Rust PulseAudio backend for OxiSound — no `libpulse`, no `alsa-lib`, no C.
//!
//! `oxisound-pulse` talks the **PulseAudio native IPC protocol** directly over a unix
//! socket, using the pure-Rust [`pulseaudio`](https://crates.io/crates/pulseaudio) crate
//! for framing and tagstruct serialisation. Nothing in the audio path links a C library:
//! this is the crate that answers "Linux audio with no C in the path" for the OxiSound
//! workspace, where the default `oxisound-cpal` backend goes through `alsa-lib`.
//!
//! It works against:
//!
//! - **PulseAudio** — the native socket at `$XDG_RUNTIME_DIR/pulse/native`.
//! - **PipeWire** — via the `pipewire-pulse` compatibility service, which serves the same
//!   protocol on the same socket. No PipeWire-specific code is needed.
//!
//! ## Quick start
//!
//! ```no_run
//! use oxisound_core::{AudioDevice, OutputStream, StreamConfig};
//! use oxisound_pulse::PulseDevice;
//!
//! let device = PulseDevice::default_output()?;
//! let mut stream = device.open_output_concrete(StreamConfig::stereo_48k())?;
//! stream.write(&vec![0.0f32; 1024 * 2])?;
//! stream.drain()?;
//! # Ok::<(), oxisound_core::OxiSoundError>(())
//! ```
//!
//! ## Architecture
//!
//! | Layer | Module | Platform |
//! |-------|--------|----------|
//! | Socket & cookie discovery | [`mod@env`] | all |
//! | Wire formats, `f32` ⇄ bytes, negotiation | [`mod@format`] | all |
//! | Device model, buffer arithmetic, `DeviceInfo` mapping | [`mod@model`] | all |
//! | Deadlines for every blocking call | [`mod@timeout`] | all |
//! | Protocol adapter, streams | `linux` | Linux only |
//! | `Unsupported` stubs | `stub` | everything else |
//!
//! Only the thin adapter layer is Linux-gated, so the discovery rules, the mapping
//! rules — including the "every device has at least one role" invariant — the format
//! conversions and the buffer arithmetic are unit-tested on every host.
//!
//! ## Threading
//!
//! The `pulseaudio` client owns one reactor thread per connection; it is the only thread
//! touching the socket. OxiSound stream handles exchange bytes with it through lock-free
//! SPSC rings and never block on it. Playback is server-clocked: when the ring is empty
//! the reactor-side source parks a [`std::task::Waker`] instead of injecting silence, and
//! the next `write()` wakes it. Every control-plane round-trip runs under an explicit
//! deadline from [`mod@timeout`], so no call can block forever.
//!
//! ## Relationship to `HostApi::PulseAudio`
//!
//! `CpalDevice::with_host(HostApi::PulseAudio)` returns an error and will keep doing so:
//! cpal has no PulseAudio host to dispatch to, which is precisely why this crate exists.
//! Use [`PulseDevice`] directly, or the `oxisound` facade's `pulse_*` functions behind its
//! opt-in `pulse` feature. [`PulseDevice::host_api`] reports
//! [`HostApi::PulseAudio`](oxisound_core::HostApi::PulseAudio) for
//! parity.
//!
//! ## Non-Linux targets
//!
//! The `pulseaudio` dependency is declared under
//! `[target.'cfg(target_os = "linux")'.dependencies]`. Elsewhere the crate still compiles
//! — as a Pure-Rust stub whose constructors return
//! [`OxiSoundError::Unsupported`](oxisound_core::OxiSoundError::Unsupported) — so
//! downstream crates can depend on it unconditionally, exactly like `oxisound-jack`.
//!
//! ## Scope
//!
//! Implemented: server discovery and cookie auth, enumeration (server info, sink list,
//! source list) with monitor tagging, playback, capture, duplex, cork/uncork,
//! flush/drain/discard, underrun and overrun counters.
//!
//! Not implemented (deliberate, see the workspace `TODO.md`): SHM/memfd zero-copy
//! transfer — audio is streamed over the socket as plain memblocks; volume and mute
//! control; module loading; the subscription/hot-plug event stream.
#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod env;
pub mod format;
pub mod model;
pub mod timeout;

#[cfg(target_os = "linux")]
mod linux;

#[cfg(not(target_os = "linux"))]
mod stub;

#[cfg(target_os = "linux")]
pub use linux::{
    DEFAULT_CLIENT_NAME, PulseDevice, PulseDuplexStream, PulseInputStream, PulseOutputStream,
};

#[cfg(not(target_os = "linux"))]
pub use stub::{
    DEFAULT_CLIENT_NAME, PulseDevice, PulseDuplexStream, PulseInputStream, PulseOutputStream,
};

pub use env::{
    EnvProvider, MapEnv, PULSE_COOKIE_LENGTH, PULSE_NATIVE_SOCKET_RELATIVE, ProcessEnv,
    load_cookie, load_cookie_from_env, resolve_cookie_path, resolve_server_socket,
    unix_path_from_server_entry,
};
pub use format::{PulseSampleFormat, negotiate_format};
pub use model::{
    DEFAULT_PERIOD_FRAMES, DEFAULT_RING_CAPACITY_SECS, MAX_PERIOD_FRAMES, MIN_PERIOD_FRAMES,
    PLAYBACK_MAX_PERIODS, PLAYBACK_TARGET_PERIODS, PULSE_MAX_CHANNELS, PULSE_MAX_SAMPLE_RATE,
    PulseBufferAttrs, PulseDeviceDescriptor, PulseDeviceRole, map_descriptors, period_frames,
    playback_buffer_attrs, predict_negotiated, record_buffer_attrs, ring_capacity_bytes,
    validate_stream_config, whole_samples,
};
pub use timeout::{
    PULSE_CONNECT_TIMEOUT, PULSE_DRAIN_TIMEOUT, PULSE_FLUSH_TIMEOUT, PULSE_OP_TIMEOUT,
    WithDeadline, block_on_timeout,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_client_name_is_stable() {
        assert_eq!(DEFAULT_CLIENT_NAME, "oxisound");
    }

    /// The crate is usable as a drop-in `AudioDevice` on every target: on Linux it opens
    /// real streams, elsewhere it reports `Unsupported`. Either way the trait is
    /// implemented, so downstream generic code compiles unchanged.
    #[test]
    fn pulse_device_implements_audio_device_everywhere() {
        fn assert_audio_device<T: oxisound_core::AudioDevice>() {}
        assert_audio_device::<PulseDevice>();
    }

    #[test]
    fn enumeration_never_yields_a_role_less_device() {
        // The real enumeration path needs a server; the invariant itself is enforced by
        // `map_descriptors`, which is exercised here with the shapes PulseAudio produces.
        let descriptors = vec![
            PulseDeviceDescriptor {
                name: "sink".into(),
                description: None,
                index: 0,
                role: PulseDeviceRole::Sink,
                is_monitor: false,
                sample_rate: 48_000,
                channels: 2,
                format: Some(PulseSampleFormat::S16Le),
                is_default: true,
            },
            PulseDeviceDescriptor {
                name: "sink.monitor".into(),
                description: None,
                index: 1,
                role: PulseDeviceRole::Source,
                is_monitor: true,
                sample_rate: 48_000,
                channels: 2,
                format: Some(PulseSampleFormat::S16Le),
                is_default: false,
            },
            PulseDeviceDescriptor {
                name: "auto_null".into(),
                description: None,
                index: 2,
                role: PulseDeviceRole::Sink,
                is_monitor: false,
                sample_rate: 0,
                channels: 0,
                format: None,
                is_default: false,
            },
        ];
        let infos = map_descriptors(&descriptors);
        assert_eq!(infos.len(), 2);
        for info in &infos {
            assert!(
                info.is_input || info.is_output,
                "{} escaped enumeration with no role",
                info.name
            );
        }
    }
}
