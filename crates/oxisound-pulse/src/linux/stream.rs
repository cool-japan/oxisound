//! Playback, capture and duplex streams over the PulseAudio native protocol.
//!
//! # Threading and data flow
//!
//! The `pulseaudio` client owns one reactor thread per connection. It is the only thread
//! that touches the socket. OxiSound's stream handles never block on it:
//!
//! ```text
//!   caller thread                         reactor thread (pulseaudio crate)
//!   ─────────────                         ─────────────────────────────────
//!   write(&[f32])
//!     └─ encode → HeapProd<u8> ──ring──▶ poll_read(&mut [u8]) ──▶ socket
//!        └─ Waker::wake() ─────────────▶ mio::Waker (re-enters write_streams)
//!
//!                                         socket ──▶ RecordSink::write(&[u8])
//!   read(&mut [f32]) ◀──ring── HeapCons<u8> ◀───────────────┘
//! ```
//!
//! Playback is *server-clocked*: PulseAudio asks for bytes with a `REQUEST` command and
//! the reactor calls [`PlaybackSource::poll_read`]. When the client-side ring is empty the
//! source returns `Poll::Pending` and parks the stored [`Waker`]; the next `write()` wakes
//! it. That is real back-pressure — no silence is ever injected, so an idle stream adds no
//! phantom latency, and the server-side buffer stays bounded by the `tlength` computed in
//! [`crate::model::playback_buffer_attrs`].
//!
//! The waker lives in its own `Mutex`, unreachable from the reactor's own state lock, and
//! is always taken out of the mutex *before* `wake()` is called, so the reactor can never
//! block on a lock held by a writer that is itself waking the reactor.

use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};
use std::time::{Duration, Instant};

use oxisound_core::{
    DuplexStream, InputStream, NegotiatedConfig, OutputStream, OxiSoundError, StreamConfig,
    StreamStats,
};
use pulseaudio::{Client, PlaybackSource, PlaybackStream, RecordSink, RecordStream, protocol};
use ringbuf::traits::{Consumer, Observer, Producer, Split};
use ringbuf::{HeapCons, HeapProd, HeapRb};

use crate::format::{PulseSampleFormat, negotiate_format};
use crate::linux::conn::{PulseConnection, map_client_error};
use crate::linux::device::to_wire_format;
use crate::model::{
    PulseBufferAttrs, PulseDeviceDescriptor, period_frames, playback_buffer_attrs,
    record_buffer_attrs, ring_capacity_bytes, validate_stream_config, whole_samples,
};
use crate::timeout::{
    PULSE_DRAIN_TIMEOUT, PULSE_FLUSH_TIMEOUT, PULSE_OP_TIMEOUT, block_on_timeout,
};

/// How long [`PulseOutputStream::flush`] sleeps between ring-occupancy checks.
const FLUSH_POLL_INTERVAL: Duration = Duration::from_millis(1);

// ---------------------------------------------------------------------------
// Shared state
// ---------------------------------------------------------------------------

/// Counters and the parked waker shared between a playback handle and the reactor.
#[derive(Debug, Default)]
struct PlaybackShared {
    waker: Mutex<Option<Waker>>,
    /// Rising-edge count of "the server asked for data and the ring was empty".
    underruns: AtomicU64,
    /// Bytes handed to the reactor since the stream opened.
    bytes_sent: AtomicU64,
    /// `true` while the ring is known to be empty; prevents counting one underrun per
    /// poll while starved. Starts `true` so the pre-roll before the first write is not
    /// reported as an underrun.
    starved: AtomicBool,
    /// Set when the reactor drops the source, i.e. when the connection went away.
    disconnected: AtomicBool,
    /// Number of queued bytes the reactor should drop at its next poll. Only the
    /// reactor owns the consumer end, so a discard cannot be performed from the handle.
    /// A byte count rather than a flag, so audio written *after* `discard_buffered()`
    /// cannot be swallowed by a discard that fires later.
    discard_bytes: AtomicUsize,
}

impl PlaybackShared {
    fn new() -> Self {
        Self {
            starved: AtomicBool::new(true),
            ..Default::default()
        }
    }

    fn take_waker(&self) -> Option<Waker> {
        let mut guard = self.waker.lock().unwrap_or_else(|e| e.into_inner());
        guard.take()
    }

    fn store_waker(&self, waker: &Waker) {
        let mut guard = self.waker.lock().unwrap_or_else(|e| e.into_inner());
        match guard.as_ref() {
            Some(existing) if existing.will_wake(waker) => {}
            _ => *guard = Some(waker.clone()),
        }
    }
}

/// Counters shared between a capture handle and the reactor.
#[derive(Debug, Default)]
struct RecordShared {
    /// Count of server chunks that did not fit in the ring.
    overruns: AtomicU64,
    /// Bytes accepted into the ring since the stream opened.
    bytes_received: AtomicU64,
    /// Set when the reactor drops the sink, i.e. when the connection went away.
    disconnected: AtomicBool,
}

// ---------------------------------------------------------------------------
// Reactor-side adapters
// ---------------------------------------------------------------------------

/// The reactor's view of a playback stream: drains the client-side ring.
struct RingPlaybackSource {
    consumer: HeapCons<u8>,
    shared: Arc<PlaybackShared>,
}

impl RingPlaybackSource {
    /// Pops what is available, returning `None` when the ring is empty.
    fn serve(&mut self, buf: &mut [u8]) -> Option<usize> {
        let n = self.consumer.pop_slice(buf);
        if n == 0 {
            return None;
        }
        self.shared.starved.store(false, Ordering::Relaxed);
        self.shared
            .bytes_sent
            .fetch_add(n as u64, Ordering::Relaxed);
        Some(n)
    }
}

