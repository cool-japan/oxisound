//! AVAudioSession / AVAudioApplication wrapper for iOS and macOS.
//!
//! This module contains the only `unsafe` code in the `oxisound-session` crate.
//! Every `unsafe` block is narrowly scoped to `objc2` message-send calls, which
//! are safe in practice because:
//!
//! - `AVAudioSession.sharedInstance` / `AVAudioApplication.sharedInstance` are
//!   documented to never return nil.
//! - `setCategory:error:` is thread-safe on Apple platforms.
//! - `AVAudioApplication.requestRecordPermissionWithCompletionHandler:` is the
//!   modern replacement for the deprecated `AVAudioSession.requestRecordPermission:`.
//! - Static category string constants are guaranteed non-null by the Apple SDK.
//!
//! Callers obtain only safe return values (`Result<(), OxiSoundError>` and
//! `Result<bool, OxiSoundError>`); no raw pointers or Objective-C objects
//! escape this module.

use std::sync::Arc;
use std::sync::{
    Mutex,
    atomic::{AtomicBool, AtomicU8, Ordering},
};

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::Bool;
use objc2_avf_audio::{AVAudioApplication, AVAudioApplicationRecordPermission};
use objc2_avf_audio::{AVAudioSession, AVAudioSessionCategory};
use objc2_foundation::NSError;

use oxisound_core::{OxiSoundError, SessionCategory};

// ── Category mapping ──────────────────────────────────────────────────────────

/// Maps an [`oxisound_core::SessionCategory`] to the corresponding
/// `AVAudioSessionCategory` `NSString` constant.
///
/// Returns `None` if the constant is unavailable on the current SDK version
/// (all constants are present since iOS 3.0 / macOS 10.15).
fn category_to_av(category: SessionCategory) -> Option<&'static AVAudioSessionCategory> {
    // SAFETY: These are static `NSString *` constants in the Apple SDK.
    //         They are `Option<&'static AVAudioSessionCategory>` in objc2-avf-audio;
    //         `None` means SDK unavailability.
    unsafe {
        match category {
            SessionCategory::Playback => objc2_avf_audio::AVAudioSessionCategoryPlayback,
            SessionCategory::Record => objc2_avf_audio::AVAudioSessionCategoryRecord,
            SessionCategory::PlayAndRecord => objc2_avf_audio::AVAudioSessionCategoryPlayAndRecord,
            SessionCategory::Ambient => objc2_avf_audio::AVAudioSessionCategoryAmbient,
            SessionCategory::SoloAmbient => objc2_avf_audio::AVAudioSessionCategorySoloAmbient,
        }
    }
}

// ── configure_session ─────────────────────────────────────────────────────────

/// Calls `[AVAudioSession sharedInstance] setCategory:error:`.
///
/// Thread-safe: protected by a `Mutex` to avoid concurrent category changes.
pub(crate) fn configure_session(category: SessionCategory) -> Result<(), OxiSoundError> {
    static LOCK: Mutex<()> = Mutex::new(());
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());

    let av_cat = category_to_av(category).ok_or_else(|| {
        OxiSoundError::UnsupportedConfig(format!(
            "AVAudioSessionCategory constant unavailable for {category:?}"
        ))
    })?;

    // SAFETY: `sharedInstance` is documented to never return nil; the returned
    //         object is valid for the duration of the application.
    let session: Retained<AVAudioSession> = unsafe { AVAudioSession::sharedInstance() };

    // SAFETY: `setCategory:error:` is a valid Objective-C message send.
    let result: Result<(), Retained<NSError>> = unsafe { session.setCategory_error(av_cat) };

    result.map_err(|ns_err| {
        // localizedDescription returns an NSString; convert to Rust String.
        let desc = ns_err.localizedDescription();
        OxiSoundError::FormatMismatch(format!("AVAudioSession setCategory failed: {desc}"))
    })
}

