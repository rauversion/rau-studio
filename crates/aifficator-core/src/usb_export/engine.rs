//! Adapt Rau's selection, metadata and conversion policy to rekordbox-usb.
//! SQLCipher storage and the recoverable USB pipeline remain owned by Rau.
use super::ExportTrack;
#[cfg(unix)]
use super::{analysis, onelibrary};
use super::{
    anlz, browse, catalog::Catalog, devicesql, invalid, AudioFormat, AudioInfo, ExportPlan,
    LibraryIdentity, Result,
};
#[cfg(unix)]
use rekordbox_usb::{
    anlz::{AnalysisFile, Beat},
    audio_analysis::AnalysisSet,
    seek::VbrTable,
};
use rekordbox_usb::{
    audio_analysis::AudioKind,
    pdb::{self, Kind, Pdb, TrackRecord},
    profile::{self, NewTrack, TrackProfile},
};
use std::{collections::BTreeMap, fs, path::Path};

#[cfg(unix)]
pub(super) const RENDERER: &str = "rekordbox-usb-0.1.0-v1";
#[cfg(not(unix))]
pub(super) const RENDERER: &str = "rau-legacy-v1";

pub(super) fn warnings(track: &ExportTrack) -> Vec<String> {
    #[cfg(unix)]
    {
        let mut warnings: Vec<_> = track
            .analysis
            .warnings
            .iter()
            .filter(|w| !w.starts_with("Cue RGB"))
            .cloned()
            .collect();
        if track.analysis.tempos.is_empty() && warnings.is_empty() {
            warnings.push("No imported beat grid".into());
        }
        warnings
    }
    #[cfg(not(unix))]
    anlz::warnings(track)
}

#[cfg(unix)]
pub(super) fn validate_cues(dat: &[u8], ext: &[u8], cues: &[onelibrary::Cue]) -> Result<()> {
    use rekordbox_usb::{
        cues::{CueSet, NativePlusCue},
        plus::Value,
    };
    let mut set = AnalysisSet {
        dat: AnalysisFile::parse(dat)?,
        ext: AnalysisFile::parse(ext)?,
        two_ex: AnalysisFile::default(),
    };
    let groups = |file: &AnalysisFile, id| -> Result<_> {
        let group = |kind| -> Result<_> {
            file.find(id)
                .find(|g| g.cue_group_kind().ok() == Some(kind))
                .cloned()
                .ok_or_else(|| invalid("missing USB cue group"))
        };
        Ok([group(1)?, group(0)?])
    };
    let cues = CueSet {
        classic: groups(&set.dat, *b"PCOB")?,
        classic_ext: groups(&set.ext, *b"PCOB")?,
        extended: groups(&set.ext, *b"PCO2")?,
        plus: cues
            .iter()
            .enumerate()
            .map(|(i, cue)| NativePlusCue {
                id: i as u32 + 1,
                values: cue.values.clone().map(|v| match v {
                    onelibrary::CueValue::Integer(n) => Value::Integer(n),
                    onelibrary::CueValue::Text(s) => Value::Text(s),
                }),
            })
            .collect(),
    };
    cues.apply(&mut set).map_err(|e| invalid(format!("Los cues de ANLZ y OneLibrary no coinciden. Sincroniza ambas bibliotecas antes de actualizar: {e}")))
}

