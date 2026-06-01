use oxisound::{StreamConfig, sine_test_tone};

#[test]
fn sine_test_tone_length_matches_spec() {
    let config = StreamConfig::stereo_48k();
    let duration_secs = 1.0f32;
    let sample_rate = config.sample_rate;
    let channels = config.channels;
    let buf = sine_test_tone(440.0, duration_secs, config);
    let expected = (sample_rate as f32 * channels as f32 * duration_secs) as usize;
    assert_eq!(buf.len(), expected);
}

#[test]
fn sine_test_tone_samples_in_range() {
    let buf = sine_test_tone(440.0, 0.1, StreamConfig::stereo_48k());
    for &s in &buf {
        assert!((-1.0..=1.0).contains(&s), "sample {s} out of [-1.0, 1.0]");
    }
}

#[test]
fn sine_test_tone_nonzero_energy() {
    let buf = sine_test_tone(440.0, 0.1, StreamConfig::stereo_48k());
    let energy: f32 = buf.iter().map(|&s| s * s).sum();
    assert!(energy > 0.0, "sine tone has zero energy");
}

#[test]
fn sine_test_tone_mono_length() {
    let config = StreamConfig::mono_16k();
    let sample_rate = config.sample_rate;
    let channels = config.channels;
    let buf = sine_test_tone(220.0, 0.5, config);
    let expected = (sample_rate as f32 * channels as f32 * 0.5) as usize;
    assert_eq!(buf.len(), expected);
}
