//! Real-time peaking EQ: capture audio from the default input device,
//! apply a BiquadFilter peaking band with oxiaudio-dsp, then play the
//! processed audio through the default output device.
//!
//! Usage: cargo run --example realtime_eq --features pure
//!
//! Applies a +6 dB peak at 1 kHz with Q=1.0 for ~10 seconds.
//! Press Ctrl+C to stop early.

fn main() {
    println!("Real-time EQ demo: +6 dB peak at 1 kHz, Q=1.0 (~10 seconds)");
    println!("Press Ctrl+C to stop.\n");

    let config = oxisound::StreamConfig::stereo_48k();
    let sample_rate = config.sample_rate;

    let mut input = match oxisound::open_input(config.clone()) {
        Ok(i) => i,
        Err(e) => {
            println!("Failed to open input: {e}");
            return;
        }
    };

    let mut output = match oxisound::open_output(config) {
        Ok(o) => o,
        Err(e) => {
            println!("Failed to open output: {e}");
            return;
        }
    };

    // peaking_eq(frequency, q, gain_db, sample_rate) — RBJ Audio EQ Cookbook.
    // Note the argument order: Q comes before gain_db.
    let filter = oxiaudio::dsp::BiquadFilter::peaking_eq(1000.0, 1.0, 6.0, sample_rate);

    // Process for ~10 seconds in 512-frame stereo blocks.
    // Choosing 512 frames (~ 10.7 ms at 48 kHz) balances latency vs. scheduling overhead.
    let block_frames = 512usize;
    let channels = 2usize;
    let block_samples = block_frames * channels;
    let total_blocks = (sample_rate as usize * 10) / block_frames;

    for _ in 0..total_blocks {
        let mut block = vec![0.0f32; block_samples];

        match input.read(&mut block) {
            Ok(_) => {}
            Err(e) => {
                println!("Read error: {e}");
                return;
            }
        }

        // BiquadFilter::process is stateless per call (zeros state at block boundaries).
        // This causes minor high-frequency artifacts at each boundary — acceptable for a demo.
        // Production code should use a stateful per-channel accumulator.
        let audio_buf = oxiaudio::AudioBuffer {
            samples: block,
            sample_rate,
            channels: oxiaudio::ChannelLayout::Stereo,
            format: oxiaudio::SampleFormat::F32,
        };
        let processed = filter.process(&audio_buf);

        if let Err(e) = output.write(&processed.samples) {
            println!("Write error: {e}");
            return;
        }
    }

    println!("\nEQ demo complete.");
}