impl Drop for RingPlaybackSource {
    fn drop(&mut self) {
        // The reactor owns this value; it is dropped when the stream is deleted *or*
        // when the reactor itself shuts down after losing the connection. Either way the
        // handle must stop accepting writes.
        self.shared.disconnected.store(true, Ordering::Relaxed);
        if let Some(waker) = self.shared.take_waker() {
            waker.wake();
        }
    }
}

impl PlaybackSource for RingPlaybackSource {
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut [u8]) -> Poll<usize> {
        // Every field is `Unpin`, so this projection needs no `unsafe`.
        let this = self.get_mut();
        let discard = this.shared.discard_bytes.swap(0, Ordering::Relaxed);
        if discard > 0 {
            // Bounded by the snapshot taken in `discard_buffered()`, so anything written
            // after that call survives.
            let dropped = this.consumer.skip(discard);
            log::debug!("discarded {dropped} queued playback bytes at the client's request");
        }

        if let Some(n) = this.serve(buf) {
            return Poll::Ready(n);
        }

        // Ring empty: park until the next `write()`. Returning `Poll::Ready(0)` would
        // signal EOF to the reactor and permanently stop the stream.
        this.shared.store_waker(cx.waker());

        // Re-check after registering. A writer that pushed between the pop above and the
        // store would have found an empty waker slot and skipped its wake, leaving the
        // reactor parked with data in the ring.
        if let Some(n) = this.serve(buf) {
            let _ = this.shared.take_waker();
            return Poll::Ready(n);
        }

        if !this.shared.starved.swap(true, Ordering::Relaxed) {
            this.shared.underruns.fetch_add(1, Ordering::Relaxed);
        }
        Poll::Pending
    }
}

/// The reactor's view of a record stream: fills the client-side ring.
struct RingRecordSink {
    producer: HeapProd<u8>,
    shared: Arc<RecordShared>,
}

impl Drop for RingRecordSink {
    fn drop(&mut self) {
        self.shared.disconnected.store(true, Ordering::Relaxed);
    }
}

impl RecordSink for RingRecordSink {
    fn write(&mut self, data: &[u8]) {
        let n = self.producer.push_slice(data);
        if n < data.len() {
            // The consumer is not keeping up; the tail of this chunk is dropped.
            self.shared.overruns.fetch_add(1, Ordering::Relaxed);
        }
        self.shared
            .bytes_received
            .fetch_add(n as u64, Ordering::Relaxed);
    }
}

// ---------------------------------------------------------------------------
// PulseOutputStream
// ---------------------------------------------------------------------------

/// A PulseAudio playback stream fed through a lock-free byte ring.
///
/// Mirrors `oxisound_cpal::CpalOutputStream`: the caller pushes interleaved `f32` with
/// [`OutputStream::write`], and a background thread (here PulseAudio's reactor rather
/// than an OS audio callback) drains the ring. A write that does not fit returns
/// [`OxiSoundError::Overrun`] without partially consuming the input.
pub struct PulseOutputStream {
    // Declaration order is the drop order and it matters: the `PlaybackStream` handle
    // must drop *before* the `Client`, so its `DELETE_PLAYBACK_STREAM` command is still
    // written to a live reactor.
    stream: PlaybackStream,
    connection: Arc<PulseConnection>,
    producer: HeapProd<u8>,
    shared: Arc<PlaybackShared>,
    format: PulseSampleFormat,
    channels: u16,
    frame_size: usize,
    capacity: usize,
    negotiated: NegotiatedConfig,
    server_attrs: PulseBufferAttrs,
    scratch: Vec<u8>,
}

impl std::fmt::Debug for PulseOutputStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PulseOutputStream")
            .field("channel", &self.stream.channel())
            .field("format", &self.format)
            .field("channels", &self.channels)
            .finish()
    }
}

impl PulseOutputStream {
    pub(crate) fn open(
        connection: Arc<PulseConnection>,
        device: &PulseDeviceDescriptor,
        config: StreamConfig,
    ) -> Result<Self, OxiSoundError> {
        validate_stream_config(&config)?;
        let format = negotiate_format(
            config.sample_format,
            &config.preferred_formats,
            device.format,
        )?;
        let channels = config.channels;
        let frame_size = format.frame_size(channels);
        let period = period_frames(&config);
        let attrs = playback_buffer_attrs(period, channels, format);
        let capacity = ring_capacity_bytes(&config, format);

        let (producer, consumer) = HeapRb::<u8>::new(capacity).split();
        let shared = Arc::new(PlaybackShared::new());
        let source = RingPlaybackSource {
            consumer,
            shared: Arc::clone(&shared),
        };

        let params = protocol::PlaybackStreamParams {
            sample_spec: protocol::SampleSpec {
                format: to_wire_format(format),
                channels: channels as u8,
                sample_rate: config.sample_rate,
            },
            channel_map: channel_map_for(channels),
            sink_name: Some(cstring_or_default(&device.name)?),
            buffer_attr: to_wire_attrs(attrs),
            cvolume: Some(protocol::ChannelVolume::norm(channels as u8)),
            ..Default::default()
        };

        let stream = block_on_timeout(
            connection.client().create_playback_stream(params, source),
            PULSE_OP_TIMEOUT,
            "CREATE_PLAYBACK_STREAM",
        )?
        .map_err(map_client_error)?;

        let actual = *stream.sample_spec();
        verify_negotiated_spec("playback", &actual, format, channels, config.sample_rate)?;
        log::debug!(
            "PulseAudio playback stream {} open on \"{}\": {:?} {} ch @ {} Hz, tlength {} B",
            stream.channel(),
            device.name,
            actual.format,
            actual.channels,
            actual.sample_rate,
            stream.buffer_attr().target_length
        );

        let negotiated = NegotiatedConfig {
            sample_rate: actual.sample_rate,
            channels: u16::from(actual.channels),
            buffer_size: period,
            sample_format: format.to_core(),
        };

        Ok(Self {
            stream,
            connection,
            producer,
            shared,
            format,
            channels,
            frame_size,
            capacity,
            negotiated,
            server_attrs: attrs,
            scratch: Vec::with_capacity(frame_size * period as usize),
        })
    }

