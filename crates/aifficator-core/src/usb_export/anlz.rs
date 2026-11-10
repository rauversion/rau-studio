//! Independently authored ANLZ subset, based on public format descriptions.
//! DAT carries the classic grid/preview/cues; EXT adds RGB and named cues.
//! 2EX carries independently generated three-band waveforms. PQT2 is omitted.

use super::analysis::{self, Beat, CuePoint};
use super::{audio, invalid, AudioInfo, ExportTrack, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::Path;

pub const MAX_FILE_BYTES: usize = 64 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AnalysisSummary {
    #[serde(default)]
    pub renderer: String,
    #[serde(default)]
    pub plus_cues: Vec<super::onelibrary::Cue>,
    pub dat_path: String,
    pub ext_path: String,
    pub three_band_path: String,
    pub beat_count: usize,
    pub memory_cues: usize,
    pub hot_cues: usize,
    pub preview_columns: usize,
    pub detail_columns: usize,
    pub color_preview_columns: usize,
    pub color_detail_columns: usize,
    pub three_band_preview_columns: usize,
    pub three_band_detail_columns: usize,
    pub warnings: Vec<String>,
}

pub struct AnalysisBundle {
    pub dat: Vec<u8>,
    pub ext: Vec<u8>,
    pub three_band: Vec<u8>,
    pub summary: AnalysisSummary,
}

/// Firmware path mapping described by the public USB format notes. Hash the
/// final device path as UTF-16 units, including its leading slash and case.
pub fn path_for_audio(path: &str) -> Result<String> {
    if !path.starts_with('/') || path.contains('\0') || path.len() > 4096 {
        return Err(invalid(
            "analysis requires an absolute device audio path without NUL",
        ));
    }
    let mut hash = 0u32;
    for unit in path.encode_utf16() {
        hash = hash
            .wrapping_mul(0x34f5_501d)
            .wrapping_add(u32::from(unit) * 0x93b6);
    }
    hash %= 0x30d43;
    let mut shard = 0;
    for (target, source) in [0, 2, 6, 7, 9, 13, 16].into_iter().enumerate() {
        shard |= ((hash >> source) & 1) << target;
    }
    Ok(format!(
        "/PIONEER/USBANLZ/P{shard:03X}/{hash:08X}/ANLZ0000.DAT"
    ))
}

pub fn path_for_track(track: &ExportTrack) -> Result<String> {
    if let Some(path) = &track.analysis_path {
        let Some(relative) = path.strip_prefix("/PIONEER/USBANLZ/") else {
            return Err(invalid("invalid analysis location"));
        };
        if path.len() > 240
            || !relative.ends_with("/ANLZ0000.DAT")
            || relative
                .split('/')
                .any(|p| p.is_empty() || p == "." || p == ".." || p.contains(['\\', '\0']))
        {
            return Err(invalid("invalid analysis location"));
        }
        Ok(path.clone())
    } else {
        path_for_audio(&track.device_path)
    }
}

fn put16(out: &mut [u8], at: usize, value: u16) {
    out[at..at + 2].copy_from_slice(&value.to_be_bytes());
}
fn put32(out: &mut [u8], at: usize, value: u32) {
    out[at..at + 4].copy_from_slice(&value.to_be_bytes());
}
fn word(bytes: &[u8], at: usize) -> Result<u16> {
    Ok(u16::from_be_bytes(
        bytes
            .get(at..at + 2)
            .ok_or_else(|| invalid("truncated ANLZ word"))?
            .try_into()
            .unwrap(),
    ))
}
fn number(bytes: &[u8], at: usize) -> Result<u32> {
    Ok(u32::from_be_bytes(
        bytes
            .get(at..at + 4)
            .ok_or_else(|| invalid("truncated ANLZ number"))?
            .try_into()
            .unwrap(),
    ))
}
fn utf16(text: &str) -> Vec<u8> {
    text.encode_utf16()
        .chain([0])
        .flat_map(u16::to_be_bytes)
        .collect()
}
fn text(bytes: &[u8]) -> Result<String> {
    if bytes.len() < 2 || !bytes.len().is_multiple_of(2) || !bytes.ends_with(&[0, 0]) {
        return Err(invalid("ANLZ string must be NUL-terminated UTF-16BE"));
    }
    let units: Vec<_> = bytes[..bytes.len() - 2]
        .chunks_exact(2)
        .map(|b| u16::from_be_bytes([b[0], b[1]]))
        .collect();
    if units.contains(&0) {
        return Err(invalid("embedded NUL in ANLZ text"));
    }
    String::from_utf16(&units).map_err(|_| invalid("invalid ANLZ UTF-16"))
}
fn section(magic: &[u8; 4], header: usize, length: usize) -> Vec<u8> {
    let mut bytes = vec![0; length];
    bytes[..4].copy_from_slice(magic);
    put32(&mut bytes, 4, header as u32);
    put32(&mut bytes, 8, length as u32);
    bytes
}
fn container(tags: Vec<Vec<u8>>) -> Vec<u8> {
    let length = 28 + tags.iter().map(Vec::len).sum::<usize>();
    let mut bytes = section(b"PMAI", 28, 28);
    put32(&mut bytes, 8, length as u32);
    put32(&mut bytes, 12, 1);
    put32(&mut bytes, 16, 0x0001_0000);
    put32(&mut bytes, 20, 0x0001_0000);
    for tag in tags {
        bytes.extend(tag);
    }
    bytes
}
fn path_tag(path: &str) -> Vec<u8> {
    let payload = utf16(path);
    let mut bytes = section(b"PPTH", 16, 16 + payload.len());
    put32(&mut bytes, 12, payload.len() as u32);
    bytes[16..].copy_from_slice(&payload);
    bytes
}

// PCM has no compressed-frame offsets to index. The compatibility record is
// still present: a 16-byte tag header, 400 index words and a trailing word.
// Its zero form was independently checked against our local AIFF USB samples.
fn empty_seek_table() -> Vec<u8> {
    section(b"PVBR", 16, 1620)
}

/// Prepare a separate diagnostic revision of a PCM DAT without recalculating
/// its grid, waveform or cues. The caller must publish it separately. Reject
/// compressed audio paths: this is not a VBR seek-index generator.
pub fn add_pcm_seek_table(bytes: &[u8]) -> Result<Vec<u8>> {
    let original = inspect(bytes)?;
    if !matches!(
        super::AudioFormat::from_path(Path::new(&original.audio_path)),
        Some(super::AudioFormat::Aiff | super::AudioFormat::Wave)
    ) || !original.tags.iter().any(|tag| tag == "PQTZ")
    {
        return Err(invalid("seek-table diagnostic requires a PCM DAT"));
    }
    if original.has_seek_table {
        return Err(invalid("analysis already contains a PVBR section"));
    }
    let first = number(bytes, 4)? as usize;
    if &bytes[first..first + 4] != b"PPTH" {
        return Err(invalid("PCM DAT must start with its audio path"));
    }
    let insert = first + number(bytes, first + 8)? as usize;
    let table = empty_seek_table();
    let mut revised = Vec::with_capacity(bytes.len() + table.len());
    revised.extend_from_slice(&bytes[..insert]);
    revised.extend(table);
    revised.extend_from_slice(&bytes[insert..]);
    let length = u32::try_from(revised.len()).map_err(|_| invalid("ANLZ too large"))?;
    put32(&mut revised, 8, length);
    inspect(&revised)?;
    Ok(revised)
}
fn grid_tag(beats: &[Beat]) -> Vec<u8> {
    let mut bytes = section(b"PQTZ", 24, 24 + beats.len() * 8);
    put32(&mut bytes, 16, 0x0008_0000); // Zero signed millisecond correction.
    put32(&mut bytes, 20, beats.len() as u32);
    for (i, beat) in beats.iter().enumerate() {
        let at = 24 + i * 8;
        put16(&mut bytes, at, beat.beat_in_bar);
        put16(&mut bytes, at + 2, beat.bpm_x100);
        put32(&mut bytes, at + 4, beat.time_ms);
    }
    bytes
}
fn waveform_tag(magic: &[u8; 4], values: &[u8]) -> Vec<u8> {
    let detail = magic == b"PWV3";
    let header = if detail { 24 } else { 20 };
    let mut bytes = section(magic, header, header + values.len());
    if detail {
        put32(&mut bytes, 12, 1);
        put32(&mut bytes, 16, values.len() as u32);
        put32(&mut bytes, 20, 0x0096_0000);
    } else {
        put32(&mut bytes, 12, values.len() as u32);
        put32(&mut bytes, 16, 0x0001_0000);
    }
    bytes[header..].copy_from_slice(values);
    bytes
}

fn color_waveform_tag(magic: &[u8; 4], values: &[u8]) -> Vec<u8> {
    let width = if magic == b"PWV4" { 6 } else { 2 };
    let mut bytes = section(magic, 24, 24 + values.len());
    put32(&mut bytes, 12, width);
    put32(&mut bytes, 16, (values.len() / width as usize) as u32);
    // Header constants corroborated against saved local reference EXT files.
    put32(&mut bytes, 20, if width == 2 { 0x0096_0305 } else { 0 });
    bytes[24..].copy_from_slice(values);
    bytes
}

/// Prepare a new EXT for a controlled RGB-only experiment. Existing sections
/// are preserved exactly. The decoded audio must reproduce Rau's original
/// monochrome detail, which guards against accidental input mismatches.
pub fn add_color_waveforms(bytes: &[u8], pcm: &Path, info: &AudioInfo) -> Result<Vec<u8>> {
    let (original, wave) = waveforms_for_ext(bytes, pcm, info)?;
    if original
        .tags
        .iter()
        .any(|t| matches!(t.as_str(), "PWV4" | "PWV5"))
    {
        return Err(invalid("RGB diagnostic requires a monochrome Rau EXT"));
    }
    let mut revised = bytes.to_vec();
    revised.extend(color_waveform_tag(b"PWV5", &wave.color_detail));
    revised.extend(color_waveform_tag(b"PWV4", &wave.color_preview));
    let length = u32::try_from(revised.len()).map_err(|_| invalid("ANLZ too large"))?;
    put32(&mut revised, 8, length);
    inspect(&revised)?;
    Ok(revised)
}

fn waveforms_for_ext(
    bytes: &[u8],
    pcm: &Path,
    info: &AudioInfo,
) -> Result<(Inspection, audio::Waveforms)> {
    let original = inspect(bytes)?;
    if original
        .tags
        .iter()
        .any(|t| matches!(t.as_str(), "PQTZ" | "PVBR"))
        || !original.tags.iter().any(|t| t == "PWV3")
    {
        return Err(invalid("waveform diagnostic requires a Rau EXT"));
    }
    let wave = audio::waveforms(pcm, info)?;
    let mut at = number(bytes, 4)? as usize;
    while at < bytes.len() {
        let header = number(bytes, at + 4)? as usize;
        let length = number(bytes, at + 8)? as usize;
        if &bytes[at..at + 4] == b"PWV3" && bytes[at + header..at + length] != wave.detail {
            return Err(invalid(
                "decoded audio does not reproduce the original Rau waveform",
            ));
        }
        at += length;
    }
    Ok((original, wave))
}

fn three_band_file(path: &str, wave: &audio::Waveforms) -> Vec<u8> {
    let mut tags = vec![path_tag(path)];
    for (magic, header, values) in [
        (b"PWV7", 24, &wave.three_band_detail),
        (b"PWV6", 20, &wave.three_band_preview),
    ] {
        let mut bytes = section(magic, header, header + values.len());
        put32(&mut bytes, 12, 3);
        put32(&mut bytes, 16, (values.len() / 3) as u32);
        if header == 24 {
            put32(&mut bytes, 20, 0x0096_0000);
        }
        bytes[header..].copy_from_slice(values);
        tags.push(bytes);
    }
    container(tags)
}

/// Prepare a separate 2EX companion without modifying an existing DAT or EXT.
pub fn create_three_band_analysis(bytes: &[u8], pcm: &Path, info: &AudioInfo) -> Result<Vec<u8>> {
    let (original, wave) = waveforms_for_ext(bytes, pcm, info)?;
    let output = three_band_file(&original.audio_path, &wave);
    inspect(&output)?;
    Ok(output)
}
fn cue_list(cues: &[&CuePoint], hot: bool, extended: bool) -> Vec<u8> {
    let header = if extended { 20 } else { 24 };
    let mut bytes = section(if extended { b"PCO2" } else { b"PCOB" }, header, header);
    put32(&mut bytes, 12, u32::from(hot));
    put16(
        &mut bytes,
        if extended { 16 } else { 18 },
        cues.len() as u16,
    );
    if !extended {
        put32(
            &mut bytes,
            20,
            if hot || cues.is_empty() {
                u32::MAX
            } else {
                cues.len() as u32 - 1
            },
        );
    }
    for (index, cue) in cues.iter().enumerate() {
        let comment = if cue.name.is_empty() {
            Vec::new()
        } else {
            utf16(&cue.name)
        };
        let mut entry = if extended {
            section(b"PCP2", 16, 88 + comment.len())
        } else {
            section(b"PCPT", 28, 56)
        };
        put32(&mut entry, 12, u32::from(cue.hot_cue));
        let kind_at = if extended { 16 } else { 28 };
        entry[kind_at] = if cue.end_ms.is_some() { 2 } else { 1 };
        entry[kind_at + 1..kind_at + 4].copy_from_slice(&[0, 3, 232]);
        put32(&mut entry, kind_at + 4, cue.start_ms);
        put32(&mut entry, kind_at + 8, cue.end_ms.unwrap_or(u32::MAX));
        if extended {
            entry[29] = 1;
            put32(&mut entry, 40, comment.len() as u32);
            entry[44..44 + comment.len()].copy_from_slice(&comment);
            // Zero loop fraction: XML does not establish quantized-loop or
            // active-loop state. Zero palette fields: colors remain in report.
        } else {
            put32(&mut entry, 20, 0x0001_0000);
            put16(
                &mut entry,
                24,
                if !hot && index > 0 {
                    index as u16 - 1
                } else {
                    u16::MAX
                },
            );
            put16(
                &mut entry,
                26,
                if !hot && index + 1 < cues.len() {
                    index as u16 + 1
                } else {
                    u16::MAX
                },
            );
        }
        bytes.extend(entry);
    }
    let length = bytes.len() as u32;
    put32(&mut bytes, 8, length);
    bytes
}

pub fn encode(track: &ExportTrack, path: &Path, info: &AudioInfo) -> Result<AnalysisBundle> {
    let beats = analysis::beats(&track.analysis, info)?;
    let wave = audio::waveforms(path, info)?;
    let mut memory: Vec<_> = track
        .analysis
        .cues
        .iter()
        .filter(|c| c.hot_cue == 0)
        .collect();
    memory.sort_by_key(|c| c.start_ms);
    let mut hot: Vec<_> = track
        .analysis
        .cues
        .iter()
        .filter(|c| c.hot_cue > 0)
        .collect();
    hot.sort_by_key(|c| c.hot_cue);
    let classic: Vec<_> = hot.iter().copied().filter(|c| c.hot_cue <= 3).collect();
    let extra: Vec<_> = hot.iter().copied().filter(|c| c.hot_cue > 3).collect();
    let dat = container(vec![
        path_tag(&track.device_path),
        empty_seek_table(),
        grid_tag(&beats),
        waveform_tag(b"PWAV", &wave.preview),
        waveform_tag(b"PWV2", &wave.tiny),
        cue_list(&classic, true, false),
        cue_list(&memory, false, false),
    ]);
    let ext = container(vec![
        path_tag(&track.device_path),
        waveform_tag(b"PWV3", &wave.detail),
        cue_list(&extra, true, false),
        cue_list(&[], false, false),
        cue_list(&hot, true, true),
        cue_list(&memory, false, true),
        color_waveform_tag(b"PWV5", &wave.color_detail),
        color_waveform_tag(b"PWV4", &wave.color_preview),
    ]);
    let decoded_dat = inspect(&dat)?;
    let decoded_ext = inspect(&ext)?;
    let three_band = three_band_file(&track.device_path, &wave);
    let decoded_three_band = inspect(&three_band)?;
    if decoded_dat.audio_path != track.device_path
        || decoded_ext.audio_path != track.device_path
        || decoded_three_band.audio_path != track.device_path
        || !decoded_dat.has_seek_table
        || decoded_dat.beats.as_ref() != Some(&beats)
        || !decoded_ext
            .waveforms
            .iter()
            .any(|w| w.tag == "PWV4" && w.columns == 1200)
        || !decoded_ext
            .waveforms
            .iter()
            .any(|w| w.tag == "PWV5" && w.columns == wave.detail.len())
        || !decoded_three_band
            .waveforms
            .iter()
            .any(|w| w.tag == "PWV6" && w.columns == 1200)
        || !decoded_three_band
            .waveforms
            .iter()
            .any(|w| w.tag == "PWV7" && w.columns == wave.detail.len())
    {
        return Err(invalid("ANLZ read-back path or grid differs from input"));
    }
    for (decoded, extended, expected_hot, expected_memory) in [
        (&decoded_dat, false, &classic, &memory),
        (&decoded_ext, false, &extra, &Vec::new()),
        (&decoded_ext, true, &hot, &memory),
    ] {
        for (is_hot, expected) in [(true, expected_hot), (false, expected_memory)] {
            let found = decoded
                .cue_lists
                .iter()
                .find(|c| c.extended == extended && c.hot == is_hot)
                .ok_or_else(|| invalid("missing ANLZ cue list"))?;
            let expected: Vec<_> = expected
                .iter()
                .map(|cue| {
                    let mut cue = (**cue).clone();
                    cue.rgb = None;
                    if !extended {
                        cue.name.clear();
                    }
                    cue
                })
                .collect();
            if found.cues != expected {
                return Err(invalid("ANLZ read-back cues differ from input"));
            }
        }
    }
    let dat_path = path_for_track(track)?;
    let ext_path = dat_path.replace(".DAT", ".EXT");
    let three_band_path = dat_path.replace(".DAT", ".2EX");
    Ok(AnalysisBundle {
        dat,
        ext,
        three_band,
        summary: AnalysisSummary {
            renderer: "rau-legacy-v1".into(),
            plus_cues: Vec::new(),
            dat_path,
            ext_path,
            three_band_path,
            beat_count: beats.len(),
            memory_cues: memory.len(),
            hot_cues: hot.len(),
            preview_columns: wave.preview.len(),
            detail_columns: wave.detail.len(),
            color_preview_columns: wave.color_preview.len() / 6,
            color_detail_columns: wave.color_detail.len() / 2,
            three_band_preview_columns: wave.three_band_preview.len() / 3,
            three_band_detail_columns: wave.three_band_detail.len() / 3,
            warnings: warnings(track),
        },
    })
}

pub(super) fn warnings(track: &ExportTrack) -> Vec<String> {
    let mut warnings = track.analysis.warnings.clone();
    if super::AudioFormat::from_path(Path::new(&track.device_path)) == Some(super::AudioFormat::Mp3)
    {
        warnings.push("PVBR framing is present but MP3 frame seek offsets are not generated; VBR seeking requires validation".into());
    }
    if track.analysis.tempos.is_empty() && warnings.is_empty() {
        warnings.push("No imported beat grid".into());
    }
    if track.analysis.cues.iter().any(|cue| cue.hot_cue > 3) {
        warnings
            .push("Hot cues after C are stored in EXT; availability depends on the player".into());
    }
    warnings
}

#[derive(Debug, Serialize)]
pub struct WaveformInspection {
    pub tag: String,
    pub columns: usize,
    pub min_height: u8,
    pub max_height: u8,
}
#[derive(Debug, Serialize)]
pub struct CueListInspection {
    pub extended: bool,
    pub hot: bool,
    pub cues: Vec<CuePoint>,
}
#[derive(Debug, Serialize)]
pub struct Inspection {
    pub audio_path: String,
    pub tags: Vec<String>,
    pub has_seek_table: bool,
    pub beats: Option<Vec<Beat>>,
    pub waveforms: Vec<WaveformInspection>,
    pub cue_lists: Vec<CueListInspection>,
}

/// Bounded subset reader. Unknown tags are skipped by their declared lengths;
/// it does not claim to interpret all analysis produced by Rekordbox versions.
pub fn inspect(bytes: &[u8]) -> Result<Inspection> {
    if bytes.len() < 12
        || bytes.len() > MAX_FILE_BYTES
        || &bytes[..4] != b"PMAI"
        || number(bytes, 8)? as usize != bytes.len()
    {
        return Err(invalid("invalid ANLZ file framing or size"));
    }
    let mut at = number(bytes, 4)? as usize;
    if at < 12 || at > bytes.len() {
        return Err(invalid("invalid ANLZ file header length"));
    }
    let mut out = Inspection {
        audio_path: String::new(),
        tags: Vec::new(),
        has_seek_table: false,
        beats: None,
        waveforms: Vec::new(),
        cue_lists: Vec::new(),
    };
    let mut seen = BTreeSet::new();
    while at < bytes.len() {
        if out.tags.len() >= 1024 {
            return Err(invalid("too many ANLZ sections"));
        }
        let header = number(bytes, at + 4)? as usize;
        let length = number(bytes, at + 8)? as usize;
        if header < 12 || length < header || length > bytes.len() - at {
            return Err(invalid("invalid ANLZ section bounds"));
        }
        let tag = &bytes[at..at + length];
        let name = std::str::from_utf8(&tag[..4]).map_err(|_| invalid("invalid ANLZ fourcc"))?;
        out.tags.push(name.to_owned());
        if matches!(
            name,
            "PPTH" | "PVBR" | "PQTZ" | "PWAV" | "PWV2" | "PWV3" | "PWV4" | "PWV5" | "PWV6" | "PWV7"
        ) && !seen.insert(name.to_owned())
        {
            return Err(invalid("duplicate analysis section"));
        }
        match name {
            "PVBR" => {
                if header != 16 || length != 1620 {
                    return Err(invalid("invalid PVBR seek-table length"));
                }
                // A reference MP3 may have nonzero offsets and trailing data.
                // Framing validation must not clear or reinterpret those words.
                out.has_seek_table = true;
            }
            "PPTH" => {
                if header != 16 || number(tag, 12)? as usize != length - header {
                    return Err(invalid("invalid analysis path length"));
                }
                out.audio_path = text(&tag[header..])?;
                path_for_audio(&out.audio_path)?;
            }
            "PQTZ" => {
                if header != 24
                    || !(length - header).is_multiple_of(8)
                    || number(tag, 20)? as usize != (length - header) / 8
                {
                    return Err(invalid("invalid beat grid length"));
                }
                let correction = i64::from(word(tag, 18)? as i16);
                let mut beats: Vec<Beat> = Vec::new();
                for entry in tag[header..].chunks_exact(8) {
                    let time = i64::from(number(entry, 4)?) + correction;
                    let beat = Beat {
                        beat_in_bar: word(entry, 0)?,
                        bpm_x100: word(entry, 2)?,
                        time_ms: u32::try_from(time)
                            .map_err(|_| invalid("grid correction overflows timestamp"))?,
                    };
                    if !(1..=4).contains(&beat.beat_in_bar)
                        || beat.bpm_x100 == 0
                        || beats.last().is_some_and(|b| b.time_ms >= beat.time_ms)
                    {
                        return Err(invalid("invalid beat sequence"));
                    }
                    beats.push(beat);
                }
                out.beats = Some(beats);
            }
            "PWAV" | "PWV2" | "PWV3" => {
                let detail = name == "PWV3";
                if header != if detail { 24 } else { 20 }
                    || number(tag, if detail { 16 } else { 12 })? as usize != length - header
                    || (detail && number(tag, 12)? != 1)
                    || (name == "PWAV" && length - header != 400)
                    || (name == "PWV2" && length - header != 100)
                {
                    return Err(invalid("invalid waveform length"));
                }
                if name == "PWV2" && tag[header..].iter().any(|b| *b > 15) {
                    return Err(invalid("tiny waveform contains unsupported upper bits"));
                }
                let heights: Vec<_> = tag[header..].iter().map(|v| *v & 31).collect();
                out.waveforms.push(WaveformInspection {
                    tag: name.into(),
                    columns: heights.len(),
                    min_height: heights.iter().copied().min().unwrap_or(0),
                    max_height: heights.iter().copied().max().unwrap_or(0),
                });
            }
            "PWV4" | "PWV5" => {
                let width = if name == "PWV4" { 6 } else { 2 };
                if header != 24
                    || !(length - header).is_multiple_of(width)
                    || number(tag, 12)? as usize != width
                    || number(tag, 16)? as usize != (length - header) / width
                    || (name == "PWV4" && length - header != 7200)
                {
                    return Err(invalid("invalid color waveform length"));
                }
                let heights: Vec<_> = tag[header..]
                    .chunks_exact(width)
                    .map(|entry| {
                        if width == 6 {
                            entry[2] & 127
                        } else {
                            ((u16::from_be_bytes([entry[0], entry[1]]) >> 2) & 31) as u8
                        }
                    })
                    .collect();
                out.waveforms.push(WaveformInspection {
                    tag: name.into(),
                    columns: heights.len(),
                    min_height: heights.iter().copied().min().unwrap_or(0),
                    max_height: heights.iter().copied().max().unwrap_or(0),
                });
            }
            "PWV6" | "PWV7" => {
                if header != if name == "PWV6" { 20 } else { 24 }
                    || !(length - header).is_multiple_of(3)
                    || number(tag, 12)? != 3
                    || number(tag, 16)? as usize != (length - header) / 3
                    || (name == "PWV6" && length - header != 3600)
                {
                    return Err(invalid("invalid three-band waveform length"));
                }
                let heights: Vec<_> = tag[header..]
                    .chunks_exact(3)
                    .map(|entry| *entry.iter().max().unwrap())
                    .collect();
                out.waveforms.push(WaveformInspection {
                    tag: name.into(),
                    columns: heights.len(),
                    min_height: heights.iter().copied().min().unwrap_or(0),
                    max_height: heights.iter().copied().max().unwrap_or(0),
                });
            }
            "PCOB" | "PCO2" => {
                let extended = name == "PCO2";
                if header != if extended { 20 } else { 24 } || number(tag, 12)? > 1 {
                    return Err(invalid("invalid cue list header"));
                }
                let hot = number(tag, 12)? == 1;
                if out
                    .cue_lists
                    .iter()
                    .any(|list| list.extended == extended && list.hot == hot)
                {
                    return Err(invalid("duplicate cue list"));
                }
                let count = word(tag, if extended { 16 } else { 18 })? as usize;
                let mut cursor = header;
                let mut cues = Vec::new();
                for _ in 0..count {
                    let size = number(tag, cursor + 8)? as usize;
                    if size < if extended { 40 } else { 56 } || size > tag.len() - cursor {
                        return Err(invalid("invalid cue record bounds"));
                    }
                    let entry = &tag[cursor..cursor + size];
                    if &entry[..4] != if extended { b"PCP2" } else { b"PCPT" }
                        || number(entry, 4)? != if extended { 16 } else { 28 }
                    {
                        return Err(invalid("invalid cue record header"));
                    }
                    let kind_at = if extended { 16 } else { 28 };
                    let slot = number(entry, 12)?;
                    if slot > 16 || (slot != 0) != hot || !matches!(entry[kind_at], 1 | 2) {
                        return Err(invalid("invalid cue kind or slot"));
                    }
                    let start_ms = number(entry, kind_at + 4)?;
                    let end_ms = if entry[kind_at] == 2 {
                        Some(number(entry, kind_at + 8)?)
                    } else {
                        None
                    };
                    if end_ms.is_some_and(|end| end <= start_ms || end == u32::MAX) {
                        return Err(invalid("invalid loop end"));
                    }
                    let mut name = String::new();
                    let mut rgb = None;
                    if extended && size >= 44 {
                        let text_size = number(entry, 40)? as usize;
                        if text_size > size - 44 {
                            return Err(invalid("cue comment crosses record boundary"));
                        }
                        if text_size > 0 {
                            name = text(&entry[44..44 + text_size])?;
                        }
                        if let Some(color) = entry.get(44 + text_size..48 + text_size) {
                            if color.iter().any(|c| *c != 0) {
                                rgb = Some([color[1], color[2], color[3]]);
                            }
                        }
                    }
                    cues.push(CuePoint {
                        hot_cue: slot as u8,
                        start_ms,
                        end_ms,
                        name,
                        rgb,
                    });
                    cursor += size;
                }
                if cursor != tag.len() {
                    return Err(invalid("cue count does not consume list payload"));
                }
                out.cue_lists.push(CueListInspection {
                    extended,
                    hot,
                    cues,
                });
            }
            _ => {}
        }
        at += length;
    }
    if out.audio_path.is_empty() {
        return Err(invalid("ANLZ file has no audio path"));
    }
    Ok(out)
}
