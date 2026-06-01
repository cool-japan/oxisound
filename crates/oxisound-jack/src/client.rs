//! Full JACK client implementation, compiled only with the `jack-backend` feature.
//!
//! # Design
//!
//! All three stream types follow the same three-stage pattern:
//!
//! 1. Register a port with the JACK client.
//! 2. Build a `ProcessHandler` that owns the port and a ring-buffer half.
//! 3. Call `client.activate_async((), handler)` to start the JACK process thread.
//!    The returned `AsyncClient` is kept alive in the stream struct's `_active` field.
//!
//! When the stream struct is dropped, `_active` is dropped, which deactivates the
//! JACK client and stops the process thread safely.

use jack::{
    AudioIn, AudioOut, Client, ClientOptions, Control, NotificationHandler, Port, ProcessHandler,
    ProcessScope,
};
use oxisound_core::{InputStream, OutputStream, OxiSoundError, StreamConfig, StreamStats};

/// Type alias for the user-supplied audio callback in `JackCallbackOutputStream`.
type AudioCallback = Box<dyn FnMut(&mut [f32]) + Send>;
use ringbuf::{
    HeapCons, HeapProd, HeapRb,
    traits::{Consumer, Producer, Split},
};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use super::{JackTransportPosition, JackTransportState};
use crate::metrics::JackMetrics;

// ---------------------------------------------------------------------------
// JackNotifier — notification handler with observability
// ---------------------------------------------------------------------------

/// JACK notification handler carrying sample-rate and xrun observability.
pub(crate) struct JackNotifier {
    metrics: JackMetrics,
}

impl NotificationHandler for JackNotifier {
    fn sample_rate(&mut self, _: &Client, srate: jack::Frames) -> Control {
        self.metrics.record_sample_rate(srate);
        Control::Continue
    }

    fn xrun(&mut self, _: &Client) -> Control {
        self.metrics.record_xrun();
        Control::Continue
    }
}

// ---------------------------------------------------------------------------
// Ring-buffer capacity helpers
// ---------------------------------------------------------------------------

/// Two seconds of mono f32 samples at the given sample rate.
fn ring_capacity(sample_rate: u32) -> usize {
    (sample_rate as usize).saturating_mul(2)
}

// ---------------------------------------------------------------------------
// JackDevice
// ---------------------------------------------------------------------------

/// A JACK Audio Server client providing ring-buffer and callback-based streams.
///
/// Constructed via [`JackDevice::new`]. Requires `jack-backend` feature and
/// a running JACK daemon (`jackd` or PipeWire-JACK).
pub struct JackDevice {
    client: Client,
    name: String,
}

impl JackDevice {
    /// Open a new JACK client with the given name.
    ///
    /// Fails if the JACK server is not running or the name is already taken.
    pub fn new(client_name: &str) -> Result<Self, OxiSoundError> {
        let (client, _status) = Client::new(client_name, ClientOptions::NO_START_SERVER)
            .map_err(|e| OxiSoundError::Device(format!("JACK client open failed: {e}")))?;
        Ok(Self {
            name: client_name.to_owned(),
            client,
        })
    }

    /// The JACK server's current sample rate (set by the server at startup).
    pub fn sample_rate(&self) -> u32 {
        self.client.sample_rate()
    }

    /// The JACK server's current buffer size in frames per process callback.
    pub fn buffer_size(&self) -> u32 {
        self.client.buffer_size()
    }

