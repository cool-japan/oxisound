//! Wire sample formats and `f32` ⇄ PulseAudio byte conversion.
//!
//! OxiSound's stream traits speak interleaved `f32`; the PulseAudio native protocol
//! carries raw PCM bytes in whatever format the stream was created with. This module
//! owns that boundary.
//!
//! [`PulseSampleFormat`] mirrors the subset of PulseAudio's `pa_sample_format_t` that
//! this backend can encode and decode losslessly-or-better. It is defined here rather
//! than re-exported from the protocol crate so that negotiation and conversion are
//! platform-independent and unit-tested on every host; the Linux adapter converts
//! between the two enums at the protocol boundary.

use oxisound_core::{OxiSoundError, SampleFormat};

/// A PCM sample format that this backend can put on the PulseAudio wire.
///
/// All multi-byte formats are little-endian: PulseAudio's big-endian variants exist
/// for big-endian hosts, and the server converts transparently when the client asks
/// for a little-endian spelling, so offering only LE keeps conversion simple without
/// losing any capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PulseSampleFormat {
    /// Unsigned 8-bit PCM (`PA_SAMPLE_U8`).
    U8,
    /// Signed 16-bit PCM, little-endian (`PA_SAMPLE_S16LE`).
    S16Le,
    /// Signed 24-bit packed PCM, little-endian (`PA_SAMPLE_S24LE`).
    S24Le,
    /// Signed 32-bit PCM, little-endian (`PA_SAMPLE_S32LE`).
    S32Le,
    /// 32-bit IEEE float, little-endian, nominal range `[-1.0, 1.0]`
    /// (`PA_SAMPLE_FLOAT32LE`).
    Float32Le,
}

impl PulseSampleFormat {
    /// Number of bytes one sample of this format occupies on the wire.
    ///
    /// # Examples
    /// ```
    /// use oxisound_pulse::PulseSampleFormat;
    /// assert_eq!(PulseSampleFormat::S24Le.bytes_per_sample(), 3);
    /// assert_eq!(PulseSampleFormat::Float32Le.bytes_per_sample(), 4);
    /// ```
    #[must_use]
    pub const fn bytes_per_sample(self) -> usize {
        match self {
            Self::U8 => 1,
            Self::S16Le => 2,
            Self::S24Le => 3,
            Self::S32Le | Self::Float32Le => 4,
        }
    }

    /// The equivalent [`oxisound_core::SampleFormat`].
    ///
    /// # Examples
    /// ```
    /// use oxisound_core::SampleFormat;
    /// use oxisound_pulse::PulseSampleFormat;
    /// assert_eq!(PulseSampleFormat::S16Le.to_core(), SampleFormat::I16);
    /// ```
    #[must_use]
    pub const fn to_core(self) -> SampleFormat {
        match self {
            Self::U8 => SampleFormat::U8,
            Self::S16Le => SampleFormat::I16,
            Self::S24Le => SampleFormat::I24,
            Self::S32Le => SampleFormat::I32,
            Self::Float32Le => SampleFormat::F32,
        }
    }

    /// The wire format for an [`oxisound_core::SampleFormat`], if one exists.
    ///
    /// Returns `None` for [`SampleFormat::F64`], which the PulseAudio protocol has no
    /// encoding for.
    ///
    /// # Examples
    /// ```
    /// use oxisound_core::SampleFormat;
    /// use oxisound_pulse::PulseSampleFormat;
    /// assert_eq!(
    ///     PulseSampleFormat::from_core(SampleFormat::F32),
    ///     Some(PulseSampleFormat::Float32Le)
    /// );
    /// assert_eq!(PulseSampleFormat::from_core(SampleFormat::F64), None);
    /// ```
    #[must_use]
    pub const fn from_core(fmt: SampleFormat) -> Option<Self> {
        match fmt {
            SampleFormat::U8 => Some(Self::U8),
            SampleFormat::I16 => Some(Self::S16Le),
            SampleFormat::I24 => Some(Self::S24Le),
            SampleFormat::I32 => Some(Self::S32Le),
            SampleFormat::F32 => Some(Self::Float32Le),
            SampleFormat::F64 => None,
        }
    }

    /// Number of bytes one interleaved frame of `channels` samples occupies.
    ///
    /// A zero channel count is clamped to one so the result is never zero and can be
    /// used as a divisor unchecked.
    #[must_use]
    pub const fn frame_size(self, channels: u16) -> usize {
        let channels = if channels == 0 { 1 } else { channels as usize };
        self.bytes_per_sample() * channels
    }

