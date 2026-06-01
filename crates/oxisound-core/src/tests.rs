use super::*;

#[test]
fn validate_accepts_in_range_config() {
    let info = DeviceInfo {
        name: "Test Device".into(),
        is_default: true,
        sample_rates: vec![44_100, 96_000],
        channel_counts: vec![1, 2],
        is_input: false,
        is_output: true,
        capabilities: None,
    };
    let cfg = StreamConfig {
        sample_rate: 48_000,
        channels: 2,
        buffer_size: None,
        sample_format: None,
        exclusive: false,
        preferred_formats: Vec::new(),
        channel_routing: None,
        buffer_capacity_secs: None,
    };
    assert!(cfg.validate(&info).is_ok());
}

#[test]
fn validate_rejects_out_of_range_sample_rate() {
    let info = DeviceInfo {
        name: "Test Device".into(),
        is_default: false,
        sample_rates: vec![44_100, 48_000],
        channel_counts: vec![2],
        is_input: false,
        is_output: true,
        capabilities: None,
    };
    let cfg = StreamConfig {
        sample_rate: 96_000,
        channels: 2,
        buffer_size: None,
        sample_format: None,
        exclusive: false,
        preferred_formats: Vec::new(),
        channel_routing: None,
        buffer_capacity_secs: None,
    };
    let err = cfg.validate(&info).unwrap_err();
    assert!(matches!(err, OxiSoundError::UnsupportedConfig(_)));
}

#[test]
fn validate_rejects_unsupported_channel_count() {
    let info = DeviceInfo {
        name: "Test Device".into(),
        is_default: false,
        sample_rates: vec![44_100, 96_000],
        channel_counts: vec![2],
        is_input: false,
        is_output: true,
        capabilities: None,
    };
    let cfg = StreamConfig {
        sample_rate: 48_000,
        channels: 1,
        buffer_size: None,
        sample_format: None,
        exclusive: false,
        preferred_formats: Vec::new(),
        channel_routing: None,
        buffer_capacity_secs: None,
    };
    let err = cfg.validate(&info).unwrap_err();
    assert!(matches!(err, OxiSoundError::UnsupportedConfig(_)));
}

#[test]
fn validate_passes_when_fields_empty() {
    let info = DeviceInfo {
        name: "Unknown Device".into(),
        is_default: false,
        sample_rates: vec![],
        channel_counts: vec![],
        is_input: true,
        is_output: false,
        capabilities: None,
    };
    let cfg = StreamConfig {
        sample_rate: 192_000,
        channels: 8,
        buffer_size: None,
        sample_format: None,
        exclusive: false,
        preferred_formats: Vec::new(),
        channel_routing: None,
        buffer_capacity_secs: None,
    };
    assert!(cfg.validate(&info).is_ok());
}

#[test]
fn host_api_display_non_empty_for_all_variants() {
    let variants = [
        HostApi::CoreAudio,
        HostApi::Wasapi,
        HostApi::Asio,
        HostApi::Alsa,
        HostApi::Jack,
        HostApi::PipeWire,
        HostApi::PulseAudio,
    ];
    for v in variants {
        let s = v.to_string();
        assert!(!s.is_empty(), "HostApi::{:?} Display must be non-empty", v);
    }
}

#[test]
fn host_api_display_expected_labels() {
    assert_eq!(HostApi::CoreAudio.to_string(), "Core Audio");
    assert_eq!(HostApi::Wasapi.to_string(), "WASAPI");
    assert_eq!(HostApi::Alsa.to_string(), "ALSA");
    assert_eq!(HostApi::PipeWire.to_string(), "PipeWire");
}

#[test]
fn disconnected_error_display() {
    let e = OxiSoundError::Disconnected("USB mic unplugged".into());
    assert!(e.to_string().contains("disconnected"));
}

