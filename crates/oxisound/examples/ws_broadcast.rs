//! Async capture -> WebSocket broadcast for remote audio monitoring.
//!
//! Usage: cargo run --example ws_broadcast --features tokio
//!
//! Starts a plain WebSocket server on 127.0.0.1:9001.
//! Any connected client receives raw audio blocks as binary frames (f32 LE).
//! An ASCII RMS level meter is printed to stdout for each block.
//! The server stops automatically after 10 seconds.

#[tokio::main(flavor = "current_thread")]
async fn main() {
    println!("WS audio broadcast — ws://127.0.0.1:9001");
    println!("Connect with any WebSocket client to receive raw f32 LE audio.");
    println!("Stops after 10 seconds.\n");

    let listener = match tokio::net::TcpListener::bind("127.0.0.1:9001").await {
        Ok(l) => l,
        Err(e) => {
            println!("Failed to bind 127.0.0.1:9001: {e}");
            return;
        }
    };

    let config = oxisound::StreamConfig::mono_16k();

    let stream = match oxisound::capture_stream(config) {
        Ok(s) => s,
        Err(e) => {
            println!("Failed to open capture stream: {e}");
            return;
        }
    };

    let (tx, _rx) = tokio::sync::broadcast::channel::<Vec<u8>>(64);
    let tx_accept = tx.clone();

    tokio::spawn(async move {
        loop {
            match listener.accept().await {
                Ok((sock, addr)) => {
                    println!("Client connected: {addr}");
                    let mut rx = tx_accept.subscribe();
                    tokio::spawn(async move {
                        let ws = match tokio_tungstenite::accept_async(sock).await {
                            Ok(w) => w,
                            Err(e) => {
                                println!("WebSocket handshake failed from {addr}: {e}");
                                return;
                            }
                        };
                        use futures_util::SinkExt;
                        let (mut sink, _) = futures_util::StreamExt::split(ws);
                        loop {
                            match rx.recv().await {
                                Ok(bytes) => {
                                    let msg = tokio_tungstenite::tungstenite::Message::Binary(
                                        bytes.into(),
                                    );
                                    if sink.send(msg).await.is_err() {
                                        break;
                                    }
                                }
                                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                                    continue;
                                }
                                Err(_) => break,
                            }
                        }
                        println!("Client disconnected: {addr}");
                    });
                }
                Err(e) => {
                    println!("Accept error: {e}");
                }
            }
        }
    });

    tokio::pin!(stream);

    let duration = std::time::Duration::from_secs(10);
    let start = std::time::Instant::now();

    let _ = tokio::time::timeout(duration, async {
        use futures_core::stream::Stream;
        use std::future::poll_fn;
        use std::task::Poll;

        loop {
            let chunk: Option<Vec<f32>> = poll_fn(|cx| match stream.as_mut().poll_next(cx) {
                Poll::Ready(v) => Poll::Ready(v),
                Poll::Pending => {
                    cx.waker().wake_by_ref();
                    Poll::Pending
                }
            })
            .await;

            match chunk {
                Some(samples) => {
                    let bytes: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
                    let _ = tx.send(bytes);
                    print_level(compute_rms(&samples), start.elapsed());
                }
                None => break,
            }
        }
    })
    .await;

    println!("\nDone.");
}

fn compute_rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let mean_sq = samples.iter().map(|&s| s * s).sum::<f32>() / samples.len() as f32;
    mean_sq.sqrt()
}

fn print_level(rms: f32, elapsed: std::time::Duration) {
    let db = if rms > 1e-10 {
        20.0 * rms.log10()
    } else {
        -60.0_f32
    };
    let bars = ((db + 60.0) / 3.0).max(0.0) as usize;
    let meter: String = "#".repeat(bars.min(20));
    println!(
        "{:.1}s |{:<20}| {:.1} dBFS",
        elapsed.as_secs_f32(),
        meter,
        db
    );
}
