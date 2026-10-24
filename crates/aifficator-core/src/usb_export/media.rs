use super::{invalid, read_pcm_wave, AudioFormat, AudioInfo, ExportTrack, Result};
use serde::Deserialize;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub(super) struct PreparedAudio {
    pub audio: AudioInfo,
    pub wave_info: AudioInfo,
    pub wave_path: PathBuf,
    // Deletes the local decode on success or error, never destination files.
    _scratch: tempfile::TempDir,
}

fn transcode(source: &Path, target: &Path, ffmpeg: &Path, format: AudioFormat) -> Result<()> {
    let (codec, container) = match format {
        AudioFormat::Aiff => ("pcm_s16be", "aiff"),
        AudioFormat::Wave => ("pcm_s16le", "wav"),
        AudioFormat::Mp3 => return Err(invalid("MP3 export must copy the original file")),
    };
    let mut command = Command::new(ffmpeg);
    command
        .args([
            "-nostdin",
            "-hide_banner",
            "-loglevel",
            "error",
            "-xerror",
            "-n",
            "-i",
        ])
        .arg(source)
        .args([
            "-map",
            "0:a:0",
            "-vn",
            "-map_metadata",
            if format == AudioFormat::Aiff {
                "0"
            } else {
                "-1"
            },
            "-ac",
            "2",
            "-ar",
            "44100",
            "-c:a",
            codec,
            "-f",
            container,
        ]);
    if format == AudioFormat::Aiff {
        command.args(["-write_id3v2", "1", "-id3v2_version", "3"]);
    }
    let result = command.arg(target).stdin(Stdio::null()).output()?;
    if !result.status.success() {
        return Err(invalid(format!(
            "audio preparation failed for {}: {}",
            source.display(),
            String::from_utf8_lossy(&result.stderr)
        )));
    }
    Ok(())
}

pub(super) fn export(track: &ExportTrack, target: &Path, ffmpeg: &Path) -> Result<()> {
    let format =
        AudioFormat::from_path(target).ok_or_else(|| invalid("unsupported export audio format"))?;
    if track.copy_source {
        if !matches!(format, AudioFormat::Mp3 | AudioFormat::Aiff) {
            return Err(invalid(
                "only MP3 and AIFF can be copied without conversion",
            ));
        }
        let mut source = File::open(&track.source_path)?;
        if source.metadata()?.len() > u64::from(u32::MAX) {
            return Err(invalid("audio file exceeds the device's 4 GiB size limit"));
        }
        let mut output = File::create_new(target)?;
        std::io::copy(&mut source, &mut output)?;
        output.sync_all()?;
        Ok(())
    } else {
        transcode(&track.source_path, target, ffmpeg, format)
    }
}

#[derive(Deserialize)]
struct Probe {
    streams: Vec<Stream>,
    format: Container,
}
#[derive(Deserialize)]
struct Container {
    format_name: String,
}
#[derive(Deserialize)]
struct Stream {
    codec_name: String,
    sample_rate: String,
    channels: u16,
    bits_per_sample: Option<u16>,
    bit_rate: Option<String>,
}

pub(super) fn analyze(path: &Path, ffmpeg: &Path, ffprobe: &Path) -> Result<PreparedAudio> {
    let size_bytes = u32::try_from(path.metadata()?.len())
        .map_err(|_| invalid("audio file exceeds the device's 4 GiB size limit"))?;
    let expected =
        AudioFormat::from_path(path).ok_or_else(|| invalid("unsupported export audio format"))?;
    let probe = Command::new(ffprobe)
        .args([
            "-v",
            "error",
            "-select_streams",
            "a:0",
            "-show_entries",
            "stream=codec_name,sample_rate,channels,bits_per_sample,bit_rate:format=format_name",
            "-of",
            "json",
        ])
        .arg(path)
        .stdin(Stdio::null())
        .output()?;
    if !probe.status.success() {
        return Err(invalid(format!(
            "could not inspect exported audio: {}",
            String::from_utf8_lossy(&probe.stderr)
        )));
    }
    let probe: Probe = serde_json::from_slice(&probe.stdout)?;
    let stream = probe
        .streams
        .first()
        .ok_or_else(|| invalid("exported file has no audio stream"))?;
    let format = match (probe.format.format_name.as_str(), stream.codec_name.as_str()) {
        ("mp3", "mp3") => AudioFormat::Mp3,
        ("aiff", "pcm_s16be" | "pcm_s24be" | "pcm_s16le" | "pcm_s24le") => AudioFormat::Aiff,
        ("wav", "pcm_s16le" | "pcm_s24le") => AudioFormat::Wave,
        _ => return Err(invalid("expected MP3 or 16/24-bit PCM AIFF audio; the file's contents do not match a supported format")),
    };
    if format != expected {
        return Err(invalid("audio format does not match the exported filename"));
    }
    let sample_rate: u32 = stream
        .sample_rate
        .parse()
        .map_err(|_| invalid("invalid audio sample rate"))?;
    let channels = stream.channels;
    let bit_depth = if format == AudioFormat::Mp3 {
        0
    } else {
        stream.bits_per_sample.unwrap_or(0)
    };
    if !(1..=384_000).contains(&sample_rate)
        || !matches!(channels, 1 | 2)
        || (format != AudioFormat::Mp3 && !matches!(bit_depth, 16 | 24))
    {
        return Err(invalid(
            "unsupported audio parameters; expected mono/stereo MP3 or 16/24-bit PCM AIFF",
        ));
    }
    let bitrate_kbps = if format == AudioFormat::Mp3 {
        stream
            .bit_rate
            .as_deref()
            .and_then(|v| v.parse::<u32>().ok())
            .unwrap_or(0)
            / 1000
    } else {
        sample_rate * u32::from(channels) * u32::from(bit_depth) / 1000
    };
    let scratch = tempfile::tempdir()?;
    let wave_path = scratch.path().join("analysis.wav");
    transcode(path, &wave_path, ffmpeg, AudioFormat::Wave)?;
    let wave_info = read_pcm_wave(&wave_path)?;
    // Duration comes from decoded samples, not estimated VBR/container time.
    let frames = (wave_info.frames * u64::from(sample_rate) + u64::from(wave_info.sample_rate) / 2)
        / u64::from(wave_info.sample_rate);
    let audio = AudioInfo {
        format,
        bitrate_kbps,
        sample_rate,
        channels,
        bit_depth,
        frames,
        size_bytes,
    };
    Ok(PreparedAudio {
        audio,
        wave_info,
        wave_path,
        _scratch: scratch,
    })
}