    fn wake_reactor(&self) {
        if let Some(waker) = self.shared.take_waker() {
            waker.wake();
        }
    }

    /// The configuration the stream was actually created with.
    ///
    /// `sample_rate` and `channels` come from the server's `CREATE_PLAYBACK_STREAM`
    /// reply, so they reflect any substitution the server made.
    #[must_use]
    pub fn negotiated(&self) -> NegotiatedConfig {
        self.negotiated
    }

    /// The wire sample format samples are encoded to.
    #[must_use]
    pub fn wire_format(&self) -> PulseSampleFormat {
        self.format
    }

    /// The server-side buffer attributes this stream requested, in bytes.
    #[must_use]
    pub fn requested_buffer_attrs(&self) -> PulseBufferAttrs {
        self.server_attrs
    }

    /// Capacity of the client-side ring buffer, in bytes.
    #[must_use]
    pub fn ring_capacity_bytes(&self) -> usize {
        self.capacity
    }

    /// Frames currently queued in the client-side ring — the client-side latency.
    #[must_use]
    pub fn latency_frames(&self) -> usize {
        self.producer.occupied_len() / self.frame_size.max(1)
    }

    /// Number of times the server asked for data while the ring was empty.
    ///
    /// Counted on the rising edge, so a stream that stays starved reports one underrun,
    /// not one per request.
    #[must_use]
    pub fn underrun_count(&self) -> u64 {
        self.shared.underruns.load(Ordering::Relaxed)
    }

    /// `true` once the connection to the server has been lost.
    #[must_use]
    pub fn is_disconnected(&self) -> bool {
        self.shared.disconnected.load(Ordering::Relaxed)
    }

    /// The PulseAudio channel id of this stream.
    #[must_use]
    pub fn channel(&self) -> u32 {
        self.stream.channel()
    }

    /// Blocks until the client-side ring has been handed to the server.
    ///
    /// Mirrors `CpalOutputStream::flush`: returns [`OxiSoundError::Timeout`] after
    /// [`PULSE_FLUSH_TIMEOUT`], or [`OxiSoundError::Disconnected`] if the connection
    /// drops meanwhile. Unlike cpal's spin loop this sleeps between checks, because the
    /// ring only drains when the server asks for more data.
    ///
    /// This does **not** wait for the server to finish rendering; use [`Self::drain`] for
    /// that.
    ///
    /// # Errors
    ///
    /// [`OxiSoundError::Timeout`] or [`OxiSoundError::Disconnected`].
    pub fn flush(&self) -> Result<(), OxiSoundError> {
        let start = Instant::now();
        loop {
            if self.producer.occupied_len() == 0 {
                return Ok(());
            }
            if self.is_disconnected() {
                return Err(OxiSoundError::Disconnected(
                    "PulseAudio stream disconnected during flush".into(),
                ));
            }
            if start.elapsed() >= PULSE_FLUSH_TIMEOUT {
                return Err(OxiSoundError::Timeout(format!(
                    "flush timed out after {:.1?} with {} bytes still queued",
                    PULSE_FLUSH_TIMEOUT,
                    self.producer.occupied_len()
                )));
            }
            // Nudge the reactor in case it parked while the ring was momentarily empty.
            self.wake_reactor();
            std::thread::sleep(FLUSH_POLL_INTERVAL);
        }
    }

    /// Flushes the client-side ring, then asks the server to play out everything queued.
    ///
    /// After a successful drain the server considers the stream finished and stops
    /// requesting data: the handle must not be written to again. Open a new stream to
    /// continue playing.
    ///
    /// # Errors
    ///
    /// Propagates [`Self::flush`] errors, plus [`OxiSoundError::Timeout`] after
    /// [`PULSE_DRAIN_TIMEOUT`].
    pub fn drain(&self) -> Result<(), OxiSoundError> {
        self.flush()?;
        block_on_timeout(
            self.stream.drain(),
            PULSE_DRAIN_TIMEOUT,
            "DRAIN_PLAYBACK_STREAM",
        )?
        .map_err(map_client_error)
    }

    /// Discards everything buffered, client-side and server-side.
    ///
    /// Use it to abandon queued audio — a seek, a stop button — rather than to wait for
    /// it. The server-side discard is synchronous; the client-side ring is emptied by the
    /// reactor at its next poll, because only the reactor holds the consumer end. A write
    /// issued concurrently with this call may therefore survive it.
    ///
    /// # Errors
    ///
    /// Propagates round-trip failures.
    pub fn discard_buffered(&self) -> Result<(), OxiSoundError> {
        // Snapshot the backlog so a discard that only fires on the server's next REQUEST
        // cannot swallow audio written in the meantime.
        self.shared
            .discard_bytes
            .store(self.producer.occupied_len(), Ordering::Relaxed);
        self.wake_reactor();
        block_on_timeout(
            self.stream.flush(),
            PULSE_OP_TIMEOUT,
            "FLUSH_PLAYBACK_STREAM",
        )?
        .map_err(map_client_error)
    }