// ── request_microphone_permission ─────────────────────────────────────────────

/// Queries or requests microphone recording permission.
///
/// Uses the modern `AVAudioApplication` API (iOS 17+ / macOS 14+).
/// Falls back to reading `AVAudioApplication.recordPermission` directly
/// if the permission is already determined.
///
/// On iOS: if undetermined, blocks the calling thread until the user responds
/// to the system prompt.
/// On macOS: reads the `recordPermission` property via TCC without prompting.
pub(crate) fn request_microphone_permission() -> Result<bool, OxiSoundError> {
    // SAFETY: `AVAudioApplication.sharedInstance` is documented to never return nil.
    let app: Retained<AVAudioApplication> = unsafe { AVAudioApplication::sharedInstance() };

    // SAFETY: `recordPermission` is a simple property getter.
    let permission: AVAudioApplicationRecordPermission = unsafe { app.recordPermission() };

    if permission == AVAudioApplicationRecordPermission::Granted {
        return Ok(true);
    }
    if permission == AVAudioApplicationRecordPermission::Denied {
        return Ok(false);
    }
    // Undetermined: request from the user.

    // Completion state shared between this thread and the callback.
    // Using u8 atomic: 0 = pending, 1 = granted, 2 = denied.
    let result = Arc::new(AtomicU8::new(0));
    let done = Arc::new(AtomicBool::new(false));

    let result_clone = Arc::clone(&result);
    let done_clone = Arc::clone(&done);

    // Build the completion block.
    // SAFETY: The block captures `Arc`s (thread-safe) and is called exactly once
    //         by AVFoundation on an internal thread.
    let block = RcBlock::new(move |granted: Bool| {
        result_clone.store(if granted.as_bool() { 1 } else { 2 }, Ordering::Release);
        done_clone.store(true, Ordering::Release);
    });

    // SAFETY: `requestRecordPermissionWithCompletionHandler:` is a valid ObjC message.
    unsafe {
        AVAudioApplication::requestRecordPermissionWithCompletionHandler(&block);
    }

    // Spin-wait for up to 30 seconds for the user to respond (or TCC to respond
    // on macOS). In practice the callback fires within milliseconds on macOS
    // (TCC pre-determined) or within seconds on iOS (user taps Allow/Deny).
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while !done.load(Ordering::Acquire) {
        if std::time::Instant::now() >= deadline {
            return Err(OxiSoundError::Timeout(
                "microphone permission request timed out after 30 seconds".into(),
            ));
        }
        std::hint::spin_loop();
    }

    Ok(result.load(Ordering::Acquire) == 1)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Verify that category_to_av never panics for any SessionCategory variant.
    /// On macOS/iOS with SDK available, all constants must be `Some`.
    #[test]
    fn category_mapping_is_exhaustive() {
        let categories = [
            SessionCategory::Playback,
            SessionCategory::Record,
            SessionCategory::PlayAndRecord,
            SessionCategory::Ambient,
            SessionCategory::SoloAmbient,
        ];
        for cat in categories {
            let result = category_to_av(cat);
            // All constants have been present since iOS 3.0 / macOS 10.15.
            assert!(
                result.is_some(),
                "category_to_av returned None for {cat:?} — SDK too old?"
            );
        }
    }

    /// Smoke-test: `configure_session(Playback)` must not panic.
    /// In a headless CI environment without an AVAudioSession context (no
    /// NSApplication run loop), the call typically succeeds on macOS.
    #[test]
    #[cfg(target_os = "macos")]
    fn configure_session_playback_no_panic() {
        let result = configure_session(SessionCategory::Playback);
        // Accept success or a known error from a headless context.
        match result {
            Ok(()) => {}
            Err(OxiSoundError::FormatMismatch(_)) => {}
            Err(OxiSoundError::UnsupportedConfig(_)) => {}
            Err(e) => panic!("unexpected error: {e:?}"),
        }
    }
}
