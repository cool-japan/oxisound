//! CpalDevice implementation: struct, AudioDevice trait, enumeration helpers, and host API selection.

use cpal::SampleFormat;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
#[cfg(not(target_arch = "wasm32"))]
use oxisound_core::DeviceEvent;
use oxisound_core::{
    AudioDevice, DeviceInfo, DuplexStream, HostApi, InputStream, NegotiatedConfig, OutputStream,
    OxiSoundError, StreamConfig as OxiStreamConfig,
};
use ringbuf::{HeapRb, traits::Split};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
};

use crate::callback::{CpalCallbackInputStream, CpalCallbackOutputStream};
use crate::config_helpers::{
    build_device_capabilities, collect_config_ranges, collect_input_config_ranges,
    is_config_supported_for_input, is_config_supported_for_output, to_cpal_stream_config,
};
use crate::error::{
    core_to_cpal_format, cpal_to_core_format, map_build_stream_err, map_default_config_err,
    map_devices_err, map_play_stream_err,
};
use crate::stream_builders::{build_input_stream_typed, build_output_stream_typed};
use crate::streams::{CpalDuplexStream, CpalInputStream, CpalOutputStream};
#[cfg(not(target_arch = "wasm32"))]
use crate::watcher;

#[cfg(feature = "tokio")]
use crate::async_streams::{CpalAsyncInputStream, CpalAsyncOutputStream};

// ---------------------------------------------------------------------------
// DeviceChangeGuard
// ---------------------------------------------------------------------------

/// RAII guard that stops the device-change listener thread when dropped.
///
/// Not available on `wasm32` (no OS threads).
#[cfg(not(target_arch = "wasm32"))]
pub struct DeviceChangeGuard {
    pub(crate) stop: Arc<AtomicBool>,
    pub(crate) thread: Option<std::thread::JoinHandle<()>>,
}

#[cfg(not(target_arch = "wasm32"))]
impl Drop for DeviceChangeGuard {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

// ---------------------------------------------------------------------------
// WASAPI exclusive mode guard
// ---------------------------------------------------------------------------

/// Emits a `log::warn!` when `config.exclusive == true`.
///
/// cpal 0.17.3 hardcodes `AUDCLNT_SHAREMODE_SHARED` in both
/// `build_input_stream_raw_inner` and `build_output_stream_raw_inner`
/// (see cpal source: "this check has been removed until someone implements
/// WASAPI exclusive mode support").  Until a future cpal version exposes
/// an exclusive-mode path, we fall back to shared mode and log a warning
/// so callers are not silently surprised.
///
/// On non-Windows platforms, exclusive mode is a WASAPI-only concept and
/// is always silently ignored; a warning is still emitted so cross-platform
/// code can discover the mismatch during development.
fn warn_if_exclusive_requested(config: &OxiStreamConfig) {
    if !config.exclusive {
        return;
    }
    #[cfg(target_os = "windows")]
    log::warn!(
        "[oxisound-cpal] WASAPI exclusive mode requested but cpal 0.17.3 does not expose \
         an exclusive-mode stream builder (IAudioClient::Initialize is hardcoded to \
         AUDCLNT_SHAREMODE_SHARED). Falling back to shared mode. \
         Track upstream progress at https://github.com/RustAudio/cpal"
    );
    #[cfg(not(target_os = "windows"))]
    log::warn!(
        "[oxisound-cpal] Exclusive mode is a WASAPI-only feature (Windows); \
         it is not applicable on this platform and will be ignored."
    );
}

// ---------------------------------------------------------------------------
// Default host API detection (compile-time, platform-based)
// ---------------------------------------------------------------------------

pub(crate) fn default_host_api() -> HostApi {
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    return HostApi::CoreAudio;
    #[cfg(target_os = "windows")]
    return HostApi::Wasapi;
    #[cfg(any(
        target_os = "linux",
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "netbsd"
    ))]
    return HostApi::Alsa;
    #[cfg(not(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "windows",
        target_os = "linux",
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "netbsd"
    )))]
    return HostApi::CoreAudio;
}

// ---------------------------------------------------------------------------
// CpalDevice
// ---------------------------------------------------------------------------

/// A cpal-backed audio device.
pub struct CpalDevice {
    pub(crate) device: cpal::Device,
    pub(crate) host_api: HostApi,
}

impl CpalDevice {
    /// Returns the host API this device belongs to.
    pub fn host_api(&self) -> HostApi {
        self.host_api
    }

    /// Returns the device name.
    pub fn name(&self) -> Result<String, OxiSoundError> {
        self.device
            .description()
            .map(|d| d.name().to_owned())
            .map_err(|e| OxiSoundError::Device(e.to_string()))
    }

    /// Returns a latency estimate in milliseconds based on the device's default output config.
    /// This is a *device-level* estimate. Returns `Ok(0.0)` if the buffer size is unknown.
    pub fn default_output_latency_ms(&self) -> Result<f32, OxiSoundError> {
        let supported = self
            .device
            .default_output_config()
            .map_err(map_default_config_err)?;
        let config = supported.config();
        let sample_rate = config.sample_rate as f32;
        if sample_rate == 0.0 {
            return Ok(0.0);
        }
        let frames = match supported.buffer_size() {
            cpal::SupportedBufferSize::Range { min, .. } => *min as f32,
            cpal::SupportedBufferSize::Unknown => 0.0,
        };
        Ok(frames * 1000.0 / sample_rate)
    }

    /// Returns the device's recommended buffer size for output.
    /// Falls back to 512 if the buffer size is unknown.
    pub fn optimal_buffer_size(&self) -> Result<u32, OxiSoundError> {
        let config = self
            .device
            .default_output_config()
            .map_err(|e| OxiSoundError::Device(e.to_string()))?;
        match config.buffer_size() {
            cpal::SupportedBufferSize::Range { min, .. } => Ok(*min),
            cpal::SupportedBufferSize::Unknown => Ok(512),
        }
    }

    /// Finds the first output device whose name contains `name_fragment` (case-insensitive).
    pub fn select_output(name_fragment: &str) -> Result<Self, OxiSoundError> {
        let host = cpal::default_host();
        let devices = host.output_devices().map_err(map_devices_err)?;
        let fragment = name_fragment.to_lowercase();
        for dev in devices {
            let desc = dev
                .description()
                .map_err(|e| OxiSoundError::Device(e.to_string()))?;
            if desc.name().to_lowercase().contains(&fragment) {
                return Ok(CpalDevice {
                    device: dev,
                    host_api: default_host_api(),
                });
            }
        }
        Err(OxiSoundError::NoDevice)
    }

    /// Finds the first input device whose name contains `name_fragment` (case-insensitive).
    pub fn select_input(name_fragment: &str) -> Result<Self, OxiSoundError> {
        let host = cpal::default_host();
        let devices = host.input_devices().map_err(map_devices_err)?;
        let fragment = name_fragment.to_lowercase();
        for dev in devices {
            let desc = dev
                .description()
                .map_err(|e| OxiSoundError::Device(e.to_string()))?;
            if desc.name().to_lowercase().contains(&fragment) {
                return Ok(CpalDevice {
                    device: dev,
                    host_api: default_host_api(),
                });
            }
        }
        Err(OxiSoundError::NoDevice)
    }