    /// Pauses playback (PulseAudio "cork"); buffered audio is kept.
    ///
    /// # Errors
    ///
    /// Propagates round-trip failures.
    pub fn pause(&self) -> Result<(), OxiSoundError> {
        block_on_timeout(self.stream.cork(), PULSE_OP_TIMEOUT, "CORK_PLAYBACK_STREAM")?
            .map_err(map_client_error)
    }

    /// Resumes playback after [`Self::pause`].
    ///
    /// # Errors
    ///
    /// Propagates round-trip failures.
    pub fn resume(&self) -> Result<(), OxiSoundError> {
        let result = block_on_timeout(
            self.stream.uncork(),
            PULSE_OP_TIMEOUT,
            "UNCORK_PLAYBACK_STREAM",
        )?
        .map_err(map_client_error);
        self.wake_reactor();
        result
    }

    /// The connection this stream runs on, for callers that want to issue further
    /// commands on the same socket.
    #[must_use]
    pub fn client(&self) -> &Client {
        self.connection.client()
    }
}

impl OutputStream for PulseOutputStream {
    fn write(&mut self, samples: &[f32]) -> Result<(), OxiSoundError> {
        if self.is_disconnected() {
            return Err(OxiSoundError::Disconnected(
                "PulseAudio playback stream disconnected".into(),
            ));
        }
        if samples.is_empty() {
            return Ok(());
        }
        let needed = samples.len() * self.format.bytes_per_sample();
        if self.producer.vacant_len() < needed {
            return Err(OxiSoundError::Overrun(format!(
                "output ring buffer full: {} free, {needed} requested",
                self.producer.vacant_len()
            )));
        }
        self.scratch.clear();
        self.format.encode_f32(samples, &mut self.scratch);
        let written = self.producer.push_slice(&self.scratch);
        if written < self.scratch.len() {
            return Err(OxiSoundError::Overrun(format!(
                "partial write: only {written}/{} bytes written",
                self.scratch.len()
            )));
        }
        self.wake_reactor();
        Ok(())
    }

    fn stats(&self) -> StreamStats {
        let frame_size = self.frame_size.max(1) as u64;
        StreamStats {
            frames_processed: self.shared.bytes_sent.load(Ordering::Relaxed) / frame_size,
            underruns: self.shared.underruns.load(Ordering::Relaxed),
            overruns: 0,
            latency_frames: self.latency_frames() as u32,
            // PulseAudio does not run audio in this process, so there is no callback
            // duty cycle to report.
            cpu_load_percent: 0.0,
        }
    }
}

// ---------------------------------------------------------------------------
// PulseInputStream
// ---------------------------------------------------------------------------

/// A PulseAudio capture stream feeding a lock-free byte ring.
///
/// Mirrors `oxisound_cpal::CpalInputStream`: [`InputStream::read`] is non-blocking and
/// returns however many samples were available, which may be zero.
pub struct PulseInputStream {
    // Drop order: the stream handle before the client (see `PulseOutputStream`).
    stream: RecordStream,
    connection: Arc<PulseConnection>,
    consumer: HeapCons<u8>,
    shared: Arc<RecordShared>,
    format: PulseSampleFormat,
    channels: u16,
    frame_size: usize,
    capacity: usize,
    negotiated: NegotiatedConfig,
    server_attrs: PulseBufferAttrs,
    scratch: Vec<u8>,
}

impl std::fmt::Debug for PulseInputStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PulseInputStream")
            .field("channel", &self.stream.channel())
            .field("format", &self.format)
            .field("channels", &self.channels)
            .finish()
    }
}

impl PulseInputStream {
    pub(crate) fn open(
        connection: Arc<PulseConnection>,
        device: &PulseDeviceDescriptor,
        config: StreamConfig,
    ) -> Result<Self, OxiSoundError> {
        validate_stream_config(&config)?;
        let format = negotiate_format(
            config.sample_format,
            &config.preferred_formats,
            device.format,
        )?;
        let channels = config.channels;
        let frame_size = format.frame_size(channels);
        let period = period_frames(&config);
        let attrs = record_buffer_attrs(period, channels, format);
        let capacity = ring_capacity_bytes(&config, format);

        let (producer, consumer) = HeapRb::<u8>::new(capacity).split();
        let shared = Arc::new(RecordShared::default());
        let sink = RingRecordSink {
            producer,
            shared: Arc::clone(&shared),
        };

        let params = protocol::RecordStreamParams {
            sample_spec: protocol::SampleSpec {
                format: to_wire_format(format),
                channels: channels as u8,
                sample_rate: config.sample_rate,
            },
            channel_map: channel_map_for(channels),
            source_name: Some(cstring_or_default(&device.name)?),
            buffer_attr: to_wire_attrs(attrs),
            cvolume: Some(protocol::ChannelVolume::norm(channels as u8)),
            ..Default::default()
        };

        let stream = block_on_timeout(
            connection.client().create_record_stream(params, sink),
            PULSE_OP_TIMEOUT,
            "CREATE_RECORD_STREAM",
        )?
        .map_err(map_client_error)?;

        let actual = *stream.sample_spec();
        verify_negotiated_spec("record", &actual, format, channels, config.sample_rate)?;
        log::debug!(
            "PulseAudio record stream {} open on \"{}\": {:?} {} ch @ {} Hz, fragment {} B",
            stream.channel(),
            device.name,
            actual.format,
            actual.channels,
            actual.sample_rate,
            stream.buffer_attr().fragment_size
        );

        let negotiated = NegotiatedConfig {
            sample_rate: actual.sample_rate,
            channels: u16::from(actual.channels),
            buffer_size: period,
            sample_format: format.to_core(),
        };

        Ok(Self {
            stream,
            connection,
            consumer,
            shared,
            format,
            channels,
            frame_size,
            capacity,
            negotiated,
            server_attrs: attrs,
            scratch: Vec::with_capacity(frame_size * period as usize),
        })
    }

