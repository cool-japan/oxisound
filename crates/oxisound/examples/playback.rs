use oxisound::AudioDevice;

fn main() {
    let config = oxisound::StreamConfig::stereo_48k();

    let device = match oxisound::default_output() {
        Ok(d) => d,
        Err(e) => {
            println!("No output device: {e}");
            return;
        }
    };

    // Print device info via enumerate
    if let Ok(devs) = oxisound::enumerate_devices()
        && let Some(d) = devs.iter().find(|d| d.is_default && d.is_output)
    {
        println!("Playing on: {}", d.name);
    }

    let buf = oxisound::sine_test_tone(440.0, 2.0, config.clone());
    let mut stream = match device.open_output(config) {
        Ok(s) => s,
        Err(e) => {
            println!("Could not open output: {e}");
            return;
        }
    };

    if let Err(e) = stream.write(&buf) {
        println!("Write error: {e}");
        return;
    }

    std::thread::sleep(std::time::Duration::from_millis(2_100));
    println!("Done.");
}
