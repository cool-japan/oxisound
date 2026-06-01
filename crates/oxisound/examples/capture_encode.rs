//! Capture ~2 seconds of audio from the default input device and encode it
//! to a WAV file using the oxiaudio encode API.
//!
//! Usage: cargo run --example capture_encode --features pure
//!
//! Writes the captured audio to a temporary WAV file and prints the path.

fn main() {
    let sample_rate = 48_000u32;
    let channels = 1u16; // mono capture
    let duration_secs = 2u32;
    let total_samples = sample_rate as usize * channels as usize * duration_secs as usize;

    println!("Capturing {duration_secs}s of audio at {sample_rate} Hz mono...");

    // Build a mono 48 kHz config from the struct literal — there is no MONO_48K preset constant,
    // so we spread from STEREO_48K and override the channel count.
    let config = oxisound::StreamConfig {
        channels,
        ..oxisound::StreamConfig::STEREO_48K
    };

    let mut input = match oxisound::open_input(config) {
        Ok(i) => i,
        Err(e) => {
            println!("Failed to open input: {e}");
            return;
        }
    };

    // Read until we have the requested number of samples.
    // The ring-buffer read call may return fewer samples than requested if the hardware
    // buffer hasn't filled yet, so we loop in small increments.
    let mut samples = Vec::with_capacity(total_samples);
    let mut chunk = vec![0.0f32; 1024];
    while samples.len() < total_samples {
        match input.read(&mut chunk) {
            Ok(n) => samples.extend_from_slice(&chunk[..n]),
            Err(e) => {
                println!("Capture error: {e}");
                return;
            }
        }
        // Yield to avoid busy-spinning while waiting for hardware to fill the buffer.
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    // Trim to exact length in case we overshot by a partial chunk.
    samples.truncate(total_samples);

    println!("Captured {} samples. Encoding to WAV...", samples.len());

    // Wrap in an oxiaudio AudioBuffer so we can use encode_wav from the oxiaudio facade.
    // oxiaudio's encode_wav produces a PCM WAV file — no external C/C++ crates involved.
    let audio_buf = oxiaudio::AudioBuffer {
        samples,
        sample_rate,
        channels: oxiaudio::ChannelLayout::Mono,
        format: oxiaudio::SampleFormat::F32,
    };

    let out_path = std::env::temp_dir().join("oxisound_capture.wav");

    match oxiaudio::encode_wav(&audio_buf, &out_path) {
        Ok(()) => println!("Saved to: {}", out_path.display()),
        Err(e) => println!("Encode error: {e}"),
    }
}