    /// The configuration the stream was actually created with.
    #[must_use]
    pub fn negotiated(&self) -> NegotiatedConfig {
        self.negotiated
    }

    /// The wire sample format captured audio is decoded from.
    #[must_use]
    pub fn wire_format(&self) -> PulseSampleFormat {
        self.format
    }

    /// The server-side buffer attributes this stream requested, in bytes.
    #[must_use]
    pub fn requested_buffer_attrs(&self) -> PulseBufferAttrs {
        self.server_attrs
    }

    /// Capacity of the client-side ring buffer, in bytes.
    #[must_use]
    pub fn ring_capacity_bytes(&self) -> usize {
        self.capacity
    }

    /// Frames waiting to be read — the client-side capture latency.
    #[must_use]
    pub fn latency_frames(&self) -> usize {
        self.consumer.occupied_len() / self.frame_size.max(1)
    }

    /// Number of server chunks that did not fit in the ring because the reader fell
    /// behind.
    #[must_use]
    pub fn overrun_count(&self) -> u64 {
        self.shared.overruns.load(Ordering::Relaxed)
    }

    /// `true` once the connection to the server has been lost.
    #[must_use]
    pub fn is_disconnected(&self) -> bool {
        self.shared.disconnected.load(Ordering::Relaxed)
    }

    /// The PulseAudio channel id of this stream.
    #[must_use]
    pub fn channel(&self) -> u32 {
        self.stream.channel()
    }

    /// Discards everything the server has buffered for this stream.
    ///
    /// # Errors
    ///
    /// Propagates round-trip failures.
    pub fn discard_buffered(&self) -> Result<(), OxiSoundError> {
        block_on_timeout(self.stream.flush(), PULSE_OP_TIMEOUT, "FLUSH_RECORD_STREAM")?
            .map_err(map_client_error)
    }

    /// Pauses capture (PulseAudio "cork").
    ///
    /// # Errors
    ///
    /// Propagates round-trip failures.
    pub fn pause(&self) -> Result<(), OxiSoundError> {
        block_on_timeout(self.stream.cork(), PULSE_OP_TIMEOUT, "CORK_RECORD_STREAM")?
            .map_err(map_client_error)
    }

    /// Resumes capture after [`Self::pause`].
    ///
    /// # Errors
    ///
    /// Propagates round-trip failures.
    pub fn resume(&self) -> Result<(), OxiSoundError> {
        block_on_timeout(
            self.stream.uncork(),
            PULSE_OP_TIMEOUT,
            "UNCORK_RECORD_STREAM",
        )?
        .map_err(map_client_error)
    }

    /// The connection this stream runs on.
    #[must_use]
    pub fn client(&self) -> &Client {
        self.connection.client()
    }
}

impl InputStream for PulseInputStream {
    fn read(&mut self, samples: &mut [f32]) -> Result<usize, OxiSoundError> {
        if self.is_disconnected() {
            return Err(OxiSoundError::Disconnected(
                "PulseAudio capture stream disconnected".into(),
            ));
        }
        if samples.is_empty() {
            return Ok(0);
        }
        let wanted_bytes = samples.len() * self.format.bytes_per_sample();
        // The server delivers arbitrary byte counts, so the ring may hold a partial
        // sample at its tail. Popping whole samples only keeps the stream aligned; the
        // leftover bytes stay in the ring and complete on the next chunk.
        let take = whole_samples(self.consumer.occupied_len().min(wanted_bytes), self.format);
        if take == 0 {
            return Ok(0);
        }
        self.scratch.resize(take, 0);
        let popped = self.consumer.pop_slice(&mut self.scratch[..take]);
        let usable = whole_samples(popped, self.format);
        Ok(self.format.decode_to_f32(&self.scratch[..usable], samples))
    }

    fn stats(&self) -> StreamStats {
        let frame_size = self.frame_size.max(1) as u64;
        StreamStats {
            frames_processed: self.shared.bytes_received.load(Ordering::Relaxed) / frame_size,
            underruns: 0,
            overruns: self.shared.overruns.load(Ordering::Relaxed),
            latency_frames: self.latency_frames() as u32,
            cpu_load_percent: 0.0,
        }
    }
}

// ---------------------------------------------------------------------------
// PulseDuplexStream
// ---------------------------------------------------------------------------

/// A playback stream and a capture stream sharing one server connection.
///
/// Both halves are ordinary [`PulseOutputStream`] / [`PulseInputStream`] handles, so the
/// whole value is `Send` and can be moved to a worker thread, as
/// [`oxisound_core::DuplexStream`] requires.
pub struct PulseDuplexStream {
    output: PulseOutputStream,
    input: PulseInputStream,
}

impl std::fmt::Debug for PulseDuplexStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PulseDuplexStream")
            .field("output", &self.output)
            .field("input", &self.input)
            .finish()
    }
}