#[test]
fn default_selector_picks_default_device() {
    let devices = vec![
        DeviceInfo {
            name: "A".into(),
            is_default: false,
            ..Default::default()
        },
        DeviceInfo {
            name: "B".into(),
            is_default: true,
            ..Default::default()
        },
        DeviceInfo {
            name: "C".into(),
            is_default: false,
            ..Default::default()
        },
    ];
    assert_eq!(DefaultSelector.select(&devices), Some(1));
}

#[test]
fn default_selector_falls_back_to_first_when_no_default() {
    let devices = vec![
        DeviceInfo {
            name: "A".into(),
            is_default: false,
            ..Default::default()
        },
        DeviceInfo {
            name: "B".into(),
            is_default: false,
            ..Default::default()
        },
    ];
    assert_eq!(DefaultSelector.select(&devices), Some(0));
}

#[test]
fn default_selector_returns_none_for_empty() {
    assert_eq!(DefaultSelector.select(&[]), None);
}

#[test]
fn latency_optimal_selector_returns_first() {
    let devices = vec![DeviceInfo {
        name: "X".into(),
        ..Default::default()
    }];
    assert_eq!(LatencyOptimalSelector.select(&devices), Some(0));
    assert_eq!(LatencyOptimalSelector.select(&[]), None);
}

#[test]
fn name_match_selector_case_insensitive() {
    let devices = vec![
        DeviceInfo {
            name: "Built-in Audio".into(),
            ..Default::default()
        },
        DeviceInfo {
            name: "USB Microphone".into(),
            ..Default::default()
        },
    ];
    assert_eq!(NameMatchSelector("usb".into()).select(&devices), Some(1));
    assert_eq!(NameMatchSelector("BUILT".into()).select(&devices), Some(0));
    assert_eq!(
        NameMatchSelector("nonexistent".into()).select(&devices),
        None
    );
}

#[test]
fn overrun_underrun_display() {
    assert!(
        OxiSoundError::Overrun("ring buffer full".into())
            .to_string()
            .contains("overrun")
    );
    assert!(
        OxiSoundError::Underrun("callback starved".into())
            .to_string()
            .contains("underrun")
    );
}

#[test]
fn const_presets_match_constructor_fns() {
    assert_eq!(StreamConfig::STEREO_48K, StreamConfig::stereo_48k());
    assert_eq!(StreamConfig::STEREO_44K, StreamConfig::stereo_44k());
    assert_eq!(StreamConfig::MONO_16K, StreamConfig::mono_16k());
}

#[test]
fn low_latency_stereo_48k_has_buffer_size() {
    let cfg = StreamConfig::low_latency_stereo_48k();
    assert_eq!(cfg.buffer_size, Some(256));
    assert_eq!(cfg.sample_rate, 48_000);
    assert_eq!(cfg.channels, 2);
}

#[cfg(feature = "serde")]
#[test]
fn serde_round_trip_stream_config() {
    let cfg = StreamConfig::stereo_48k();
    let json = serde_json::to_string(&cfg).unwrap();
    let back: StreamConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(cfg, back);
}

#[cfg(feature = "serde")]
#[test]
fn serde_round_trip_device_info() {
    let info = DeviceInfo {
        name: "Test Device".into(),
        is_default: true,
        sample_rates: vec![44_100, 48_000],
        channel_counts: vec![1, 2],
        is_input: true,
        is_output: true,
        capabilities: None,
    };
    let json = serde_json::to_string(&info).unwrap();
    let back: DeviceInfo = serde_json::from_str(&json).unwrap();
    assert_eq!(info, back);
}

#[cfg(feature = "serde")]
#[test]
fn serde_round_trip_host_api() {
    let variants = [
        HostApi::CoreAudio,
        HostApi::Wasapi,
        HostApi::Asio,
        HostApi::Alsa,
        HostApi::Jack,
        HostApi::PipeWire,
        HostApi::PulseAudio,
    ];
    for v in variants {
        let json = serde_json::to_string(&v).unwrap();
        let back: HostApi = serde_json::from_str(&json).unwrap();
        assert_eq!(v, back);
    }
}

