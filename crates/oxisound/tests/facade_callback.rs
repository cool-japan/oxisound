// Callback API integration tests.
// Hardware-gated: tests pass gracefully when no audio device is present
// (CI, headless servers) by treating `Err(_)` from stream-open as a skip.

#[cfg(target_os = "macos")]
#[test]
fn test_play_callback_no_panic() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU32, Ordering};

    // Render 100ms of silence through the callback API.
    let frame_count = Arc::new(AtomicU32::new(0));
    let fc = Arc::clone(&frame_count);
    let config = oxisound::StreamConfig::STEREO_48K;
    let guard = oxisound::play_callback(config, move |buf| {
        buf.fill(0.0);
        fc.fetch_add(buf.len() as u32, Ordering::Relaxed);
    });

    // Guard returning Err means no hardware — skip silently.
    match guard {
        Ok(_g) => {
            std::thread::sleep(std::time::Duration::from_millis(150));
            assert!(
                frame_count.load(Ordering::Relaxed) > 0,
                "no frames rendered"
            );
        }
        Err(_) => { /* no audio hardware in this environment */ }
    }
}

/// Verifies that `default_output()` does not panic on any system.
/// On hardware-equipped systems it returns Ok; on headless/CI systems Err is acceptable.
#[cfg(target_os = "macos")]
#[test]
fn test_default_output_no_panic() {
    // default_output() must not panic even on headless systems.
    let result = oxisound::default_output();
    match result {
        Ok(_dev) => {
            // We have a device — that is sufficient for this test.
        }
        Err(_) => { /* no audio hardware in this environment — acceptable */ }
    }
}