    /// Open a ring-buffer-backed output stream.
    ///
    /// Write samples via [`OutputStream::write`]. The JACK process callback drains the
    /// ring buffer into the `"out"` port at each process cycle.
    pub fn open_output(self, _config: StreamConfig) -> Result<JackOutputStream, OxiSoundError> {
        let sample_rate = self.sample_rate();
        let capacity = ring_capacity(sample_rate);

        let rb = HeapRb::<f32>::new(capacity);
        let (producer, consumer) = rb.split();

        let underruns = Arc::new(AtomicU64::new(0));
        let frames_processed = Arc::new(AtomicU64::new(0));

        let out_port = self
            .client
            .register_port("out", AudioOut::default())
            .map_err(|e| OxiSoundError::Device(format!("JACK port register failed: {e}")))?;

        let metrics = JackMetrics::new();
        let notifier = JackNotifier {
            metrics: metrics.clone(),
        };
        let handler = JackOutputHandler {
            port: out_port,
            consumer,
            underruns: Arc::clone(&underruns),
            frames_processed: Arc::clone(&frames_processed),
            metrics: metrics.clone(),
        };

        let active = self
            .client
            .activate_async(notifier, handler)
            .map_err(|e| OxiSoundError::Device(format!("JACK activate_async failed: {e}")))?;

        Ok(JackOutputStream {
            producer,
            frames_processed,
            underruns,
            metrics,
            _active: active,
        })
    }

    /// Open a ring-buffer-backed input stream.
    ///
    /// The JACK process callback pushes captured samples into the ring buffer.
    /// Read them via [`InputStream::read`].
    pub fn open_input(self, _config: StreamConfig) -> Result<JackInputStream, OxiSoundError> {
        let sample_rate = self.sample_rate();
        let capacity = ring_capacity(sample_rate);

        let rb = HeapRb::<f32>::new(capacity);
        let (producer, consumer) = rb.split();

        let overruns = Arc::new(AtomicU64::new(0));
        let frames_processed = Arc::new(AtomicU64::new(0));

        let in_port = self
            .client
            .register_port("in", AudioIn::default())
            .map_err(|e| OxiSoundError::Device(format!("JACK port register failed: {e}")))?;

        let metrics = JackMetrics::new();
        let notifier = JackNotifier {
            metrics: metrics.clone(),
        };
        let handler = JackInputHandler {
            port: in_port,
            producer,
            overruns: Arc::clone(&overruns),
            frames_processed: Arc::clone(&frames_processed),
            metrics: metrics.clone(),
        };

        let active = self
            .client
            .activate_async(notifier, handler)
            .map_err(|e| OxiSoundError::Device(format!("JACK activate_async failed: {e}")))?;

        Ok(JackInputStream {
            consumer,
            frames_processed,
            overruns,
            metrics,
            _active: active,
        })
    }

    /// Open a zero-copy callback output stream.
    ///
    /// `callback` is called directly in the JACK process thread with a mutable slice
    /// sized to the JACK buffer size. This provides the lowest possible latency,
    /// bypassing the ring buffer entirely.
    ///
    /// # Thread Safety
    ///
    /// `callback` must be `Send + 'static`. Do not block or allocate inside the callback.
    pub fn open_output_callback<F>(
        self,
        _config: StreamConfig,
        callback: F,
    ) -> Result<JackCallbackOutputStream, OxiSoundError>
    where
        F: FnMut(&mut [f32]) + Send + 'static,
    {
        let frames_processed = Arc::new(AtomicU64::new(0));

        let out_port = self
            .client
            .register_port("out", AudioOut::default())
            .map_err(|e| OxiSoundError::Device(format!("JACK port register failed: {e}")))?;

        let metrics = JackMetrics::new();
        let notifier = JackNotifier {
            metrics: metrics.clone(),
        };
        let callback_boxed: AudioCallback = Box::new(callback);
        let handler = JackCallbackOutputHandler {
            port: out_port,
            callback: callback_boxed,
            frames_processed: Arc::clone(&frames_processed),
            metrics: metrics.clone(),
        };

        let active = self
            .client
            .activate_async(notifier, handler)
            .map_err(|e| OxiSoundError::Device(format!("JACK activate_async failed: {e}")))?;

        Ok(JackCallbackOutputStream {
            frames_processed,
            metrics,
            _active: active,
        })
    }

