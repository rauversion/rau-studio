//! Local analysis proposals and explicitly saved preparation. Source files and
//! XML are never rewritten. A new proposal cannot replace saved preparation.
use super::*;
use aifficator_core::music_analysis::{self as dsp, GridCorrection, MusicAnalysis};
use aifficator_core::usb_export::hash_file;
use std::sync::Mutex;

static ANALYSIS_WORKER: Mutex<()> = Mutex::new(());

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
struct SourceStamp {
    path: String,
    size: u64,
    modified: String,
    sha256: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Preparation {
    pub analysis: MusicAnalysis,
    pub grid: Option<GridCorrection>,
    pub key: Option<String>,
    source: SourceStamp,
    pub revision: String,
    pub saved_at: Option<String>,
}

impl Preparation {
    pub(crate) fn source_sha256(&self) -> String {
        self.source.sha256.clone()
    }
}

#[derive(Default, Serialize)]
pub(crate) struct AnalysisState {
    proposal: Option<Preparation>,
    prepared: Option<Preparation>,
    stale: bool,
}

fn source_path(conn: &Connection, library: &str, track: &str) -> Result<PathBuf, String> {
    let track =
        get_index_track(conn, library, track)?.ok_or("La pista ya no existe en la biblioteca.")?;
    let path = track
        .source_path
        .ok_or("La pista no tiene un archivo local.")?;
    fs::canonicalize(path).map_err(|e| format!("No se pudo abrir el audio original: {e}"))
}

fn stamp(path: &Path) -> Result<SourceStamp, String> {
    let metadata = fs::metadata(path).map_err(|e| e.to_string())?;
    if !metadata.is_file() || metadata.len() == 0 {
        return Err("El audio original está vacío o no es un archivo.".into());
    }
    let modified = metadata.modified().map_err(|e| e.to_string())?;
    let sha256 = hash_file(path).map_err(|e| e.to_string())?;
    let after = fs::metadata(path).map_err(|e| e.to_string())?;
    if after.len() != metadata.len() || after.modified().map_err(|e| e.to_string())? != modified {
        return Err("El audio cambió durante la lectura. Vuelve a analizar la pista.".into());
    }
    Ok(SourceStamp {
        path: path.to_string_lossy().into_owned(),
        size: metadata.len(),
        modified: format!(
            "{:?}",
            modified
                .duration_since(UNIX_EPOCH)
                .map_err(|e| e.to_string())?
        ),
        sha256,
    })
}

fn records(conn: &Connection, library: &str, track: &str) -> Result<AnalysisState, String> {
    let row: Option<(Option<String>, Option<String>)> = conn.query_row(
        "SELECT proposal_json, prepared_json FROM playlist_track_analysis WHERE library_id=?1 AND track_id=?2",
        params![library, track], |row| Ok((row.get(0)?, row.get(1)?)),
    ).optional().map_err(|e| e.to_string())?;
    let Some((proposal, prepared)) = row else {
        return Ok(AnalysisState::default());
    };
    let parse = |value: Option<String>| -> Result<Option<Preparation>, String> {
        value
            .map(|json| {
                if json.len() > 16 * 1024 * 1024 {
                    return Err("El análisis guardado supera el límite de tamaño.".into());
                }
                serde_json::from_str(&json)
                    .map_err(|e| format!("No se pudo leer la preparación musical: {e}"))
            })
            .transpose()
    };
    Ok(AnalysisState {
        proposal: parse(proposal)?,
        prepared: parse(prepared)?,
        stale: false,
    })
}

#[tauri::command]
pub(crate) async fn track_analysis_load(
    app: AppHandle,
    library_id: String,
    track_id: String,
) -> Result<AnalysisState, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let conn = open_db(&app)?;
        let mut state = records(&conn, &library_id, &track_id)?;
        if state.proposal.is_none() && state.prepared.is_none() {
            return Ok(state);
        }
        let path = source_path(&conn, &library_id, &track_id)?;
        let current = stamp(&path)?;
        for value in [&mut state.proposal, &mut state.prepared] {
            if value
                .as_ref()
                .is_some_and(|v| v.source != current || v.analysis.version != dsp::VERSION)
            {
                *value = None;
                state.stale = true;
            }
        }
        Ok(state)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub(crate) async fn track_analysis_run(
    app: AppHandle,
    library_id: String,
    track_id: String,
    min_bpm: f64,
    max_bpm: f64,
) -> Result<Preparation, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let _worker = ANALYSIS_WORKER.try_lock().map_err(|_| "Hay otra pista analizándose. Espera a que termine.")?;
        let conn = open_db(&app)?;
        let path = source_path(&conn, &library_id, &track_id)?;
        drop(conn);
        let source = stamp(&path)?;
        let ffmpeg = system::binary_status(&app, "ffmpeg");
        let binary = ffmpeg.path.filter(|_| ffmpeg.installed)
            .ok_or("No está disponible el conversor de audio. Revisa FFmpeg en Ajustes.")?;
        let analysis = dsp::analyze_file(&path, Path::new(&binary), min_bpm, max_bpm)?;
        if source != stamp(&path)? { return Err("El audio cambió durante el análisis. Vuelve a analizarlo.".into()); }
        let grid = analysis.bpm.zip(analysis.anchor_seconds).map(|(bpm, anchor_seconds)| GridCorrection { bpm, anchor_seconds, beat_in_bar: analysis.beat_in_bar });
        let preparation = Preparation { key: analysis.key.clone(), analysis, grid, source,
            revision: Uuid::new_v4().to_string(), saved_at: None };
        let conn = open_db(&app)?;
        if source_path(&conn, &library_id, &track_id)? != path {
            return Err("La pista cambió de archivo durante el análisis. Vuelve a abrirla.".into());
        }
        conn.execute("INSERT INTO playlist_track_analysis (library_id,track_id,proposal_json) VALUES (?1,?2,?3)
            ON CONFLICT(library_id,track_id) DO UPDATE SET proposal_json=excluded.proposal_json",
            params![library_id, track_id, serde_json::to_string(&preparation).map_err(|e| e.to_string())?]).map_err(|e| e.to_string())?;
        Ok(preparation)
    }).await.map_err(|e| format!("No se pudo completar el análisis: {e}"))?
}

