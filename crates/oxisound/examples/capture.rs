fn main() {
    let config = oxisound::StreamConfig::mono_16k();
    let sample_rate = config.sample_rate;
    let channels = config.channels;

    let mut stream = match oxisound::open_input(config) {
        Ok(s) => s,
        Err(e) => {
            println!("No input device available: {e}");
            return;
        }
    };

    println!("Capturing 3 seconds of audio...");
    let total_samples = sample_rate as usize * channels as usize * 3;
    let mut buf = vec![0.0f32; 4096];
    let mut collected: Vec<f32> = Vec::with_capacity(total_samples);

    let start = std::time::Instant::now();
    while start.elapsed().as_secs_f32() < 3.0 {
        match stream.read(&mut buf) {
            Ok(n) => collected.extend_from_slice(&buf[..n]),
            Err(e) => {
                println!("Read error: {e}");
                return;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }

    let peak = collected.iter().copied().fold(0.0f32, f32::max);
    println!(
        "Captured {} samples. Peak amplitude: {:.4}",
        collected.len(),
        peak
    );
}
