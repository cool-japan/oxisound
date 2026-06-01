//! Bridges between `oxisound-core` audio types and `oxiaudio-core` audio types.
//!
//! Enabled by the `oxiaudio` cargo feature. All conversions are zero-copy (enum dispatch).
//!
//! ## SampleFormat
//!
//! Both crates expose the same 6 variants (F32, I16, I24, I32, U8, F64), so the bridge is
//! bijective and infallible in both directions. Use [`SampleFormat::to_oxiaudio`] to convert
//! from oxisound to oxiaudio, and `From<oxiaudio_core::SampleFormat>` in the other direction.
//!
//! ## Channel / ChannelId
//!
//! `oxisound_core::Channel` has 8 variants; `oxiaudio_core::ChannelId` has 12 (adds 4 height
//! channels: TopFrontLeft/Right, TopRearLeft/Right). Conversion from `Channel` to `ChannelId`
//! is total. The reverse is fallible: the 4 `Top*` height channels have no oxisound analogue
//! and return [`OxiSoundError::FormatMismatch`].
//!
//! Channel mapping convention:
//! - `SurroundLeft/Right` ↔ `ChannelId::SideLeft/Right` (ITU side surround)
//! - `BackLeft/Right` ↔ `ChannelId::RearLeft/Right` (ITU rear surround)
//! - `Center` ↔ `ChannelId::FrontCenter`
//! - `Lfe` ↔ `ChannelId::LowFrequency`
//!
//! ## ChannelRouting / ChannelMap
//!
//! `From<&ChannelRouting> for oxiaudio_core::ChannelMap` sorts pairs by physical channel index
//! and maps each `Channel` to its `ChannelId`. `TryFrom<&oxiaudio_core::ChannelMap> for
//! ChannelRouting` is fallible because the source may contain `Top*` height channels.

use alloc::format;
use alloc::vec::Vec;

use oxiaudio_core::SampleFormat as AudioSampleFormat;

use crate::{Channel, ChannelRouting, OxiSoundError, SampleFormat};

// ---------------------------------------------------------------------------
// SampleFormat bridge
// ---------------------------------------------------------------------------

impl From<AudioSampleFormat> for SampleFormat {
    fn from(f: AudioSampleFormat) -> Self {
        match f {
            AudioSampleFormat::F32 => SampleFormat::F32,
            AudioSampleFormat::I16 => SampleFormat::I16,
            AudioSampleFormat::I24 => SampleFormat::I24,
            AudioSampleFormat::I32 => SampleFormat::I32,
            AudioSampleFormat::U8 => SampleFormat::U8,
            AudioSampleFormat::F64 => SampleFormat::F64,
        }
    }
}

impl SampleFormat {
    /// Convert to the `oxiaudio_core` sample format representation.
    pub fn to_oxiaudio(self) -> AudioSampleFormat {
        match self {
            SampleFormat::F32 => AudioSampleFormat::F32,
            SampleFormat::I16 => AudioSampleFormat::I16,
            SampleFormat::I24 => AudioSampleFormat::I24,
            SampleFormat::I32 => AudioSampleFormat::I32,
            SampleFormat::U8 => AudioSampleFormat::U8,
            SampleFormat::F64 => AudioSampleFormat::F64,
        }
    }
}

// ---------------------------------------------------------------------------
// Channel / ChannelId bridge
// ---------------------------------------------------------------------------

use oxiaudio_core::ChannelId;

impl From<Channel> for ChannelId {
    fn from(c: Channel) -> Self {
        match c {
            Channel::FrontLeft => ChannelId::FrontLeft,
            Channel::FrontRight => ChannelId::FrontRight,
            Channel::Center => ChannelId::FrontCenter,
            Channel::Lfe => ChannelId::LowFrequency,
            Channel::SurroundLeft => ChannelId::SideLeft,
            Channel::SurroundRight => ChannelId::SideRight,
            Channel::BackLeft => ChannelId::RearLeft,
            Channel::BackRight => ChannelId::RearRight,
        }
    }
}

impl TryFrom<ChannelId> for Channel {
    type Error = OxiSoundError;

    fn try_from(id: ChannelId) -> Result<Self, Self::Error> {
        match id {
            ChannelId::FrontLeft => Ok(Channel::FrontLeft),
            ChannelId::FrontRight => Ok(Channel::FrontRight),
            ChannelId::FrontCenter => Ok(Channel::Center),
            ChannelId::LowFrequency => Ok(Channel::Lfe),
            ChannelId::SideLeft => Ok(Channel::SurroundLeft),
            ChannelId::SideRight => Ok(Channel::SurroundRight),
            ChannelId::RearLeft => Ok(Channel::BackLeft),
            ChannelId::RearRight => Ok(Channel::BackRight),
            other => Err(OxiSoundError::FormatMismatch(format!(
                "oxiaudio_core::ChannelId::{other:?} has no oxisound_core::Channel equivalent (height channel)",
            ))),
        }
    }
}

// ---------------------------------------------------------------------------
// ChannelRouting / ChannelMap bridge
// ---------------------------------------------------------------------------

