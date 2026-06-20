//! Platform audio session management for OxiSound.
//!
//! This crate provides safe wrappers around platform-specific audio session
//! management APIs:
//!
//! - **iOS / macOS**: `AVAudioSession` via `objc2-avf-audio` (enabled by the
//!   `avf-audio` feature)
//! - **All other platforms**: stub implementations that document platform support
//!   and return `OxiSoundError::UnsupportedConfig` where appropriate.
//!
//! # COOLJAPAN Policy
//!
//! The `avf-audio` feature introduces Objective-C FFI via `objc2`, which requires
//! `unsafe` call sites. This crate therefore does **not** carry
//! `#![forbid(unsafe_code)]`. All unsafe blocks are narrowly scoped to `objc2`
//! API calls and do not expose unsafe to callers — every public function in this
//! crate has a safe signature.
//!
//! Default features are 100% Pure Rust (the `avf-audio` feature is opt-in).
//!
//! # Usage
//!
//! ```no_run
//! use oxisound_session::{configure_session, SessionCategory};
//! configure_session(SessionCategory::Playback).expect("session configure failed");
//! ```
//!
//! ```no_run
//! let granted = oxisound_session::request_microphone_permission()
//!     .unwrap_or(false);
//! println!("microphone granted: {granted}");
//! ```

use oxisound_core::OxiSoundError;

// ── Re-exports ────────────────────────────────────────────────────────────────

pub use oxisound_core::{AudioSession, SessionCategory, SessionInterruptionEvent};

// ── Platform implementations ──────────────────────────────────────────────────

#[cfg(all(any(target_os = "ios", target_os = "macos"), feature = "avf-audio"))]
mod avf;

// ── Public API ────────────────────────────────────────────────────────────────

/// Configures the platform audio session category.
///
/// On iOS and macOS (with `avf-audio` feature), this calls
/// `[AVAudioSession sharedInstance] setCategory:error:` to set the category.
/// On macOS without the `avf-audio` feature, the category is logged and `Ok(())`
/// is returned (CoreAudio desktop does not require explicit session management).
/// On all other platforms, returns
/// [`OxiSoundError::UnsupportedConfig`].
///
/// # Errors
///
/// Returns [`OxiSoundError::UnsupportedConfig`] on non-Apple platforms.
/// Returns [`OxiSoundError::FormatMismatch`] if the AVAudioSession call fails
/// on Apple platforms.
///
/// # Examples
///
/// ```no_run
/// use oxisound_session::configure_session;
/// use oxisound_core::SessionCategory;
/// configure_session(SessionCategory::PlayAndRecord).ok();
/// ```
pub fn configure_session(category: SessionCategory) -> Result<(), OxiSoundError> {
    configure_session_impl(category)
}

/// Requests microphone recording permission from the OS.
///
/// - On **iOS** (with `avf-audio` feature): Shows the system microphone
///   permission dialog via `[AVAudioSession requestRecordPermission:]` if not
///   yet determined. Returns `Ok(true)` if granted, `Ok(false)` if denied.
///   Note: this blocks the calling thread until the user responds.
/// - On **macOS** (with `avf-audio` feature): Returns `Ok(true)` if
///   `recordPermission` is `Granted`; otherwise returns
///   `Ok(false)`. macOS does not show a system prompt from the same API
///   as iOS — the TCC (Transparency, Consent, and Control) framework
///   manages permissions system-wide.
/// - On **macOS without `avf-audio`**: Returns `Ok(true)` (assumed granted;
///   CoreAudio desktop does not require explicit permission prompts in most
///   configurations).
/// - On **all other platforms**: Returns
///   `Err(`[`OxiSoundError::PermissionDenied`]`)` indicating the API is not
///   available.
///
/// # Errors
///
/// - [`OxiSoundError::PermissionDenied`] — on non-Apple platforms.
/// - [`OxiSoundError::FormatMismatch`] — if the AVAudioSession call fails.
///
/// # Examples
///
/// ```no_run
/// let granted = oxisound_session::request_microphone_permission()
///     .unwrap_or(false);
/// println!("microphone granted: {granted}");
/// ```
pub fn request_microphone_permission() -> Result<bool, OxiSoundError> {
    request_microphone_permission_impl()
}

// ── configure_session_impl: platform dispatch ─────────────────────────────────

// iOS + macOS with avf-audio feature: real AVAudioSession implementation.
#[cfg(all(any(target_os = "ios", target_os = "macos"), feature = "avf-audio"))]
fn configure_session_impl(category: SessionCategory) -> Result<(), OxiSoundError> {
    avf::configure_session(category)
}

// macOS without avf-audio feature: CoreAudio desktop — no session management needed.
#[cfg(all(target_os = "macos", not(feature = "avf-audio")))]
fn configure_session_impl(category: SessionCategory) -> Result<(), OxiSoundError> {
    log::debug!(
        "configure_session({category:?}): macOS CoreAudio desktop does not require \
         AVAudioSession management; returning Ok(()). Enable the `avf-audio` feature \
         for Mac Catalyst / iOS targets."
    );
    Ok(())
}

