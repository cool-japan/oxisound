/// Hardware-gated: plays 100ms of 440 Hz sine and verifies no error.
/// Requires a working audio output device. Run with:
///   cargo test -p oxisound -- --ignored integration_sine_to_output
#[test]
#[ignore = "requires audio hardware"]
fn integration_sine_to_output() {
    let config = oxisound::StreamConfig::stereo_48k();
    let mut output = match oxisound::open_output(config.clone()) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("Skipping: {e}");
            return;
        }
    };
    let samples = oxisound::sine_test_tone(440.0, 0.1, config);
    assert!(
        output.write(&samples).is_ok(),
        "write to output stream must not error"
    );
    std::thread::sleep(std::time::Duration::from_millis(150));
}
