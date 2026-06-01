//! Config validation and conversion helpers for cpal streams.

use cpal::traits::DeviceTrait;
use oxisound_core::{
    DeviceCapabilities, OxiSoundError, SampleFormat as CoreSampleFormat,
    StreamConfig as OxiStreamConfig,
};

use crate::error::{cpal_to_core_format, map_supported_configs_err};

// ---------------------------------------------------------------------------
// Config range helpers
// ---------------------------------------------------------------------------

pub(crate) fn collect_config_ranges(dev: &cpal::Device) -> (Vec<u32>, Vec<u16>) {
    let configs = match dev.supported_output_configs() {
        Ok(c) => c,
        Err(_) => return (vec![], vec![]),
    };
    let mut rates: Vec<u32> = Vec::new();
    let mut chans: Vec<u16> = Vec::new();
    for range in configs {
        chans.push(range.channels());
        rates.push(range.min_sample_rate());
        rates.push(range.max_sample_rate());
    }
    rates.sort_unstable();
    rates.dedup();
    chans.sort_unstable();
    chans.dedup();
    (rates, chans)
}

pub(crate) fn collect_input_config_ranges(dev: &cpal::Device) -> (Vec<u32>, Vec<u16>) {
    let configs = match dev.supported_input_configs() {
        Ok(c) => c,
        Err(_) => return (vec![], vec![]),
    };
    let mut rates: Vec<u32> = Vec::new();
    let mut chans: Vec<u16> = Vec::new();
    for range in configs {
        chans.push(range.channels());
        rates.push(range.min_sample_rate());
        rates.push(range.max_sample_rate());
    }
    rates.sort_unstable();
    rates.dedup();
    chans.sort_unstable();
    chans.dedup();
    (rates, chans)
}

// ---------------------------------------------------------------------------
// DeviceCapabilities builder
// ---------------------------------------------------------------------------

/// Queries a device for its capability data and returns a populated `DeviceCapabilities`.
pub(crate) fn build_device_capabilities(
    device: &cpal::Device,
    is_output: bool,
) -> Option<DeviceCapabilities> {
    let configs: Vec<_> = if is_output {
        device.supported_output_configs().ok()?.collect()
    } else {
        device.supported_input_configs().ok()?.collect()
    };

    let mut min_buf: Option<u32> = None;
    let mut max_buf: Option<u32> = None;
    let mut formats: Vec<CoreSampleFormat> = Vec::new();

    for sc in &configs {
        match sc.buffer_size() {
            cpal::SupportedBufferSize::Range { min, max } => {
                min_buf = Some(min_buf.unwrap_or(*min).min(*min));
                max_buf = Some(max_buf.unwrap_or(*max).max(*max));
            }
            cpal::SupportedBufferSize::Unknown => {}
        }
        let fmt = cpal_to_core_format(sc.sample_format());
        if !formats.contains(&fmt) {
            formats.push(fmt);
        }
    }

    Some(DeviceCapabilities {
        min_buffer_size: min_buf,
        max_buffer_size: max_buf,
        supported_formats: formats,
        exclusive_mode: false,
    })
}

// ---------------------------------------------------------------------------
// Config support validation
// ---------------------------------------------------------------------------

pub(crate) fn is_config_supported_for_output(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
) -> Result<(), OxiSoundError> {
    let mut configs_iter = device
        .supported_output_configs()
        .map_err(map_supported_configs_err)?;
    let sr = config.sample_rate;
    let ch = config.channels;
    let found = configs_iter.any(|range| {
        range.channels() == ch && sr >= range.min_sample_rate() && sr <= range.max_sample_rate()
    });
    if found {
        Ok(())
    } else {
        Err(OxiSoundError::UnsupportedConfig(format!(
            "no supported output config for {}ch @ {}Hz",
            ch, sr
        )))
    }
}

pub(crate) fn is_config_supported_for_input(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
) -> Result<(), OxiSoundError> {
    let mut configs_iter = device
        .supported_input_configs()
        .map_err(map_supported_configs_err)?;
    let sr = config.sample_rate;
    let ch = config.channels;
    let found = configs_iter.any(|range| {
        range.channels() == ch && sr >= range.min_sample_rate() && sr <= range.max_sample_rate()
    });
    if found {
        Ok(())
    } else {
        Err(OxiSoundError::UnsupportedConfig(format!(
            "no supported input config for {}ch @ {}Hz",
            ch, sr
        )))
    }
}

// ---------------------------------------------------------------------------
// Config conversion helper
// ---------------------------------------------------------------------------

pub(crate) fn to_cpal_stream_config(
    config: &OxiStreamConfig,
    supported: &cpal::SupportedStreamConfig,
) -> cpal::StreamConfig {
    let mut cpal_config = supported.config();
    cpal_config.sample_rate = config.sample_rate;
    cpal_config.channels = config.channels;
    if let Some(buf_size) = config.buffer_size {
        cpal_config.buffer_size = cpal::BufferSize::Fixed(buf_size);
    }
    cpal_config
}