// iOS without avf-audio feature: return Unsupported.
#[cfg(all(target_os = "ios", not(feature = "avf-audio")))]
fn configure_session_impl(category: SessionCategory) -> Result<(), OxiSoundError> {
    let _ = category;
    Err(OxiSoundError::UnsupportedConfig(
        "configure_session on iOS requires the `avf-audio` feature of oxisound-session".into(),
    ))
}

// All other platforms: not supported.
#[cfg(not(any(target_os = "macos", target_os = "ios")))]
fn configure_session_impl(category: SessionCategory) -> Result<(), OxiSoundError> {
    let _ = category;
    Err(OxiSoundError::UnsupportedConfig(
        "audio session management is only available on iOS and macOS".into(),
    ))
}

// ── request_microphone_permission_impl: platform dispatch ─────────────────────

// iOS + macOS with avf-audio feature: real AVAudioSession implementation.
#[cfg(all(any(target_os = "ios", target_os = "macos"), feature = "avf-audio"))]
fn request_microphone_permission_impl() -> Result<bool, OxiSoundError> {
    avf::request_microphone_permission()
}

// macOS without avf-audio feature: assume granted (CoreAudio desktop).
#[cfg(all(target_os = "macos", not(feature = "avf-audio")))]
fn request_microphone_permission_impl() -> Result<bool, OxiSoundError> {
    log::debug!(
        "request_microphone_permission: macOS CoreAudio desktop — assumed granted. \
         Enable the `avf-audio` feature for TCC permission status checking."
    );
    Ok(true)
}

// iOS without avf-audio feature: return PermissionDenied.
#[cfg(all(target_os = "ios", not(feature = "avf-audio")))]
fn request_microphone_permission_impl() -> Result<bool, OxiSoundError> {
    Err(OxiSoundError::PermissionDenied(
        "request_microphone_permission on iOS requires the `avf-audio` feature of \
         oxisound-session"
            .into(),
    ))
}

// All other platforms: not supported.
#[cfg(not(any(target_os = "macos", target_os = "ios")))]
fn request_microphone_permission_impl() -> Result<bool, OxiSoundError> {
    Err(OxiSoundError::PermissionDenied(
        "microphone permission prompt is not available on this platform".into(),
    ))
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use oxisound_core::SessionCategory;

    /// On non-Apple platforms, `configure_session` must return `UnsupportedConfig`.
    #[test]
    #[cfg(not(any(target_os = "macos", target_os = "ios")))]
    fn configure_session_unsupported_on_other_platforms() {
        let result = configure_session(SessionCategory::Playback);
        assert!(
            matches!(result, Err(OxiSoundError::UnsupportedConfig(_))),
            "expected UnsupportedConfig, got {result:?}"
        );
    }

    /// On non-Apple platforms, `request_microphone_permission` must return
    /// `PermissionDenied`.
    #[test]
    #[cfg(not(any(target_os = "macos", target_os = "ios")))]
    fn microphone_permission_denied_on_other_platforms() {
        let result = request_microphone_permission();
        assert!(
            matches!(result, Err(OxiSoundError::PermissionDenied(_))),
            "expected PermissionDenied, got {result:?}"
        );
    }

    /// On macOS without `avf-audio` feature, `configure_session` must return `Ok(())`.
    #[test]
    #[cfg(all(target_os = "macos", not(feature = "avf-audio")))]
    fn configure_session_ok_on_macos_no_avf() {
        let result = configure_session(SessionCategory::Playback);
        assert!(result.is_ok(), "expected Ok(()), got {result:?}");
    }

    /// On macOS without `avf-audio` feature, `request_microphone_permission`
    /// returns `Ok(true)` (assumed granted for CoreAudio desktop).
    #[test]
    #[cfg(all(target_os = "macos", not(feature = "avf-audio")))]
    fn microphone_permission_assumed_granted_on_macos_no_avf() {
        let result = request_microphone_permission();
        assert!(
            matches!(result, Ok(true)),
            "expected Ok(true), got {result:?}"
        );
    }

    /// Smoke-test: `configure_session` for every `SessionCategory` variant must
    /// not panic (regardless of whether it succeeds or returns an error).
    #[test]
    fn configure_session_all_categories_no_panic() {
        let categories = [
            SessionCategory::Playback,
            SessionCategory::Record,
            SessionCategory::PlayAndRecord,
            SessionCategory::Ambient,
            SessionCategory::SoloAmbient,
        ];
        for cat in categories {
            let _ = configure_session(cat);
        }
    }

    /// Smoke-test: `request_microphone_permission` must not panic.
    #[test]
    fn request_microphone_permission_no_panic() {
        let _ = request_microphone_permission();
    }
}
