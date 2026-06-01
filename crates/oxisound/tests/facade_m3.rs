#![cfg(feature = "pure")]

use oxisound::{OxiSoundError, select_device};

#[test]
fn select_device_nonexistent_returns_no_device() {
    // This name cannot match any real audio device.
    let result = select_device("nonexistent-device-xyz-impossible");
    assert!(
        matches!(result, Err(OxiSoundError::NoDevice)),
        "select_device with impossible name must return NoDevice",
    );
}

#[test]
#[cfg(target_os = "macos")]
fn duplex_stream_ok_or_graceful_err() {
    use oxisound::{StreamConfig, duplex_stream};
    // Duplex requires both input and output. Ok or Err are both acceptable.
    match duplex_stream(StreamConfig::stereo_48k()) {
        Ok(_) => println!("duplex_stream() succeeded"),
        Err(e) => println!("duplex_stream() returned Err (OK in CI): {e}"),
    }
}