#[cfg(feature = "tokio")]
#[test]
fn async_output_stream_trait_is_implementable() {
    struct DummyAsync;
    impl AsyncOutputStream for DummyAsync {
        async fn write(&mut self, _samples: &[f32]) -> Result<(), OxiSoundError> {
            Ok(())
        }
    }
    // Compile-check: DummyAsync implements the trait — no runtime needed.
    let _ = std::marker::PhantomData::<DummyAsync>;
}

#[test]
fn device_info_builder_sets_fields() {
    let info = DeviceInfo::builder("Test Speaker")
        .default_device()
        .output(true)
        .sample_rates(vec![44_100, 48_000])
        .channel_counts(vec![2])
        .build();
    assert_eq!(info.name, "Test Speaker");
    assert!(info.is_default);
    assert!(info.is_output);
    assert!(!info.is_input);
    assert_eq!(info.sample_rates, vec![44_100, 48_000]);
    assert_eq!(info.channel_counts, vec![2]);
}

#[test]
fn device_info_builder_defaults_are_sane() {
    let info = DeviceInfo::builder("Mic").build();
    assert_eq!(info.name, "Mic");
    assert!(!info.is_default);
    assert!(!info.is_input);
    assert!(!info.is_output);
    assert!(info.sample_rates.is_empty());
    assert!(info.channel_counts.is_empty());
}

#[test]
fn stream_config_display() {
    let cfg = StreamConfig::stereo_48k();
    let s = cfg.to_string();
    assert!(s.contains("48000"), "should contain sample rate: {s}");
    assert!(s.contains("2"), "should contain channel count: {s}");
    assert!(s.contains("auto"), "no buffer size should show 'auto': {s}");
}

#[test]
fn stream_config_display_with_buffer() {
    let cfg = StreamConfig::low_latency_stereo_48k();
    let s = cfg.to_string();
    assert!(s.contains("256"), "should contain buffer size 256: {s}");
}

#[test]
fn device_info_display() {
    let info = DeviceInfo::builder("Built-in Mic")
        .default_device()
        .input(true)
        .build();
    let s = info.to_string();
    assert!(s.contains("Built-in Mic"), "name in display: {s}");
    assert!(s.contains("[default]"), "default marker in display: {s}");
    assert!(s.contains("[in]"), "input marker in display: {s}");
}

// ---- New tests for Slice A ----

#[test]
fn sample_format_display() {
    assert_eq!(SampleFormat::F32.to_string(), "f32");
    assert_eq!(SampleFormat::I16.to_string(), "i16");
    assert_eq!(SampleFormat::I32.to_string(), "i32");
    assert_eq!(SampleFormat::U8.to_string(), "u8");
    assert_eq!(SampleFormat::F64.to_string(), "f64");
}

#[cfg(feature = "serde")]
#[test]
fn sample_format_serde_roundtrip() {
    for fmt in [
        SampleFormat::F32,
        SampleFormat::I16,
        SampleFormat::I32,
        SampleFormat::U8,
        SampleFormat::F64,
    ] {
        let json = serde_json::to_string(&fmt).unwrap();
        let back: SampleFormat = serde_json::from_str(&json).unwrap();
        assert_eq!(fmt, back);
    }
}

#[test]
fn device_capabilities_default_is_empty() {
    let caps = DeviceCapabilities::default();
    assert!(caps.min_buffer_size.is_none());
    assert!(caps.max_buffer_size.is_none());
    assert!(caps.supported_formats.is_empty());
    assert!(!caps.exclusive_mode);
}

#[test]
fn stream_config_builder_basic() {
    let cfg = StreamConfig::builder()
        .sample_rate(48_000)
        .channels(2)
        .build();
    assert_eq!(cfg.sample_rate, 48_000);
    assert_eq!(cfg.channels, 2);
    assert_eq!(cfg.buffer_size, None);
    assert_eq!(cfg.sample_format, None);
    assert!(!cfg.exclusive);
}