    /// Connect a JACK port to another by their full names.
    ///
    /// Example: `device.connect_ports("myapp:out", "system:playback_1")`.
    ///
    /// Ignores `PortAlreadyConnected` errors (idempotent).
    pub fn connect_ports(&self, src: &str, dst: &str) -> Result<(), OxiSoundError> {
        match self.client.connect_ports_by_name(src, dst) {
            Ok(()) => Ok(()),
            Err(jack::Error::PortAlreadyConnected(_, _)) => Ok(()),
            Err(e) => Err(OxiSoundError::Device(format!(
                "JACK port connect {src} → {dst} failed: {e}"
            ))),
        }
    }

    /// Auto-connect the named registered output port to `system:playback_*` sinks.
    ///
    /// Connects to up to the first two system playback ports (stereo). Failures
    /// are logged as warnings rather than returned as errors.
    pub fn auto_connect_output(&self, port_name: &str) -> Result<(), OxiSoundError> {
        let full_src = format!("{}:{}", self.name, port_name);
        let system_ports =
            self.client
                .ports(Some("system:playback_"), None, jack::PortFlags::IS_INPUT);
        for sys_port in system_ports.iter().take(2) {
            match self.client.connect_ports_by_name(&full_src, sys_port) {
                Ok(()) | Err(jack::Error::PortAlreadyConnected(_, _)) => {}
                Err(e) => {
                    log::warn!("[oxisound-jack] auto-connect {full_src} → {sys_port}: {e}");
                }
            }
        }
        Ok(())
    }

    /// Query the JACK transport state.
    pub fn transport_state(&self) -> JackTransportState {
        match self.client.transport().query_state() {
            Ok(jack::TransportState::Rolling) => JackTransportState::Rolling,
            Ok(jack::TransportState::Starting) => JackTransportState::Starting,
            _ => JackTransportState::Stopped,
        }
    }

    /// Query the JACK transport position (frame count and BPM if available).
    ///
    /// BPM is present only when the JACK time master provides BBT (Bar-Beat-Tick) data.
    pub fn transport_position(&self) -> JackTransportPosition {
        match self.client.transport().query() {
            Ok(state_pos) => {
                let frame = state_pos.pos.frame() as u64;
                let bpm = state_pos.pos.bbt().map(|b| b.bpm);
                JackTransportPosition { frame, bpm }
            }
            Err(_) => JackTransportPosition {
                frame: 0,
                bpm: None,
            },
        }
    }

    /// Enable or disable JACK freewheel mode.
    ///
    /// **Note:** `set_freewheel` is not implemented in the `jack` 0.13.5 safe API
    /// (upstream TODO: <https://github.com/RustAudio/rust-jack>). This method always
    /// returns `OxiSoundError::Unsupported`. Revisit when a newer jack release exposes
    /// this in its safe API.
    pub fn set_freewheel(&self, _enabled: bool) -> Result<(), OxiSoundError> {
        Err(OxiSoundError::Unsupported(
            "JACK freewheel is not implemented in the jack 0.13.5 safe API (upstream TODO). \
             Track: https://github.com/RustAudio/rust-jack"
                .into(),
        ))
    }
}

// ---------------------------------------------------------------------------
// JackOutputStream (ring-buffer backed)
// ---------------------------------------------------------------------------

/// Ring-buffer-backed JACK output stream.
///
/// Write interleaved `f32` samples via [`OutputStream::write`]. The JACK process
/// callback drains them into the hardware output at each JACK buffer boundary.
pub struct JackOutputStream {
    producer: HeapProd<f32>,
    frames_processed: Arc<AtomicU64>,
    underruns: Arc<AtomicU64>,
    pub(crate) metrics: JackMetrics,
    /// Keeps the JACK client alive; dropped when the stream is dropped.
    _active: jack::AsyncClient<JackNotifier, JackOutputHandler>,
}

struct JackOutputHandler {
    port: Port<AudioOut>,
    consumer: HeapCons<f32>,
    underruns: Arc<AtomicU64>,
    frames_processed: Arc<AtomicU64>,
    pub(crate) metrics: JackMetrics,
}

impl ProcessHandler for JackOutputHandler {
    fn buffer_size(&mut self, _: &Client, size: jack::Frames) -> Control {
        self.metrics.record_buffer_size(size);
        Control::Continue
    }

