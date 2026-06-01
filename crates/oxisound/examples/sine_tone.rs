fn main() {
    let config = oxisound::StreamConfig::stereo_48k();
    let buf = oxisound::sine_test_tone(440.0, 1.0, config.clone());

    let mut stream = match oxisound::open_output(config) {
        Ok(s) => s,
        Err(e) => {
            println!("No output device available: {e}");
            return;
        }
    };

    if let Err(e) = stream.write(&buf) {
        println!("Write error: {e}");
        return;
    }

    // Wait for playback to finish (approx 1 second)
    std::thread::sleep(std::time::Duration::from_millis(1_100));
    println!("Done.");
}
