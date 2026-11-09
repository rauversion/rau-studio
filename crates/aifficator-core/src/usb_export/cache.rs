//! Persistent ANLZ cache, keyed by actual exported audio bytes, preparation,
//! audio path and renderer version. No decoded PCM or source audio is stored.
use super::*;
const VERSION: &str = super::engine::RENDERER;
#[derive(Serialize, Deserialize)]
struct Entry {
    version: String,
    audio_hash: String,
    audio: AudioInfo,
    summary: anlz::AnalysisSummary,
    hashes: Vec<String>,
}
const FILES: [&str; 3] = ["ANLZ0000.DAT", "ANLZ0000.EXT", "ANLZ0000.2EX"];

pub(super) fn prepare(
    track: &ExportTrack,
    audio_path: &Path,
    ffmpeg: &Path,
    ffprobe: &Path,
    cache: Option<&Path>,
    notify: &dyn Fn(&str, &str),
) -> Result<(AudioInfo, anlz::AnalysisBundle)> {
    let hash = hash_file(audio_path)?;
    let mut digest = Sha256::new();
    digest.update(VERSION);
    digest.update(&hash);
    digest.update(serde_json::to_vec(&(
        &track.device_path,
        &track.analysis.tempos,
        &track.analysis.cues,
    ))?);
    let key = format!("{:x}", digest.finalize());
    let location = cache.map(|root| root.join(&key[..2]).join(&key));
    if let Some(location) = &location {
        match read(location, &hash, track, audio_path) {
            Ok(Some(value)) => {
                notify(
                    "cache_hit",
                    "Análisis reutilizado desde la caché local; no se vuelve a decodificar",
                );
                return Ok(value);
            }
            Ok(None) => (),
            Err(_) => notify(
                "cache_error",
                "La caché no es válida; se generará de nuevo el análisis de esta pista",
            ),
        }
    }
    notify(
        "analysis",
        "Decodificando audio y generando waveforms, grilla y cues",
    );
    #[cfg(unix)]
    let (audio, bundle) = {
        let _ = ffprobe;
        super::engine::analyze(track, audio_path, ffmpeg)?
    };
    #[cfg(not(unix))]
    let (audio, bundle) = {
        let prepared = media::analyze(audio_path, ffmpeg, ffprobe)?;
        let bundle = anlz::encode(track, &prepared.wave_path, &prepared.wave_info)?;
        (prepared.audio, bundle)
    };
    if let Some(location) = &location {
        if store(location, &hash, &audio, &bundle).is_err() {
            notify(
                "cache_error",
                "No se pudo guardar la caché local; la exportación continúa",
            );
        } else {
            notify(
                "cache_store",
                "Análisis guardado para reutilizarlo en próximas exportaciones",
            );
        }
    }
    Ok((audio, bundle))
}

fn read(
    location: &Path,
    hash: &str,
    track: &ExportTrack,
    audio_path: &Path,
) -> Result<Option<(AudioInfo, anlz::AnalysisBundle)>> {
    if !location.exists() {
        return Ok(None);
    }
    let manifest = location.join("entry.json");
    if fs::metadata(&manifest)?.len() > 2 * 1024 * 1024 {
        return Err(invalid("cache manifest too large"));
    }
    let mut entry: Entry = serde_json::from_slice(&fs::read(manifest)?)?;
    if entry.version != VERSION
        || entry.summary.renderer != VERSION
        || entry.audio_hash != hash
        || entry.hashes.len() != 3
        || u64::from(entry.audio.size_bytes) != fs::metadata(audio_path)?.len()
        || AudioFormat::from_path(audio_path) != Some(entry.audio.format)
    {
        return Err(invalid("cache identity mismatch"));
    }
    let mut payloads = Vec::new();
    for (index, name) in FILES.iter().enumerate() {
        let path = location.join(name);
        if fs::metadata(&path)?.len() > anlz::MAX_FILE_BYTES as u64
            || hash_file(&path)? != entry.hashes[index]
        {
            return Err(invalid("cache checksum mismatch"));
        }
        let bytes = fs::read(path)?;
        if anlz::inspect(&bytes)?.audio_path != track.device_path {
            return Err(invalid("cache audio path mismatch"));
        }
        payloads.push(bytes);
    }
    #[cfg(unix)]
    super::engine::validate_cues(&payloads[0], &payloads[1], &entry.summary.plus_cues)?;
    entry.summary.dat_path = anlz::path_for_track(track)?;
    entry.summary.ext_path = entry.summary.dat_path.replace(".DAT", ".EXT");
    entry.summary.three_band_path = entry.summary.dat_path.replace(".DAT", ".2EX");
    entry.summary.warnings = super::engine::warnings(track);
    Ok(Some((
        entry.audio,
        anlz::AnalysisBundle {
            dat: payloads.remove(0),
            ext: payloads.remove(0),
            three_band: payloads.remove(0),
            summary: entry.summary,
        },
    )))
}

fn store(
    location: &Path,
    hash: &str,
    audio: &AudioInfo,
    bundle: &anlz::AnalysisBundle,
) -> Result<()> {
    let parent = location
        .parent()
        .ok_or_else(|| invalid("invalid cache directory"))?;
    fs::create_dir_all(parent)?;
    let temp = tempfile::Builder::new()
        .prefix(".preparing-")
        .tempdir_in(parent)?;
    let mut hashes = Vec::new();
    for (name, bytes) in FILES
        .iter()
        .zip([&bundle.dat, &bundle.ext, &bundle.three_band])
    {
        let p = temp.path().join(name);
        write_new(&p, bytes)?;
        hashes.push(hash_file(&p)?);
    }
    let entry = Entry {
        version: VERSION.into(),
        audio_hash: hash.into(),
        audio: audio.clone(),
        summary: bundle.summary.clone(),
        hashes,
    };
    write_new(
        &temp.path().join("entry.json"),
        &serde_json::to_vec(&entry)?,
    )?;
    if location.exists() {
        // Another job may already have stored the same deterministic result.
        if let Ok(bytes) = fs::read(location.join("entry.json")) {
            if let Ok(saved) = serde_json::from_slice::<Entry>(&bytes) {
                if saved.version == entry.version
                    && saved.audio_hash == entry.audio_hash
                    && saved.hashes == entry.hashes
                    && FILES.iter().zip(&saved.hashes).all(|(name, expected)| {
                        hash_file(&location.join(name)).is_ok_and(|actual| &actual == expected)
                    })
                {
                    return Ok(());
                }
            }
        }
        // A corrupt entry is disposable cache data; preserve it for diagnosis.
        let old = location.with_extension(format!(
            "invalid-{:08X}",
            LibraryIdentity::new().master_database_id
        ));
        fs::rename(location, old)?;
    }
    fs::rename(temp.path(), location)?;
    Ok(())
}