#[test]
fn stream_config_builder_with_format() {
    let cfg = StreamConfig::builder()
        .sample_rate(48_000)
        .channels(2)
        .sample_format(SampleFormat::F32)
        .exclusive()
        .build();
    assert_eq!(cfg.sample_format, Some(SampleFormat::F32));
    assert!(cfg.exclusive);
}

#[test]
fn negotiated_config_construction() {
    let nc = NegotiatedConfig {
        sample_rate: 48_000,
        channels: 2,
        buffer_size: 256,
        sample_format: SampleFormat::F32,
    };
    assert_eq!(nc.sample_rate, 48_000);
    assert_eq!(nc.sample_format, SampleFormat::F32);
}

#[test]
fn channel_routing_stereo_count() {
    assert_eq!(ChannelRouting::stereo().channel_count(), 2);
}

#[test]
fn channel_routing_5_1_count() {
    assert_eq!(ChannelRouting::surround_5_1().channel_count(), 6);
}

#[test]
fn channel_routing_7_1_count() {
    assert_eq!(ChannelRouting::surround_7_1().channel_count(), 8);
}

#[test]
fn device_event_display_non_empty() {
    let added = DeviceEvent::DeviceAdded(DeviceInfo::builder("Mic").build());
    let removed = DeviceEvent::DeviceRemoved("Speaker".into());
    assert!(!added.to_string().is_empty());
    assert!(!removed.to_string().is_empty());
}

#[test]
fn error_variants_display() {
    assert!(
        OxiSoundError::HotPlugError("test".into())
            .to_string()
            .contains("hot-plug")
    );
    assert!(
        OxiSoundError::PermissionDenied("mic".into())
            .to_string()
            .contains("permission")
    );
    assert!(
        OxiSoundError::Timeout("2s".into())
            .to_string()
            .contains("timed out")
    );
    assert!(
        OxiSoundError::FormatMismatch("f64 not supported".into())
            .to_string()
            .contains("format mismatch")
    );
}

#[test]
fn stream_stats_default_zeros() {
    let stats = StreamStats::default();
    assert_eq!(stats.frames_processed, 0);
    assert_eq!(stats.underruns, 0);
    assert_eq!(stats.overruns, 0);
    assert_eq!(stats.latency_frames, 0);
    assert_eq!(stats.cpu_load_percent, 0.0);
}

#[test]
fn supports_config_accepts_valid() {
    let info = DeviceInfo::builder("Test")
        .sample_rates(vec![44_100, 48_000])
        .channel_counts(vec![2])
        .build();
    let cfg = StreamConfig::builder()
        .sample_rate(48_000)
        .channels(2)
        .build();
    assert!(info.supports_config(&cfg));
}

#[test]
fn supports_config_rejects_invalid_rate() {
    let info = DeviceInfo::builder("Test")
        .sample_rates(vec![44_100, 48_000])
        .channel_counts(vec![2])
        .build();
    let cfg = StreamConfig::builder()
        .sample_rate(96_000)
        .channels(2)
        .build();
    assert!(!info.supports_config(&cfg));
}

#[test]
fn supports_config_rejects_invalid_channels() {
    let info = DeviceInfo::builder("Test")
        .sample_rates(vec![44_100, 48_000])
        .channel_counts(vec![2])
        .build();
    let cfg = StreamConfig::builder()
        .sample_rate(48_000)
        .channels(8)
        .build();
    assert!(!info.supports_config(&cfg));
}

#[test]
fn supports_config_empty_means_any() {
    let info = DeviceInfo::builder("Unknown").build();
    let cfg = StreamConfig::builder()
        .sample_rate(192_000)
        .channels(8)
        .build();
    assert!(info.supports_config(&cfg));
}