    fn process(&mut self, _client: &Client, ps: &ProcessScope) -> Control {
        let out = self.port.as_mut_slice(ps);
        let len = out.len() as u64;
        for sample in out.iter_mut() {
            *sample = self.consumer.try_pop().unwrap_or_else(|| {
                self.underruns.fetch_add(1, Ordering::Relaxed);
                0.0
            });
        }
        self.frames_processed.fetch_add(len, Ordering::Relaxed);
        let latency = self.port.get_latency_range(jack::LatencyType::Playback).1;
        self.metrics.record_latency(latency);
        Control::Continue
    }
}

impl OutputStream for JackOutputStream {
    fn write(&mut self, samples: &[f32]) -> Result<(), OxiSoundError> {
        for &s in samples {
            // Silently discard if ring buffer is full (capacity cap prevents unbounded growth).
            let _ = self.producer.try_push(s);
        }
        Ok(())
    }

    fn stats(&self) -> StreamStats {
        StreamStats {
            frames_processed: self.frames_processed.load(Ordering::Relaxed),
            underruns: self.underruns.load(Ordering::Relaxed),
            overruns: 0,
            latency_frames: self.metrics.snapshot().latency_frames,
            cpu_load_percent: 0.0,
        }
    }
}

impl JackOutputStream {
    /// Current sample rate as reported by the JACK notification handler.
    pub fn current_sample_rate(&self) -> u32 {
        self.metrics.snapshot().sample_rate
    }

    /// Total number of xruns recorded since stream activation.
    pub fn xrun_count(&self) -> u64 {
        self.metrics.snapshot().xrun_count
    }

    /// Current buffer size in frames as reported by the process handler.
    pub fn current_buffer_size(&self) -> u32 {
        self.metrics.snapshot().buffer_size
    }

    /// Connect this stream's `"out"` port to another JACK port by full name.
    ///
    /// Example: `stream.connect_ports("myapp:out", "system:playback_1")`.
    ///
    /// Ignores `PortAlreadyConnected` errors (idempotent).
    ///
    /// Note: [`JackDevice::connect_ports`] is also available for wiring third-party
    /// ports before a stream is opened.
    pub fn connect_ports(&self, src: &str, dst: &str) -> Result<(), OxiSoundError> {
        let client = self._active.as_client();
        match client.connect_ports_by_name(src, dst) {
            Ok(()) => Ok(()),
            Err(jack::Error::PortAlreadyConnected(_, _)) => Ok(()),
            Err(e) => Err(OxiSoundError::Device(format!(
                "JACK port connect {src} → {dst} failed: {e}"
            ))),
        }
    }

    /// Auto-connect this stream's registered `"out"` port to `system:playback_*` sinks.
    ///
    /// Connects to up to the first two system playback ports (stereo). Failures
    /// are logged as warnings rather than returned as errors.
    pub fn auto_connect(&self) -> Result<(), OxiSoundError> {
        let client = self._active.as_client();
        let client_name = client.name();
        let full_src = format!("{client_name}:out");
        let system_ports = client.ports(Some("system:playback_"), None, jack::PortFlags::IS_INPUT);
        for sys_port in system_ports.iter().take(2) {
            match client.connect_ports_by_name(&full_src, sys_port) {
                Ok(()) | Err(jack::Error::PortAlreadyConnected(_, _)) => {}
                Err(e) => {
                    log::warn!("[oxisound-jack] auto-connect {full_src} → {sys_port}: {e}");
                }
            }
        }
        Ok(())
    }

    /// CPU load reported by the JACK server (0.0–100.0 percent).
    ///
    /// This is a running average over all clients; useful for detecting overload
    /// before audio glitches appear.
    pub fn cpu_load(&self) -> f32 {
        self._active.as_client().cpu_load()
    }

    /// List all JACK port names visible on the server.
    ///
    /// Useful for discovering which ports exist before calling `connect_ports`.
    pub fn list_ports(&self) -> Vec<String> {
        self._active
            .as_client()
            .ports(None, None, jack::PortFlags::empty())
    }

