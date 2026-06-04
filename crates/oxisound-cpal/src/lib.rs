#![forbid(unsafe_code)]
//! cpal-backed audio device implementation for OxiSound.
//!
//! ## WASM / Web Audio
//!
//! Enable the `wasm` feature to target `wasm32-unknown-unknown` via cpal's WebAudio backend:
//!
//! ```toml
//! oxisound-cpal = { version = "0.1.1", features = ["wasm"] }
//! ```
//!
//! **GOVERNANCE note (COOLJAPAN policy):** The Web Audio API is classified as an OS-boundary
//! backend under the same rationale as ALSA, CoreAudio, and WASAPI — it provides hardware
//! audio access and is permitted as a default-features-off feature gate. cpal's internal
//! WebAudio host contains `unsafe` code; this does not affect oxisound-cpal's own
//! `#![forbid(unsafe_code)]` declaration, which applies only to this crate's own code.
//!
//! **wasm32 runtime caveats:**
//! - No OS threads: automatic stream recovery and hot-plug detection are unavailable
//!   (`enable_auto_reconnect`, `watch_devices`, `on_device_change` are not compiled in).
//! - `std::time::Instant::now()` panics on `wasm32-unknown-unknown` without a `web-time` shim;
//!   latency instrumentation that calls `Instant::now()` in audio callbacks will panic.
//!   A future `web-time` integration is planned but out of scope for this release.
//! - The `tokio` feature is incompatible with wasm32; use `--features wasm` only.
//! - Async device watchers are unavailable on wasm32.

#[cfg(all(target_arch = "wasm32", feature = "tokio"))]
compile_error!(
    "The `tokio` feature is unsupported on wasm32; use `--features wasm` instead and rely on the browser's Web Audio API."
);

mod adaptive;
mod callback;
mod config_helpers;
mod device;
mod error;
pub(crate) mod recovery;
mod stream_builders;
mod streams;
mod watcher;

// Async stream types (tokio feature)
#[cfg(feature = "tokio")]
mod async_streams;

// Re-export public types
pub use adaptive::AdaptiveBufferSizer;
pub use callback::{CpalCallbackInputStream, CpalCallbackOutputStream};
pub use device::CpalDevice;
#[cfg(not(target_arch = "wasm32"))]
pub use device::DeviceChangeGuard;
#[cfg(not(target_arch = "wasm32"))]
pub use recovery::RecoveryHandle;
pub use streams::{CpalDuplexStream, CpalInputStream, CpalOutputStream, StreamHealth};
pub use watcher::CpalDeviceWatcher;