#[test]
fn host_api_native_is_available() {
    #[cfg(target_os = "macos")]
    assert!(HostApi::CoreAudio.is_available());
    #[cfg(target_os = "linux")]
    assert!(HostApi::Alsa.is_available());
    // Jack and Asio are feature-gated at the backend crate level; core always reports false.
    assert!(!HostApi::Jack.is_available());
    assert!(!HostApi::Asio.is_available());
}

#[test]
fn callback_priority_default_is_normal() {
    assert_eq!(CallbackPriority::default(), CallbackPriority::Normal);
}

#[test]
#[expect(
    clippy::assertions_on_constants,
    reason = "deliberately testing that compile-time-known assertions hold at runtime for documentation purposes"
)]
fn stream_config_const_presets_have_new_fields() {
    assert_eq!(StreamConfig::STEREO_48K.sample_format, None);
    assert!(!StreamConfig::STEREO_48K.exclusive);
    assert_eq!(StreamConfig::MONO_16K.sample_format, None);
    // New fields are empty/None in all presets.
    assert!(StreamConfig::STEREO_48K.preferred_formats.is_empty());
    assert!(StreamConfig::STEREO_44K.preferred_formats.is_empty());
    assert!(StreamConfig::MONO_16K.preferred_formats.is_empty());
    assert!(StreamConfig::STEREO_48K.channel_routing.is_none());
}

#[test]
fn device_info_builder_with_capabilities() {
    let caps = DeviceCapabilities {
        min_buffer_size: Some(64),
        max_buffer_size: Some(4096),
        supported_formats: vec![SampleFormat::F32, SampleFormat::I16],
        exclusive_mode: true,
    };
    let info = DeviceInfo::builder("Pro Audio").capabilities(caps).build();
    assert!(info.capabilities.is_some());
    assert_eq!(info.capabilities.unwrap().min_buffer_size, Some(64));
}

// ---- DeviceEvent Display + serde tests ----

#[test]
fn test_device_event_display_added() {
    let info = DeviceInfo::builder("test_mic".to_string()).build();
    let ev = DeviceEvent::DeviceAdded(info);
    let s = format!("{ev}");
    assert!(
        s.contains("added") || s.contains("Added") || s.contains("test_mic"),
        "DeviceEvent::DeviceAdded Display was: {s}"
    );
}

#[test]
fn test_device_event_display_removed() {
    let ev = DeviceEvent::DeviceRemoved("old_speaker".to_string());
    let s = format!("{ev}");
    assert!(
        s.contains("removed") || s.contains("Removed") || s.contains("old_speaker"),
        "DeviceEvent::DeviceRemoved Display was: {s}"
    );
}

#[test]
fn test_device_event_display_default_changed() {
    let info = DeviceInfo::builder("new_default".to_string()).build();
    let ev = DeviceEvent::DefaultChanged(info);
    let s = format!("{ev}");
    assert!(
        s.contains("default") || s.contains("Default") || s.contains("new_default"),
        "DeviceEvent::DefaultChanged Display was: {s}"
    );
}

#[cfg(feature = "serde")]
#[test]
fn test_device_event_serde_removed() {
    let ev = DeviceEvent::DeviceRemoved("headphones".to_string());
    let json = serde_json::to_string(&ev).unwrap();
    assert!(json.contains("headphones"));
    let back: DeviceEvent = serde_json::from_str(&json).unwrap();
    if let DeviceEvent::DeviceRemoved(name) = back {
        assert_eq!(name, "headphones");
    } else {
        panic!("serde roundtrip changed variant");
    }
}

// ---- Error source chain tests ----

#[test]
fn test_error_io_source() {
    use std::error::Error;
    let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "test");
    let oxi_err = OxiSoundError::Io(io_err);
    assert!(
        oxi_err.source().is_some(),
        "Io variant should have a source"
    );
}

