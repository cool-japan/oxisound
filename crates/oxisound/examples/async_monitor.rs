//! Async audio level monitor: captures audio from the default input device and
//! displays a live RMS level meter as an ASCII bar graph.
//!
//! Usage: cargo run --example async_monitor --features tokio
//!
//! Runs for ~5 seconds then exits.

// capture_stream returns `impl futures_core::Stream<Item = Vec<f32>>`.
// We pin the stream onto the stack and poll it via the futures_core::Stream trait
// so we do not need to import tokio_stream::StreamExt explicitly.
// tokio::time::timeout provides the 5-second deadline.

#[tokio::main(flavor = "current_thread")]
async fn main() {
    println!("Audio level monitor (5 seconds)");
    println!("Legend: each '|' ≈ 3 dB above -60 dBFS\n");

    let config = oxisound::StreamConfig::stereo_48k();

    // capture_stream is a non-async function that returns an `impl Stream<Item = Vec<f32>>`.
    // Errors surface here (e.g., no microphone permission, no input device).
    let stream = match oxisound::capture_stream(config) {
        Ok(s) => s,
        Err(e) => {
            println!("Failed to open capture stream: {e}");
            return;
        }
    };

    let start = std::time::Instant::now();
    let duration = std::time::Duration::from_secs(5);

    // Pin the stream onto the stack; futures_core::Stream requires Pin<&mut Self> to poll.
    // After tokio::pin!, `stream` is already `Pin<&mut impl Stream>` — do NOT wrap in Pin::new again.
    tokio::pin!(stream);

    // tokio::time::timeout stops the async block after 5 seconds regardless of stream state.
    // We discard the Err(Elapsed) — a timeout is the expected exit condition here.
    let _ = tokio::time::timeout(duration, async {
        use std::future::poll_fn;
        use std::task::Poll;
        // Bring poll_next into scope from the futures_core::Stream trait.
        use futures_core::stream::Stream;

        loop {
            // poll_fn wraps a one-shot poll call into a Future that the executor can await.
            // `stream` is already pinned by tokio::pin!, so we reborrow it as Pin<&mut _>.
            let chunk: Option<Vec<f32>> = poll_fn(|cx| {
                // Re-register the waker on Pending so the executor reschedules us
                // once the audio thread delivers the next frame batch.
                match stream.as_mut().poll_next(cx) {
                    Poll::Ready(v) => Poll::Ready(v),
                    Poll::Pending => {
                        cx.waker().wake_by_ref();
                        Poll::Pending
                    }
                }
            })
            .await;

            match chunk {
                Some(samples) => {
                    let rms = compute_rms(&samples);
                    print_level(rms, start.elapsed());
                }
                // Stream closed — input device removed or hardware error.
                None => break,
            }
        }
    })
    .await;

    println!("\nDone.");
}

/// Root-mean-square amplitude of a sample block.
/// Used as a proxy for perceived loudness over a short window.
fn compute_rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let mean_sq = samples.iter().map(|&s| s * s).sum::<f32>() / samples.len() as f32;
    mean_sq.sqrt()
}

/// Print a single line showing elapsed time, a bar-graph, and dBFS value.
fn print_level(rms: f32, elapsed: std::time::Duration) {
    // 1e-10 threshold prevents log10(0) → -inf.
    let db = if rms > 1e-10 {
        20.0 * rms.log10()
    } else {
        -60.0_f32
    };
    // Map [-60, 0] dBFS linearly to [0, 20] bars (each bar ≈ 3 dB).
    let bars = ((db + 60.0) / 3.0).max(0.0) as usize;
    let meter: String = "|".repeat(bars.min(20));
    println!(
        "{:.1}s [{:<20}] {:.1} dBFS",
        elapsed.as_secs_f32(),
        meter,
        db
    );
}