/// PCM is decoded by the new crate at its original rate/depth. MP3 keeps the
/// bundled FFmpeg decoder, with trimming disabled, and uses the crate's MPEG
/// scanner for PVBR/cue seek. No Rekordbox dylib is loaded or redistributed.
#[cfg(unix)]
pub(super) fn analyze(
    track: &ExportTrack,
    path: &Path,
    ffmpeg: &Path,
) -> Result<(AudioInfo, anlz::AnalysisBundle)> {
    use rekordbox_usb::{
        cues::{CueSet, NewCue},
        mpeg::MpegFrames,
        plus::Value,
    };
    use std::process::{Command, Stdio};
    let bytes = fs::read(path)?;
    let size_bytes = u32::try_from(bytes.len()).map_err(|_| invalid("audio exceeds 4 GiB"))?;
    let format =
        AudioFormat::from_path(path).ok_or_else(|| invalid("unsupported export audio format"))?;
    let (pcm, rate, channels, depth, bitrate, seek, mpeg) = if format == AudioFormat::Mp3 {
        let frames = MpegFrames::parse(&bytes)?;
        let decoded = Command::new(ffmpeg)
            .args([
                "-nostdin",
                "-hide_banner",
                "-loglevel",
                "error",
                "-xerror",
                "-threads",
                "1",
                "-flags2",
                "+skip_manual",
                "-i",
            ])
            .arg(path)
            .args(["-map", "0:a:0", "-vn", "-ac", "2", "-ar"])
            .arg(frames.sample_rate.to_string())
            .args(["-c:a", "pcm_s16le", "-f", "s16le", "pipe:1"])
            .stdin(Stdio::null())
            .output()?;
        if !decoded.status.success() || decoded.stdout.is_empty() || decoded.stdout.len() % 4 != 0 {
            return Err(invalid(format!(
                "MP3 decode failed: {}",
                String::from_utf8_lossy(&decoded.stderr)
            )));
        }
        let pcm = decoded
            .stdout
            .chunks_exact(4)
            .map(|s| {
                [
                    i16::from_le_bytes([s[0], s[1]]),
                    i16::from_le_bytes([s[2], s[3]]),
                ]
            })
            .collect::<Vec<_>>();
        (
            pcm,
            frames.sample_rate,
            frames.channels,
            0,
            frames.bitrate_kbps,
            frames.vbr_table()?,
            Some(frames),
        )
    } else {
        let decoded = rekordbox_usb::media::decode_pcm(&bytes)?;
        let actual = match decoded.format {
            rekordbox_usb::media::PcmFormat::Wav => AudioFormat::Wave,
            rekordbox_usb::media::PcmFormat::Aiff => AudioFormat::Aiff,
        };
        if format != actual {
            return Err(invalid("audio format does not match exported filename"));
        }
        let bitrate = decoded.sample_rate
            * u32::from(decoded.source_channels)
            * u32::from(decoded.source_bit_depth)
            / 1000;
        (
            decoded.pcm16(),
            decoded.sample_rate,
            decoded.source_channels,
            decoded.source_bit_depth,
            bitrate,
            VbrTable::pcm(),
            None,
        )
    };
    let audio = AudioInfo {
        format,
        bitrate_kbps: bitrate,
        sample_rate: rate,
        channels,
        bit_depth: depth,
        frames: pcm.len() as u64,
        size_bytes,
    };
    let beats = analysis::beats(&track.analysis, &audio)?
        .into_iter()
        .map(|b| Beat {
            beat: b.beat_in_bar,
            tempo: b.bpm_x100,
            time_ms: b.time_ms,
        })
        .collect::<Vec<_>>();
    let mut set = AnalysisSet::from_pcm(&pcm, rate, &track.device_path, &beats, &seek)?;
    let duration_ms = audio.frames * 1000 / u64::from(rate);
    let cues = track
        .analysis
        .cues
        .iter()
        .enumerate()
        .map(|(i, c)| {
            if u64::from(c.start_ms) > duration_ms
                || c.end_ms.is_some_and(|end| u64::from(end) > duration_ms)
            {
                return Err(invalid("cue lies beyond decoded audio"));
            }
            let mut cue = NewCue::point(i as u32 + 1, c.hot_cue.into(), c.start_ms);
            cue.end_ms = c.end_ms;
            cue.comment = c.name.clone();
            if let Some([r, g, b]) = c.rgb {
                cue.color.palette_and_rgb = [0, r, g, b];
            }
            Ok(cue)
        })
        .collect::<Result<Vec<_>>>()?;
    let cues = match &mpeg {
        Some(frames) => CueSet::for_mp3(&cues, frames)?,
        None => CueSet::for_pcm(&cues)?,
    };
    cues.apply(&mut set)?;
    let plus_cues = cues
        .plus
        .iter()
        .map(|cue| {
            let values = cue
                .values
                .iter()
                .map(|v| match v {
                    Value::Integer(n) => Ok(onelibrary::CueValue::Integer(*n)),
                    Value::Text(s) => Ok(onelibrary::CueValue::Text(s.clone())),
                    _ => Err(invalid("unexpected native cue value")),
                })
                .collect::<Result<Vec<_>>>()?;
            Ok(onelibrary::Cue {
                values: values
                    .try_into()
                    .map_err(|_| invalid("invalid cue column count"))?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let columns = |file: &AnalysisFile, id| -> Result<usize> {
        let wave = file
            .find(id)
            .next()
            .ok_or_else(|| invalid("missing generated waveform"))?
            .waveform()?;
        Ok(wave.entries.len() / usize::from(wave.entry_size))
    };
    let dat_path = anlz::path_for_track(track)?;
    let summary = anlz::AnalysisSummary {
        renderer: RENDERER.into(),
        plus_cues,
        ext_path: dat_path.replace(".DAT", ".EXT"),
        three_band_path: dat_path.replace(".DAT", ".2EX"),
        dat_path,
        beat_count: beats.len(),
        memory_cues: track
            .analysis
            .cues
            .iter()
            .filter(|c| c.hot_cue == 0)
            .count(),
        hot_cues: track.analysis.cues.iter().filter(|c| c.hot_cue > 0).count(),
        preview_columns: columns(&set.dat, *b"PWAV")?,
        detail_columns: columns(&set.ext, *b"PWV3")?,
        color_preview_columns: columns(&set.ext, *b"PWV4")?,
        color_detail_columns: columns(&set.ext, *b"PWV5")?,
        three_band_preview_columns: columns(&set.two_ex, *b"PWV6")?,
        three_band_detail_columns: columns(&set.two_ex, *b"PWV7")?,
        warnings: warnings(track),
    };
    Ok((
        audio,
        anlz::AnalysisBundle {
            dat: set.dat.to_bytes()?,
            ext: set.ext.to_bytes()?,
            three_band: set.two_ex.to_bytes()?,
            summary,
        },
    ))
}

/// Rau deliberately supplies its existing browsing and playback defaults; they
/// are application policy, not a claim of universal native Rekordbox defaults.
fn track_profile() -> Result<TrackProfile> {
    let mut fixed = [0; 94];
    fixed[..2].copy_from_slice(&0x24u16.to_le_bytes());
    fixed[72..76].copy_from_slice(&1u32.to_le_bytes());
    fixed[86] = 0x29;
    fixed[92] = 3;
    let record = TrackRecord::from_fields(fixed, &std::array::from_fn(|_| String::new()))?;
    let mut seed = Pdb::empty(Kind::Export, &[0])?;
    seed.edit(|e| e.append(0, &record.to_bytes()))?;
    Ok(TrackProfile::capture(seed.rows(0)?[0])?)
}

pub(super) fn database(
    plan: &ExportPlan,
    audio: &[AudioInfo],
    date: &str,
    identity: &LibraryIdentity,
) -> Result<Vec<u8>> {
    devicesql::validate_plan(plan)?;
    if audio.len() != plan.tracks.len()
        || chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").is_err()
    {
        return Err(invalid("invalid audio count or export date"));
    }
    let catalog = Catalog::new(plan);
    let profile = track_profile()?;
    let mut db = Pdb::empty(Kind::Export, &(0..20).collect::<Vec<_>>())?;
    let id = |map: &BTreeMap<String, u32>, name: &String| map.get(name).copied().unwrap_or(0);
    let mut rows = Vec::new();
    for (t, a) in plan.tracks.iter().zip(audio) {
        if AudioFormat::from_path(Path::new(&t.device_path)) != Some(a.format)
            || !(1..=384_000).contains(&a.sample_rate)
            || a.frames == 0
            || !matches!(a.channels, 1 | 2)
            || (a.format == AudioFormat::Mp3 && a.bit_depth != 0)
            || (a.format != AudioFormat::Mp3 && !matches!(a.bit_depth, 16 | 24))
            || a.size_bytes < 44
            || (a.format != AudioFormat::Mp3
                && (a.frames
                    > u64::from(a.size_bytes)
                        / (u64::from(a.channels) * u64::from(a.bit_depth) / 8)
                    || a.bitrate_kbps
                        != a.sample_rate * u32::from(a.channels) * u32::from(a.bit_depth) / 1000))
        {
            return Err(invalid("invalid exported audio metadata"));
        }
        let mut track = NewTrack::new(t.id, &t.title, &t.device_path, &anlz::path_for_track(t)?);
        track.source_content_link = devicesql::GENERATED_ANALYSIS_CONTENT_LINK;
        let f = &mut track.fields;
        f.sample_rate = a.sample_rate;
        f.file_size = a.size_bytes;
        f.native_word_20 = if identity.master_database_id == 0 {
            0
        } else {
            t.id
        };
        f.native_word_24 = identity.master_database_id;
        f.key = id(&catalog.keys, &t.key);
        f.label = id(&catalog.labels, &t.label);
        f.bitrate = a.bitrate_kbps;
        f.track_number = t.track_number;
        f.tempo = t.bpm_x100;
        f.genre = id(&catalog.genres, &t.genre);
        f.artist = id(&catalog.artists, &t.artist);
        f.album = catalog
            .albums
            .get(&(t.artist.clone(), t.album.clone()))
            .copied()
            .unwrap_or(0);
        f.year = t.year;
        f.bit_depth = a.bit_depth;
        f.rating = t.rating;
        f.duration_seconds = u16::try_from(a.frames.div_ceil(u64::from(a.sample_rate)))
            .map_err(|_| invalid("track duration exceeds device limit"))?;
        track.strings[7] = if t.analysis.cues.iter().any(|c| c.hot_cue > 0) {
            "ON"
        } else {
            ""
        }
        .into();
        track.strings[10] = date.into();
        track.strings[15] = date.into();
        track.strings[16] = t.comment.clone();
        let kind = match a.format {
            AudioFormat::Mp3 => AudioKind::Mp3,
            AudioFormat::Aiff => AudioKind::Aiff,
            AudioFormat::Wave => AudioKind::Wav,
        };
        rows.push((0, profile.record_for_audio(&track, kind)?.to_bytes()));
    }
    for (table, names) in [(1, &catalog.genres), (4, &catalog.labels)] {
        for (name, id) in names {
            rows.push((table, pdb::named_row(*id, name)?));
        }
    }
    for (name, id) in &catalog.keys {
        rows.push((5, pdb::key_row(*id, *id, name)?));
    }
    for (name, id) in &catalog.artists {
        rows.push((2, pdb::artist_row(*id, name, "")?));
    }
    for ((artist, name), album) in &catalog.albums {
        rows.push((
            3,
            pdb::album_row(*album, id(&catalog.artists, artist), 0, 0, name, "")?,
        ));
    }
    for (i, name) in [
        "Pink", "Red", "Orange", "Yellow", "Green", "Aqua", "Blue", "Purple",
    ]
    .iter()
    .enumerate()
    {
        let mut row = vec![0; 8];
        row[4] = (i + 1) as u8;
        row[5..7].copy_from_slice(&((i + 1) as u16).to_le_bytes());
        row.extend(pdb::encode_string(name)?);
        rows.push((6, row));
    }
    for p in &plan.playlists {
        rows.push((
            7,
            pdb::playlist_row(p.id, p.parent_id, p.position, p.is_folder, &p.name)?,
        ));
        for (i, t) in p.tracks.iter().enumerate() {
            rows.push((8, pdb::playlist_entry_row(p.id, *t, i as u32 + 1)));
        }
    }
    for c in browse::columns() {
        let mut row = c.id.to_le_bytes().to_vec();
        row.extend(c.selector.to_le_bytes());
        row.extend(pdb::encode_string(&c.name)?);
        rows.push((16, row));
    }
    for c in browse::categories() {
        let mut row = c.column_id.to_le_bytes().to_vec();
        row.extend(c.id.to_le_bytes());
        row.extend([c.kind, c.flags]);
        row.extend(c.position.to_le_bytes());
        rows.push((17, row));
    }
    for s in browse::sorts() {
        let mut row = s.column_id.to_le_bytes().to_vec();
        row.extend(s.id.to_le_bytes());
        row.extend([s.flags, s.position, 0, 0]);
        rows.push((18, row));
    }
    let mut property = vec![0; 12];
    property[..2].copy_from_slice(&0x280u16.to_le_bytes());
    property.extend(pdb::encode_string(date)?);
    property.extend([0x19, 0x1e]);
    property.extend(pdb::encode_string("1000")?);
    property.extend(pdb::encode_string(&identity.name)?);
    property.extend([0; 8]);
    rows.push((19, property));
    db.edit(|e| {
        for (table, row) in &rows {
            e.append(*table, row)?;
        }
        Ok(())
    })?;
    profile::update_track_count(&mut db)?;
    db.validate_catalog()?;
    db.validate_indexes()?;
    profile::validate_browse(&db)?;
    Ok(db.to_bytes())
}
