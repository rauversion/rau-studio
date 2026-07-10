use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConversionSettings {
    pub bit_depth: AiffBitDepth,
    pub sample_rate: u32,
    pub channels: u8,
    pub overwrite_existing: bool,
}

impl Default for ConversionSettings {
    fn default() -> Self {
        Self {
            bit_depth: AiffBitDepth::Pcm16,
            sample_rate: 44_100,
            channels: 2,
            overwrite_existing: false,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AiffBitDepth {
    Pcm16,
    Pcm24,
}

impl AiffBitDepth {
    pub fn ffmpeg_codec(&self) -> &'static str {
        match self {
            AiffBitDepth::Pcm16 => "pcm_s16be",
            AiffBitDepth::Pcm24 => "pcm_s24be",
        }
    }
}

pub fn ffmpeg_args(
    source_path: &Path,
    target_path: &Path,
    settings: &ConversionSettings,
) -> Vec<String> {
    let overwrite_flag = if settings.overwrite_existing {
        "-y"
    } else {
        "-n"
    };

    vec![
        "-hide_banner".to_string(),
        "-nostdin".to_string(),
        overwrite_flag.to_string(),
        "-i".to_string(),
        source_path.to_string_lossy().into_owned(),
        "-map".to_string(),
        "0:a:0".to_string(),
        "-vn".to_string(),
        "-ac".to_string(),
        settings.channels.to_string(),
        "-ar".to_string(),
        settings.sample_rate.to_string(),
        "-c:a".to_string(),
        settings.bit_depth.ffmpeg_codec().to_string(),
        "-progress".to_string(),
        "pipe:1".to_string(),
        "-nostats".to_string(),
        target_path.to_string_lossy().into_owned(),
    ]
}