#[test]
fn test_error_string_variants_no_source() {
    use std::error::Error;
    let err = OxiSoundError::NoDevice;
    assert!(err.source().is_none());
    let err = OxiSoundError::HotPlugError("test".into());
    assert!(err.source().is_none());
    let err = OxiSoundError::FormatMismatch("test".into());
    assert!(err.source().is_none());
}

// ---- OxiSoundError::kind() tests ----

#[test]
fn test_error_kind_stable() {
    assert_eq!(OxiSoundError::NoDevice.kind(), "no-device");
    assert_eq!(OxiSoundError::Timeout("x".into()).kind(), "timeout");
    assert_eq!(
        OxiSoundError::FormatMismatch("x".into()).kind(),
        "format-mismatch"
    );
    assert_eq!(
        OxiSoundError::PermissionDenied("x".into()).kind(),
        "permission-denied"
    );
    assert_eq!(
        OxiSoundError::Disconnected("x".into()).kind(),
        "disconnected"
    );
    assert_eq!(OxiSoundError::Overrun("x".into()).kind(), "overrun");
    assert_eq!(OxiSoundError::Underrun("x".into()).kind(), "underrun");
    assert_eq!(
        OxiSoundError::HotPlugError("x".into()).kind(),
        "hot-plug-error"
    );
    assert_eq!(OxiSoundError::Device("x".into()).kind(), "device");
    assert_eq!(OxiSoundError::Stream("x".into()).kind(), "stream");
    assert_eq!(
        OxiSoundError::UnsupportedConfig("x".into()).kind(),
        "unsupported-config"
    );
    let io_err = std::io::Error::other("test");
    assert_eq!(OxiSoundError::Io(io_err).kind(), "io");
}

#[test]
fn test_error_kind_never_empty() {
    let errs: Vec<OxiSoundError> = vec![
        OxiSoundError::NoDevice,
        OxiSoundError::Timeout("x".into()),
        OxiSoundError::FormatMismatch("x".into()),
    ];
    for e in &errs {
        assert!(!e.kind().is_empty(), "kind() was empty for {e:?}");
    }
}

// ---- Proptest tests ----

use proptest::prelude::*;

proptest! {
    #[test]
    fn test_stream_config_validate_never_panics(rate in 0u32..200_000, channels in 0u16..64) {
        let info = DeviceInfo::default();
        let config = StreamConfig {
            sample_rate: rate,
            channels,
            buffer_size: None,
            sample_format: None,
            exclusive: false,
            preferred_formats: Vec::new(),
            channel_routing: None,
            buffer_capacity_secs: None,
        };
        // validate() must not panic, just Ok or Err
        let _ = config.validate(&info);
    }

    #[test]
    fn test_stream_config_validate_valid_range(
        rate in 8_000u32..=192_000,
        channels in 1u16..=8,
    ) {
        // DeviceInfo with empty sample_rates/channel_counts passes all configs
        let info = DeviceInfo::default();
        let config = StreamConfig {
            sample_rate: rate,
            channels,
            buffer_size: None,
            sample_format: None,
            exclusive: false,
            preferred_formats: Vec::new(),
            channel_routing: None,
            buffer_capacity_secs: None,
        };
        // All valid rates/channels should pass when device has no constraints
        assert!(config.validate(&info).is_ok(), "should be valid: rate={rate}, ch={channels}");
    }

    #[test]
    fn test_supports_config_within_range(rate in 8_000u32..=48_000) {
        let mut info = DeviceInfo::builder("test".to_string()).build();
        info.sample_rates = vec![rate, rate + 1000];
        info.is_output = true;
        let config = StreamConfig {
            sample_rate: rate,
            channels: 2,
            buffer_size: None,
            sample_format: None,
            exclusive: false,
            preferred_formats: Vec::new(),
            channel_routing: None,
            buffer_capacity_secs: None,
        };
        assert!(info.supports_config(&config));
    }
}