#[tauri::command]
pub(crate) async fn track_analysis_save(
    app: AppHandle,
    library_id: String,
    track_id: String,
    revision: String,
    grid: Option<GridCorrection>,
    key: Option<String>,
) -> Result<Preparation, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut conn = open_db(&app)?;
        let path = source_path(&conn, &library_id, &track_id)?;
        let current = stamp(&path)?;
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate).map_err(|e| e.to_string())?;
        if source_path(&tx, &library_id, &track_id)? != path { return Err("La pista cambió de archivo. Vuelve a abrirla.".into()); }
        let state = records(&tx, &library_id, &track_id)?;
        let previous = state.prepared.clone().filter(|p| p.source == current && p.analysis.version == dsp::VERSION);
        let mut preparation = state.proposal.into_iter().chain(state.prepared)
            .find(|p| p.revision == revision).ok_or("El análisis cambió. Vuelve a abrir la pista antes de guardar.")?;
        if preparation.source != current || preparation.analysis.version != dsp::VERSION {
            return Err("El audio cambió desde el análisis. Vuelve a analizar la pista antes de guardar.".into());
        }
        if let Some(grid) = &grid { grid.validate(preparation.analysis.duration_seconds)?; }
        dsp::validate_key(key.as_deref())?;
        if grid.is_none() && key.is_none() { return Err("Prepara una grilla o elige una tonalidad antes de guardar.".into()); }
        preparation.grid = grid.or_else(|| previous.as_ref().and_then(|p| p.grid.clone()));
        preparation.key = key.or_else(|| previous.as_ref().and_then(|p| p.key.clone()));
        preparation.revision = Uuid::new_v4().to_string();
        preparation.saved_at = Some(timestamp());
        let track = get_index_track(&tx, &library_id, &track_id)?.ok_or("La pista ya no existe.")?;
        let mut attributes = track.attributes.clone();
        if let Some(grid) = &preparation.grid { attributes.insert("AverageBpm".into(), format!("{:.2}", grid.bpm)); }
        if let Some(key) = &preparation.key { attributes.insert("Tonality".into(), key.clone()); }
        let paths = indexed_playlist_paths_for_track(&tx, &library_id, &track_id)?;
        let search_text = indexed_track_search_text(&track, &attributes, &paths);
        tx.execute("UPDATE playlist_index_tracks SET attributes_json=?3, search_text=?4, updated_at=?5 WHERE library_id=?1 AND track_id=?2",
            params![library_id, track_id, serde_json::to_string(&attributes).map_err(|e| e.to_string())?, search_text, timestamp()]).map_err(|e| e.to_string())?;
        tx.execute("UPDATE playlist_track_analysis SET prepared_json=?3,proposal_json=NULL WHERE library_id=?1 AND track_id=?2",
            params![library_id, track_id, serde_json::to_string(&preparation).map_err(|e| e.to_string())?]).map_err(|e| e.to_string())?;
        tx.execute("DELETE FROM playlist_track_embeddings WHERE library_id=?1 AND track_id=?2", params![library_id, track_id]).map_err(|e| e.to_string())?;
        tx.execute("DELETE FROM playlist_track_fts WHERE library_id=?1 AND track_id=?2", params![library_id, track_id]).map_err(|e| e.to_string())?;
        tx.execute("INSERT INTO playlist_track_fts (library_id,track_id,name,artist,album,kind,location,source_path,search_text)
            SELECT library_id,track_id,COALESCE(name,''),COALESCE(artist,''),COALESCE(album,''),kind,location,source_path,search_text
            FROM playlist_index_tracks WHERE library_id=?1 AND track_id=?2", params![library_id,track_id]).map_err(|e| e.to_string())?;
        tx.execute("UPDATE playlist_index_libraries SET updated_at=?2 WHERE id=?1", params![library_id,timestamp()]).map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        let _ = app.emit("track-analysis-saved", json!({ "library_id": library_id, "track_id": track_id }));
        Ok(preparation)
    }).await.map_err(|e| e.to_string())?
}

/// Only explicitly saved preparation is eligible for export. Recheck bytes,
/// path, size and mtime; a stale grid must never silently accompany new audio.
pub(crate) fn prepared_for_export(
    conn: &Connection,
    library: &str,
    track: &str,
    path: &Path,
) -> Result<Option<Preparation>, String> {
    let Some(prepared) = records(conn, library, track)?.prepared else {
        return Ok(None);
    };
    let actual = stamp(&fs::canonicalize(path).map_err(|e| e.to_string())?)?;
    if prepared.source != actual || prepared.analysis.version != dsp::VERSION {
        return Err("La preparación musical guardada corresponde a otra versión del audio. Vuelve a analizar y guardar la pista.".into());
    }
    if let Some(grid) = &prepared.grid {
        grid.validate(prepared.analysis.duration_seconds)?;
    }
    dsp::validate_key(prepared.key.as_deref())?;
    Ok(Some(prepared))
}