use oxiaudio_core::ChannelMap;

impl From<&ChannelRouting> for ChannelMap {
    fn from(r: &ChannelRouting) -> Self {
        let mut pairs = r.0.clone();
        pairs.sort_by_key(|&(_, idx)| idx);
        let ids: Vec<ChannelId> = pairs
            .into_iter()
            .map(|(ch, _)| ChannelId::from(ch))
            .collect();
        ChannelMap::new(ids)
    }
}

impl ChannelRouting {
    /// Convert this routing to an `oxiaudio_core::ChannelMap`.
    pub fn to_oxiaudio_channel_map(&self) -> ChannelMap {
        ChannelMap::from(self)
    }
}

impl TryFrom<&ChannelMap> for ChannelRouting {
    type Error = OxiSoundError;

    fn try_from(map: &ChannelMap) -> Result<Self, Self::Error> {
        let pairs = map
            .iter()
            .enumerate()
            .map(|(idx, &id)| Channel::try_from(id).map(|ch| (ch, idx)))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(ChannelRouting(pairs))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;
    use oxiaudio_core::{ChannelId, ChannelMap, SampleFormat as AF};

    #[test]
    fn sample_format_roundtrip_from() {
        let variants = [AF::F32, AF::I16, AF::I24, AF::I32, AF::U8, AF::F64];
        for v in variants {
            let ours = SampleFormat::from(v);
            let back = ours.to_oxiaudio();
            assert_eq!(v, back, "roundtrip failed for {v:?}");
        }
    }

    #[test]
    fn sample_format_roundtrip_to_oxiaudio() {
        use crate::SampleFormat as SF;
        let variants = [SF::F32, SF::I16, SF::I24, SF::I32, SF::U8, SF::F64];
        for v in variants {
            let theirs = v.to_oxiaudio();
            let back = SampleFormat::from(theirs);
            assert_eq!(v, back, "roundtrip failed for {v:?}");
        }
    }

    #[test]
    fn channel_to_channel_id_all() {
        use crate::Channel as C;
        let pairs = [
            (C::FrontLeft, ChannelId::FrontLeft),
            (C::FrontRight, ChannelId::FrontRight),
            (C::Center, ChannelId::FrontCenter),
            (C::Lfe, ChannelId::LowFrequency),
            (C::SurroundLeft, ChannelId::SideLeft),
            (C::SurroundRight, ChannelId::SideRight),
            (C::BackLeft, ChannelId::RearLeft),
            (C::BackRight, ChannelId::RearRight),
        ];
        for (ch, expected) in pairs {
            assert_eq!(ChannelId::from(ch), expected);
        }
    }

    #[test]
    fn channel_id_to_channel_mappable() {
        use crate::Channel as C;
        let pairs = [
            (ChannelId::FrontLeft, C::FrontLeft),
            (ChannelId::FrontRight, C::FrontRight),
            (ChannelId::FrontCenter, C::Center),
            (ChannelId::LowFrequency, C::Lfe),
            (ChannelId::SideLeft, C::SurroundLeft),
            (ChannelId::SideRight, C::SurroundRight),
            (ChannelId::RearLeft, C::BackLeft),
            (ChannelId::RearRight, C::BackRight),
        ];
        for (id, expected) in pairs {
            assert_eq!(
                Channel::try_from(id).expect("non-Top* ChannelId must convert to Channel"),
                expected
            );
        }
    }

    #[test]
    fn channel_id_top_variants_err() {
        let top_variants = [
            ChannelId::TopFrontLeft,
            ChannelId::TopFrontRight,
            ChannelId::TopRearLeft,
            ChannelId::TopRearRight,
        ];
        for id in top_variants {
            assert!(Channel::try_from(id).is_err(), "{id:?} should fail");
        }
    }

    #[test]
    fn channel_routing_stereo_roundtrip() {
        let r = ChannelRouting::stereo();
        let map = ChannelMap::from(&r);
        let back = ChannelRouting::try_from(&map)
            .expect("stereo ChannelMap must convert back to ChannelRouting");
        assert_eq!(r, back);
    }

    #[test]
    fn channel_routing_5_1_roundtrip() {
        let r = ChannelRouting::surround_5_1();
        let map = r.to_oxiaudio_channel_map();
        let back = ChannelRouting::try_from(&map)
            .expect("5.1 ChannelMap must convert back to ChannelRouting");
        assert_eq!(r, back);
    }

    #[test]
    fn channel_routing_7_1_roundtrip() {
        let r = ChannelRouting::surround_7_1();
        let map = ChannelMap::from(&r);
        let back = ChannelRouting::try_from(&map)
            .expect("7.1 ChannelMap must convert back to ChannelRouting");
        assert_eq!(r, back);
    }

    #[test]
    fn channel_map_with_top_channel_fails() {
        let map = ChannelMap::new(vec![ChannelId::FrontLeft, ChannelId::TopFrontLeft]);
        assert!(ChannelRouting::try_from(&map).is_err());
    }
}