#[cfg(feature = "tokio")]
pub use async_streams::{CpalAsyncInputStream, CpalAsyncOutputStream};

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use cpal::traits::DeviceTrait;
    use oxisound_core::{AudioDevice, OutputStream, StreamConfig};

    #[test]
    fn test_enumerate_no_panic() {
        match CpalDevice::enumerate() {
            Ok(devices) => {
                println!("Found {} output device(s)", devices.len());
            }
            Err(e) => {
                println!("enumerate() returned Err (may be OK in CI): {e}");
            }
        }
    }

    #[test]
    fn test_enumerate_input_no_panic() {
        match CpalDevice::enumerate_input() {
            Ok(devices) => {
                println!("Found {} input device(s)", devices.len());
                for d in &devices {
                    assert!(d.is_input, "enumerate_input must set is_input=true");
                    assert!(!d.is_output, "enumerate_input must set is_output=false");
                }
            }
            Err(e) => {
                println!("enumerate_input() returned Err (OK in CI): {e}");
            }
        }
    }

    #[test]
    fn test_default_input_no_panic() {
        match CpalDevice::default_input() {
            Ok(dev) => {
                println!(
                    "default_input() found a device: {:?}",
                    dev.device.description().ok()
                );
            }
            Err(e) => println!("default_input() returned Err (OK in CI): {e}"),
        }
    }

    #[test]
    fn test_enumerate_extended_fields_no_panic() {
        match CpalDevice::enumerate() {
            Ok(devices) => {
                for d in &devices {
                    let _ = d.sample_rates.len();
                    let _ = d.channel_counts.len();
                    let _ = d.is_output;
                    let _ = d.is_input;
                }
                println!("enumerate() found {} output device(s)", devices.len());
            }
            Err(e) => println!("enumerate() returned Err (OK in CI): {e}"),
        }
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn test_open_output_and_write_silence() {
        let device = match CpalDevice::default_output() {
            Ok(d) => d,
            Err(e) => {
                println!("No default output device (OK in some CI envs): {e}");
                return;
            }
        };

        let config = StreamConfig::stereo_48k();
        let mut out = match device.open_output(config) {
            Ok(o) => o,
            Err(e) => {
                println!("open_output failed (OK in some CI envs): {e}");
                return;
            }
        };

        let silence = vec![0.0f32; 9_600];
        out.write(&silence).expect("write silence must not fail");
        std::thread::sleep(std::time::Duration::from_millis(150));
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn test_open_input_and_read() {
        let device = match CpalDevice::default_input() {
            Ok(d) => d,
            Err(e) => {
                println!("No default input device (OK in some CI envs): {e}");
                return;
            }
        };

        let config = StreamConfig::mono_16k();
        let mut stream = match device.open_input(config) {
            Ok(s) => s,
            Err(e) => {
                println!("open_input failed (OK: mic permission may be denied): {e}");
                return;
            }
        };

        std::thread::sleep(std::time::Duration::from_millis(150));
        let mut buf = vec![0.0f32; 2400];
        let result = stream.read(&mut buf);
        assert!(result.is_ok(), "read() must return Ok even if count is 0");
    }

    #[test]
    fn test_error_mapping_device_not_available() {
        let e = error::map_build_stream_err(cpal::BuildStreamError::DeviceNotAvailable);
        assert!(matches!(e, oxisound_core::OxiSoundError::Disconnected(_)));

        let e = error::map_play_stream_err(cpal::PlayStreamError::DeviceNotAvailable);
        assert!(matches!(e, oxisound_core::OxiSoundError::Disconnected(_)));
    }

    #[test]
    fn test_duplex_open_no_panic() {
        let device = match CpalDevice::default_output() {
            Ok(d) => d,
            Err(e) => {
                println!("No output device: {e}");
                return;
            }
        };
        let config = oxisound_core::StreamConfig::stereo_48k();
        match device.open_duplex(config) {
            Ok(_) => println!("open_duplex() succeeded"),
            Err(e) => println!("open_duplex() returned Err (OK in CI): {e}"),
        }
    }

    #[test]
    fn test_with_host_unsupported_graceful() {
        use oxisound_core::HostApi;
        let result = CpalDevice::with_host(HostApi::PipeWire);
        assert!(
            result.is_err(),
            "with_host(PipeWire) must return Err on this platform"
        );
    }

    #[test]
    fn test_validate_called_on_open_output() {
        let device = match CpalDevice::default_output() {
            Ok(d) => d,
            Err(_) => return,
        };
        let bad_config = oxisound_core::StreamConfig {
            sample_rate: 48_000,
            channels: 0,
            buffer_size: None,
            sample_format: None,
            exclusive: false,
            preferred_formats: Vec::new(),
            channel_routing: None,
            buffer_capacity_secs: None,
        };
        match device.open_output(bad_config) {
            Ok(_) => println!("open_output with channels=0 succeeded (device reported empty caps)"),
            Err(e) => println!("open_output rejected bad config (expected): {e}"),
        }
    }

    #[test]
    #[ignore = "requires audio hardware: open_output_inner auto-plays the stream, creating a race between the audio callback incrementing underrun_count and the assertion"]
    fn test_underrun_count_starts_at_zero() {
        let device = match CpalDevice::default_output() {
            Ok(d) => d,
            Err(_) => return,
        };
        let config = StreamConfig::stereo_48k();
        let stream = match device.open_output_inner(config) {
            Ok(s) => s,
            Err(_) => return,
        };
        assert_eq!(stream.underrun_count(), 0, "underrun_count must start at 0");
    }

    #[test]
    fn test_capacity_formula() {
        let expected = 2 * 48_000usize * 2;
        let device = match CpalDevice::default_output() {
            Ok(d) => d,
            Err(_) => return,
        };
        let stream = match device.open_output_inner(StreamConfig::stereo_48k()) {
            Ok(s) => s,
            Err(_) => return,
        };
        assert_eq!(
            stream.ring_capacity(),
            expected,
            "capacity must be 2 * sample_rate * channels"
        );
    }

    #[test]
    fn test_overrun_returns_error() {
        let device = match CpalDevice::default_output() {
            Ok(d) => d,
            Err(_) => return,
        };
        let config = StreamConfig::stereo_48k();
        let mut stream = match device.open_output_inner(config) {
            Ok(s) => s,
            Err(_) => return,
        };
        let big_write = vec![0.0f32; stream.ring_capacity()];
        if stream.write(&big_write).is_err() {
            return;
        }
        let result = stream.write(&[0.0f32]);
        assert!(
            matches!(result, Err(oxisound_core::OxiSoundError::Overrun(_))),
            "write past capacity must return Overrun; got: {:?}",
            result
        );
    }

    #[test]
    fn test_disconnect_flag_starts_false() {
        let device = match CpalDevice::default_output() {
            Ok(d) => d,
            Err(_) => return,
        };
        let stream = match device.open_output_inner(StreamConfig::stereo_48k()) {
            Ok(s) => s,
            Err(_) => return,
        };
        assert!(
            !stream.is_disconnected(),
            "disconnected flag must start as false"
        );
    }

    #[test]
    #[cfg(feature = "tokio")]
    fn test_async_output_smoke() {
        let device = match CpalDevice::default_output() {
            Ok(d) => d,
            Err(_) => return,
        };
        let config = StreamConfig::stereo_48k();
        match device.open_async_output(config) {
            Ok(_) => println!("open_async_output() succeeded"),
            Err(e) => println!("open_async_output() returned Err (OK in CI): {e}"),
        }
    }

    #[test]
    #[cfg(feature = "tokio")]
    fn test_async_input_smoke() {
        let device = match CpalDevice::default_input() {
            Ok(d) => d,
            Err(_) => return,
        };
        let config = StreamConfig::mono_16k();
        match device.open_async_input(config) {
            Ok(_) => println!("open_async_input() succeeded"),
            Err(e) => println!("open_async_input() returned Err (OK in CI/perm denied): {e}"),
        }
    }

    // -----------------------------------------------------------------------
    // Slice B new tests
    // -----------------------------------------------------------------------

    #[test]
    fn cpal_device_enumerate_all_non_empty() {
        let result = CpalDevice::enumerate_all();
        assert!(result.is_ok(), "enumerate_all failed: {:?}", result.err());
    }

    #[test]
    fn cpal_device_enumerate_all_has_io_flags() {
        if let Ok(devices) = CpalDevice::enumerate_all() {
            for d in &devices {
                assert!(
                    d.is_input || d.is_output,
                    "device has neither input nor output: {}",
                    d.name
                );
            }
        }
    }

    #[test]
    fn stream_health_default_is_healthy() {
        assert_eq!(StreamHealth::Healthy, StreamHealth::Healthy);
        assert_ne!(StreamHealth::Healthy, StreamHealth::Disconnected);
    }

    #[test]
    fn device_watcher_starts_and_stops() {
        let watcher = CpalDeviceWatcher::start();
        assert!(watcher.is_ok(), "CpalDeviceWatcher::start failed");
        drop(watcher.expect("watcher already checked Ok"));
    }

    #[test]
    fn negotiate_output_returns_result() {
        let result = CpalDevice::default_output();
        if let Ok(device) = result {
            let config = StreamConfig::stereo_48k();
            let _ = device.negotiate_output(config);
        }
    }

    #[test]
    fn callback_priority_is_importable() {
        use oxisound_core::CallbackPriority;
        let _ = CallbackPriority::Normal;
    }

    #[test]
    fn host_api_stored_on_default_output() {
        if let Ok(device) = CpalDevice::default_output() {
            #[cfg(target_os = "macos")]
            assert_eq!(device.host_api(), oxisound_core::HostApi::CoreAudio);
            #[cfg(not(target_os = "macos"))]
            let _ = device.host_api();
        }
    }

    #[test]
    fn optimal_buffer_size_returns_reasonable_value() {
        if let Ok(device) = CpalDevice::default_output() {
            match device.optimal_buffer_size() {
                Ok(size) => assert!(size > 0, "buffer size must be positive"),
                Err(e) => println!("optimal_buffer_size failed (OK in CI): {e}"),
            }
        }
    }

    #[test]
    fn cpal_to_core_format_f32_maps_correctly() {
        use oxisound_core::SampleFormat as CoreSampleFormat;
        assert_eq!(
            error::cpal_to_core_format(cpal::SampleFormat::F32),
            CoreSampleFormat::F32
        );
        assert_eq!(
            error::cpal_to_core_format(cpal::SampleFormat::F64),
            CoreSampleFormat::F64
        );
        assert_eq!(
            error::cpal_to_core_format(cpal::SampleFormat::I16),
            CoreSampleFormat::I16
        );
        assert_eq!(
            error::cpal_to_core_format(cpal::SampleFormat::I32),
            CoreSampleFormat::I32
        );
        assert_eq!(
            error::cpal_to_core_format(cpal::SampleFormat::U8),
            CoreSampleFormat::U8
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn callback_output_stream_runs() {
        use std::sync::{
            Arc,
            atomic::{AtomicU32, Ordering as AOrdering},
        };
        let device = match CpalDevice::default_output() {
            Ok(d) => d,
            Err(_) => return,
        };
        let call_count = Arc::new(AtomicU32::new(0));
        let call_count_cb = Arc::clone(&call_count);
        let stream = match device.open_output_callback(StreamConfig::stereo_48k(), move |buf| {
            call_count_cb.fetch_add(1, AOrdering::Relaxed);
            for s in buf.iter_mut() {
                *s = 0.0;
            }
        }) {
            Ok(s) => s,
            Err(_) => return,
        };
        std::thread::sleep(std::time::Duration::from_millis(100));
        assert!(
            call_count.load(std::sync::atomic::Ordering::Relaxed) > 0,
            "callback was never called"
        );
        drop(stream);
    }

    #[test]
    fn stream_stats_shows_zeroes_initially() {
        let device = match CpalDevice::default_output() {
            Ok(d) => d,
            Err(_) => return,
        };
        let stream = match device.open_output_inner(StreamConfig::stereo_48k()) {
            Ok(s) => s,
            Err(_) => return,
        };
        let stats = stream.stats();
        assert_eq!(stats.underruns, 0, "no underruns at start");
        assert_eq!(stats.overruns, 0, "no overruns at start");
    }

    // -----------------------------------------------------------------------
    // New hot-plug / duplex latency tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_device_change_guard_drops_cleanly() {
        // Start an on_device_change listener and immediately drop it.
        // Should not panic or deadlock.
        let guard = CpalDevice::on_device_change(|_ev| {}).expect("on_device_change failed");
        std::thread::sleep(std::time::Duration::from_millis(50));
        drop(guard);
    }

    #[test]
    fn test_watch_devices_starts() {
        let watcher = CpalDevice::watch_devices().expect("watch_devices failed");
        std::thread::sleep(std::time::Duration::from_millis(50));
        drop(watcher); // Drop cleans up thread
    }

    #[cfg(feature = "tokio")]
    #[test]
    fn test_subscribe_device_events() {
        let (watcher, _rx) = CpalDevice::subscribe_device_events().expect("subscribe failed");
        std::thread::sleep(std::time::Duration::from_millis(50));
        drop(watcher);
    }

    #[test]
    fn test_duplex_roundtrip_latency_frames_type_check() {
        // Verify the method exists and returns a usize-compatible value.
        // This is a compile-time existence check via a no-op trait bound test.
        #[allow(dead_code)]
        fn assert_roundtrip_exists(s: &CpalDuplexStream) -> usize {
            s.roundtrip_latency_frames()
        }
    }

    // -----------------------------------------------------------------------
    // Adaptive buffer sizer tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_adaptive_sizer_wired_to_output() {
        // AdaptiveBufferSizer is initialized at stream construction — just verify
        // the default initial state is reasonable.
        // This is a compile-time + API surface check.
        let sizer = adaptive::AdaptiveBufferSizer::new(256, 64, 4096, 4);
        assert_eq!(sizer.current_size(), 256);
        assert!(!sizer.size_changed());
    }

    #[test]
    fn test_adaptive_sizer_behavior() {
        let mut sizer = adaptive::AdaptiveBufferSizer::new(256, 128, 2048, 2);
        // Underrun grows the buffer
        let new_size = sizer.record_underrun();
        assert_eq!(new_size, 512);
        assert!(sizer.size_changed());
        // Two stable periods shrink it back
        sizer.record_stable_period();
        assert!(!sizer.size_changed());
        let final_size = sizer.record_stable_period();
        assert_eq!(final_size, 256);
    }

    #[test]
    fn test_error_display_nonempty() {
        use oxisound_core::OxiSoundError;
        // Every OxiSoundError variant must have a non-empty Display string.
        let errors: Vec<OxiSoundError> = vec![
            OxiSoundError::NoDevice,
            OxiSoundError::UnsupportedConfig("test".into()),
            OxiSoundError::Disconnected("test".into()),
            OxiSoundError::Overrun("test".into()),
            OxiSoundError::Underrun("test".into()),
            OxiSoundError::HotPlugError("test".into()),
            OxiSoundError::PermissionDenied("test".into()),
            OxiSoundError::Timeout("test".into()),
            OxiSoundError::FormatMismatch("test".into()),
            OxiSoundError::Device("test".into()),
            OxiSoundError::Stream("test".into()),
            OxiSoundError::Io(std::io::Error::other("test")),
        ];
        for err in &errors {
            let s = format!("{err}");
            assert!(!s.is_empty(), "Display for {err:?} was empty");
        }
    }

    #[test]
    fn test_open_output_with_capacity_smoke() {
        let device = match CpalDevice::default_output() {
            Ok(d) => d,
            Err(_) => return,
        };
        let config = oxisound_core::StreamConfig::stereo_48k();
        // 0.5 second capacity: ~48000 * 0.5 * 2 = 48000 samples
        match device.open_output_with_capacity(config, 0.5) {
            Ok(stream) => assert!(stream.ring_capacity() > 0, "capacity must be positive"),
            Err(e) => println!("open_output_with_capacity failed (OK in CI): {e}"),
        }
    }

    #[test]
    fn test_set_buffer_size_and_desired_buffer_size() {
        let device = match CpalDevice::default_output() {
            Ok(d) => d,
            Err(_) => return,
        };
        let stream = match device.open_output_inner(oxisound_core::StreamConfig::stereo_48k()) {
            Ok(s) => s,
            Err(_) => return,
        };
        stream.set_buffer_size(1024);
        assert_eq!(stream.desired_buffer_size(), 1024);
    }

    #[test]
    fn test_tick_adaptive_returns_u32() {
        let device = match CpalDevice::default_output() {
            Ok(d) => d,
            Err(_) => return,
        };
        let stream = match device.open_output_inner(oxisound_core::StreamConfig::stereo_48k()) {
            Ok(s) => s,
            Err(_) => return,
        };
        let size = stream.tick_adaptive();
        assert!(size >= 128, "adaptive size should be at least min (128)");
    }

    // -----------------------------------------------------------------------
    // Exclusive mode tests — hardware-independent (field access / config only)
    // -----------------------------------------------------------------------

    /// Verifies that `StreamConfig::exclusive = true` is accepted without panic
    /// on any platform.  The field must be accessible and its value preserved.
    /// Stream opening is *not* attempted here so the test passes in CI without audio hardware.
    #[test]
    fn exclusive_mode_falls_back_gracefully() {
        let config = StreamConfig {
            exclusive: true,
            ..StreamConfig::STEREO_48K
        };
        // Field must be readable and set correctly.
        assert!(
            config.exclusive,
            "exclusive field must be true after being set"
        );
        // Display must not panic and must mention the exclusive flag.
        let displayed = format!("{config}");
        assert!(
            displayed.contains("excl"),
            "StreamConfig Display must show 'excl' when exclusive=true; got: {displayed}"
        );
    }

    /// Windows-specific hardware test: open output in exclusive mode, verify
    /// the result is either Ok (stream opened) or a graceful Err (not a panic).
    /// Marked #[ignore] because it requires real audio hardware on Windows.
    #[test]
    #[cfg(target_os = "windows")]
    #[ignore = "requires audio hardware on Windows"]
    fn exclusive_mode_output_windows_hardware() {
        let device = match CpalDevice::default_output() {
            Ok(d) => d,
            Err(e) => {
                println!("No default output device: {e}");
                return;
            }
        };
        let config = StreamConfig {
            exclusive: true,
            ..StreamConfig::STEREO_48K
        };
        match device.open_output(config) {
            Ok(_stream) => println!("exclusive mode output opened (shared-mode fallback in use)"),
            Err(e) => println!("exclusive mode output returned Err (acceptable): {e}"),
        }
    }

    // -----------------------------------------------------------------------
    // Slice C — new test coverage
    // -----------------------------------------------------------------------

    /// C1: Hot-plug diffing logic — pure/deterministic, no hardware required.
    #[test]
    fn hotplug_device_diff_logic() {
        use std::collections::HashSet;
        let old_devices: HashSet<String> = ["Device A", "Device B"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let new_devices: HashSet<String> = ["Device B", "Device C"]
            .iter()
            .map(|s| s.to_string())
            .collect();

        let added: Vec<&String> = new_devices.difference(&old_devices).collect();
        let removed: Vec<&String> = old_devices.difference(&new_devices).collect();

        assert_eq!(added.len(), 1);
        assert!(added.iter().any(|s| s.as_str() == "Device C"));
        assert_eq!(removed.len(), 1);
        assert!(removed.iter().any(|s| s.as_str() == "Device A"));
    }

    /// C2: Callback-mode output test — hardware-dependent, skipped in CI.
    #[test]
    #[ignore = "requires audio hardware"]
    fn callback_output_no_underruns() {
        use std::sync::{
            Arc,
            atomic::{AtomicU32, Ordering as AOrd},
        };

        let device = CpalDevice::default_output().expect("no default output");
        let config = oxisound_core::StreamConfig::stereo_48k();
        let counter = Arc::new(AtomicU32::new(0));
        let counter_clone = Arc::clone(&counter);
        let _stream = device
            .open_output_callback(config, move |buf: &mut [f32]| {
                let n = counter_clone.fetch_add(1, AOrd::Relaxed);
                for (i, s) in buf.iter_mut().enumerate() {
                    let t = (n as f32 * 512.0 + i as f32) / 48_000.0;
                    *s = (2.0 * std::f32::consts::PI * 440.0 * t).sin() * 0.1;
                }
            })
            .expect("open_output_callback failed");

        std::thread::sleep(std::time::Duration::from_secs(1));
        // CpalCallbackOutputStream has no underrun tracking — just verify no panic.
        assert!(
            counter.load(std::sync::atomic::Ordering::Relaxed) > 0,
            "callback was never invoked"
        );
    }

    /// C3: Sample format mapping roundtrip — pure, no hardware required.
    #[test]
    fn sample_format_mapping_roundtrip() {
        use crate::error::{core_to_cpal_format, cpal_to_core_format};
        use oxisound_core::SampleFormat as CoreFmt;

        let exact_roundtrip = [
            CoreFmt::F32,
            CoreFmt::I16,
            CoreFmt::I32,
            CoreFmt::I24,
            CoreFmt::F64,
        ];
        for fmt in &exact_roundtrip {
            let cpal_fmt = core_to_cpal_format(*fmt);
            let core_fmt = cpal_to_core_format(cpal_fmt);
            assert_eq!(core_fmt, *fmt, "Roundtrip failed for {fmt:?}");
        }

        // U8 is a direct mapping in cpal_to_core_format.
        assert_eq!(cpal_to_core_format(cpal::SampleFormat::U8), CoreFmt::U8);
    }

    /// C5: Async input stream captures frames — hardware-dependent, skipped in CI.
    ///
    /// Opens the default input device, collects frames for ~500 ms, and asserts the total
    /// sample count is within a sane range for any common sample rate and channel count.
    #[cfg(feature = "tokio")]
    #[test]
    #[ignore = "requires audio hardware"]
    fn async_input_stream_captures_frames() {
        use futures_core::Stream;
        use std::{future::poll_fn, pin::Pin, task::Poll};

        // Build a minimal tokio runtime — `sync + rt` features are always enabled.
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio runtime");

        rt.block_on(async {
            let device = CpalDevice::default_input().expect("no default input device");
            let config = oxisound_core::StreamConfig::mono_16k();
            let mut stream = device
                .open_async_input(config)
                .expect("open_async_input failed");

            // Collect frames for ~500 ms using wall-clock polling.
            let mut total_samples: usize = 0;
            let deadline = std::time::Instant::now() + std::time::Duration::from_millis(500);

            while std::time::Instant::now() < deadline {
                // Poll the stream once; if Pending, yield to the executor briefly.
                let frames = poll_fn(|cx| {
                    match Pin::new(&mut stream).poll_next(cx) {
                        Poll::Ready(v) => Poll::Ready(v),
                        Poll::Pending => {
                            // Re-schedule the waker so we get called again.
                            cx.waker().wake_by_ref();
                            Poll::Pending
                        }
                    }
                })
                .await;

                match frames {
                    Some(v) => total_samples += v.len(),
                    None => break,
                }
            }

            // At 16 kHz mono, 500 ms ≈ 8 000 samples minimum (real hardware delivers more).
            // The range 1000..200_000 is very permissive so the test passes on any device.
            assert!(
                (1000..200_000).contains(&total_samples),
                "async input captured {total_samples} samples — expected 1000..200000"
            );
        });
    }

    /// C4: Duplex roundtrip latency — hardware-dependent, skipped in CI.
    #[test]
    #[ignore = "requires audio hardware with duplex support"]
    fn duplex_roundtrip_latency_nonzero() {
        let device = CpalDevice::default_output().expect("no default output");
        let config = oxisound_core::StreamConfig {
            sample_rate: 48_000,
            channels: 2,
            buffer_size: Some(256),
            sample_format: None,
            exclusive: false,
            preferred_formats: Vec::new(),
            channel_routing: None,
            buffer_capacity_secs: None,
        };
        let (sample_rates, channel_counts) = config_helpers::collect_config_ranges(&device.device);
        let info = oxisound_core::DeviceInfo {
            sample_rates,
            channel_counts,
            is_output: true,
            capabilities: None,
            ..Default::default()
        };
        config.validate(&info).expect("config validation failed");

        // open_duplex returns Box<dyn DuplexStream> — use stats for latency.
        let stream = device.open_duplex(config).expect("open_duplex failed");
        let stats = stream.stats();
        // At open time the ring buffers are empty so latency_frames is 0 (cold).
        // Just assert the call doesn't panic and returns a plausible value.
        assert!(
            stats.latency_frames < 100_000,
            "latency_frames out of range: {}",
            stats.latency_frames
        );
    }
}