    /// Construct a `CpalDevice` pointing at the default output device of a specific audio host.
    ///
    /// On `wasm32`, all host APIs except WebAudio are unsupported (always returns `Err`).
    #[cfg(target_arch = "wasm32")]
    pub fn with_host(_api: HostApi) -> Result<Self, OxiSoundError> {
        Err(OxiSoundError::UnsupportedConfig(
            "with_host() is not supported on wasm32; the WebAudio backend is selected automatically".into(),
        ))
    }

    /// Construct a `CpalDevice` pointing at the default output device of a specific audio host.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn with_host(api: HostApi) -> Result<Self, OxiSoundError> {
        let host_id = match api {
            #[cfg(any(target_os = "macos", target_os = "ios"))]
            HostApi::CoreAudio => cpal::HostId::CoreAudio,
            #[cfg(not(any(target_os = "macos", target_os = "ios")))]
            HostApi::CoreAudio => {
                return Err(OxiSoundError::UnsupportedConfig(
                    "CoreAudio is macOS/iOS-only".into(),
                ));
            }

            #[cfg(target_os = "windows")]
            HostApi::Wasapi => cpal::HostId::Wasapi,
            #[cfg(not(target_os = "windows"))]
            HostApi::Wasapi => {
                return Err(OxiSoundError::UnsupportedConfig(
                    "WASAPI is Windows-only".into(),
                ));
            }

            #[cfg(any(
                target_os = "linux",
                target_os = "dragonfly",
                target_os = "freebsd",
                target_os = "netbsd"
            ))]
            HostApi::Alsa => cpal::HostId::Alsa,
            #[cfg(not(any(
                target_os = "linux",
                target_os = "dragonfly",
                target_os = "freebsd",
                target_os = "netbsd"
            )))]
            HostApi::Alsa => {
                return Err(OxiSoundError::UnsupportedConfig(
                    "ALSA is Linux/BSD-only".into(),
                ));
            }

            HostApi::Jack => {
                return Err(OxiSoundError::UnsupportedConfig(
                    "JACK is not available via cpal in OxiSound; depend on the \
                     `oxisound-jack` quarantine crate (libjack2) directly"
                        .into(),
                ));
            }

            HostApi::Asio => {
                return Err(OxiSoundError::UnsupportedConfig(
                    "ASIO is not available in this build; it requires a dedicated \
                     `oxisound-*-asio` quarantine crate (Steinberg ASIO SDK)"
                        .into(),
                ));
            }

            HostApi::PipeWire => {
                return Err(OxiSoundError::UnsupportedConfig(
                    "PipeWire support is not available in this build".into(),
                ));
            }
            HostApi::PulseAudio => {
                return Err(OxiSoundError::UnsupportedConfig(
                    "PulseAudio support is not available in this build".into(),
                ));
            }
        };

        let host = cpal::host_from_id(host_id).map_err(|e| OxiSoundError::Device(e.to_string()))?;
        let device = host
            .default_output_device()
            .ok_or(OxiSoundError::NoDevice)?;
        Ok(CpalDevice {
            device,
            host_api: api,
        })
    }

    /// Returns all available audio devices (both input and output) in a single call.
    pub fn enumerate_all() -> Result<Vec<DeviceInfo>, OxiSoundError> {
        let host = cpal::default_host();
        let mut devices: Vec<DeviceInfo> = Vec::new();
        let all = host
            .devices()
            .map_err(|e| OxiSoundError::Device(e.to_string()))?;
        for device in all {
            let name = device
                .description()
                .map(|d| d.name().to_owned())
                .unwrap_or_else(|_| "<unknown>".into());
            let is_output = device
                .supported_output_configs()
                .map(|c| c.count() > 0)
                .unwrap_or(false);
            let is_input = device
                .supported_input_configs()
                .map(|c| c.count() > 0)
                .unwrap_or(false);
            let capabilities = if is_output {
                build_device_capabilities(&device, true)
            } else {
                build_device_capabilities(&device, false)
            };
            let (sample_rates, channel_counts) = if is_output {
                collect_config_ranges(&device)
            } else {
                collect_input_config_ranges(&device)
            };
            let info = DeviceInfo::builder(name)
                .output(is_output)
                .input(is_input)
                .sample_rates(sample_rates)
                .channel_counts(channel_counts)
                .capabilities(capabilities.unwrap_or_default())
                .build();
            devices.push(info);
        }
        // Mark defaults
        if let Some(def_out) = host.default_output_device()
            && let Ok(desc) = def_out.description()
        {
            let def_name = desc.name().to_owned();
            for d in &mut devices {
                if d.name == def_name && d.is_output {
                    d.is_default = true;
                }
            }
        }
        Ok(devices)
    }

    /// Enumerates available audio input devices on the default host.
    pub fn enumerate_input() -> Result<Vec<DeviceInfo>, OxiSoundError> {
        let host = cpal::default_host();
        let default_name = host
            .default_input_device()
            .and_then(|d| d.description().ok())
            .map(|desc| desc.name().to_owned());

        let mut devices = Vec::new();
        for device in host.input_devices().map_err(map_devices_err)? {
            let desc = device
                .description()
                .map_err(|e| OxiSoundError::Device(format!("description error: {e}")))?;
            let name = desc.name().to_owned();
            let is_default = default_name.as_deref() == Some(&name);

            let (sample_rates, channel_counts) = collect_input_config_ranges(&device);
            let capabilities = build_device_capabilities(&device, false);

            devices.push(DeviceInfo {
                name,
                is_default,
                sample_rates,
                channel_counts,
                is_input: true,
                is_output: false,
                capabilities,
            });
        }
        Ok(devices)
    }

    /// Attempts to open an output stream with exponential backoff on failure.
    ///
    /// Not available on `wasm32` (no OS threads / blocking sleep).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn open_output_with_retry(
        &self,
        config: OxiStreamConfig,
    ) -> Result<CpalOutputStream, OxiSoundError> {
        let delays = [10u64, 50, 200, 1_000];
        for delay_ms in delays.iter() {
            match self.open_output_inner(config.clone()) {
                Ok(stream) => return Ok(stream),
                Err(e) => {
                    log::warn!(
                        "open_output_with_retry: attempt failed ({e}), retrying in {delay_ms}ms..."
                    );
                    std::thread::sleep(std::time::Duration::from_millis(*delay_ms));
                }
            }
        }
        self.open_output_inner(config)
    }

    fn resolve_output_config(
        &self,
        _config: &OxiStreamConfig,
    ) -> Result<cpal::SupportedStreamConfig, OxiSoundError> {
        self.device
            .default_output_config()
            .map_err(map_default_config_err)
    }

    fn resolve_input_config(
        &self,
        _config: &OxiStreamConfig,
    ) -> Result<cpal::SupportedStreamConfig, OxiSoundError> {
        self.device
            .default_input_config()
            .map_err(map_default_config_err)
    }

    /// Opens a zero-copy output stream with a user-provided callback.
    pub fn open_output_callback(
        &self,
        config: OxiStreamConfig,
        mut callback: impl FnMut(&mut [f32]) + Send + 'static,
    ) -> Result<CpalCallbackOutputStream, OxiSoundError> {
        warn_if_exclusive_requested(&config);
        let supported = self.resolve_output_config(&config)?;
        let cpal_config = to_cpal_stream_config(&config, &supported);
        is_config_supported_for_output(&self.device, &cpal_config)?;

        let disconnect = Arc::new(AtomicBool::new(false));
        let disconnect_err = Arc::clone(&disconnect);
        let last_nanos = Arc::new(AtomicU64::new(0));
        #[cfg(not(target_arch = "wasm32"))]
        let last_nanos_cb = Arc::clone(&last_nanos);

        let stream = self
            .device
            .build_output_stream(
                cpal_config,
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    #[cfg(not(target_arch = "wasm32"))]
                    let t0 = std::time::Instant::now();
                    callback(data);
                    #[cfg(not(target_arch = "wasm32"))]
                    last_nanos_cb.store(t0.elapsed().as_nanos() as u64, Ordering::Relaxed);
                },
                move |e| {
                    log::error!("[oxisound-cpal] callback output stream error: {e}");
                    disconnect_err.store(true, Ordering::Relaxed);
                },
                None,
            )
            .map_err(map_build_stream_err)?;

        stream.play().map_err(map_play_stream_err)?;
        Ok(CpalCallbackOutputStream {
            stream,
            disconnect,
            last_callback_nanos: last_nanos,
        })
    }

    /// Opens a zero-copy input stream with a user-provided callback.
    pub fn open_input_callback(
        &self,
        config: OxiStreamConfig,
        mut callback: impl FnMut(&[f32]) + Send + 'static,
    ) -> Result<CpalCallbackInputStream, OxiSoundError> {
        warn_if_exclusive_requested(&config);
        let supported = self.resolve_input_config(&config)?;
        let cpal_config = to_cpal_stream_config(&config, &supported);
        is_config_supported_for_input(&self.device, &cpal_config)?;

        let disconnect = Arc::new(AtomicBool::new(false));
        let disconnect_err = Arc::clone(&disconnect);

        let stream = self
            .device
            .build_input_stream(
                cpal_config,
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    callback(data);
                },
                move |e| {
                    log::error!("[oxisound-cpal] callback input stream error: {e}");
                    disconnect_err.store(true, Ordering::Relaxed);
                },
                None,
            )
            .map_err(map_build_stream_err)?;

        stream.play().map_err(map_play_stream_err)?;
        Ok(CpalCallbackInputStream { stream, disconnect })
    }

    // -----------------------------------------------------------------------
    // Internal: open_output_inner — returns CpalOutputStream (not boxed)
    // -----------------------------------------------------------------------

    /// Opens the output device and returns a concrete [`CpalOutputStream`] (not boxed).
    ///
    /// This is the public concrete-returning variant used by the facade crate for
    /// features like `auto_reconnect_output` that need to inspect stream health directly.
    pub fn open_output_concrete(
        &self,
        config: OxiStreamConfig,
    ) -> Result<CpalOutputStream, OxiSoundError> {
        self.open_output_inner(config)
    }

    pub(crate) fn open_output_inner(
        &self,
        config: OxiStreamConfig,
    ) -> Result<CpalOutputStream, OxiSoundError> {
        let capacity_secs = config.buffer_capacity_secs.unwrap_or(2.0);
        self.open_output_inner_with_capacity(config, capacity_secs)
    }

    /// Opens an output stream with a custom ring buffer capacity.
    ///
    /// `capacity_secs` controls the maximum audio that can be buffered before an overrun.
    /// The default `open_output` uses ~2 seconds. Use smaller values (e.g., `0.5`) to reduce
    /// memory usage; larger values (e.g., `5.0`) for high-latency but glitch-resistant playback.
    ///
    /// # Errors
    ///
    /// Returns `OxiSoundError::UnsupportedConfig` if the config is invalid, or
    /// `OxiSoundError::Device` if the stream could not be opened.
    pub fn open_output_with_capacity(
        &self,
        config: OxiStreamConfig,
        capacity_secs: f32,
    ) -> Result<CpalOutputStream, OxiSoundError> {
        let capacity_secs = capacity_secs.clamp(0.1, 30.0);
        self.open_output_inner_with_capacity(config, capacity_secs)
    }

    fn open_output_inner_with_capacity(
        &self,
        config: OxiStreamConfig,
        capacity_secs: f32,
    ) -> Result<CpalOutputStream, OxiSoundError> {
        warn_if_exclusive_requested(&config);
        let (sample_rates, channel_counts) = collect_config_ranges(&self.device);
        let info = DeviceInfo {
            sample_rates,
            channel_counts,
            is_output: true,
            capabilities: None,
            ..Default::default()
        };
        config.validate(&info)?;

        // Use negotiate_output to honour preferred_formats ranking; fall back to
        // default_output_config when no preference is specified (fast path).
        let routing = config.channel_routing.clone();
        let supported = self
            .device
            .default_output_config()
            .map_err(map_default_config_err)?;

        let cpal_config = to_cpal_stream_config(&config, &supported);
        is_config_supported_for_output(&self.device, &cpal_config)?;

        // Determine the physical sample format to open the stream with, honouring
        // preferred_formats when non-empty.
        let open_format = if config.preferred_formats.is_empty() {
            supported.sample_format()
        } else {
            match self.negotiate_output(config.clone()) {
                Ok(neg) => core_to_cpal_format(neg.sample_format),
                Err(_) => supported.sample_format(),
            }
        };

        let channels = cpal_config.channels;
        let capacity = (capacity_secs * cpal_config.sample_rate as f32) as usize
            * usize::from(channels.max(1));
        let disconnected = Arc::new(AtomicBool::new(false));
        let underrun_count = Arc::new(AtomicU64::new(0));
        let frames_processed = Arc::new(AtomicU64::new(0));
        let callback_duration_ns = Arc::new(AtomicU64::new(0));
        let buffer_frames = config.buffer_size.unwrap_or(512) as u64;
        let sample_rate = cpal_config.sample_rate as u64;
        let buffer_period_ns = Arc::new(AtomicU64::new(
            (buffer_frames * 1_000_000_000_u64)
                .checked_div(sample_rate)
                .unwrap_or(0),
        ));

        let rb = HeapRb::<f32>::new(capacity);
        let (producer, consumer) = rb.split();

        let stream = match open_format {
            SampleFormat::F32 => build_output_stream_typed::<f32>(
                &self.device,
                &cpal_config,
                consumer,
                Arc::clone(&disconnected),
                Arc::clone(&underrun_count),
                Arc::clone(&frames_processed),
                channels,
                routing,
                Arc::clone(&callback_duration_ns),
            ),
            SampleFormat::I16 => build_output_stream_typed::<i16>(
                &self.device,
                &cpal_config,
                consumer,
                Arc::clone(&disconnected),
                Arc::clone(&underrun_count),
                Arc::clone(&frames_processed),
                channels,
                routing,
                Arc::clone(&callback_duration_ns),
            ),
            SampleFormat::U16 => build_output_stream_typed::<u16>(
                &self.device,
                &cpal_config,
                consumer,
                Arc::clone(&disconnected),
                Arc::clone(&underrun_count),
                Arc::clone(&frames_processed),
                channels,
                routing,
                Arc::clone(&callback_duration_ns),
            ),
            SampleFormat::I8 => build_output_stream_typed::<i8>(
                &self.device,
                &cpal_config,
                consumer,
                Arc::clone(&disconnected),
                Arc::clone(&underrun_count),
                Arc::clone(&frames_processed),
                channels,
                routing,
                Arc::clone(&callback_duration_ns),
            ),
            SampleFormat::I32 => build_output_stream_typed::<i32>(
                &self.device,
                &cpal_config,
                consumer,
                Arc::clone(&disconnected),
                Arc::clone(&underrun_count),
                Arc::clone(&frames_processed),
                channels,
                routing,
                Arc::clone(&callback_duration_ns),
            ),
            SampleFormat::I24 => build_output_stream_typed::<i32>(
                &self.device,
                &cpal_config,
                consumer,
                Arc::clone(&disconnected),
                Arc::clone(&underrun_count),
                Arc::clone(&frames_processed),
                channels,
                routing,
                Arc::clone(&callback_duration_ns),
            ),
            SampleFormat::F64 => build_output_stream_typed::<f64>(
                &self.device,
                &cpal_config,
                consumer,
                Arc::clone(&disconnected),
                Arc::clone(&underrun_count),
                Arc::clone(&frames_processed),
                channels,
                routing,
                Arc::clone(&callback_duration_ns),
            ),
            SampleFormat::U8 => build_output_stream_typed::<u8>(
                &self.device,
                &cpal_config,
                consumer,
                Arc::clone(&disconnected),
                Arc::clone(&underrun_count),
                Arc::clone(&frames_processed),
                channels,
                routing,
                Arc::clone(&callback_duration_ns),
            ),
            fmt => {
                return Err(OxiSoundError::UnsupportedConfig(format!(
                    "unsupported output sample format: {fmt:?}"
                )));
            }
        }?;

        stream.play().map_err(map_play_stream_err)?;

        let initial_buffer_size = config.buffer_size.unwrap_or(512);
        Ok(CpalOutputStream {
            stream,
            producer,
            channels,
            capacity,
            disconnected,
            underrun_count,
            frames_processed,
            #[cfg(not(target_arch = "wasm32"))]
            stream_start: std::time::Instant::now(),
            adaptive: Arc::new(std::sync::Mutex::new(
                crate::adaptive::AdaptiveBufferSizer::new(initial_buffer_size, 128, 8192, 4),
            )),
            last_tick_underruns: Arc::new(AtomicU64::new(0)),
            desired_buffer_size: Arc::new(AtomicU32::new(initial_buffer_size)),
            callback_duration_ns,
            buffer_period_ns,
            #[cfg(not(target_arch = "wasm32"))]
            reconnect_inner: None,
        })
    }

    // -----------------------------------------------------------------------
    // Internal: open_output_inner_with_shared_arcs — for stream recovery
    // -----------------------------------------------------------------------

    /// Opens an output stream, reusing existing shared `Arc` counters instead of allocating new ones.
    ///
    /// Used by the auto-reconnect recovery thread so that disconnect detection and stats remain
    /// continuous across reconnects.  All five Arcs (`disconnected`, `underrun_count`,
    /// `frames_processed`, `callback_duration_ns`, `buffer_period_ns`) are threaded into the new
    /// stream's audio and error callbacks, which write directly to the same Arcs the caller reads.
    ///
    /// A fresh ring buffer is always allocated (the capacity is derived from `config`).
    /// The returned `CpalOutputStream.producer` holds the new producer; the caller is responsible
    /// for swapping it into the shared `ReconnectInner.producer` slot.
    ///
    /// Not available on `wasm32`.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn open_output_inner_with_shared_arcs(
        &self,
        config: OxiStreamConfig,
        disconnected: Arc<AtomicBool>,
        underrun_count: Arc<AtomicU64>,
        frames_processed: Arc<AtomicU64>,
        callback_duration_ns: Arc<AtomicU64>,
        buffer_period_ns: Arc<AtomicU64>,
    ) -> Result<CpalOutputStream, OxiSoundError> {
        warn_if_exclusive_requested(&config);
        let (sample_rates, channel_counts) = collect_config_ranges(&self.device);
        let info = DeviceInfo {
            sample_rates,
            channel_counts,
            is_output: true,
            capabilities: None,
            ..Default::default()
        };
        config.validate(&info)?;

        let routing = config.channel_routing.clone();
        let supported = self
            .device
            .default_output_config()
            .map_err(map_default_config_err)?;

        let cpal_config = to_cpal_stream_config(&config, &supported);
        is_config_supported_for_output(&self.device, &cpal_config)?;

        let open_format = if config.preferred_formats.is_empty() {
            supported.sample_format()
        } else {
            match self.negotiate_output(config.clone()) {
                Ok(neg) => core_to_cpal_format(neg.sample_format),
                Err(_) => supported.sample_format(),
            }
        };

        let channels = cpal_config.channels;
        let capacity_secs = config.buffer_capacity_secs.unwrap_or(2.0).clamp(0.1, 30.0);
        let capacity = (capacity_secs * cpal_config.sample_rate as f32) as usize
            * usize::from(channels.max(1));

        // Update buffer_period_ns with new stream's parameters (sample rate / buffer size may differ).
        let buffer_frames = config.buffer_size.unwrap_or(512) as u64;
        let sample_rate = cpal_config.sample_rate as u64;
        buffer_period_ns.store(
            (buffer_frames * 1_000_000_000_u64)
                .checked_div(sample_rate)
                .unwrap_or(0),
            Ordering::Relaxed,
        );

        let rb = HeapRb::<f32>::new(capacity);
        let (producer, consumer) = rb.split();

        let stream = match open_format {
            SampleFormat::F32 => build_output_stream_typed::<f32>(
                &self.device,
                &cpal_config,
                consumer,
                Arc::clone(&disconnected),
                Arc::clone(&underrun_count),
                Arc::clone(&frames_processed),
                channels,
                routing,
                Arc::clone(&callback_duration_ns),
            ),
            SampleFormat::I16 => build_output_stream_typed::<i16>(
                &self.device,
                &cpal_config,
                consumer,
                Arc::clone(&disconnected),
                Arc::clone(&underrun_count),
                Arc::clone(&frames_processed),
                channels,
                routing,
                Arc::clone(&callback_duration_ns),
            ),
            SampleFormat::U16 => build_output_stream_typed::<u16>(
                &self.device,
                &cpal_config,
                consumer,
                Arc::clone(&disconnected),
                Arc::clone(&underrun_count),
                Arc::clone(&frames_processed),
                channels,
                routing,
                Arc::clone(&callback_duration_ns),
            ),
            SampleFormat::I8 => build_output_stream_typed::<i8>(
                &self.device,
                &cpal_config,
                consumer,
                Arc::clone(&disconnected),
                Arc::clone(&underrun_count),
                Arc::clone(&frames_processed),
                channels,
                routing,
                Arc::clone(&callback_duration_ns),
            ),
            SampleFormat::I32 => build_output_stream_typed::<i32>(
                &self.device,
                &cpal_config,
                consumer,
                Arc::clone(&disconnected),
                Arc::clone(&underrun_count),
                Arc::clone(&frames_processed),
                channels,
                routing,
                Arc::clone(&callback_duration_ns),
            ),
            SampleFormat::I24 => build_output_stream_typed::<i32>(
                &self.device,
                &cpal_config,
                consumer,
                Arc::clone(&disconnected),
                Arc::clone(&underrun_count),
                Arc::clone(&frames_processed),
                channels,
                routing,
                Arc::clone(&callback_duration_ns),
            ),
            SampleFormat::F64 => build_output_stream_typed::<f64>(
                &self.device,
                &cpal_config,
                consumer,
                Arc::clone(&disconnected),
                Arc::clone(&underrun_count),
                Arc::clone(&frames_processed),
                channels,
                routing,
                Arc::clone(&callback_duration_ns),
            ),
            SampleFormat::U8 => build_output_stream_typed::<u8>(
                &self.device,
                &cpal_config,
                consumer,
                Arc::clone(&disconnected),
                Arc::clone(&underrun_count),
                Arc::clone(&frames_processed),
                channels,
                routing,
                Arc::clone(&callback_duration_ns),
            ),
            fmt => {
                return Err(OxiSoundError::UnsupportedConfig(format!(
                    "unsupported output sample format: {fmt:?}"
                )));
            }
        }?;

        stream.play().map_err(map_play_stream_err)?;

        let initial_buffer_size = config.buffer_size.unwrap_or(512);
        Ok(CpalOutputStream {
            stream,
            producer,
            channels,
            capacity,
            disconnected,
            underrun_count,
            frames_processed,
            #[cfg(not(target_arch = "wasm32"))]
            stream_start: std::time::Instant::now(),
            adaptive: Arc::new(std::sync::Mutex::new(
                crate::adaptive::AdaptiveBufferSizer::new(initial_buffer_size, 128, 8192, 4),
            )),
            last_tick_underruns: Arc::new(AtomicU64::new(0)),
            desired_buffer_size: Arc::new(AtomicU32::new(initial_buffer_size)),
            callback_duration_ns,
            buffer_period_ns,
            #[cfg(not(target_arch = "wasm32"))]
            reconnect_inner: None,
        })
    }

    // -----------------------------------------------------------------------
    // Internal: open_input_inner — returns CpalInputStream (not boxed)
    // -----------------------------------------------------------------------

    pub(crate) fn open_input_inner(
        &self,
        config: OxiStreamConfig,
    ) -> Result<CpalInputStream, OxiSoundError> {
        warn_if_exclusive_requested(&config);
        let (sample_rates, channel_counts) = collect_input_config_ranges(&self.device);
        let info = DeviceInfo {
            sample_rates,
            channel_counts,
            is_input: true,
            capabilities: None,
            ..Default::default()
        };
        config.validate(&info)?;

        let supported = self
            .device
            .default_input_config()
            .map_err(map_default_config_err)?;

        let cpal_config = to_cpal_stream_config(&config, &supported);
        is_config_supported_for_input(&self.device, &cpal_config)?;

        let channels = cpal_config.channels;
        let capacity_secs = config.buffer_capacity_secs.unwrap_or(2.0);
        let capacity =
            (capacity_secs * cpal_config.sample_rate as f32 * f32::from(channels.max(1))) as usize;
        let disconnected = Arc::new(AtomicBool::new(false));
        let frames_processed = Arc::new(AtomicU64::new(0));
        let callback_duration_ns = Arc::new(AtomicU64::new(0));
        let buffer_frames = config.buffer_size.unwrap_or(512) as u64;
        let sample_rate = cpal_config.sample_rate as u64;
        let buffer_period_ns = Arc::new(AtomicU64::new(
            (buffer_frames * 1_000_000_000_u64)
                .checked_div(sample_rate)
                .unwrap_or(0),
        ));

        let rb = HeapRb::<f32>::new(capacity);
        let (producer, consumer) = rb.split();

        let stream = match supported.sample_format() {
            SampleFormat::F32 => build_input_stream_typed::<f32>(
                &self.device,
                &cpal_config,
                producer,
                Arc::clone(&disconnected),
                Arc::clone(&frames_processed),
                channels,
                Arc::clone(&callback_duration_ns),
            ),
            SampleFormat::I16 => build_input_stream_typed::<i16>(
                &self.device,
                &cpal_config,
                producer,
                Arc::clone(&disconnected),
                Arc::clone(&frames_processed),
                channels,
                Arc::clone(&callback_duration_ns),
            ),
            SampleFormat::U16 => build_input_stream_typed::<u16>(
                &self.device,
                &cpal_config,
                producer,
                Arc::clone(&disconnected),
                Arc::clone(&frames_processed),
                channels,
                Arc::clone(&callback_duration_ns),
            ),
            SampleFormat::I8 => build_input_stream_typed::<i8>(
                &self.device,
                &cpal_config,
                producer,
                Arc::clone(&disconnected),
                Arc::clone(&frames_processed),
                channels,
                Arc::clone(&callback_duration_ns),
            ),
            SampleFormat::I32 => build_input_stream_typed::<i32>(
                &self.device,
                &cpal_config,
                producer,
                Arc::clone(&disconnected),
                Arc::clone(&frames_processed),
                channels,
                Arc::clone(&callback_duration_ns),
            ),
            SampleFormat::I24 => build_input_stream_typed::<i32>(
                &self.device,
                &cpal_config,
                producer,
                Arc::clone(&disconnected),
                Arc::clone(&frames_processed),
                channels,
                Arc::clone(&callback_duration_ns),
            ),
            SampleFormat::F64 => build_input_stream_typed::<f64>(
                &self.device,
                &cpal_config,
                producer,
                Arc::clone(&disconnected),
                Arc::clone(&frames_processed),
                channels,
                Arc::clone(&callback_duration_ns),
            ),
            SampleFormat::U8 => build_input_stream_typed::<u8>(
                &self.device,
                &cpal_config,
                producer,
                Arc::clone(&disconnected),
                Arc::clone(&frames_processed),
                channels,
                Arc::clone(&callback_duration_ns),
            ),
            fmt => {
                return Err(OxiSoundError::UnsupportedConfig(format!(
                    "unsupported input sample format: {fmt:?}"
                )));
            }
        }?;

        stream.play().map_err(map_play_stream_err)?;

        Ok(CpalInputStream {
            stream,
            consumer,
            channels,
            capacity,
            disconnected,
            frames_processed,
            callback_duration_ns,
            buffer_period_ns,
        })
    }

    // -----------------------------------------------------------------------
    // Loopback capture
    // -----------------------------------------------------------------------

    /// Open a system audio loopback stream.
    ///
    /// On Linux with PulseAudio or PipeWire-ALSA, loopback capture is provided by
    /// `.monitor` input sources that appear as regular ALSA input devices. This
    /// method enumerates all input devices on the default host and opens the first
    /// one whose name contains `"monitor"` (case-insensitive).
    ///
    /// On other platforms (macOS, Windows, WASM) this returns
    /// [`OxiSoundError::Unsupported`] because cpal 0.17.3 does not expose a
    /// loopback API natively. macOS users can install BlackHole or SoundFlower
    /// and pass the virtual device name to [`AudioDevice::open_input`] directly.
    pub fn open_loopback(
        &self,
        config: OxiStreamConfig,
    ) -> Result<Box<dyn InputStream>, OxiSoundError> {
        #[cfg(target_os = "linux")]
        {
            let host = cpal::default_host();
            let monitor_device = host.input_devices().map_err(map_devices_err)?.find(|d| {
                d.description()
                    .map(|desc| desc.name().to_ascii_lowercase().contains("monitor"))
                    .unwrap_or(false)
            });

            match monitor_device {
                Some(device) => {
                    let loopback = CpalDevice {
                        device,
                        host_api: default_host_api(),
                    };
                    Ok(Box::new(loopback.open_input_inner(config)?))
                }
                None => Err(OxiSoundError::Unsupported(
                    "No loopback/monitor input device found. \
                     Ensure PulseAudio or PipeWire is running and a monitor source is present."
                        .into(),
                )),
            }
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = config;
            Err(OxiSoundError::Unsupported(
                "Loopback capture is not natively available on this platform. \
                 On macOS, install BlackHole or SoundFlower and open the virtual \
                 device via open_input(). On Windows, WASAPI loopback is not yet \
                 exposed via cpal 0.17.3."
                    .into(),
            ))
        }
    }

    // -----------------------------------------------------------------------
    // Async variants (tokio feature)
    // -----------------------------------------------------------------------

    #[cfg(feature = "tokio")]
    pub fn open_async_output(
        &self,
        config: OxiStreamConfig,
    ) -> Result<CpalAsyncOutputStream, OxiSoundError> {
        // warn_if_exclusive_requested is called by open_output_inner → open_output_inner_with_capacity
        Ok(CpalAsyncOutputStream {
            inner: self.open_output_inner(config)?,
        })
    }

    #[cfg(feature = "tokio")]
    pub fn open_async_input(
        &self,
        config: OxiStreamConfig,
    ) -> Result<CpalAsyncInputStream, OxiSoundError> {
        use tokio::sync::mpsc;

        warn_if_exclusive_requested(&config);
        let (sample_rates, channel_counts) = collect_input_config_ranges(&self.device);
        let info = DeviceInfo {
            sample_rates,
            channel_counts,
            is_input: true,
            capabilities: None,
            ..Default::default()
        };
        config.validate(&info)?;

        let supported = self
            .device
            .default_input_config()
            .map_err(map_default_config_err)?;

        let cpal_config = to_cpal_stream_config(&config, &supported);
        is_config_supported_for_input(&self.device, &cpal_config)?;

        let (tx, rx) = mpsc::unbounded_channel::<Vec<f32>>();
        let disconnected = Arc::new(AtomicBool::new(false));
        let disc_cb = Arc::clone(&disconnected);

        let stream = match supported.sample_format() {
            SampleFormat::F32 => {
                let sender = tx.clone();
                self.device
                    .build_input_stream(
                        cpal_config,
                        move |data: &[f32], _: &cpal::InputCallbackInfo| {
                            let _ = sender.send(data.to_vec());
                        },
                        move |err| {
                            use cpal::ErrorKind;
                            match err.kind() {
                                ErrorKind::DeviceNotAvailable
                                | ErrorKind::StreamInvalidated
                                | ErrorKind::BackendError => {
                                    disc_cb.store(true, Ordering::Relaxed);
                                    log::error!("[oxisound-cpal] async input error: {err}");
                                }
                                ErrorKind::Xrun => {
                                    log::warn!("[oxisound-cpal] async input buffer underrun");
                                }
                                _ => {
                                    log::error!("[oxisound-cpal] async input error: {err}");
                                }
                            }
                        },
                        None,
                    )
                    .map_err(map_build_stream_err)?
            }
            SampleFormat::I16 => {
                let sender = tx.clone();
                self.device
                    .build_input_stream(
                        cpal_config,
                        move |data: &[i16], _: &cpal::InputCallbackInfo| {
                            use dasp_sample::Sample as DaspSample;
                            let frames: Vec<f32> = data
                                .iter()
                                .map(|&s| DaspSample::to_sample::<f32>(s))
                                .collect();
                            let _ = sender.send(frames);
                        },
                        move |err| {
                            use cpal::ErrorKind;
                            match err.kind() {
                                ErrorKind::DeviceNotAvailable
                                | ErrorKind::StreamInvalidated
                                | ErrorKind::BackendError => {
                                    disc_cb.store(true, Ordering::Relaxed);
                                    log::error!("[oxisound-cpal] async i16 input error: {err}");
                                }
                                ErrorKind::Xrun => {
                                    log::warn!("[oxisound-cpal] async i16 input buffer underrun");
                                }
                                _ => {
                                    log::error!("[oxisound-cpal] async i16 input error: {err}");
                                }
                            }
                        },
                        None,
                    )
                    .map_err(map_build_stream_err)?
            }
            SampleFormat::I24 | SampleFormat::I32 => {
                let sender = tx.clone();
                self.device
                    .build_input_stream(
                        cpal_config,
                        move |data: &[i32], _: &cpal::InputCallbackInfo| {
                            use dasp_sample::Sample as DaspSample;
                            let frames: Vec<f32> = data
                                .iter()
                                .map(|&s| DaspSample::to_sample::<f32>(s))
                                .collect();
                            let _ = sender.send(frames);
                        },
                        move |err| {
                            use cpal::ErrorKind;
                            match err.kind() {
                                ErrorKind::DeviceNotAvailable
                                | ErrorKind::StreamInvalidated
                                | ErrorKind::BackendError => {
                                    disc_cb.store(true, Ordering::Relaxed);
                                    log::error!("[oxisound-cpal] async i24/i32 input error: {err}");
                                }
                                ErrorKind::Xrun => {
                                    log::warn!(
                                        "[oxisound-cpal] async i24/i32 input buffer underrun"
                                    );
                                }
                                _ => {
                                    log::error!("[oxisound-cpal] async i24/i32 input error: {err}");
                                }
                            }
                        },
                        None,
                    )
                    .map_err(map_build_stream_err)?
            }
            fmt => {
                return Err(OxiSoundError::UnsupportedConfig(format!(
                    "async input: unsupported sample format {fmt:?}"
                )));
            }
        };

        stream.play().map_err(map_play_stream_err)?;

        Ok(CpalAsyncInputStream {
            _stream: stream,
            receiver: rx,
        })
    }

    // -----------------------------------------------------------------------
    // Device hot-plug API (not available on wasm32 — no OS threads)
    // -----------------------------------------------------------------------

    /// Starts a device hot-plug watcher and returns it.
    ///
    /// The watcher polls device changes every 500ms. Call `try_recv()` on it for sync use
    /// or `subscribe()` (tokio feature) for async use.
    ///
    /// Not available on `wasm32`.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn watch_devices() -> Result<crate::watcher::CpalDeviceWatcher, OxiSoundError> {
        watcher::CpalDeviceWatcher::start()
    }

    /// Returns a tokio broadcast receiver delivering `DeviceEvent` items as devices are added,
    /// removed, or the default device changes.
    ///
    /// Internally starts a `CpalDeviceWatcher`. The receiver is valid for the watcher's lifetime;
    /// hold the returned `CpalDeviceWatcher` to keep events flowing.
    ///
    /// Not available on `wasm32`.
    #[cfg(all(feature = "tokio", not(target_arch = "wasm32")))]
    pub fn subscribe_device_events() -> Result<
        (
            crate::watcher::CpalDeviceWatcher,
            tokio::sync::broadcast::Receiver<DeviceEvent>,
        ),
        OxiSoundError,
    > {
        let watcher = watcher::CpalDeviceWatcher::start()?;
        let rx = watcher.subscribe();
        Ok((watcher, rx))
    }

    /// Registers a synchronous callback for device change events.
    ///
    /// Spawns a background thread that polls device changes every 500ms
    /// and calls `callback` for each `DeviceEvent`.
    ///
    /// Returns a [`DeviceChangeGuard`]; dropping it stops the polling thread.
    ///
    /// Not available on `wasm32`.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn on_device_change(
        callback: impl Fn(DeviceEvent) + Send + 'static,
    ) -> Result<DeviceChangeGuard, OxiSoundError> {
        let stop = Arc::new(AtomicBool::new(false));
        let stop_clone = Arc::clone(&stop);
        let thread = std::thread::spawn(move || {
            let mut prev_names = watcher::CpalDeviceWatcher::current_device_names();
            while !stop_clone.load(Ordering::Relaxed) {
                std::thread::sleep(std::time::Duration::from_millis(500));
                let current = watcher::CpalDeviceWatcher::current_device_names();
                for name in current.difference(&prev_names) {
                    let info = DeviceInfo::builder(name.clone()).build();
                    callback(DeviceEvent::DeviceAdded(info));
                }
                for name in prev_names.difference(&current) {
                    callback(DeviceEvent::DeviceRemoved(name.clone()));
                }
                prev_names = current;
            }
        });
        Ok(DeviceChangeGuard {
            stop,
            thread: Some(thread),
        })
    }
}