    /// Encodes interleaved `f32` samples, appending the wire bytes to `out`.
    ///
    /// Samples are clamped to `[-1.0, 1.0]` before scaling, so out-of-range input
    /// saturates instead of wrapping.
    ///
    /// # Examples
    /// ```
    /// use oxisound_pulse::PulseSampleFormat;
    /// let mut out = Vec::new();
    /// PulseSampleFormat::S16Le.encode_f32(&[0.0, 1.0], &mut out);
    /// assert_eq!(out, vec![0x00, 0x00, 0xFF, 0x7F]);
    /// ```
    pub fn encode_f32(self, samples: &[f32], out: &mut Vec<u8>) {
        out.reserve(samples.len() * self.bytes_per_sample());
        match self {
            Self::U8 => {
                for &s in samples {
                    let scaled = (clamp_unit(s) * 127.0).round() as i32 + 128;
                    out.push(scaled.clamp(0, 255) as u8);
                }
            }
            Self::S16Le => {
                for &s in samples {
                    let v = (clamp_unit(s) * f32::from(i16::MAX)).round() as i32;
                    out.extend_from_slice(
                        &(v.clamp(i16::MIN as i32, i16::MAX as i32) as i16).to_le_bytes(),
                    );
                }
            }
            Self::S24Le => {
                const MAX_24: f32 = 8_388_607.0;
                for &s in samples {
                    let v = (clamp_unit(s) * MAX_24).round() as i32;
                    let v = v.clamp(-8_388_608, 8_388_607);
                    let bytes = v.to_le_bytes();
                    out.extend_from_slice(&bytes[..3]);
                }
            }
            Self::S32Le => {
                for &s in samples {
                    let v = (f64::from(clamp_unit(s)) * f64::from(i32::MAX)).round();
                    let v = v.clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32;
                    out.extend_from_slice(&v.to_le_bytes());
                }
            }
            Self::Float32Le => {
                for &s in samples {
                    out.extend_from_slice(&s.to_le_bytes());
                }
            }
        }
    }

    /// Decodes wire bytes into interleaved `f32` samples.
    ///
    /// Decodes `min(out.len(), bytes.len() / bytes_per_sample())` samples and returns
    /// that count. Trailing bytes that do not form a whole sample are ignored — the
    /// caller is responsible for only handing over whole samples if it wants to keep
    /// stream alignment (see [`crate::model::whole_samples`]).
    ///
    /// # Examples
    /// ```
    /// use oxisound_pulse::PulseSampleFormat;
    /// let mut out = [0.0f32; 2];
    /// let n = PulseSampleFormat::S16Le.decode_to_f32(&[0x00, 0x00, 0xFF, 0x7F], &mut out);
    /// assert_eq!(n, 2);
    /// assert!((out[1] - 1.0).abs() < 1e-4);
    /// ```
    pub fn decode_to_f32(self, bytes: &[u8], out: &mut [f32]) -> usize {
        let width = self.bytes_per_sample();
        let count = (bytes.len() / width).min(out.len());
        for (i, slot) in out.iter_mut().enumerate().take(count) {
            let chunk = &bytes[i * width..(i + 1) * width];
            *slot = match self {
                Self::U8 => (f32::from(chunk[0]) - 128.0) / 128.0,
                Self::S16Le => f32::from(i16::from_le_bytes([chunk[0], chunk[1]])) / 32_768.0,
                Self::S24Le => {
                    // Sign-extend the packed 24-bit value into an i32.
                    let raw = i32::from_le_bytes([chunk[0], chunk[1], chunk[2], 0]);
                    let signed = (raw << 8) >> 8;
                    signed as f32 / 8_388_608.0
                }
                Self::S32Le => {
                    let v = i32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
                    (f64::from(v) / 2_147_483_648.0) as f32
                }
                Self::Float32Le => f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]),
            };
        }
        count
    }
}

fn clamp_unit(s: f32) -> f32 {
    if s.is_nan() { 0.0 } else { s.clamp(-1.0, 1.0) }
}