    /// List input ports matching an optional name pattern.
    ///
    /// `pattern` is a POSIX regular expression matched against the full port name.
    pub fn list_input_ports(&self, pattern: Option<&str>) -> Vec<String> {
        self._active
            .as_client()
            .ports(pattern, None, jack::PortFlags::IS_INPUT)
    }

    /// List output ports matching an optional name pattern.
    ///
    /// `pattern` is a POSIX regular expression matched against the full port name.
    pub fn list_output_ports(&self, pattern: Option<&str>) -> Vec<String> {
        self._active
            .as_client()
            .ports(pattern, None, jack::PortFlags::IS_OUTPUT)
    }
}

// ---------------------------------------------------------------------------
// JackInputStream (ring-buffer backed)
// ---------------------------------------------------------------------------

/// Ring-buffer-backed JACK input stream.
///
/// The JACK process callback pushes captured samples into the ring buffer.
/// Read them via [`InputStream::read`].
pub struct JackInputStream {
    consumer: HeapCons<f32>,
    frames_processed: Arc<AtomicU64>,
    overruns: Arc<AtomicU64>,
    pub(crate) metrics: JackMetrics,
    /// Keeps the JACK client alive; dropped when the stream is dropped.
    _active: jack::AsyncClient<JackNotifier, JackInputHandler>,
}

struct JackInputHandler {
    port: Port<AudioIn>,
    producer: HeapProd<f32>,
    overruns: Arc<AtomicU64>,
    frames_processed: Arc<AtomicU64>,
    pub(crate) metrics: JackMetrics,
}

impl ProcessHandler for JackInputHandler {
    fn buffer_size(&mut self, _: &Client, size: jack::Frames) -> Control {
        self.metrics.record_buffer_size(size);
        Control::Continue
    }

    fn process(&mut self, _client: &Client, ps: &ProcessScope) -> Control {
        let inp = self.port.as_slice(ps);
        let len = inp.len() as u64;
        for &sample in inp {
            if self.producer.try_push(sample).is_err() {
                self.overruns.fetch_add(1, Ordering::Relaxed);
            }
        }
        self.frames_processed.fetch_add(len, Ordering::Relaxed);
        let latency = self.port.get_latency_range(jack::LatencyType::Capture).1;
        self.metrics.record_latency(latency);
        Control::Continue
    }
}

impl InputStream for JackInputStream {
    fn read(&mut self, samples: &mut [f32]) -> Result<usize, OxiSoundError> {
        let mut count = 0;
        for slot in samples.iter_mut() {
            match self.consumer.try_pop() {
                Some(s) => {
                    *slot = s;
                    count += 1;
                }
                None => break,
            }
        }
        Ok(count)
    }

    fn stats(&self) -> StreamStats {
        StreamStats {
            frames_processed: self.frames_processed.load(Ordering::Relaxed),
            underruns: 0,
            overruns: self.overruns.load(Ordering::Relaxed),
            latency_frames: self.metrics.snapshot().latency_frames,
            cpu_load_percent: 0.0,
        }
    }
}

impl JackInputStream {
    /// Current sample rate as reported by the JACK notification handler.
    pub fn current_sample_rate(&self) -> u32 {
        self.metrics.snapshot().sample_rate
    }

    /// Total number of xruns recorded since stream activation.
    pub fn xrun_count(&self) -> u64 {
        self.metrics.snapshot().xrun_count
    }

    /// Current buffer size in frames as reported by the process handler.
    pub fn current_buffer_size(&self) -> u32 {
        self.metrics.snapshot().buffer_size
    }

