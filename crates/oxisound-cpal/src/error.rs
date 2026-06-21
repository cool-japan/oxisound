//! Error mapping helpers for cpal error types.
//!
//! These helpers convert cpal-specific errors into `OxiSoundError` without
//! using `impl From<...>` (which would violate the orphan rule).

use oxisound_core::{OxiSoundError, SampleFormat as CoreSampleFormat};

/// Maps a cpal [`Error`](cpal::Error) to the closest [`OxiSoundError`].
///
/// As of cpal 0.18 every fallible operation reports the unified [`cpal::Error`]
/// carrying an [`ErrorKind`](cpal::ErrorKind), replacing the per-operation error
/// enums (`BuildStreamError`, `PlayStreamError`, `DevicesError`, …) used by
/// earlier releases. `context` becomes the human-readable prefix, and
/// `fallback` builds the error for kinds without a more specific mapping.
fn map_cpal_err(
    e: cpal::Error,
    context: &str,
    fallback: fn(String) -> OxiSoundError,
) -> OxiSoundError {
    use cpal::ErrorKind;
    let detail = format!("{context}: {e}");
    match e.kind() {
        ErrorKind::DeviceNotAvailable | ErrorKind::StreamInvalidated => {
            OxiSoundError::Disconnected(detail)
        }
        ErrorKind::PermissionDenied => OxiSoundError::PermissionDenied(detail),
        ErrorKind::InvalidInput | ErrorKind::UnsupportedConfig => {
            OxiSoundError::UnsupportedConfig(detail)
        }
        ErrorKind::UnsupportedOperation => OxiSoundError::Unsupported(detail),
        ErrorKind::Xrun => OxiSoundError::Underrun(detail),
        _ => fallback(detail),
    }
}

pub(crate) fn map_build_stream_err(e: cpal::Error) -> OxiSoundError {
    map_cpal_err(e, "build stream", OxiSoundError::Stream)
}

pub(crate) fn map_play_stream_err(e: cpal::Error) -> OxiSoundError {
    map_cpal_err(e, "play stream", OxiSoundError::Stream)
}

pub(crate) fn map_devices_err(e: cpal::Error) -> OxiSoundError {
    map_cpal_err(e, "enumerate devices", OxiSoundError::Device)
}

pub(crate) fn map_default_config_err(e: cpal::Error) -> OxiSoundError {
    map_cpal_err(e, "default config", OxiSoundError::UnsupportedConfig)
}

pub(crate) fn map_supported_configs_err(e: cpal::Error) -> OxiSoundError {
    map_cpal_err(e, "supported configs", OxiSoundError::Device)
}

/// Maps a cpal sample format to the closest core `SampleFormat`.
/// Some cpal formats (U16, I8, I24, etc.) are not 1:1 — they're mapped to the
/// nearest core type. DSD and exotic formats fall back to F32.
pub(crate) fn cpal_to_core_format(fmt: cpal::SampleFormat) -> CoreSampleFormat {
    match fmt {
        cpal::SampleFormat::F32 => CoreSampleFormat::F32,
        cpal::SampleFormat::F64 => CoreSampleFormat::F64,
        cpal::SampleFormat::I16 => CoreSampleFormat::I16,
        cpal::SampleFormat::I32 => CoreSampleFormat::I32,
        cpal::SampleFormat::U8 => CoreSampleFormat::U8,
        // Closest available core type for formats without a direct mapping:
        cpal::SampleFormat::I8 => CoreSampleFormat::I16,
        cpal::SampleFormat::U16 => CoreSampleFormat::I16,
        cpal::SampleFormat::I24 => CoreSampleFormat::I24,
        cpal::SampleFormat::U24 => CoreSampleFormat::I32,
        cpal::SampleFormat::U32 => CoreSampleFormat::I32,
        cpal::SampleFormat::I64 => CoreSampleFormat::F64,
        cpal::SampleFormat::U64 => CoreSampleFormat::F64,
        // DSD and any future variants fall back to F32
        _ => CoreSampleFormat::F32,
    }
}

/// Maps a core `SampleFormat` back to the corresponding cpal `SampleFormat`.
///
/// Used when the negotiated format must be used to open a typed cpal stream.
pub(crate) fn core_to_cpal_format(fmt: CoreSampleFormat) -> cpal::SampleFormat {
    match fmt {
        CoreSampleFormat::F32 => cpal::SampleFormat::F32,
        CoreSampleFormat::F64 => cpal::SampleFormat::F64,
        CoreSampleFormat::I16 => cpal::SampleFormat::I16,
        CoreSampleFormat::I32 => cpal::SampleFormat::I32,
        CoreSampleFormat::U8 => cpal::SampleFormat::U8,
        CoreSampleFormat::I24 => cpal::SampleFormat::I24,
    }
}
