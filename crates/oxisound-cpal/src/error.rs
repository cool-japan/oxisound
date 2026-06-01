//! Error mapping helpers for cpal error types.
//!
//! These helpers convert cpal-specific errors into `OxiSoundError` without
//! using `impl From<...>` (which would violate the orphan rule).

use oxisound_core::{OxiSoundError, SampleFormat as CoreSampleFormat};

pub(crate) fn map_build_stream_err(e: cpal::BuildStreamError) -> OxiSoundError {
    match e {
        cpal::BuildStreamError::DeviceNotAvailable => {
            OxiSoundError::Disconnected("build stream: device not available".into())
        }
        cpal::BuildStreamError::InvalidArgument => {
            OxiSoundError::UnsupportedConfig("build stream: invalid argument".into())
        }
        _ => OxiSoundError::Stream(e.to_string()),
    }
}

pub(crate) fn map_play_stream_err(e: cpal::PlayStreamError) -> OxiSoundError {
    match e {
        cpal::PlayStreamError::DeviceNotAvailable => {
            OxiSoundError::Disconnected("play stream: device not available".into())
        }
        _ => OxiSoundError::Stream(e.to_string()),
    }
}

pub(crate) fn map_devices_err(e: cpal::DevicesError) -> OxiSoundError {
    OxiSoundError::Device(e.to_string())
}

pub(crate) fn map_default_config_err(e: cpal::DefaultStreamConfigError) -> OxiSoundError {
    match e {
        cpal::DefaultStreamConfigError::DeviceNotAvailable => {
            OxiSoundError::Disconnected("default config: device not available".into())
        }
        _ => OxiSoundError::UnsupportedConfig(e.to_string()),
    }
}

pub(crate) fn map_supported_configs_err(e: cpal::SupportedStreamConfigsError) -> OxiSoundError {
    match e {
        cpal::SupportedStreamConfigsError::DeviceNotAvailable => {
            OxiSoundError::Disconnected("supported configs: device not available".into())
        }
        _ => OxiSoundError::Device(e.to_string()),
    }
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