    /// Connect a JACK port to this stream's `"in"` port by full names.
    ///
    /// Example: `stream.connect_ports("system:capture_1", "myapp:in")`.
    ///
    /// Ignores `PortAlreadyConnected` errors (idempotent).
    ///
    /// Note: [`JackDevice::connect_ports`] is also available for wiring third-party
    /// ports before a stream is opened.
    pub fn connect_ports(&self, src: &str, dst: &str) -> Result<(), OxiSoundError> {
        let client = self._active.as_client();
        match client.connect_ports_by_name(src, dst) {
            Ok(()) => Ok(()),
            Err(jack::Error::PortAlreadyConnected(_, _)) => Ok(()),
            Err(e) => Err(OxiSoundError::Device(format!(
                "JACK port connect {src} → {dst} failed: {e}"
            ))),
        }
    }

    /// Auto-connect `system:capture_*` sources to this stream's registered `"in"` port.
    ///
    /// Connects from up to the first two system capture ports (stereo). Failures
    /// are logged as warnings rather than returned as errors.
    pub fn auto_connect(&self) -> Result<(), OxiSoundError> {
        let client = self._active.as_client();
        let client_name = client.name();
        let full_dst = format!("{client_name}:in");
        let system_ports = client.ports(Some("system:capture_"), None, jack::PortFlags::IS_OUTPUT);
        for sys_port in system_ports.iter().take(2) {
            match client.connect_ports_by_name(sys_port, &full_dst) {
                Ok(()) | Err(jack::Error::PortAlreadyConnected(_, _)) => {}
                Err(e) => {
                    log::warn!("[oxisound-jack] auto-connect {sys_port} → {full_dst}: {e}");
                }
            }
        }
        Ok(())
    }

    /// CPU load reported by the JACK server (0.0–100.0 percent).
    ///
    /// This is a running average over all clients; useful for detecting overload
    /// before audio glitches appear.
    pub fn cpu_load(&self) -> f32 {
        self._active.as_client().cpu_load()
    }

    /// List all JACK port names visible on the server.
    ///
    /// Useful for discovering which ports exist before calling `connect_ports`.
    pub fn list_ports(&self) -> Vec<String> {
        self._active
            .as_client()
            .ports(None, None, jack::PortFlags::empty())
    }

    /// List input ports matching an optional name pattern.
    ///
    /// `pattern` is a POSIX regular expression matched against the full port name.
    pub fn list_input_ports(&self, pattern: Option<&str>) -> Vec<String> {
        self._active
            .as_client()
            .ports(pattern, None, jack::PortFlags::IS_INPUT)
    }

    /// List output ports matching an optional name pattern.
    ///
    /// `pattern` is a POSIX regular expression matched against the full port name.
    pub fn list_output_ports(&self, pattern: Option<&str>) -> Vec<String> {
        self._active
            .as_client()
            .ports(pattern, None, jack::PortFlags::IS_OUTPUT)
    }
}

// ---------------------------------------------------------------------------
// JackCallbackOutputStream (zero-copy, direct callback)
// ---------------------------------------------------------------------------

/// Zero-copy JACK output stream driven by a user-supplied callback.
///
/// The JACK process thread calls `callback(&mut [f32])` directly at each
/// process cycle. This is the lowest-latency mode, bypassing the ring buffer.
///
/// Constructed via [`JackDevice::open_output_callback`].
pub struct JackCallbackOutputStream {
    frames_processed: Arc<AtomicU64>,
    pub(crate) metrics: JackMetrics,
    /// Keeps the JACK client alive; dropped when the stream is dropped.
    _active: jack::AsyncClient<JackNotifier, JackCallbackOutputHandler>,
}

struct JackCallbackOutputHandler {
    port: Port<AudioOut>,
    callback: AudioCallback,
    frames_processed: Arc<AtomicU64>,
    pub(crate) metrics: JackMetrics,
}

impl ProcessHandler for JackCallbackOutputHandler {
    fn buffer_size(&mut self, _: &Client, size: jack::Frames) -> Control {
        self.metrics.record_buffer_size(size);
        Control::Continue
    }

    fn process(&mut self, _client: &Client, ps: &ProcessScope) -> Control {
        let out = self.port.as_mut_slice(ps);
        let len = out.len() as u64;
        (self.callback)(out);
        self.frames_processed.fetch_add(len, Ordering::Relaxed);
        let latency = self.port.get_latency_range(jack::LatencyType::Playback).1;
        self.metrics.record_latency(latency);
        Control::Continue
    }
}