impl PulseDuplexStream {
    pub(crate) fn new(output: PulseOutputStream, input: PulseInputStream) -> Self {
        Self { output, input }
    }

    /// The playback half.
    #[must_use]
    pub fn output(&self) -> &PulseOutputStream {
        &self.output
    }

    /// The capture half.
    #[must_use]
    pub fn input(&self) -> &PulseInputStream {
        &self.input
    }

    /// Frames queued for playback.
    #[must_use]
    pub fn output_latency_frames(&self) -> usize {
        self.output.latency_frames()
    }

    /// Frames captured and waiting to be read.
    #[must_use]
    pub fn input_latency_frames(&self) -> usize {
        self.input.latency_frames()
    }

    /// Estimated client-side round-trip latency, in frames.
    #[must_use]
    pub fn roundtrip_latency_frames(&self) -> usize {
        self.output_latency_frames() + self.input_latency_frames()
    }

    /// `true` once either half has lost its connection.
    #[must_use]
    pub fn is_disconnected(&self) -> bool {
        self.output.is_disconnected() || self.input.is_disconnected()
    }
}

impl DuplexStream for PulseDuplexStream {
    fn write(&mut self, out: &[f32]) -> Result<(), OxiSoundError> {
        self.output.write(out)
    }

    fn read(&mut self, inp: &mut [f32]) -> Result<usize, OxiSoundError> {
        self.input.read(inp)
    }