/// Chooses the wire format for a stream.
///
/// The policy, applied in order:
///
/// 1. [`StreamConfig::sample_format`](oxisound_core::StreamConfig::sample_format), when set.
///    An unrepresentable request (`F64`) is a hard error — silently substituting a
///    different format would violate an explicit caller contract.
/// 2. The first representable entry of
///    [`StreamConfig::preferred_formats`](oxisound_core::StreamConfig::preferred_formats).
/// 3. The device's native format, when this backend can encode it. Matching the device
///    avoids a server-side conversion pass.
/// 4. [`PulseSampleFormat::Float32Le`], the lossless choice for OxiSound's `f32` API.
///
/// # Errors
///
/// Returns [`OxiSoundError::FormatMismatch`] when `requested` is a format the
/// PulseAudio protocol cannot express.
///
/// # Examples
/// ```
/// use oxisound_core::SampleFormat;
/// use oxisound_pulse::{negotiate_format, PulseSampleFormat};
///
/// // Explicit request wins.
/// assert_eq!(
///     negotiate_format(Some(SampleFormat::I16), &[], Some(PulseSampleFormat::Float32Le)).unwrap(),
///     PulseSampleFormat::S16Le
/// );
/// // Otherwise the device's native format is matched.
/// assert_eq!(
///     negotiate_format(None, &[], Some(PulseSampleFormat::S16Le)).unwrap(),
///     PulseSampleFormat::S16Le
/// );
/// // With nothing to go on, f32 is used.
/// assert_eq!(
///     negotiate_format(None, &[], None).unwrap(),
///     PulseSampleFormat::Float32Le
/// );
/// ```
pub fn negotiate_format(
    requested: Option<SampleFormat>,
    preferred: &[SampleFormat],
    native: Option<PulseSampleFormat>,
) -> Result<PulseSampleFormat, OxiSoundError> {
    if let Some(fmt) = requested {
        return PulseSampleFormat::from_core(fmt).ok_or_else(|| {
            OxiSoundError::FormatMismatch(format!(
                "sample format {fmt} has no PulseAudio wire encoding; use f32, i32, i24, i16 or u8"
            ))
        });
    }
    for &fmt in preferred {
        if let Some(wire) = PulseSampleFormat::from_core(fmt) {
            return Ok(wire);
        }
    }
    Ok(native.unwrap_or(PulseSampleFormat::Float32Le))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [PulseSampleFormat; 5] = [
        PulseSampleFormat::U8,
        PulseSampleFormat::S16Le,
        PulseSampleFormat::S24Le,
        PulseSampleFormat::S32Le,
        PulseSampleFormat::Float32Le,
    ];

    #[test]
    fn byte_widths_match_core_widths() {
        for fmt in ALL {
            assert_eq!(
                fmt.bytes_per_sample(),
                fmt.to_core().byte_size(),
                "{fmt:?} width must agree with oxisound-core"
            );
        }
    }

    #[test]
    fn core_roundtrip_is_stable() {
        for fmt in ALL {
            assert_eq!(PulseSampleFormat::from_core(fmt.to_core()), Some(fmt));
        }
        assert_eq!(PulseSampleFormat::from_core(SampleFormat::F64), None);
    }

    #[test]
    fn frame_size_scales_with_channels_and_never_zero() {
        assert_eq!(PulseSampleFormat::S16Le.frame_size(2), 4);
        assert_eq!(PulseSampleFormat::Float32Le.frame_size(6), 24);
        assert_eq!(PulseSampleFormat::U8.frame_size(0), 1);
    }

    #[test]
    fn encode_decode_roundtrip_within_quantisation_error() {
        let input = [-1.0f32, -0.5, -0.25, 0.0, 0.25, 0.5, 0.999];
        for fmt in ALL {
            let mut bytes = Vec::new();
            fmt.encode_f32(&input, &mut bytes);
            assert_eq!(bytes.len(), input.len() * fmt.bytes_per_sample());

            let mut out = vec![0.0f32; input.len()];
            let n = fmt.decode_to_f32(&bytes, &mut out);
            assert_eq!(n, input.len());

            // Encoding scales by `MAX` (2^(n-1) - 1) and decoding divides by 2^(n-1) —
            // the conventional asymmetric pair, which keeps -1.0 and +1.0 both
            // representable at the cost of up to two LSBs of round-trip error.
            let tolerance = match fmt {
                PulseSampleFormat::U8 => 2.0 / 128.0,
                PulseSampleFormat::S16Le => 2.0 / 32_768.0,
                PulseSampleFormat::S24Le => 2.0 / 8_388_608.0,
                PulseSampleFormat::S32Le | PulseSampleFormat::Float32Le => 1e-6,
            };
            for (a, b) in input.iter().zip(out.iter()) {
                assert!(
                    (a - b).abs() <= tolerance,
                    "{fmt:?}: {a} round-tripped to {b} (tolerance {tolerance})"
                );
            }
        }
    }

    #[test]
    fn encoding_saturates_instead_of_wrapping() {
        for fmt in ALL {
            let mut bytes = Vec::new();
            fmt.encode_f32(&[10.0, -10.0], &mut bytes);
            let mut out = [0.0f32; 2];
            assert_eq!(fmt.decode_to_f32(&bytes, &mut out), 2);
            assert!(out[0] > 0.9, "{fmt:?} positive overload must clip high");
            assert!(out[1] < -0.9, "{fmt:?} negative overload must clip low");
        }
    }

    #[test]
    fn nan_encodes_as_silence() {
        // Float32Le is a bit-exact passthrough, so it is exempt by construction.
        for fmt in [
            PulseSampleFormat::U8,
            PulseSampleFormat::S16Le,
            PulseSampleFormat::S24Le,
            PulseSampleFormat::S32Le,
        ] {
            let mut bytes = Vec::new();
            fmt.encode_f32(&[f32::NAN], &mut bytes);
            let mut out = [1.0f32];
            assert_eq!(fmt.decode_to_f32(&bytes, &mut out), 1);
            assert!(out[0].abs() < 0.01, "{fmt:?} NaN must encode as silence");
        }
    }

    #[test]
    fn s24_negative_values_sign_extend() {
        let mut bytes = Vec::new();
        PulseSampleFormat::S24Le.encode_f32(&[-1.0], &mut bytes);
        assert_eq!(bytes.len(), 3);
        let mut out = [0.0f32];
        PulseSampleFormat::S24Le.decode_to_f32(&bytes, &mut out);
        assert!(out[0] < -0.99, "sign extension broken: {}", out[0]);
    }

    #[test]
    fn decode_ignores_trailing_partial_sample() {
        // 5 bytes of a 4-byte format: exactly one whole sample is decodable.
        let bytes = [0u8, 0, 0, 0, 0x7F];
        let mut out = [1.0f32; 2];
        assert_eq!(
            PulseSampleFormat::Float32Le.decode_to_f32(&bytes, &mut out),
            1
        );
        assert_eq!(out[0], 0.0);
        assert_eq!(out[1], 1.0, "second slot must be untouched");
    }

    #[test]
    fn decode_is_bounded_by_the_output_slice() {
        let mut bytes = Vec::new();
        PulseSampleFormat::S16Le.encode_f32(&[0.1, 0.2, 0.3, 0.4], &mut bytes);
        let mut out = [0.0f32; 2];
        assert_eq!(PulseSampleFormat::S16Le.decode_to_f32(&bytes, &mut out), 2);
    }

    #[test]
    fn negotiation_explicit_request_wins_over_native() {
        let picked = negotiate_format(
            Some(SampleFormat::U8),
            &[SampleFormat::F32],
            Some(PulseSampleFormat::S16Le),
        )
        .expect("u8 is representable");
        assert_eq!(picked, PulseSampleFormat::U8);
    }

    #[test]
    fn negotiation_rejects_unrepresentable_explicit_request() {
        let err = negotiate_format(Some(SampleFormat::F64), &[], None)
            .expect_err("f64 has no PA encoding");
        assert_eq!(err.kind(), "format-mismatch");
    }

    #[test]
    fn negotiation_walks_preferred_list_in_order() {
        let picked = negotiate_format(
            None,
            &[SampleFormat::F64, SampleFormat::I32, SampleFormat::I16],
            Some(PulseSampleFormat::U8),
        )
        .expect("i32 is representable");
        assert_eq!(picked, PulseSampleFormat::S32Le);
    }

    #[test]
    fn negotiation_falls_back_to_native_then_f32() {
        assert_eq!(
            negotiate_format(None, &[], Some(PulseSampleFormat::S24Le)).expect("native"),
            PulseSampleFormat::S24Le
        );
        assert_eq!(
            negotiate_format(None, &[SampleFormat::F64], None).expect("fallback"),
            PulseSampleFormat::Float32Le
        );
    }
}