impl JackCallbackOutputStream {
    /// Current stream statistics (frames processed since activation).
    pub fn stats(&self) -> StreamStats {
        StreamStats {
            frames_processed: self.frames_processed.load(Ordering::Relaxed),
            underruns: 0,
            overruns: 0,
            latency_frames: self.metrics.snapshot().latency_frames,
            cpu_load_percent: 0.0,
        }
    }

    /// Current sample rate as reported by the JACK notification handler.
    pub fn current_sample_rate(&self) -> u32 {
        self.metrics.snapshot().sample_rate
    }

    /// Total number of xruns recorded since stream activation.
    pub fn xrun_count(&self) -> u64 {
        self.metrics.snapshot().xrun_count
    }

    /// Current buffer size in frames as reported by the process handler.
    pub fn current_buffer_size(&self) -> u32 {
        self.metrics.snapshot().buffer_size
    }

    /// Connect this stream's `"out"` port to another JACK port by full name.
    ///
    /// Example: `stream.connect_ports("myapp:out", "system:playback_1")`.
    ///
    /// Ignores `PortAlreadyConnected` errors (idempotent).
    ///
    /// Note: [`JackDevice::connect_ports`] is also available for wiring third-party
    /// ports before a stream is opened.
    pub fn connect_ports(&self, src: &str, dst: &str) -> Result<(), OxiSoundError> {
        let client = self._active.as_client();
        match client.connect_ports_by_name(src, dst) {
            Ok(()) => Ok(()),
            Err(jack::Error::PortAlreadyConnected(_, _)) => Ok(()),
            Err(e) => Err(OxiSoundError::Device(format!(
                "JACK port connect {src} → {dst} failed: {e}"
            ))),
        }
    }

    /// Auto-connect this stream's registered `"out"` port to `system:playback_*` sinks.
    ///
    /// Connects to up to the first two system playback ports (stereo). Failures
    /// are logged as warnings rather than returned as errors.
    pub fn auto_connect(&self) -> Result<(), OxiSoundError> {
        let client = self._active.as_client();
        let client_name = client.name();
        let full_src = format!("{client_name}:out");
        let system_ports = client.ports(Some("system:playback_"), None, jack::PortFlags::IS_INPUT);
        for sys_port in system_ports.iter().take(2) {
            match client.connect_ports_by_name(&full_src, sys_port) {
                Ok(()) | Err(jack::Error::PortAlreadyConnected(_, _)) => {}
                Err(e) => {
                    log::warn!("[oxisound-jack] auto-connect {full_src} → {sys_port}: {e}");
                }
            }
        }
        Ok(())
    }

    /// CPU load reported by the JACK server (0.0–100.0 percent).
    ///
    /// This is a running average over all clients; useful for detecting overload
    /// before audio glitches appear.
    pub fn cpu_load(&self) -> f32 {
        self._active.as_client().cpu_load()
    }

    /// List all JACK port names visible on the server.
    ///
    /// Useful for discovering which ports exist before calling `connect_ports`.
    pub fn list_ports(&self) -> Vec<String> {
        self._active
            .as_client()
            .ports(None, None, jack::PortFlags::empty())
    }

    /// List input ports matching an optional name pattern.
    ///
    /// `pattern` is a POSIX regular expression matched against the full port name.
    pub fn list_input_ports(&self, pattern: Option<&str>) -> Vec<String> {
        self._active
            .as_client()
            .ports(pattern, None, jack::PortFlags::IS_INPUT)
    }

    /// List output ports matching an optional name pattern.
    ///
    /// `pattern` is a POSIX regular expression matched against the full port name.
    pub fn list_output_ports(&self, pattern: Option<&str>) -> Vec<String> {
        self._active
            .as_client()
            .ports(pattern, None, jack::PortFlags::IS_OUTPUT)
    }
}