#[test]
fn preferred_formats_builder_roundtrip() {
    let fmts = vec![SampleFormat::F32, SampleFormat::I16];
    let config = StreamConfig::builder()
        .sample_rate(48_000)
        .channels(2)
        .preferred_formats(fmts.clone())
        .build();
    assert_eq!(config.preferred_formats, fmts);
    assert_eq!(config.channel_routing, None);
}

#[test]
fn channel_routing_builder_roundtrip() {
    let routing = ChannelRouting::stereo();
    let config = StreamConfig::builder()
        .sample_rate(48_000)
        .channels(2)
        .channel_routing(routing.clone())
        .build();
    assert_eq!(config.channel_routing, Some(routing));
}

#[test]
fn channel_routing_apply_interleaved_identity() {
    // Stereo routing FL→0, FR→1 on a stereo buffer: result must be identical.
    let routing = ChannelRouting::stereo();
    let mut buf = vec![0.5f32, -0.5, 0.25, -0.25];
    let original = buf.clone();
    routing.apply_interleaved(&mut buf, 2);
    assert_eq!(buf, original, "identity routing must not change the buffer");
}

#[test]
fn channel_routing_apply_interleaved_swap() {
    // Route FL→1, FR→0 (swap left and right).
    let routing = ChannelRouting(vec![(Channel::FrontLeft, 1), (Channel::FrontRight, 0)]);
    // Frame: [0.8, 0.2] — FL=0.8, FR=0.2
    let mut buf = vec![0.8f32, 0.2];
    routing.apply_interleaved(&mut buf, 2);
    // After swap: slot 0 gets FR (standard_index=1), slot 1 gets FL (standard_index=0)
    // standard_index: FL=0, FR=1 → src=0→dst=1, src=1→dst=0
    assert!(
        (buf[0] - 0.2f32).abs() < 1e-6,
        "expected slot 0 = FR = 0.2, got {}",
        buf[0]
    );
    assert!(
        (buf[1] - 0.8f32).abs() < 1e-6,
        "expected slot 1 = FL = 0.8, got {}",
        buf[1]
    );
}

#[test]
fn channel_standard_index_matches_layout() {
    assert_eq!(Channel::FrontLeft.standard_index(), 0);
    assert_eq!(Channel::FrontRight.standard_index(), 1);
    assert_eq!(Channel::Center.standard_index(), 2);
    assert_eq!(Channel::Lfe.standard_index(), 3);
    assert_eq!(Channel::SurroundLeft.standard_index(), 4);
    assert_eq!(Channel::SurroundRight.standard_index(), 5);
    assert_eq!(Channel::BackLeft.standard_index(), 6);
    assert_eq!(Channel::BackRight.standard_index(), 7);
}

#[test]
fn channel_routing_apply_interleaved_empty_routing_is_noop() {
    let routing = ChannelRouting(vec![]);
    let mut buf = vec![1.0f32, 2.0, 3.0, 4.0];
    let original = buf.clone();
    routing.apply_interleaved(&mut buf, 2);
    assert_eq!(buf, original);
}

#[cfg(feature = "serde")]
#[test]
fn serde_roundtrip_with_new_fields_absent() {
    // JSON without preferred_formats or channel_routing — serde(default) fills them in.
    let json = r#"{"sample_rate":48000,"channels":2,"buffer_size":null,"sample_format":null,"exclusive":false}"#;
    let config: StreamConfig = serde_json::from_str(json).expect("deserialize failed");
    assert!(config.preferred_formats.is_empty());
    assert_eq!(config.channel_routing, None);
}

#[cfg(feature = "serde")]
#[test]
fn serde_roundtrip_with_new_fields_present() {
    let original = StreamConfig::builder()
        .sample_rate(44_100)
        .channels(2)
        .preferred_formats(vec![SampleFormat::F32, SampleFormat::I16])
        .channel_routing(ChannelRouting::stereo())
        .build();
    let json = serde_json::to_string(&original).expect("serialize failed");
    let roundtripped: StreamConfig = serde_json::from_str(&json).expect("deserialize failed");
    assert_eq!(roundtripped, original);
}