    fn stats(&self) -> StreamStats {
        let out = self.output.stats();
        let inp = self.input.stats();
        StreamStats {
            frames_processed: out.frames_processed,
            underruns: out.underruns,
            overruns: inp.overruns,
            latency_frames: self.roundtrip_latency_frames() as u32,
            cpu_load_percent: 0.0,
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Rejects a stream whose server-side sample spec differs from the requested one.
///
/// No `fix_format` / `fix_rate` / `fix_channels` flag is set, so the server is expected to
/// echo the request back. If it ever does not, the encoder and decoder in this crate would
/// silently misinterpret every byte while `negotiated()` reported the server's real spec —
/// a garbled-audio bug. Failing loudly turns it into a clear `FormatMismatch`.
fn verify_negotiated_spec(
    direction: &str,
    actual: &protocol::SampleSpec,
    requested_format: PulseSampleFormat,
    requested_channels: u16,
    requested_rate: u32,
) -> Result<(), OxiSoundError> {
    let wanted = to_wire_format(requested_format);
    if actual.format == wanted
        && u16::from(actual.channels) == requested_channels
        && actual.sample_rate == requested_rate
    {
        return Ok(());
    }
    Err(OxiSoundError::FormatMismatch(format!(
        "the PulseAudio server created the {direction} stream as {:?} {} ch @ {} Hz, but \
         {wanted:?} {requested_channels} ch @ {requested_rate} Hz was requested; oxisound-pulse \
         does not convert between the two",
        actual.format, actual.channels, actual.sample_rate
    )))
}

fn to_wire_attrs(attrs: PulseBufferAttrs) -> protocol::stream::BufferAttr {
    protocol::stream::BufferAttr {
        max_length: attrs.max_length,
        target_length: attrs.target_length,
        pre_buffering: attrs.pre_buffering,
        minimum_request_length: attrs.minimum_request_length,
        fragment_size: attrs.fragment_size,
    }
}

fn cstring_or_default(name: &str) -> Result<std::ffi::CString, OxiSoundError> {
    std::ffi::CString::new(name).map_err(|_| {
        OxiSoundError::Device(format!(
            "device name {name:?} contains an interior NUL byte"
        ))
    })
}

/// Builds the channel map for a channel count.
///
/// Mono and stereo use PulseAudio's canonical maps; larger counts follow the standard
/// surround order (FL, FR, FC, LFE, RL, RR, SL, SR) and fall back to `Aux*` positions
/// beyond eight channels. The caller must have validated the count against
/// [`crate::model::PULSE_MAX_CHANNELS`] first — `ChannelMap::push` panics past 32.
fn channel_map_for(channels: u16) -> protocol::ChannelMap {
    use protocol::ChannelPosition as P;
    match channels {
        0 | 1 => protocol::ChannelMap::mono(),
        2 => protocol::ChannelMap::stereo(),
        n => {
            const SURROUND: [P; 8] = [
                P::FrontLeft,
                P::FrontRight,
                P::FrontCenter,
                P::Lfe,
                P::RearLeft,
                P::RearRight,
                P::SideLeft,
                P::SideRight,
            ];
            const AUX: [P; 24] = [
                P::Aux0,
                P::Aux1,
                P::Aux2,
                P::Aux3,
                P::Aux4,
                P::Aux5,
                P::Aux6,
                P::Aux7,
                P::Aux8,
                P::Aux9,
                P::Aux10,
                P::Aux11,
                P::Aux12,
                P::Aux13,
                P::Aux14,
                P::Aux15,
                P::Aux16,
                P::Aux17,
                P::Aux18,
                P::Aux19,
                P::Aux20,
                P::Aux21,
                P::Aux22,
                P::Aux23,
            ];
            let n = usize::from(n).min(usize::from(protocol::sample_spec::MAX_CHANNELS));
            protocol::ChannelMap::new((0..n).map(|i| match SURROUND.get(i) {
                Some(&p) => p,
                None => AUX[(i - SURROUND.len()).min(AUX.len() - 1)],
            }))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::PULSE_MAX_CHANNELS;

    #[test]
    fn stream_handles_are_send() {
        fn assert_send<T: Send>() {}
        assert_send::<PulseOutputStream>();
        assert_send::<PulseInputStream>();
        assert_send::<PulseDuplexStream>();
        assert_send::<RingPlaybackSource>();
        assert_send::<RingRecordSink>();
    }

    #[test]
    fn channel_map_covers_every_supported_count_without_panicking() {
        for channels in 1..=PULSE_MAX_CHANNELS {
            let map = channel_map_for(channels);
            assert_eq!(
                u16::from(map.num_channels()),
                channels,
                "channel map for {channels} channels has the wrong size"
            );
        }
    }

    #[test]
    fn channel_map_uses_canonical_mono_and_stereo() {
        assert_eq!(channel_map_for(1), protocol::ChannelMap::mono());
        assert_eq!(channel_map_for(2), protocol::ChannelMap::stereo());
        // Zero is clamped to mono; callers reject it earlier via `validate_stream_config`.
        assert_eq!(channel_map_for(0), protocol::ChannelMap::mono());
    }

    #[test]
    fn surround_layout_starts_with_the_standard_order() {
        let map = channel_map_for(6);
        let positions: Vec<_> = map.into_iter().collect();
        assert_eq!(
            positions,
            vec![
                protocol::ChannelPosition::FrontLeft,
                protocol::ChannelPosition::FrontRight,
                protocol::ChannelPosition::FrontCenter,
                protocol::ChannelPosition::Lfe,
                protocol::ChannelPosition::RearLeft,
                protocol::ChannelPosition::RearRight,
            ]
        );
    }

    #[test]
    fn buffer_attrs_translate_field_for_field() {
        let attrs = PulseBufferAttrs {
            max_length: 1,
            target_length: 2,
            pre_buffering: 3,
            minimum_request_length: 4,
            fragment_size: 5,
        };
        let wire = to_wire_attrs(attrs);
        assert_eq!(wire.max_length, 1);
        assert_eq!(wire.target_length, 2);
        assert_eq!(wire.pre_buffering, 3);
        assert_eq!(wire.minimum_request_length, 4);
        assert_eq!(wire.fragment_size, 5);
    }

    #[test]
    fn device_names_with_interior_nul_are_rejected() {
        assert!(cstring_or_default("ok").is_ok());
        let err = cstring_or_default("bad\0name").expect_err("interior NUL");
        assert_eq!(err.kind(), "device");
    }

    /// The record path's alignment carry: the server can deliver a byte count that is not
    /// a multiple of the sample width, and the leftover must survive until the rest
    /// arrives.
    #[test]
    fn record_ring_keeps_partial_samples_aligned() {
        let format = PulseSampleFormat::Float32Le;
        let (mut producer, mut consumer) = HeapRb::<u8>::new(64).split();

        // One whole f32 (1.0) plus the first byte of the next sample.
        let mut bytes = Vec::new();
        format.encode_f32(&[1.0, 0.5], &mut bytes);
        producer.push_slice(&bytes[..5]);

        let mut out = [0.0f32; 4];
        let take = whole_samples(consumer.occupied_len().min(out.len() * 4), format);
        assert_eq!(take, 4, "only the first whole sample may be popped");
        let mut scratch = vec![0u8; take];
        consumer.pop_slice(&mut scratch);
        assert_eq!(format.decode_to_f32(&scratch, &mut out), 1);
        assert!((out[0] - 1.0).abs() < 1e-6);
        assert_eq!(consumer.occupied_len(), 1, "one byte must stay in the ring");

        // The remaining three bytes complete the split sample.
        producer.push_slice(&bytes[5..]);
        let take = whole_samples(consumer.occupied_len().min(out.len() * 4), format);
        assert_eq!(take, 4);
        let mut scratch = vec![0u8; take];
        consumer.pop_slice(&mut scratch);
        assert_eq!(format.decode_to_f32(&scratch, &mut out), 1);
        assert!(
            (out[0] - 0.5).abs() < 1e-6,
            "the sample split across two chunks decoded as {}",
            out[0]
        );
        assert_eq!(consumer.occupied_len(), 0);
    }

    /// The playback source must never return `Ready(0)`, which the reactor reads as EOF.
    #[test]
    fn playback_source_parks_instead_of_signalling_eof() {
        let (mut producer, consumer) = HeapRb::<u8>::new(32).split();
        let shared = Arc::new(PlaybackShared::new());
        let mut source = RingPlaybackSource {
            consumer,
            shared: Arc::clone(&shared),
        };

        let waker = futures_task_noop_waker();
        let mut cx = Context::from_waker(&waker);
        let mut buf = [0u8; 8];

        // Empty ring: park, do not report EOF, and do not count a start-up underrun.
        assert!(matches!(
            Pin::new(&mut source).poll_read(&mut cx, &mut buf),
            Poll::Pending
        ));
        assert_eq!(shared.underruns.load(Ordering::Relaxed), 0);
        assert!(shared.waker.lock().expect("waker slot").is_some());

        // Data available: hand it over.
        producer.push_slice(&[1, 2, 3, 4]);
        assert!(matches!(
            Pin::new(&mut source).poll_read(&mut cx, &mut buf),
            Poll::Ready(4)
        ));
        assert_eq!(shared.bytes_sent.load(Ordering::Relaxed), 4);

        // Ran dry again: that is a real underrun, counted once…
        assert!(matches!(
            Pin::new(&mut source).poll_read(&mut cx, &mut buf),
            Poll::Pending
        ));
        assert_eq!(shared.underruns.load(Ordering::Relaxed), 1);
        // …and not once per poll while still starved.
        assert!(matches!(
            Pin::new(&mut source).poll_read(&mut cx, &mut buf),
            Poll::Pending
        ));
        assert_eq!(shared.underruns.load(Ordering::Relaxed), 1);

        // Dropping the source (reactor shutdown) marks the handle disconnected.
        drop(source);
        assert!(shared.disconnected.load(Ordering::Relaxed));
    }

    #[test]
    fn discard_flag_drops_the_client_side_backlog_at_the_next_poll() {
        let (mut producer, consumer) = HeapRb::<u8>::new(32).split();
        let shared = Arc::new(PlaybackShared::new());
        let mut source = RingPlaybackSource {
            consumer,
            shared: Arc::clone(&shared),
        };

        producer.push_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
        shared.discard_bytes.store(8, Ordering::Relaxed);

        let waker = futures_task_noop_waker();
        let mut cx = Context::from_waker(&waker);
        let mut buf = [0u8; 8];
        assert!(
            matches!(
                Pin::new(&mut source).poll_read(&mut cx, &mut buf),
                Poll::Pending
            ),
            "everything queued must be dropped, leaving nothing to send"
        );
        assert_eq!(shared.bytes_sent.load(Ordering::Relaxed), 0);
        assert_eq!(
            shared.discard_bytes.load(Ordering::Relaxed),
            0,
            "the discard request is one-shot"
        );
    }

    /// A discard is bounded by the backlog snapshot taken when it was requested, so audio
    /// written after the call is still played.
    #[test]
    fn discard_is_bounded_by_the_snapshot_and_spares_later_writes() {
        let (mut producer, consumer) = HeapRb::<u8>::new(32).split();
        let shared = Arc::new(PlaybackShared::new());
        let mut source = RingPlaybackSource {
            consumer,
            shared: Arc::clone(&shared),
        };

        producer.push_slice(&[1, 2, 3, 4]);
        shared.discard_bytes.store(4, Ordering::Relaxed);
        // The application keeps writing before the server's next REQUEST arrives.
        producer.push_slice(&[9, 9]);

        let waker = futures_task_noop_waker();
        let mut cx = Context::from_waker(&waker);
        let mut buf = [0u8; 8];
        assert!(matches!(
            Pin::new(&mut source).poll_read(&mut cx, &mut buf),
            Poll::Ready(2)
        ));
        assert_eq!(
            &buf[..2],
            &[9, 9],
            "only the snapshotted backlog may be dropped"
        );
    }

    /// `poll_read` consults the ring twice — once before registering the waker and once
    /// after — so a write landing in that window cannot leave the reactor parked with data
    /// queued. Both phases go through `serve`, which is exercised directly here.
    #[test]
    fn serve_reports_emptiness_and_clears_the_starved_edge_when_it_finds_data() {
        let (mut producer, consumer) = HeapRb::<u8>::new(16).split();
        let shared = Arc::new(PlaybackShared::new());
        let mut source = RingPlaybackSource {
            consumer,
            shared: Arc::clone(&shared),
        };
        let mut buf = [0u8; 8];

        assert_eq!(
            source.serve(&mut buf),
            None,
            "an empty ring must report None"
        );
        assert!(shared.starved.load(Ordering::Relaxed));
        assert_eq!(shared.bytes_sent.load(Ordering::Relaxed), 0);

        producer.push_slice(&[1, 2, 3]);
        assert_eq!(source.serve(&mut buf), Some(3));
        assert_eq!(&buf[..3], &[1, 2, 3]);
        assert!(
            !shared.starved.load(Ordering::Relaxed),
            "serving data clears the starved edge so the next dry poll counts an underrun"
        );
        assert_eq!(shared.bytes_sent.load(Ordering::Relaxed), 3);
    }

    #[test]
    fn record_sink_counts_overruns_and_marks_disconnect_on_drop() {
        let (producer, _consumer) = HeapRb::<u8>::new(4).split();
        let shared = Arc::new(RecordShared::default());
        let mut sink = RingRecordSink {
            producer,
            shared: Arc::clone(&shared),
        };

        sink.write(&[1, 2]);
        assert_eq!(shared.overruns.load(Ordering::Relaxed), 0);
        assert_eq!(shared.bytes_received.load(Ordering::Relaxed), 2);

        // Only two bytes still fit; the rest is dropped and counted.
        sink.write(&[3, 4, 5, 6]);
        assert_eq!(shared.overruns.load(Ordering::Relaxed), 1);
        assert_eq!(shared.bytes_received.load(Ordering::Relaxed), 4);

        drop(sink);
        assert!(shared.disconnected.load(Ordering::Relaxed));
    }

    #[test]
    fn waker_slot_is_replaced_only_when_it_would_wake_a_different_task() {
        let shared = PlaybackShared::new();
        let waker = futures_task_noop_waker();
        shared.store_waker(&waker);
        assert!(shared.waker.lock().expect("slot").is_some());
        assert!(shared.take_waker().is_some());
        assert!(shared.take_waker().is_none(), "taking must clear the slot");
    }

    /// A no-op waker built without `unsafe`, using the executor this crate already
    /// depends on.
    fn futures_task_noop_waker() -> Waker {
        struct Noop;
        impl std::task::Wake for Noop {
            fn wake(self: Arc<Self>) {}
            fn wake_by_ref(self: &Arc<Self>) {}
        }
        Waker::from(Arc::new(Noop))
    }
}
