use oxisound::{DeviceInfo, format_devices};

#[test]
fn format_devices_empty_returns_empty_string() {
    assert_eq!(format_devices(&[]), "");
}

#[test]
fn format_devices_marks_default_and_includes_name() {
    let devices = vec![
        DeviceInfo {
            name: "Built-in Audio".into(),
            is_default: true,
            sample_rates: vec![44_100, 96_000],
            channel_counts: vec![2],
            is_input: false,
            is_output: true,
            capabilities: None,
        },
        DeviceInfo {
            name: "USB Audio Device".into(),
            is_default: false,
            sample_rates: vec![44_100, 48_000],
            channel_counts: vec![1, 2],
            is_input: false,
            is_output: true,
            capabilities: None,
        },
    ];
    let output = format_devices(&devices);
    assert!(
        output.contains("Built-in Audio"),
        "must contain first device name"
    );
    assert!(
        output.contains("USB Audio Device"),
        "must contain second device name"
    );
    assert!(
        output.contains('*'),
        "must have a * marker for the default device"
    );
    assert!(
        output.contains("44100") || output.contains("44_100"),
        "must contain a sample rate figure"
    );
}

#[test]
fn format_devices_unknown_fields_do_not_panic() {
    // DeviceInfo with empty sample_rates and channel_counts is valid (unknown caps = pass).
    let devices = vec![DeviceInfo {
        name: "Mystery Device".into(),
        is_default: false,
        ..Default::default()
    }];
    let output = format_devices(&devices);
    assert!(output.contains("Mystery Device"));
}

#[test]
#[cfg(target_os = "macos")]
fn open_output_and_write_silence() {
    use oxisound::{StreamConfig, open_output};
    let mut stream = match open_output(StreamConfig::stereo_48k()) {
        Ok(s) => s,
        Err(e) => {
            println!("open_output failed (OK in some CI envs): {e}");
            return;
        }
    };
    let silence = vec![0.0f32; 9_600]; // 100 ms at 48 kHz stereo
    stream.write(&silence).expect("write silence must not fail");
    std::thread::sleep(std::time::Duration::from_millis(150));
}