// ---- New tests for MidiMessage helpers, MidiClock, SampleFormat::I24, buffer_capacity_secs ----

#[test]
fn midi_message_sysex_roundtrip() {
    let payload = &[0x41u8, 0x10, 0x00, 0x00, 0x00, 0x27, 0x12];
    let msg = MidiMessage::new_sysex(payload);
    assert!(msg.is_sysex());
    assert_eq!(msg.sysex_payload(), Some(payload as &[u8]));
    let bytes = msg.to_bytes();
    assert_eq!(bytes[0], 0xF0);
    assert_eq!(*bytes.last().expect("bytes must be non-empty"), 0xF7);
    assert_eq!(&bytes[1..bytes.len() - 1], payload);
}

#[test]
fn midi_message_to_bytes_non_sysex() {
    let msg = MidiMessage {
        status: 0x90,
        data: vec![0x3C, 0x7F],
        timestamp_micros: 0,
    };
    assert!(!msg.is_sysex());
    assert_eq!(msg.to_bytes(), vec![0x90, 0x3C, 0x7F]);
}

#[test]
fn midi_clock_bpm_120() {
    let mut clock = MidiClock::new();
    // 24 ticks per quarter note, 120 BPM → quarter note = 500_000 µs → tick interval = 20_833 µs
    for i in 0..24u64 {
        clock.tick(i * 20_833);
    }
    let bpm = clock.bpm().expect("bpm should be Some after 24 ticks");
    assert!((bpm - 120.0).abs() < 0.5, "Expected ~120 BPM, got {}", bpm);
}

#[test]
fn midi_clock_fewer_than_two_ticks_returns_none() {
    let mut clock = MidiClock::new();
    assert_eq!(clock.bpm(), None);
    clock.tick(0);
    assert_eq!(clock.bpm(), None);
    clock.tick(20_833);
    assert!(clock.bpm().is_some());
}

#[test]
fn midi_clock_handle_message_lifecycle() {
    let mut clock = MidiClock::new();
    assert!(!clock.is_running());
    clock.handle_message(&MidiMessage {
        status: MIDI_START,
        data: vec![],
        timestamp_micros: 0,
    });
    assert!(clock.is_running());
    clock.handle_message(&MidiMessage {
        status: MIDI_STOP,
        data: vec![],
        timestamp_micros: 0,
    });
    assert!(!clock.is_running());
    clock.handle_message(&MidiMessage {
        status: MIDI_CONTINUE,
        data: vec![],
        timestamp_micros: 0,
    });
    assert!(clock.is_running());
}

#[test]
fn sample_format_i24_properties() {
    assert_eq!(SampleFormat::I24.byte_size(), 3);
    assert!(!SampleFormat::I24.is_float());
    assert_eq!(format!("{}", SampleFormat::I24), "I24");
}

#[test]
fn stream_config_buffer_capacity_secs_builder() {
    let config = StreamConfig::builder()
        .sample_rate(48_000)
        .channels(2)
        .buffer_size(256)
        .buffer_capacity_secs(0.5)
        .build();
    assert_eq!(config.buffer_capacity_secs, Some(0.5));
}

#[cfg(feature = "serde")]
#[test]
fn stream_config_serde_with_buffer_capacity() {
    let config = StreamConfig::builder()
        .sample_rate(48_000)
        .channels(2)
        .buffer_size(256)
        .buffer_capacity_secs(1.0)
        .build();
    let json = serde_json::to_string(&config).expect("serialize failed");
    let restored: StreamConfig = serde_json::from_str(&json).expect("deserialize failed");
    assert_eq!(restored.buffer_capacity_secs, Some(1.0));
}

#[test]
fn error_kind_unsupported() {
    assert_eq!(
        OxiSoundError::Unsupported("test feature".into()).kind(),
        "unsupported"
    );
}
