//! Async stream types for the tokio feature.

use crate::streams::CpalOutputStream;
use oxisound_core::OutputStream;
use oxisound_core::OxiSoundError;

/// Async output stream wrapping [`CpalOutputStream`].
///
/// Implements `oxisound_core::AsyncOutputStream`. The underlying `write` call is
/// non-blocking (it pushes samples into a lock-free ring buffer), so it is safe to call
/// from async contexts without spawning a blocking task.
pub struct CpalAsyncOutputStream {
    pub(crate) inner: CpalOutputStream,
}

impl oxisound_core::AsyncOutputStream for CpalAsyncOutputStream {
    async fn write(&mut self, samples: &[f32]) -> Result<(), OxiSoundError> {
        self.inner.write(samples)
    }
}

/// Async input stream that produces frames via a tokio mpsc channel.
///
/// Implements `oxisound_core::AsyncInputStream`.
pub struct CpalAsyncInputStream {
    /// Keeps the underlying cpal stream alive.
    pub(crate) _stream: cpal::Stream,
    pub(crate) receiver: tokio::sync::mpsc::UnboundedReceiver<Vec<f32>>,
}

impl oxisound_core::AsyncInputStream for CpalAsyncInputStream {
    fn stream(&mut self) -> impl futures_core::Stream<Item = Vec<f32>> + '_ {
        CpalReceiverStream {
            recv: &mut self.receiver,
        }
    }
}

struct CpalReceiverStream<'a> {
    recv: &'a mut tokio::sync::mpsc::UnboundedReceiver<Vec<f32>>,
}

impl futures_core::Stream for CpalReceiverStream<'_> {
    type Item = Vec<f32>;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Vec<f32>>> {
        self.recv.poll_recv(cx)
    }
}

/// Implement `futures_core::Stream` directly on `CpalAsyncInputStream` so the
/// facade can return `impl Stream<Item = Vec<f32>>` with `'static` lifetime
/// (no borrow from `self`).
impl futures_core::Stream for CpalAsyncInputStream {
    type Item = Vec<f32>;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Vec<f32>>> {
        self.receiver.poll_recv(cx)
    }
}
