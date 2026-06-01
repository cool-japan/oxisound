//! Decode an audio file with oxiaudio and play it back through oxisound.
//!
//! Usage: cargo run --example decode_play --features pure -- /path/to/audio.wav
//! Without a path argument: plays a synthetic 440 Hz sine tone instead.

fn main() {
    let args: Vec<String> = std::env::args().collect();

    if args.len() > 1 {
        let path = &args[1];

        // Decode the file using oxiaudio — returns AudioBuffer<f32> with interleaved samples.
        let buffer = match oxiaudio::decode_file(std::path::Path::new(path)) {
            Ok(b) => b,
            Err(e) => {
                println!("Failed to decode {path}: {e}");
                return;
            }
        };

        // ChannelLayout is an enum, not a u16 — use channel_count() to get the numeric count.
        let n_channels = buffer.channels.channel_count() as u16;
        println!(
            "Decoded: {} frames, {} ch, {} Hz",
            buffer.frame_count(),
            n_channels,
            buffer.sample_rate
        );

        // Build a StreamConfig whose sample rate and channel count match the decoded audio.
        // The device must support these exact values; a mismatch will surface as open_output Err.
        let config = oxisound::StreamConfig {
            sample_rate: buffer.sample_rate,
            channels: n_channels,
            ..oxisound::StreamConfig::STEREO_48K
        };

        let mut output = match oxisound::open_output(config) {
            Ok(o) => o,
            Err(e) => {
                println!("Failed to open output: {e}");
                return;
            }
        };

        // Write the complete decoded sample block into the ring buffer.
        // The ring buffer streams to the hardware asynchronously.
        if let Err(e) = output.write(&buffer.samples) {
            println!("Write error: {e}");
            return;
        }

        // Sleep just past the audio duration so the ring buffer drains fully before exit.
        let duration_secs = buffer.duration_secs();
        std::thread::sleep(std::time::Duration::from_secs_f64(duration_secs + 0.5));
        println!("Playback complete.");
    } else {
        // No file argument supplied — synthesise a 440 Hz sine tone so the example runs standalone.
        println!("No file argument. Playing 440 Hz sine tone for 2 seconds.");
        println!("Usage: cargo run --example decode_play --features pure -- <audio_file>");

        let config = oxisound::StreamConfig::stereo_48k();
        let mut output = match oxisound::open_output(config.clone()) {
            Ok(o) => o,
            Err(e) => {
                println!("Failed to open output: {e}");
                return;
            }
        };

        // Use the facade's built-in sine generator — no oxiaudio dependency needed.
        let samples = oxisound::sine_test_tone(440.0, 2.0, config);

        if let Err(e) = output.write(&samples) {
            println!("Write error: {e}");
            return;
        }

        // 2 s of audio + 0.5 s tail to let the ring buffer drain before the stream is dropped.
        std::thread::sleep(std::time::Duration::from_secs_f64(2.5));
        println!("Sine tone complete.");
    }
}