impl AudioDevice for CpalDevice {
    fn enumerate() -> Result<Vec<DeviceInfo>, OxiSoundError> {
        let host = cpal::default_host();
        let devices = host.output_devices().map_err(map_devices_err)?;

        let default_name = host
            .default_output_device()
            .and_then(|d| d.description().ok())
            .map(|desc| desc.name().to_owned());

        let mut infos = Vec::new();
        for dev in devices {
            let desc = dev
                .description()
                .map_err(|e| OxiSoundError::Device(e.to_string()))?;
            let name = desc.name().to_owned();
            let is_default = Some(&name) == default_name.as_ref();

            let (sample_rates, channel_counts) = collect_config_ranges(&dev);
            let capabilities = build_device_capabilities(&dev, true);

            infos.push(DeviceInfo {
                name,
                is_default,
                sample_rates,
                channel_counts,
                is_input: false,
                is_output: true,
                capabilities,
            });
        }
        Ok(infos)
    }

    fn default_output() -> Result<Self, OxiSoundError> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or(OxiSoundError::NoDevice)?;
        Ok(CpalDevice {
            device,
            host_api: default_host_api(),
        })
    }

    fn default_input() -> Result<Self, OxiSoundError> {
        let host = cpal::default_host();
        let device = host.default_input_device().ok_or(OxiSoundError::NoDevice)?;
        Ok(CpalDevice {
            device,
            host_api: default_host_api(),
        })
    }

    fn open_output(&self, config: OxiStreamConfig) -> Result<Box<dyn OutputStream>, OxiSoundError> {
        Ok(Box::new(self.open_output_inner(config)?))
    }

    fn open_input(&self, config: OxiStreamConfig) -> Result<Box<dyn InputStream>, OxiSoundError> {
        Ok(Box::new(self.open_input_inner(config)?))
    }

    fn open_duplex(&self, config: OxiStreamConfig) -> Result<Box<dyn DuplexStream>, OxiSoundError> {
        warn_if_exclusive_requested(&config);
        let (sample_rates, channel_counts) = collect_config_ranges(&self.device);
        let info = DeviceInfo {
            sample_rates,
            channel_counts,
            is_output: true,
            capabilities: None,
            ..Default::default()
        };
        config.validate(&info)?;

        let out_supported = self
            .device
            .default_output_config()
            .map_err(map_default_config_err)?;
        let out_config = to_cpal_stream_config(&config, &out_supported);
        is_config_supported_for_output(&self.device, &out_config)?;

        let out_channels = out_config.channels;
        let out_capacity_secs = config.buffer_capacity_secs.unwrap_or(2.0);
        let out_capacity = (out_capacity_secs
            * out_config.sample_rate as f32
            * f32::from(out_channels.max(1))) as usize;
        let out_disconnected = Arc::new(AtomicBool::new(false));
        let out_underrun = Arc::new(AtomicU64::new(0));
        let out_frames = Arc::new(AtomicU64::new(0));
        let out_cb_duration_ns = Arc::new(AtomicU64::new(0));

        let out_rb = HeapRb::<f32>::new(out_capacity);
        let (out_producer, out_consumer) = out_rb.split();

        let duplex_routing = config.channel_routing.clone();
        let out_stream = match out_supported.sample_format() {
            SampleFormat::F32 => build_output_stream_typed::<f32>(
                &self.device,
                &out_config,
                out_consumer,
                Arc::clone(&out_disconnected),
                Arc::clone(&out_underrun),
                Arc::clone(&out_frames),
                out_channels,
                duplex_routing.clone(),
                Arc::clone(&out_cb_duration_ns),
            ),
            SampleFormat::I16 => build_output_stream_typed::<i16>(
                &self.device,
                &out_config,
                out_consumer,
                Arc::clone(&out_disconnected),
                Arc::clone(&out_underrun),
                Arc::clone(&out_frames),
                out_channels,
                duplex_routing.clone(),
                Arc::clone(&out_cb_duration_ns),
            ),
            SampleFormat::I32 => build_output_stream_typed::<i32>(
                &self.device,
                &out_config,
                out_consumer,
                Arc::clone(&out_disconnected),
                Arc::clone(&out_underrun),
                Arc::clone(&out_frames),
                out_channels,
                duplex_routing.clone(),
                Arc::clone(&out_cb_duration_ns),
            ),
            SampleFormat::I24 => build_output_stream_typed::<i32>(
                &self.device,
                &out_config,
                out_consumer,
                Arc::clone(&out_disconnected),
                Arc::clone(&out_underrun),
                Arc::clone(&out_frames),
                out_channels,
                duplex_routing.clone(),
                Arc::clone(&out_cb_duration_ns),
            ),
            SampleFormat::F64 => build_output_stream_typed::<f64>(
                &self.device,
                &out_config,
                out_consumer,
                Arc::clone(&out_disconnected),
                Arc::clone(&out_underrun),
                Arc::clone(&out_frames),
                out_channels,
                duplex_routing,
                Arc::clone(&out_cb_duration_ns),
            ),
            fmt => {
                return Err(OxiSoundError::UnsupportedConfig(format!(
                    "duplex output: unsupported sample format {fmt:?}"
                )));
            }
        }?;
        out_stream.play().map_err(map_play_stream_err)?;

        let in_supported = self
            .device
            .default_input_config()
            .map_err(map_default_config_err)?;
        let in_config = to_cpal_stream_config(&config, &in_supported);
        is_config_supported_for_input(&self.device, &in_config)?;

        let in_channels = in_config.channels;
        let in_capacity_secs = config.buffer_capacity_secs.unwrap_or(2.0);
        let in_capacity = (in_capacity_secs
            * in_config.sample_rate as f32
            * f32::from(in_channels.max(1))) as usize;
        let in_disconnected = Arc::new(AtomicBool::new(false));
        let in_frames = Arc::new(AtomicU64::new(0));
        let in_cb_duration_ns = Arc::new(AtomicU64::new(0));

        let in_rb = HeapRb::<f32>::new(in_capacity);
        let (in_producer, in_consumer) = in_rb.split();

        let in_stream = match in_supported.sample_format() {
            SampleFormat::F32 => build_input_stream_typed::<f32>(
                &self.device,
                &in_config,
                in_producer,
                Arc::clone(&in_disconnected),
                Arc::clone(&in_frames),
                in_channels,
                Arc::clone(&in_cb_duration_ns),
            ),
            SampleFormat::I16 => build_input_stream_typed::<i16>(
                &self.device,
                &in_config,
                in_producer,
                Arc::clone(&in_disconnected),
                Arc::clone(&in_frames),
                in_channels,
                Arc::clone(&in_cb_duration_ns),
            ),
            SampleFormat::I32 => build_input_stream_typed::<i32>(
                &self.device,
                &in_config,
                in_producer,
                Arc::clone(&in_disconnected),
                Arc::clone(&in_frames),
                in_channels,
                Arc::clone(&in_cb_duration_ns),
            ),
            SampleFormat::I24 => build_input_stream_typed::<i32>(
                &self.device,
                &in_config,
                in_producer,
                Arc::clone(&in_disconnected),
                Arc::clone(&in_frames),
                in_channels,
                Arc::clone(&in_cb_duration_ns),
            ),
            SampleFormat::F64 => build_input_stream_typed::<f64>(
                &self.device,
                &in_config,
                in_producer,
                Arc::clone(&in_disconnected),
                Arc::clone(&in_frames),
                in_channels,
                Arc::clone(&in_cb_duration_ns),
            ),
            fmt => {
                return Err(OxiSoundError::UnsupportedConfig(format!(
                    "duplex input: unsupported sample format {fmt:?}"
                )));
            }
        }?;
        in_stream.play().map_err(map_play_stream_err)?;

        Ok(Box::new(CpalDuplexStream {
            _input: in_stream,
            _output: out_stream,
            in_consumer,
            out_producer,
            in_channels,
            out_channels,
            out_disconnected,
            in_disconnected,
            in_frames_processed: in_frames,
            out_frames_processed: out_frames,
            drift_ema: 1.0,
            resample_phase: 0.0,
        }))
    }

    fn negotiate_output(&self, config: OxiStreamConfig) -> Result<NegotiatedConfig, OxiSoundError> {
        let supported = self
            .device
            .supported_output_configs()
            .map_err(|e| OxiSoundError::Device(e.to_string()))?;

        // Collect every format the device supports for the requested channels/rate so we can
        // rank them by preference rather than taking whichever config cpal yields first.
        let mut matching_formats: Vec<oxisound_core::SampleFormat> = Vec::new();
        let mut buf_size: Option<u32> = None;
        for sc in supported {
            let min_rate = sc.min_sample_rate();
            let max_rate = sc.max_sample_rate();
            if config.sample_rate < min_rate || config.sample_rate > max_rate {
                continue;
            }
            if sc.channels() != config.channels {
                continue;
            }
            let fmt = cpal_to_core_format(sc.sample_format());
            if !matching_formats.contains(&fmt) {
                matching_formats.push(fmt);
            }
            if buf_size.is_none() {
                buf_size = match sc.buffer_size() {
                    cpal::SupportedBufferSize::Range { min, .. } => Some(*min),
                    cpal::SupportedBufferSize::Unknown => Some(512),
                };
            }
        }

        if !matching_formats.is_empty() {
            // Preference order: ranked `preferred_formats` list first, then the single
            // `sample_format` hint (if any), so callers get fine-grained control.
            // `pick_preferred_format` falls back to the first device-supported format when
            // none of the preferred entries match.
            let preferred: Vec<oxisound_core::SampleFormat> = config
                .preferred_formats
                .iter()
                .copied()
                .chain(config.sample_format)
                .collect();
            let chosen = oxisound_core::pick_preferred_format(&preferred, &matching_formats)
                .ok_or_else(|| {
                    OxiSoundError::FormatMismatch("no negotiable output format".into())
                })?;
            return Ok(NegotiatedConfig {
                sample_rate: config.sample_rate,
                channels: config.channels,
                buffer_size: config.buffer_size.unwrap_or(buf_size.unwrap_or(512)),
                sample_format: chosen,
            });
        }
        Err(OxiSoundError::FormatMismatch(format!(
            "no supported output config for {}Hz {}ch",
            config.sample_rate, config.channels
        )))
    }
}
