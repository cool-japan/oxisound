use oxisound::{StreamConfig, chirp_test_tone, click_track, silence, white_noise_test};

#[test]
fn white_noise_test_length() {
    let config = StreamConfig::stereo_48k();
    let dur = 0.5f32;
    let sample_rate = config.sample_rate;
    let channels = config.channels;
    let buf = white_noise_test(dur, config);
    let expected = (sample_rate as f32 * channels as f32 * dur) as usize;
    assert_eq!(buf.len(), expected);
}

#[test]
fn white_noise_test_range() {
    let buf = white_noise_test(0.1, StreamConfig::stereo_48k());
    for &s in &buf {
        assert!((-1.0..=1.0).contains(&s), "sample out of range: {s}");
    }
}

#[test]
fn white_noise_test_non_zero_energy() {
    let buf = white_noise_test(0.1, StreamConfig::stereo_48k());
    let energy: f32 = buf.iter().map(|s| s * s).sum();
    assert!(energy > 0.0, "white noise has zero energy");
}

#[test]
fn chirp_test_tone_length() {
    let config = StreamConfig::stereo_48k();
    let dur = 1.0f32;
    let sample_rate = config.sample_rate;
    let channels = config.channels;
    let buf = chirp_test_tone(20.0, 20_000.0, dur, config);
    let expected = (sample_rate as f32 * channels as f32 * dur) as usize;
    assert_eq!(buf.len(), expected);
}

#[test]
fn chirp_test_tone_range() {
    let buf = chirp_test_tone(20.0, 20_000.0, 0.1, StreamConfig::stereo_48k());
    for &s in &buf {
        assert!((-1.0..=1.0).contains(&s), "chirp sample out of range: {s}");
    }
}

#[test]
fn silence_is_all_zeros() {
    let buf = silence(0.1, StreamConfig::stereo_48k());
    assert!(
        buf.iter().all(|&s| s == 0.0),
        "silence buffer contains non-zero values"
    );
}

#[test]
fn silence_length() {
    let config = StreamConfig::mono_16k();
    let dur = 0.5f32;
    let sample_rate = config.sample_rate;
    let channels = config.channels;
    let buf = silence(dur, config);
    let expected = (sample_rate as f32 * channels as f32 * dur) as usize;
    assert_eq!(buf.len(), expected);
}

#[test]
fn click_track_length() {
    let config = StreamConfig::stereo_48k();
    let dur = 2.0f32;
    let sample_rate = config.sample_rate;
    let channels = config.channels;
    let buf = click_track(120.0, dur, config);
    let expected = (sample_rate as f32 * channels as f32 * dur) as usize;
    assert_eq!(buf.len(), expected);
}

#[test]
fn click_track_has_nonzero_energy() {
    let buf = click_track(120.0, 2.0, StreamConfig::stereo_48k());
    let energy: f32 = buf.iter().map(|s| s * s).sum();
    assert!(energy > 0.0, "click track has zero energy");
}

#[test]
fn device_by_index_out_of_range() {
    // Device at index 999 should always fail
    let result = oxisound::device_by_index(999);
    assert!(
        matches!(result, Err(oxisound::OxiSoundError::NoDevice)),
        "expected NoDevice, got: {:?}",
        result.err()
    );
}

#[test]
fn test_enumerate_all_devices_has_output() {
    let devices = oxisound::enumerate_all_devices().expect("enumerate_all_devices failed");
    // On any platform with audio hardware, there should be at least one output device.
    // In CI without audio hardware, this may be empty — don't assert non-empty.
    for dev in &devices {
        // Every DeviceInfo must have at least one of is_input/is_output set.
        assert!(
            dev.is_input || dev.is_output,
            "device '{}' has neither is_input nor is_output",
            dev.name
        );
    }
}

#[test]
fn test_monitor_guard_drops_cleanly() {
    let guard = oxisound::monitor_stream(oxisound_core::StreamStats::default, 50, |_stats| {});
    std::thread::sleep(std::time::Duration::from_millis(120));
    drop(guard); // must not hang
}

#[test]
fn test_format_devices_large_list_is_linear() {
    use oxisound_core::DeviceInfo;
    use std::time::Instant;

    let devices: Vec<DeviceInfo> = (0..100)
        .map(|i| {
            let mut d = DeviceInfo::builder(format!("Device {i}")).build();
            d.is_output = i % 2 == 0;
            d.is_input = i % 2 != 0;
            d
        })
        .collect();

    let start = Instant::now();
    let s = oxisound::format_devices(&devices);
    let elapsed = start.elapsed();

    assert!(
        s.contains("Device 0"),
        "format_devices output missing device 0"
    );
    assert!(
        s.contains("Device 99"),
        "format_devices output missing device 99"
    );
    // O(n) check: 100 devices should format in well under 100ms
    assert!(
        elapsed.as_millis() < 100,
        "format_devices took {}ms for 100 devices (expected < 100ms)",
        elapsed.as_millis()
    );
}
