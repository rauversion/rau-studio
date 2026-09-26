//! Exact-file duplicate detection. Hashes are evidence, never track IDs.
use super::*;
use ring::digest::{Context, SHA256};
use std::fs::{File, Metadata};
use std::io::Read;
use std::sync::{Arc, Mutex};

#[derive(Clone, Default)]
pub struct DuplicateManager(Arc<Mutex<Control>>);

#[derive(Default)]
struct Control {
    progress: Option<ScanProgress>,
    cancel: bool,
    skip_path: Option<String>,
    merging: bool,
    merge_batch: Option<MergeBatchProgress>,
    cancel_merge_batch: bool,
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct ScanProgress {
    library_id: String,
    status: String,
    total: usize,
    prepared: usize,
    processed: usize,
    hashed: usize,
    cached: usize,
    unique_size: usize,
    missing: usize,
    failed: usize,
    skipped: usize,
    bytes_total: u64,
    bytes_done: u64,
    bytes_read: u64,
    current_path: Option<String>,
    elapsed_seconds: f64,
    remaining_seconds: Option<f64>,
    message: Option<String>,
}

impl ScanProgress {
    fn running(&self) -> bool {
        matches!(self.status.as_str(), "preparing" | "hashing")
    }
}

#[derive(Clone, Serialize)]
pub struct DuplicateTrack {
    track_id: String,
    name: Option<String>,
    artist: Option<String>,
    source_path: Option<String>,
    playlist_count: usize,
    status: String,
    message: Option<String>,
}

#[derive(Serialize)]
pub struct DuplicateGroup {
    checksum: String,
    size_bytes: u64,
    tracks: Vec<DuplicateTrack>,
}

#[derive(Serialize)]
pub struct DuplicateReport {
    groups: Vec<DuplicateGroup>,
    omitted: Vec<DuplicateTrack>,
    unchecked: usize,
}

#[derive(Clone, PartialEq, Eq)]
struct Signature {
    size: u64,
    stamp: String,
}

fn signature(metadata: &Metadata) -> Result<Signature, String> {
    if !metadata.is_file() || metadata.len() == 0 {
        return Err("Archivo vacio o no regular".into());
    }
    let modified = metadata
        .modified()
        .map_err(|e| e.to_string())?
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    // ctime/inode also invalidate files whose modification time was preserved.
    #[cfg(unix)]
    let stamp = {
        use std::os::unix::fs::MetadataExt;
        format!(
            "{modified}:{}:{}:{}:{}",
            metadata.dev(),
            metadata.ino(),
            metadata.ctime(),
            metadata.ctime_nsec()
        )
    };
    #[cfg(not(unix))]
    let stamp = modified.to_string();
    Ok(Signature {
        size: metadata.len(),
        stamp,
    })
}

fn file_signature(path: &Path) -> Result<Signature, String> {
    signature(&fs::metadata(path).map_err(|e| e.to_string())?)
}

pub(super) fn refresh_regenerated_aliases(conn: &Connection, path: &Path) -> Result<(), String> {
    let sig = file_signature(path)?;
    // Only aliases of this same physical destination remain valid after an
    // intentional replacement. Other duplicate files must be compared again.
    conn.execute("UPDATE playlist_track_merge_aliases SET size_bytes = ?2, signature = ?3
        WHERE source_path = ?1 AND EXISTS (
            SELECT 1 FROM playlist_index_tracks t WHERE t.library_id = playlist_track_merge_aliases.library_id
            AND t.track_id = playlist_track_merge_aliases.track_id AND t.source_path = ?1
        )", params![path.to_string_lossy(), sig.size, sig.stamp]).map_err(|e| e.to_string())?;
    Ok(())
}

pub(super) fn init_db(conn: &Connection) -> Result<(), String> {
    conn.execute_batch("
        CREATE TABLE IF NOT EXISTS playlist_file_checksums (
          source_path TEXT PRIMARY KEY,
          size_bytes INTEGER,
          signature TEXT,
          checksum TEXT CHECK(checksum IS NULL OR length(checksum) = 64),
          status TEXT NOT NULL,
          message TEXT,
          checked_at TEXT NOT NULL,
          CHECK(status != 'ready' OR checksum IS NOT NULL)
        );
        CREATE INDEX IF NOT EXISTS idx_playlist_file_checksum
          ON playlist_file_checksums(checksum, size_bytes) WHERE checksum IS NOT NULL AND status = 'ready';
        CREATE TABLE IF NOT EXISTS playlist_duplicate_scans (
          library_id TEXT PRIMARY KEY,
          progress_json TEXT NOT NULL,
          FOREIGN KEY(library_id) REFERENCES playlist_index_libraries(id) ON DELETE CASCADE
        );
        CREATE TABLE IF NOT EXISTS playlist_track_merge_aliases (
          library_id TEXT NOT NULL,
          old_track_id TEXT NOT NULL,
          track_id TEXT NOT NULL,
          source_path TEXT NOT NULL,
          signature TEXT NOT NULL,
          size_bytes INTEGER NOT NULL,
          checksum TEXT NOT NULL,
          created_at TEXT NOT NULL,
          PRIMARY KEY(library_id, old_track_id),
          FOREIGN KEY(library_id, track_id) REFERENCES playlist_index_tracks(library_id, track_id) ON DELETE CASCADE
        );
        CREATE TABLE IF NOT EXISTS playlist_track_merge_history (
          id TEXT PRIMARY KEY,
          library_id TEXT NOT NULL,
          survivor_id TEXT NOT NULL,
          checksum TEXT NOT NULL,
          snapshot_json TEXT NOT NULL,
          created_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_playlist_merge_aliases_path
          ON playlist_track_merge_aliases(library_id, source_path);
    ").map_err(|e| e.to_string())
}

// Keep explicit user merges stable on reimport, but release the alias when that
// ID now points to a different or modified file. Never overwrite survivor tags.
pub(super) fn resolve_alias(
    conn: &Connection,
    library: &str,
    id: &str,
    path: Option<&Path>,
) -> Result<String, String> {
    let alias: Option<(String, String, u64, String, String)> = conn.query_row(
        "SELECT track_id, source_path, size_bytes, signature, old_track_id FROM playlist_track_merge_aliases
         WHERE library_id = ?1 AND (old_track_id = ?2 OR source_path = ?3)
         ORDER BY CASE WHEN old_track_id = ?2 THEN 0 ELSE 1 END, old_track_id LIMIT 1",
        params![library, id, path.map(|p| p.to_string_lossy().into_owned())], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
    ).optional().map_err(|e| e.to_string())?;
    if let Some((target, old_path, size, stamp, alias_id)) = alias {
        if let Some(path) = path {
            if path == Path::new(&old_path) {
                match fs::metadata(path) {
                    Ok(meta) if signature(&meta).ok() == Some(Signature { size, stamp }) => {
                        return Ok(target)
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(target),
                    _ => {}
                }
            }
        }
        conn.execute(
            "DELETE FROM playlist_track_merge_aliases WHERE library_id = ?1 AND old_track_id = ?2",
            params![library, alias_id],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(id.to_string())
}

fn save_file(
    conn: &Connection,
    path: &str,
    sig: Option<&Signature>,
    checksum: Option<&str>,
    status: &str,
    message: Option<&str>,
) -> Result<(), String> {
    conn.execute(
        "INSERT INTO playlist_file_checksums (source_path, size_bytes, signature, checksum, status, message, checked_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) ON CONFLICT(source_path) DO UPDATE SET
           size_bytes = excluded.size_bytes, signature = excluded.signature, checksum = excluded.checksum,
           status = excluded.status, message = excluded.message, checked_at = excluded.checked_at",
        params![path, sig.map(|s| s.size), sig.map(|s| &s.stamp), checksum, status, message, timestamp()],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

fn cached_hash(conn: &Connection, path: &str, sig: &Signature) -> Result<Option<String>, String> {
    conn.query_row(
        "SELECT checksum FROM playlist_file_checksums WHERE source_path = ?1 AND size_bytes = ?2
           AND signature = ?3 AND status = 'ready' AND checksum IS NOT NULL",
        params![path, sig.size, &sig.stamp],
        |r| r.get(0),
    )
    .optional()
    .map_err(|e| e.to_string())
}

fn publish(manager: &DuplicateManager, progress: &ScanProgress) {
    manager.0.lock().unwrap().progress = Some(progress.clone());
}

fn persist_progress(conn: &Connection, progress: &ScanProgress) -> Result<(), String> {
    conn.execute(
        "INSERT INTO playlist_duplicate_scans (library_id, progress_json) VALUES (?1, ?2)
        ON CONFLICT(library_id) DO UPDATE SET progress_json = excluded.progress_json",
        params![
            &progress.library_id,
            serde_json::to_string(progress).map_err(|e| e.to_string())?
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn playlist_duplicates_status(
    app: AppHandle,
    library_id: String,
) -> Result<Option<ScanProgress>, String> {
    let manager = app.state::<DuplicateManager>();
    let current = manager.0.lock().unwrap().progress.clone();
    if current
        .as_ref()
        .is_some_and(|p| p.running() || p.library_id == library_id)
    {
        return Ok(current);
    }
    tauri::async_runtime::spawn_blocking(move || {
        let conn = open_db(&app)?;
        let saved: Option<String> = conn
            .query_row(
                "SELECT progress_json FROM playlist_duplicate_scans WHERE library_id = ?1",
                params![library_id],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        saved
            .map(|json| {
                let mut progress: ScanProgress =
                    serde_json::from_str(&json).map_err(|e| e.to_string())?;
                if progress.running() {
                    progress.status = "interrupted".into();
                    progress.current_path = None;
                }
                Ok(progress)
            })
            .transpose()
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn playlist_duplicates_cancel(app: AppHandle) {
    app.state::<DuplicateManager>().0.lock().unwrap().cancel = true;
}

#[tauri::command]
pub fn playlist_duplicates_skip(app: AppHandle, source_path: String) {
    let manager = app.state::<DuplicateManager>();
    let mut control = manager.0.lock().unwrap();
    if control
        .progress
        .as_ref()
        .is_some_and(|p| p.status == "hashing" && p.current_path.as_deref() == Some(&source_path))
    {
        control.skip_path = Some(source_path);
    }
}

#[tauri::command]
pub fn playlist_duplicates_start(app: AppHandle, library_id: String) -> Result<(), String> {
    let manager = app.state::<DuplicateManager>().inner().clone();
    {
        let mut control = manager.0.lock().unwrap();
        if control.merging || control.progress.as_ref().is_some_and(ScanProgress::running) {
            return Err("Ya hay un escaneo o fusion en curso".into());
        }
        *control = Control {
            progress: Some(ScanProgress {
                library_id: library_id.clone(),
                status: "preparing".into(),
                ..Default::default()
            }),
            ..Default::default()
        };
    }
    tauri::async_runtime::spawn(async move {
        let worker_manager = manager.clone();
        let worker_app = app.clone();
        let result = tauri::async_runtime::spawn_blocking(move || {
            let conn = open_db(&worker_app)?;
            if get_library(&conn, &library_id)?.is_none() {
                return Err("Biblioteca no encontrada".into());
            }
            scan_library(&conn, &library_id, &worker_manager)
        })
        .await
        .map_err(|e| e.to_string())
        .and_then(|result| result);
        if let Err(error) = result {
            let mut control = manager.0.lock().unwrap();
            if let Some(p) = &mut control.progress {
                p.status = "failed".into();
                p.message = Some(error);
                p.current_path = None;
            }
        }
        // Persist the final state too, including cancellation or errors.
        let progress = manager.0.lock().unwrap().progress.clone();
        if let Some(p) = progress {
            let _ = tauri::async_runtime::spawn_blocking(move || {
                if let Ok(conn) = open_db(&app) {
                    let _ = persist_progress(&conn, &p);
                }
            })
            .await;
        }
    });
    Ok(())
}

#[derive(Debug)]
enum HashFailure {
    Cancelled,
    Skipped,
    Changed,
    Io(String),
}

fn hash_file(
    path: &Path,
    expected: &Signature,
    mut on_chunk: impl FnMut(u64) -> Result<(), HashFailure>,
) -> Result<String, HashFailure> {
    on_chunk(0)?;
    let mut file = File::open(path).map_err(|e| HashFailure::Io(e.to_string()))?;
    if signature(
        &file
            .metadata()
            .map_err(|e| HashFailure::Io(e.to_string()))?,
    )
    .ok()
    .as_ref()
        != Some(expected)
    {
        return Err(HashFailure::Changed);
    }
    let mut digest = Context::new(&SHA256);
    let mut buffer = vec![0u8; 1024 * 1024];
    let mut total = 0;
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|e| HashFailure::Io(e.to_string()))?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
        total += count as u64;
        on_chunk(count as u64)?;
    }
    if total != expected.size
        || file_signature(path).ok().as_ref() != Some(expected)
        || signature(
            &file
                .metadata()
                .map_err(|e| HashFailure::Io(e.to_string()))?,
        )
        .ok()
        .as_ref()
            != Some(expected)
    {
        return Err(HashFailure::Changed);
    }
    Ok(digest
        .finish()
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

fn scan_library(
    conn: &Connection,
    library: &str,
    manager: &DuplicateManager,
) -> Result<(), String> {
    let start = Instant::now();
    let paths = {
        let mut stmt = conn.prepare("SELECT COALESCE(source_path, ''), COUNT(*) FROM playlist_index_tracks WHERE library_id = ?1 GROUP BY source_path ORDER BY source_path").map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params![library], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, usize>(1)?))
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?
    };
    let mut p = ScanProgress {
        library_id: library.into(),
        status: "preparing".into(),
        total: paths.len(),
        ..Default::default()
    };
    let mut candidates = Vec::new();
    let mut sizes = HashMap::<u64, usize>::new();
    let mut last_publish = Instant::now();
    publish(manager, &p);
    persist_progress(conn, &p)?;
    for (path, references) in paths {
        if manager.0.lock().unwrap().cancel {
            break;
        }
        p.current_path = Some(path.clone());
        let meta = fs::metadata(&path);
        let available = meta.as_ref().is_ok_and(|m| m.is_file());
        if path.is_empty() {
            conn.execute("UPDATE playlist_index_tracks SET source_exists = 0 WHERE library_id = ?1 AND (source_path IS NULL OR source_path = '') AND source_exists != 0", params![library]).map_err(|e| e.to_string())?;
        } else {
            conn.execute("UPDATE playlist_index_tracks SET source_exists = ?3 WHERE source_path = ?2 AND library_id = ?1 AND source_exists != ?3", params![library, &path, available]).map_err(|e| e.to_string())?;
        }
        match meta {
            Ok(meta) => match signature(&meta) {
                Ok(sig) => {
                    // Clear obsolete evidence immediately, even if this run is cancelled.
                    if cached_hash(conn, &path, &sig)?.is_none() {
                        save_file(conn, &path, Some(&sig), None, "pending", None)?;
                    }
                    *sizes.entry(sig.size).or_default() += references;
                    candidates.push((path, sig));
                }
                Err(message) => {
                    p.failed += 1;
                    p.processed += 1;
                    save_file(conn, &path, None, None, "unreadable", Some(&message))?;
                }
            },
            Err(error) => {
                let missing = path.is_empty() || error.kind() == std::io::ErrorKind::NotFound;
                if missing {
                    p.missing += 1;
                } else {
                    p.failed += 1;
                }
                p.processed += 1;
                save_file(
                    conn,
                    &path,
                    None,
                    None,
                    if missing { "missing" } else { "unreadable" },
                    Some(&error.to_string()),
                )?;
            }
        }
        p.prepared += 1;
        if last_publish.elapsed() >= Duration::from_millis(200) {
            p.elapsed_seconds = start.elapsed().as_secs_f64();
            publish(manager, &p);
            last_publish = Instant::now();
        }
    }
    let mut work = Vec::new();
    for (path, sig) in candidates {
        if cached_hash(conn, &path, &sig)?.is_some() {
            p.cached += 1;
            p.processed += 1;
        } else if sizes.get(&sig.size).copied().unwrap_or(0) < 2 {
            p.unique_size += 1;
            p.processed += 1;
            save_file(conn, &path, Some(&sig), None, "unique_size", None)?;
        } else {
            p.bytes_total += sig.size;
            work.push((path, sig));
        }
    }
    p.status = "hashing".into();
    publish(manager, &p);
    persist_progress(conn, &p)?;
    let hashing_start = Instant::now();
    for (path, sig) in work {
        if manager.0.lock().unwrap().cancel {
            break;
        }
        p.current_path = Some(path.clone());
        publish(manager, &p);
        let before = p.bytes_done;
        let result = hash_file(Path::new(&path), &sig, |bytes| {
            {
                let mut control = manager.0.lock().unwrap();
                if control.cancel {
                    return Err(HashFailure::Cancelled);
                }
                if control.skip_path.as_deref() == Some(&path) {
                    control.skip_path = None;
                    return Err(HashFailure::Skipped);
                }
            }
            p.bytes_done += bytes;
            p.bytes_read += bytes;
            if last_publish.elapsed() >= Duration::from_millis(200) {
                p.elapsed_seconds = start.elapsed().as_secs_f64();
                let elapsed = hashing_start.elapsed().as_secs_f64();
                p.remaining_seconds = if elapsed >= 2.0 && p.bytes_read > 0 {
                    Some(
                        p.bytes_total.saturating_sub(p.bytes_done) as f64 * elapsed
                            / p.bytes_read as f64,
                    )
                } else {
                    None
                };
                publish(manager, &p);
                last_publish = Instant::now();
            }
            Ok(())
        });
        match result {
            Ok(hash) => {
                save_file(conn, &path, Some(&sig), Some(&hash), "ready", None)?;
                p.hashed += 1;
            }
            Err(HashFailure::Cancelled) => {
                save_file(conn, &path, None, None, "pending", None)?;
                break;
            }
            Err(HashFailure::Skipped) => {
                save_file(
                    conn,
                    &path,
                    None,
                    None,
                    "skipped",
                    Some("Omitido por el usuario"),
                )?;
                p.skipped += 1;
            }
            Err(error) => {
                let message = match error {
                    HashFailure::Changed => "El archivo cambio durante la lectura".into(),
                    HashFailure::Io(message) => message,
                    _ => unreachable!(),
                };
                save_file(conn, &path, None, None, "unreadable", Some(&message))?;
                p.failed += 1;
            }
        }
        p.bytes_done = before + sig.size;
        p.processed += 1;
        p.elapsed_seconds = start.elapsed().as_secs_f64();
        publish(manager, &p);
    }
    p.status = if manager.0.lock().unwrap().cancel {
        "cancelled"
    } else {
        "completed"
    }
    .into();
    p.current_path = None;
    p.remaining_seconds = None;
    p.elapsed_seconds = start.elapsed().as_secs_f64();
    persist_progress(conn, &p)?;
    publish(manager, &p);
    Ok(())
}

#[tauri::command]
pub async fn playlist_duplicates_report(
    app: AppHandle,
    library_id: String,
) -> Result<DuplicateReport, String> {
    tauri::async_runtime::spawn_blocking(move || report(&open_db(&app)?, &library_id))
        .await
        .map_err(|e| e.to_string())?
}

fn report(conn: &Connection, library: &str) -> Result<DuplicateReport, String> {
    let mut stmt = conn.prepare("SELECT t.track_id, t.name, t.artist, t.source_path,
        (SELECT COUNT(DISTINCT playlist_path) FROM playlist_index_memberships m WHERE m.library_id = t.library_id AND m.track_id = t.track_id)
        + (SELECT COUNT(*) FROM playlist_draft_tracks dt JOIN playlist_drafts d ON d.id = dt.draft_id WHERE d.library_id = t.library_id AND dt.track_id = t.track_id),
        COALESCE(c.status, 'pending'), c.message, c.checksum, c.size_bytes, c.signature
        FROM playlist_index_tracks t LEFT JOIN playlist_file_checksums c ON c.source_path = COALESCE(t.source_path, '')
        WHERE t.library_id = ?1 ORDER BY t.name, t.track_id").map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![library], |r| {
            Ok((
                DuplicateTrack {
                    track_id: r.get(0)?,
                    name: r.get(1)?,
                    artist: r.get(2)?,
                    source_path: r.get(3)?,
                    playlist_count: r.get(4)?,
                    status: r.get(5)?,
                    message: r.get(6)?,
                },
                r.get::<_, Option<String>>(7)?,
                r.get::<_, Option<u64>>(8)?,
                r.get::<_, Option<String>>(9)?,
            ))
        })
        .map_err(|e| e.to_string())?;
    let mut groups: BTreeMap<(String, u64), Vec<DuplicateTrack>> = BTreeMap::new();
    let mut omitted = Vec::new();
    let mut unchecked = 0;
    let mut live_signatures = HashMap::new();
    for row in rows {
        let (mut track, hash, size, stamp) = row.map_err(|e| e.to_string())?;
        if track.status == "ready" {
            let path = track.source_path.as_deref().unwrap_or_default();
            let live =
                live_signatures.entry(path.to_string()).or_insert_with(|| {
                    match fs::metadata(path) {
                        Ok(meta) => signature(&meta).map_err(|message| ("unreadable", message)),
                        Err(error) => Err((
                            if error.kind() == std::io::ErrorKind::NotFound {
                                "missing"
                            } else {
                                "unreadable"
                            },
                            error.to_string(),
                        )),
                    }
                });
            match live {
                Ok(sig) if Some(sig.size) == size && Some(&sig.stamp) == stamp.as_ref() => {}
                Ok(_) => {
                    track.status = "pending".into();
                    track.message = Some("El archivo cambio desde el ultimo escaneo".into());
                }
                Err((status, message)) => {
                    track.status = (*status).into();
                    track.message = Some(message.clone());
                }
            }
        }
        match (track.status.as_str(), hash, size) {
            ("ready", Some(hash), Some(size)) if hash.len() == 64 => {
                groups.entry((hash, size)).or_default().push(track)
            }
            ("missing" | "unreadable" | "skipped", _, _) => omitted.push(track),
            ("unique_size", _, _) => {}
            _ => unchecked += 1,
        }
    }
    Ok(DuplicateReport {
        groups: groups
            .into_iter()
            .filter(|(_, tracks)| tracks.len() > 1)
            .map(|((checksum, size_bytes), tracks)| DuplicateGroup {
                checksum,
                size_bytes,
                tracks,
            })
            .collect(),
        omitted,
        unchecked,
    })
}

#[derive(Serialize)]
pub struct MergeResult {
    merged: usize,
    warning: Option<String>,
}

#[derive(Clone, Deserialize)]
pub struct MergeBatchGroup {
    checksum: String,
    survivor_id: String,
    track_ids: Vec<String>,
    label: String,
}

#[derive(Clone, Serialize)]
pub struct MergeBatchError {
    label: String,
    message: String,
}

#[derive(Clone, Default, Serialize)]
pub struct MergeBatchProgress {
    id: String,
    library_id: String,
    status: String,
    total: usize,
    processed: usize,
    merged_groups: usize,
    merged_records: usize,
    removed_track_ids: Vec<String>,
    current_group: Option<String>,
    errors: Vec<MergeBatchError>,
    message: Option<String>,
}

#[tauri::command]
pub fn playlist_duplicates_merge_batch_status(
    app: AppHandle,
    library_id: String,
) -> Option<MergeBatchProgress> {
    app.state::<DuplicateManager>()
        .0
        .lock()
        .unwrap()
        .merge_batch
        .clone()
        .filter(|batch| batch.status == "running" || batch.library_id == library_id)
}

#[tauri::command]
pub fn playlist_duplicates_merge_batch_cancel(app: AppHandle) {
    app.state::<DuplicateManager>()
        .0
        .lock()
        .unwrap()
        .cancel_merge_batch = true;
}

fn validate_merge_batch(groups: &[MergeBatchGroup]) -> Result<(), String> {
    if groups.is_empty() {
        return Err("No hay grupos para fusionar".into());
    }
    let mut all_ids = BTreeSet::new();
    for group in groups {
        let ids: BTreeSet<_> = group.track_ids.iter().collect();
        if ids.len() < 2
            || !ids.contains(&group.survivor_id)
            || group.checksum.len() != 64
            || ids.into_iter().any(|id| !all_ids.insert(id))
        {
            return Err("Cada grupo debe tener al menos dos tracks, un registro a conservar y no compartir tracks con otro grupo".into());
        }
    }
    Ok(())
}

// Each approved group commits independently. Failed groups remain reviewable;
// stopping never undoes successful groups and takes effect between groups.
fn run_merge_batch(
    conn: &mut Connection,
    library: &str,
    groups: &[MergeBatchGroup],
    manager: &DuplicateManager,
) {
    for group in groups {
        {
            let mut control = manager.0.lock().unwrap();
            if control.cancel_merge_batch {
                break;
            }
            if let Some(batch) = &mut control.merge_batch {
                batch.current_group = Some(group.label.clone());
            }
        }
        let result = merge_tracks(
            conn,
            library,
            &group.survivor_id,
            &group.track_ids,
            &group.checksum,
        );
        let mut control = manager.0.lock().unwrap();
        if let Some(batch) = &mut control.merge_batch {
            batch.processed += 1;
            match result {
                Ok(merged) => {
                    batch.merged_groups += 1;
                    batch.merged_records += merged;
                    batch.removed_track_ids.extend(
                        group
                            .track_ids
                            .iter()
                            .filter(|id| *id != &group.survivor_id)
                            .cloned(),
                    );
                }
                Err(message) => batch.errors.push(MergeBatchError {
                    label: group.label.clone(),
                    message,
                }),
            }
        }
    }
}

#[tauri::command]
pub fn playlist_duplicates_merge_batch_start(
    app: AppHandle,
    library_id: String,
    groups: Vec<MergeBatchGroup>,
) -> Result<(), String> {
    validate_merge_batch(&groups)?;
    let manager = app.state::<DuplicateManager>().inner().clone();
    {
        let mut control = manager.0.lock().unwrap();
        if control.merging || control.progress.as_ref().is_some_and(ScanProgress::running) {
            return Err("Espera a que termine el escaneo o la fusion actual".into());
        }
        control.merging = true;
        control.cancel_merge_batch = false;
        control.merge_batch = Some(MergeBatchProgress {
            id: Uuid::new_v4().to_string(),
            library_id: library_id.clone(),
            status: "running".into(),
            total: groups.len(),
            ..Default::default()
        });
    }
    tauri::async_runtime::spawn(async move {
        let worker = manager.clone();
        let result = tauri::async_runtime::spawn_blocking(move || {
            let mut conn = open_db(&app)?;
            run_merge_batch(&mut conn, &library_id, &groups, &worker);
            if library_id == LOCAL_CONVERSION_LIBRARY_ID
                && worker
                    .0
                    .lock()
                    .unwrap()
                    .merge_batch
                    .as_ref()
                    .is_some_and(|b| b.merged_records > 0)
            {
                // A failed XML refresh must not turn committed merges into failures.
                if let Err(error) = sync_local_library_xml(&conn) {
                    if let Some(batch) = &mut worker.0.lock().unwrap().merge_batch {
                        batch.message = Some(error);
                    }
                }
            }
            Ok::<_, String>(())
        })
        .await
        .map_err(|e| e.to_string())
        .and_then(|result| result);
        let mut control = manager.0.lock().unwrap();
        if let Some(batch) = &mut control.merge_batch {
            batch.current_group = None;
            match result {
                Ok(()) => {
                    batch.status = if batch.processed == batch.total {
                        "completed"
                    } else {
                        "cancelled"
                    }
                    .into()
                }
                Err(error) => {
                    batch.status = "failed".into();
                    batch.message = Some(error);
                }
            }
        }
        control.merging = false;
    });
    Ok(())
}

#[tauri::command]
pub async fn playlist_duplicates_merge(
    app: AppHandle,
    library_id: String,
    survivor_id: String,
    track_ids: Vec<String>,
    checksum: String,
) -> Result<MergeResult, String> {
    let manager = app.state::<DuplicateManager>().inner().clone();
    {
        let mut control = manager.0.lock().unwrap();
        if control.merging || control.progress.as_ref().is_some_and(ScanProgress::running) {
            return Err("Espera a que termine el escaneo o la fusion actual".into());
        }
        control.merging = true;
    }
    let result = tauri::async_runtime::spawn_blocking(move || {
        let mut conn = open_db(&app)?;
        let merged = merge_tracks(&mut conn, &library_id, &survivor_id, &track_ids, &checksum)?;
        let warning = if library_id == LOCAL_CONVERSION_LIBRARY_ID {
            sync_local_library_xml(&conn).err()
        } else {
            None
        };
        Ok(MergeResult { merged, warning })
    })
    .await
    .map_err(|e| e.to_string())
    .and_then(|result| result);
    manager.0.lock().unwrap().merging = false;
    result
}

fn snapshot_rows(
    conn: &Connection,
    table: &str,
    library: &str,
    id: &str,
) -> Result<Vec<Value>, String> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT * FROM {table} WHERE library_id = ?1 AND track_id = ?2"
        ))
        .map_err(|e| e.to_string())?;
    let names: Vec<String> = stmt
        .column_names()
        .into_iter()
        .map(str::to_string)
        .collect();
    let rows = stmt
        .query_map(params![library, id], |r| {
            let mut object = serde_json::Map::new();
            for (i, name) in names.iter().enumerate() {
                use rusqlite::types::ValueRef;
                object.insert(
                    name.clone(),
                    match r.get_ref(i)? {
                        ValueRef::Null => Value::Null,
                        ValueRef::Integer(v) => json!(v),
                        ValueRef::Real(v) => json!(v),
                        ValueRef::Text(v) => json!(String::from_utf8_lossy(v)),
                        ValueRef::Blob(_) => Value::Null,
                    },
                );
            }
            Ok(Value::Object(object))
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

fn merge_tracks(
    conn: &mut Connection,
    library: &str,
    survivor: &str,
    ids: &[String],
    expected_hash: &str,
) -> Result<usize, String> {
    let ids: BTreeSet<_> = ids.iter().cloned().collect();
    if ids.len() < 2 || !ids.contains(survivor) || expected_hash.len() != 64 {
        return Err("Selecciona al menos dos duplicados y el registro que deseas conservar".into());
    }
    let mut verified = BTreeMap::new();
    // Read every file again: a stale result must never authorize a merge.
    for id in &ids {
        let path: Option<String> = conn.query_row("SELECT source_path FROM playlist_index_tracks WHERE library_id = ?1 AND track_id = ?2", params![library, id], |r| r.get(0)).map_err(|e| e.to_string())?;
        let path = path
            .filter(|p| !p.is_empty())
            .ok_or("Un track no tiene archivo. Vuelve a escanear.")?;
        let validation: Result<Signature, String> = (|| {
            let sig = file_signature(Path::new(&path))?;
            if cached_hash(conn, &path, &sig)?.as_deref() != Some(expected_hash) {
                return Err(
                    "El archivo cambio o no tiene un checksum valido. Vuelve a escanear.".into(),
                );
            }
            let actual = hash_file(Path::new(&path), &sig, |_| Ok(()))
                .map_err(|e| format!("No se pudo verificar {path}: {e:?}"))?;
            if actual != expected_hash {
                return Err("Los archivos ya no son identicos. Vuelve a escanear.".into());
            }
            Ok(sig)
        })();
        match validation {
            Ok(sig) => {
                verified.insert(id.clone(), (path, sig));
            }
            Err(error) => {
                let status = match fs::metadata(&path) {
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => "missing",
                    Err(_) => "unreadable",
                    Ok(_) => "pending",
                };
                save_file(conn, &path, None, None, status, Some(&error))?;
                return Err(error);
            }
        }
    }
    let tx = conn
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    for (id, (path, sig)) in &verified {
        let current: Option<String> = tx.query_row("SELECT source_path FROM playlist_index_tracks WHERE library_id = ?1 AND track_id = ?2", params![library, id], |r| r.get(0)).map_err(|e| e.to_string())?;
        if current.as_deref() != Some(path)
            || file_signature(Path::new(path)).ok().as_ref() != Some(sig)
        {
            return Err(
                "Un archivo o registro cambio durante la verificacion. Vuelve a escanear.".into(),
            );
        }
    }
    let now = timestamp();
    let mut history = serde_json::Map::new();
    for id in &ids {
        let mut entry = serde_json::Map::new();
        for table in [
            "playlist_index_tracks",
            "playlist_track_enrichments",
            "playlist_enrichment_observations",
            "playlist_index_memberships",
            "playlist_index_source_tracks",
            "playlist_index_source_memberships",
            "playlist_index_playlist_additions",
        ] {
            entry.insert(table.into(), json!(snapshot_rows(&tx, table, library, id)?));
        }
        history.insert(id.clone(), Value::Object(entry));
    }
    tx.execute("INSERT INTO playlist_track_merge_history (id, library_id, survivor_id, checksum, snapshot_json, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)", params![Uuid::new_v4().to_string(), library, survivor, expected_hash, Value::Object(history).to_string(), &now]).map_err(|e| e.to_string())?;
    let attributes: String = tx.query_row("SELECT attributes_json FROM playlist_index_tracks WHERE library_id = ?1 AND track_id = ?2", params![library, survivor], |r| r.get(0)).map_err(|e| e.to_string())?;
    let mut attributes: BTreeMap<String, String> =
        serde_json::from_str(&attributes).map_err(|e| e.to_string())?;
    for id in ids.iter().filter(|id| id.as_str() != survivor) {
        let (path, sig) = &verified[id];
        let old_attributes: String = tx.query_row("SELECT attributes_json FROM playlist_index_tracks WHERE library_id = ?1 AND track_id = ?2", params![library, id], |r| r.get(0)).map_err(|e| e.to_string())?;
        for (key, value) in serde_json::from_str::<BTreeMap<String, String>>(&old_attributes)
            .map_err(|e| e.to_string())?
        {
            if attributes.get(&key).is_none_or(|v| v.trim().is_empty()) {
                attributes.insert(key, value);
            }
        }
        for field in ["name", "artist", "album"] {
            tx.execute(&format!("UPDATE playlist_index_tracks SET {field} = COALESCE(NULLIF(TRIM({field}), ''), (SELECT {field} FROM playlist_index_tracks WHERE library_id = ?1 AND track_id = ?3)) WHERE library_id = ?1 AND track_id = ?2"), params![library, survivor, id]).map_err(|e| e.to_string())?;
        }
        tx.execute("UPDATE playlist_index_tracks SET user_rating = COALESCE(user_rating, (SELECT user_rating FROM playlist_index_tracks WHERE library_id = ?1 AND track_id = ?3)),
            search_text = search_text || ' ' || (SELECT search_text FROM playlist_index_tracks WHERE library_id = ?1 AND track_id = ?3), updated_at = ?4 WHERE library_id = ?1 AND track_id = ?2", params![library, survivor, id, &now]).map_err(|e| e.to_string())?;

        tx.execute("INSERT OR IGNORE INTO playlist_index_memberships (library_id, playlist_path, track_id, position)
            SELECT library_id, playlist_path, ?2, MIN(position) FROM playlist_index_memberships WHERE library_id = ?1 AND track_id IN (?2, ?3) GROUP BY library_id, playlist_path", params![library, survivor, id]).map_err(|e| e.to_string())?;
        tx.execute("DELETE FROM playlist_index_memberships WHERE library_id = ?1 AND (track_id = ?3 OR (track_id = ?2 AND position > (SELECT MIN(m.position) FROM playlist_index_memberships m WHERE m.library_id = ?1 AND m.track_id = ?2 AND m.playlist_path = playlist_index_memberships.playlist_path)))", params![library, survivor, id]).map_err(|e| e.to_string())?;
        tx.execute("INSERT INTO playlist_index_source_memberships (library_id, source_path, playlist_path, track_id, position)
            SELECT library_id, source_path, playlist_path, ?2, position FROM playlist_index_source_memberships WHERE library_id = ?1 AND track_id = ?3
            ON CONFLICT(library_id, source_path, playlist_path, track_id) DO UPDATE SET position = MIN(position, excluded.position)", params![library, survivor, id]).map_err(|e| e.to_string())?;
        tx.execute("INSERT INTO playlist_index_playlist_additions (library_id, playlist_path, track_id, position, created_at)
            SELECT library_id, playlist_path, ?2, position, created_at FROM playlist_index_playlist_additions WHERE library_id = ?1 AND track_id = ?3
            ON CONFLICT(library_id, playlist_path, track_id) DO UPDATE SET position = MIN(position, excluded.position)", params![library, survivor, id]).map_err(|e| e.to_string())?;
        tx.execute("UPDATE playlist_index_source_tracks SET track_id = ?2 WHERE library_id = ?1 AND track_id = ?3", params![library, survivor, id]).map_err(|e| e.to_string())?;
        tx.execute("INSERT INTO playlist_draft_tracks (draft_id, track_id, position, created_at)
            SELECT draft_id, ?2, position, created_at FROM playlist_draft_tracks WHERE track_id = ?3 AND draft_id IN (SELECT id FROM playlist_drafts WHERE library_id = ?1)
            ON CONFLICT(draft_id, track_id) DO UPDATE SET position = MIN(position, excluded.position)", params![library, survivor, id]).map_err(|e| e.to_string())?;
        tx.execute("DELETE FROM playlist_draft_tracks WHERE track_id = ?3 AND draft_id IN (SELECT id FROM playlist_drafts WHERE library_id = ?1)", params![library, survivor, id]).map_err(|e| e.to_string())?;
        tx.execute("INSERT INTO playlist_copilot_candidate_tracks (candidate_set_id, track_id, position, score, reasons_json, score_components_json)
            SELECT candidate_set_id, ?2, position, score, reasons_json, score_components_json FROM playlist_copilot_candidate_tracks WHERE track_id = ?3 AND candidate_set_id IN (SELECT c.id FROM playlist_copilot_candidate_sets c JOIN playlist_copilot_sessions s ON s.id = c.session_id WHERE s.library_id = ?1)
            ON CONFLICT(candidate_set_id, track_id) DO UPDATE SET position = MIN(position, excluded.position), score = MAX(score, excluded.score)", params![library, survivor, id]).map_err(|e| e.to_string())?;
        tx.execute("DELETE FROM playlist_copilot_candidate_tracks WHERE track_id = ?3 AND candidate_set_id IN (SELECT c.id FROM playlist_copilot_candidate_sets c JOIN playlist_copilot_sessions s ON s.id = c.session_id WHERE s.library_id = ?1)", params![library, survivor, id]).map_err(|e| e.to_string())?;

        // Preserve observations even when both tracks had work in the same run.
        tx.execute("UPDATE playlist_enrichment_observations SET task_id = (SELECT target.id FROM playlist_enrichment_tasks old JOIN playlist_enrichment_tasks target ON target.run_id = old.run_id AND target.provider = old.provider AND target.library_id = old.library_id AND target.track_id = ?2 WHERE old.id = playlist_enrichment_observations.task_id)
            WHERE library_id = ?1 AND track_id = ?3 AND EXISTS (SELECT 1 FROM playlist_enrichment_tasks old JOIN playlist_enrichment_tasks target ON target.run_id = old.run_id AND target.provider = old.provider AND target.library_id = old.library_id AND target.track_id = ?2 WHERE old.id = playlist_enrichment_observations.task_id)", params![library, survivor, id]).map_err(|e| e.to_string())?;
        tx.execute("UPDATE playlist_enrichment_observations SET track_id = ?2 WHERE library_id = ?1 AND track_id = ?3", params![library, survivor, id]).map_err(|e| e.to_string())?;
        for table in ["playlist_enrichment_tasks", "playlist_track_enrichments"] {
            tx.execute(&format!("UPDATE OR IGNORE {table} SET track_id = ?2 WHERE library_id = ?1 AND track_id = ?3"), params![library, survivor, id]).map_err(|e| e.to_string())?;
        }
        // Broadcast owns path snapshots: retain those, updating only catalog identity.
        for table in ["broadcast_queue_entries", "broadcast_schedule_tracks"] {
            let exists: bool = tx
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
                    params![table],
                    |r| r.get(0),
                )
                .map_err(|e| e.to_string())?;
            if exists {
                tx.execute(
                    &format!(
                        "UPDATE {table} SET track_id = ?2 WHERE library_id = ?1 AND track_id = ?3"
                    ),
                    params![library, survivor, id],
                )
                .map_err(|e| e.to_string())?;
            }
        }
        let has_schedule: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'broadcast_schedule_items')", [], |r| r.get(0)).map_err(|e| e.to_string())?;
        if has_schedule {
            tx.execute("UPDATE broadcast_schedule_items SET source_id = ?2 WHERE library_id = ?1 AND source_kind = 'track' AND source_id = ?3", params![library, survivor, id]).map_err(|e| e.to_string())?;
        }
        let has_bed: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'broadcast_automation_settings')", [], |r| r.get(0)).map_err(|e| e.to_string())?;
        if has_bed {
            tx.execute("UPDATE broadcast_automation_settings SET bed_track_id = ?2 WHERE bed_library_id = ?1 AND bed_track_id = ?3", params![library, survivor, id]).map_err(|e| e.to_string())?;
        }
        tx.execute("UPDATE playlist_track_merge_aliases SET track_id = ?2 WHERE library_id = ?1 AND track_id = ?3", params![library, survivor, id]).map_err(|e| e.to_string())?;
        tx.execute("INSERT INTO playlist_track_merge_aliases (library_id, old_track_id, track_id, source_path, signature, size_bytes, checksum, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
            ON CONFLICT(library_id, old_track_id) DO UPDATE SET track_id = excluded.track_id, source_path = excluded.source_path, signature = excluded.signature, size_bytes = excluded.size_bytes, checksum = excluded.checksum, created_at = excluded.created_at", params![library, id, survivor, path, &sig.stamp, sig.size, expected_hash, &now]).map_err(|e| e.to_string())?;
        tx.execute(
            "DELETE FROM playlist_index_tracks WHERE library_id = ?1 AND track_id = ?2",
            params![library, id],
        )
        .map_err(|e| e.to_string())?;
    }
    tx.execute("UPDATE playlist_index_tracks SET attributes_json = ?3 WHERE library_id = ?1 AND track_id = ?2", params![library, survivor, serde_json::to_string(&attributes).map_err(|e| e.to_string())?]).map_err(|e| e.to_string())?;
    // Merged text has changed; old embeddings are no longer valid.
    tx.execute(
        "DELETE FROM playlist_track_embeddings WHERE library_id = ?1 AND track_id = ?2",
        params![library, survivor],
    )
    .map_err(|e| e.to_string())?;
    tx.execute("UPDATE playlist_index_playlists SET track_count = (SELECT COUNT(DISTINCT track_id) FROM playlist_index_memberships m WHERE m.library_id = ?1 AND m.playlist_path = playlist_index_playlists.path), updated_at = ?2 WHERE library_id = ?1", params![library, &now]).map_err(|e| e.to_string())?;
    tx.execute("UPDATE playlist_index_libraries SET track_count = (SELECT COUNT(*) FROM playlist_index_tracks WHERE library_id = ?1), updated_at = ?2 WHERE id = ?1", params![library, &now]).map_err(|e| e.to_string())?;
    tx.execute(
        "UPDATE playlist_drafts SET updated_at = ?2 WHERE library_id = ?1",
        params![library, &now],
    )
    .map_err(|e| e.to_string())?;
    rebuild_fts(&tx)?;
    for saved in list_catalog_saved_searches(&tx, library)? {
        let criteria = catalog_criteria(&tx, library, saved.filters, &saved.query)?;
        let count = list_taxonomy_tracks(&tx, library)?
            .iter()
            .filter(|t| catalog_track_matches(t, &criteria, None))
            .count();
        tx.execute("UPDATE playlist_catalog_saved_searches SET result_count = ?2, last_evaluated_at = ?3 WHERE id = ?1", params![saved.id, count, &now]).map_err(|e| e.to_string())?;
    }
    tx.commit().map_err(|e| e.to_string())?;
    Ok(ids.len() - 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture {
        conn: Connection,
        dir: PathBuf,
    }
    impl Fixture {
        fn new() -> Self {
            let conn = Connection::open_in_memory().unwrap();
            conn.execute_batch("PRAGMA foreign_keys = ON").unwrap();
            super::super::init_db(&conn).unwrap();
            for library in ["library", "other"] {
                conn.execute("INSERT INTO playlist_index_libraries (id, source_path, source_name, indexed_at, updated_at) VALUES (?1, ?1, ?1, 'now', 'now')", params![library]).unwrap();
            }
            let dir = std::env::temp_dir().join(format!("rau-duplicates-test-{}", Uuid::new_v4()));
            fs::create_dir(&dir).unwrap();
            Self { conn, dir }
        }
        fn track(&self, id: &str, content: &[u8]) -> PathBuf {
            let path = self.dir.join(id);
            fs::write(&path, content).unwrap();
            self.reference("library", id, Some(&path));
            path
        }
        fn reference(&self, library: &str, id: &str, path: Option<&Path>) {
            self.conn.execute("INSERT INTO playlist_index_tracks (library_id, track_id, name, source_path, search_text, created_at, updated_at) VALUES (?1, ?2, ?2, ?3, ?2, 'now', 'now')", params![library, id, path.map(|p| p.to_string_lossy().into_owned())]).unwrap();
        }
        fn scan(&self) -> ScanProgress {
            let manager = DuplicateManager::default();
            scan_library(&self.conn, "library", &manager).unwrap();
            let progress = manager.0.lock().unwrap().progress.clone().unwrap();
            progress
        }
        fn count(&self, sql: &str) -> usize {
            self.conn.query_row(sql, [], |r| r.get(0)).unwrap()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.dir);
        }
    }

    #[test]
    fn scan_excludes_null_missing_empty_and_unique_sizes_and_reuses_cache() {
        let f = Fixture::new();
        f.track("a", b"abc");
        f.track("b", b"abc");
        f.track("c", b"xyz");
        f.track("unique", b"different size");
        f.track("empty", b"");
        f.reference("library", "missing1", Some(&f.dir.join("missing1")));
        f.reference("library", "missing2", Some(&f.dir.join("missing2")));
        f.reference("library", "no-path", None);
        f.reference("library", "also-no-path", None);
        let first = f.scan();
        assert_eq!(first.hashed, 3);
        assert_eq!(first.missing, 3);
        assert_eq!(first.unique_size, 1);
        assert_eq!(first.failed, 1);
        assert_eq!(first.processed, first.total);
        let result = report(&f.conn, "library").unwrap();
        assert_eq!(result.groups.len(), 1);
        assert_eq!(result.groups[0].tracks.len(), 2);
        assert_eq!(
            result.groups[0].checksum,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(result.omitted.len(), 5);
        assert_eq!(
            f.count("SELECT COUNT(*) FROM playlist_file_checksums WHERE checksum IS NOT NULL"),
            3
        );
        let second = f.scan();
        assert_eq!(second.cached, 3);
        assert_eq!(second.hashed, 0);
        assert_eq!(second.bytes_read, 0);
        fs::write(f.dir.join("b"), b"xyz").unwrap();
        let third = f.scan();
        assert_eq!(third.hashed, 1);
        assert_eq!(third.cached, 2);
        let result = report(&f.conn, "library").unwrap();
        assert_eq!(
            result.groups[0]
                .tracks
                .iter()
                .map(|t| t.track_id.as_str())
                .collect::<Vec<_>>(),
            ["b", "c"]
        );
        fs::remove_file(f.dir.join("b")).unwrap();
        f.scan();
        assert!(report(&f.conn, "library").unwrap().groups.is_empty());
        assert_eq!(f.count("SELECT COUNT(*) FROM playlist_file_checksums WHERE status = 'missing' AND checksum IS NOT NULL"), 0);
    }

    #[test]
    fn skip_and_cancel_keep_finished_work_and_can_retry() {
        let f = Fixture::new();
        let a = f.track("a", b"same");
        f.track("b", b"same");
        let manager = DuplicateManager::default();
        manager.0.lock().unwrap().skip_path = Some(a.to_string_lossy().into_owned());
        scan_library(&f.conn, "library", &manager).unwrap();
        let p = manager.0.lock().unwrap().progress.clone().unwrap();
        assert_eq!(p.skipped, 1);
        assert_eq!(p.hashed, 1);
        assert!(report(&f.conn, "library").unwrap().groups.is_empty());
        assert_eq!(f.scan().cached, 1);
        manager.0.lock().unwrap().cancel = true;
        scan_library(&f.conn, "library", &manager).unwrap();
        assert_eq!(
            manager.0.lock().unwrap().progress.as_ref().unwrap().status,
            "cancelled"
        );
        assert_eq!(f.scan().cached, 2);
    }

    #[test]
    fn interrupted_or_changing_reads_never_produce_hashes() {
        let f = Fixture::new();
        let path = f.track("a", &vec![1; 2 * 1024 * 1024]);
        let sig = file_signature(&path).unwrap();
        assert!(matches!(
            hash_file(&path, &sig, |_| Err(HashFailure::Cancelled)),
            Err(HashFailure::Cancelled)
        ));
        assert!(matches!(
            hash_file(&path, &sig, |_| Err(HashFailure::Skipped)),
            Err(HashFailure::Skipped)
        ));
        let mut changed = false;
        assert!(matches!(
            hash_file(&path, &sig, |n| {
                if n > 0 && !changed {
                    fs::write(&path, b"changed").unwrap();
                    changed = true;
                }
                Ok(())
            }),
            Err(HashFailure::Changed)
        ));
    }

    #[test]
    fn missing_or_changed_files_block_merge_without_mutating_catalog() {
        let mut f = Fixture::new();
        f.track("a", b"same");
        let b = f.track("b", b"same");
        f.scan();
        let hash = report(&f.conn, "library").unwrap().groups[0]
            .checksum
            .clone();
        fs::write(&b, b"diff").unwrap();
        assert!(merge_tracks(
            &mut f.conn,
            "library",
            "a",
            &["a".into(), "b".into()],
            &hash
        )
        .is_err());
        assert_eq!(f.count("SELECT COUNT(*) FROM playlist_index_tracks"), 2);
        assert_eq!(
            f.count("SELECT COUNT(*) FROM playlist_track_merge_history"),
            0
        );
        fs::remove_file(b).unwrap();
        assert!(merge_tracks(
            &mut f.conn,
            "library",
            "a",
            &["a".into(), "b".into()],
            &hash
        )
        .is_err());
        assert!(merge_tracks(&mut f.conn, "library", "a", &["a".into(), "b".into()], "").is_err());
        assert!(merge_tracks(
            &mut f.conn,
            "library",
            "outside",
            &["a".into(), "b".into()],
            &hash
        )
        .is_err());
    }

    #[test]
    fn merge_preserves_playlists_provenance_observations_and_files() {
        let mut f = Fixture::new();
        let a = f.track("a", b"same");
        let b = f.track("b", b"same");
        f.reference("other", "a", Some(&a));
        f.reference("other", "b", Some(&b));
        f.conn.execute_batch(r#"
          UPDATE playlist_index_tracks SET artist = 'Artist', user_rating = 4, attributes_json = '{"Genre":"House","Comments":"Keep me"}' WHERE library_id = 'library' AND track_id = 'b';
          UPDATE playlist_index_tracks SET attributes_json = '{"Genre":"Techno"}' WHERE library_id = 'library' AND track_id = 'a';
          INSERT INTO playlist_index_playlists (library_id, path, name, node_type, created_at, updated_at) VALUES ('library', 'p', 'p', '1', 'now', 'now'), ('library', 'q', 'q', '1', 'now', 'now');
          INSERT INTO playlist_index_memberships VALUES ('library', 'p', 'a', 8), ('library', 'p', 'b', 2), ('library', 'q', 'b', 3);
          INSERT INTO playlist_index_playlist_additions VALUES ('library', 'q', 'b', 3, 'now');
          INSERT INTO playlist_index_sources (library_id, source_path, source_name, indexed_at, updated_at) VALUES ('library', 'xml', 'xml', 'now', 'now');
          INSERT INTO playlist_index_source_tracks VALUES ('library', 'xml', '1', 'a'), ('library', 'xml', '2', 'b');
          INSERT INTO playlist_index_source_playlists VALUES ('library', 'xml', 'p', 'p', '1', 1);
          INSERT INTO playlist_index_source_memberships VALUES ('library', 'xml', 'p', 'a', 8), ('library', 'xml', 'p', 'b', 2);
          INSERT INTO playlist_drafts (id, library_id, name, created_at, updated_at) VALUES ('d', 'library', 'd', 'now', 'now'), ('other-d', 'other', 'd', 'now', 'now');
          INSERT INTO playlist_draft_tracks VALUES ('d', 'a', 8, 'now'), ('d', 'b', 2, 'now'), ('other-d', 'b', 9, 'now');
          INSERT INTO playlist_enrichment_runs (id, library_id, status, created_at, started_at) VALUES ('run', 'library', 'completed', 'now', 'now');
          INSERT INTO playlist_enrichment_tasks (id, run_id, library_id, track_id, provider, status, started_at) VALUES ('ta', 'run', 'library', 'a', 'test', 'matched', 'now'), ('tb', 'run', 'library', 'b', 'test', 'matched', 'now');
          INSERT INTO playlist_enrichment_observations (id, task_id, run_id, library_id, track_id, provider, field, value, observed_at) VALUES ('ob', 'tb', 'run', 'library', 'b', 'test', 'genre', 'House', 'now');
          INSERT INTO playlist_track_enrichments (id, library_id, track_id, provider, status, created_at, updated_at) VALUES ('ea', 'library', 'a', 'test', 'matched', 'now', 'now'), ('eb', 'library', 'b', 'test', 'matched', 'now', 'now');
        "#).unwrap();
        f.scan();
        let hash = report(&f.conn, "library").unwrap().groups[0]
            .checksum
            .clone();
        assert_eq!(
            merge_tracks(
                &mut f.conn,
                "library",
                "a",
                &["a".into(), "b".into()],
                &hash
            )
            .unwrap(),
            1
        );
        assert_eq!(
            f.count("SELECT COUNT(*) FROM playlist_index_memberships WHERE track_id = 'a'"),
            2
        );
        assert_eq!(
            f.count("SELECT position FROM playlist_index_memberships WHERE playlist_path = 'p'"),
            2
        );
        assert_eq!(
            f.count("SELECT COUNT(*) FROM playlist_index_source_tracks WHERE track_id = 'a'"),
            2
        );
        assert_eq!(
            f.count("SELECT position FROM playlist_index_source_memberships"),
            2
        );
        assert_eq!(
            f.count("SELECT position FROM playlist_draft_tracks WHERE draft_id = 'd'"),
            2
        );
        assert_eq!(f.count("SELECT COUNT(*) FROM playlist_draft_tracks WHERE draft_id = 'other-d' AND track_id = 'b'"), 1);
        assert_eq!(f.count("SELECT COUNT(*) FROM playlist_enrichment_observations WHERE track_id = 'a' AND task_id = 'ta'"), 1);
        assert_eq!(
            f.count("SELECT COUNT(*) FROM playlist_track_merge_history"),
            1
        );
        assert_eq!(
            f.count("SELECT COUNT(*) FROM playlist_index_tracks WHERE library_id = 'other'"),
            2
        );
        assert_eq!(f.count("SELECT COUNT(*) FROM playlist_track_fts WHERE library_id = 'library' AND track_id = 'b'"), 0);
        assert_eq!(
            f.count("SELECT track_count FROM playlist_index_libraries WHERE id = 'library'"),
            1
        );
        let tags: String = f
            .conn
            .query_row(
                "SELECT attributes_json FROM playlist_index_tracks WHERE library_id = 'library'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let tags: Value = serde_json::from_str(&tags).unwrap();
        assert_eq!(tags["Genre"], "Techno");
        assert_eq!(tags["Comments"], "Keep me");
        assert_eq!(fs::read(a).unwrap(), b"same");
        assert_eq!(fs::read(&b).unwrap(), b"same");
        assert_eq!(
            resolve_alias(&f.conn, "library", "b", Some(&b)).unwrap(),
            "a"
        );
        let violations = f
            .conn
            .prepare("PRAGMA foreign_key_check")
            .unwrap()
            .query_map([], |_| Ok(()))
            .unwrap()
            .count();
        assert_eq!(violations, 0);
        fs::write(&b, b"different").unwrap();
        assert_eq!(
            resolve_alias(&f.conn, "library", "b", Some(&b)).unwrap(),
            "b"
        );
    }

    #[test]
    fn report_rechecks_cached_matches_and_handles_repeated_paths() {
        let f = Fixture::new();
        let a = f.track("a", b"same");
        let b = f.track("b", b"same");
        f.reference("library", "same-path", Some(&a));
        assert_eq!(f.scan().hashed, 2);
        assert_eq!(
            report(&f.conn, "library").unwrap().groups[0].tracks.len(),
            3
        );
        fs::remove_file(b).unwrap();
        let result = report(&f.conn, "library").unwrap();
        assert_eq!(result.groups[0].tracks.len(), 2);
        assert_eq!(result.omitted.len(), 1);
        fs::write(a, b"changed").unwrap();
        let result = report(&f.conn, "library").unwrap();
        assert!(result.groups.is_empty());
        assert_eq!(result.unchecked, 2);
    }

    #[test]
    fn chained_merges_keep_import_aliases_pointing_to_the_survivor() {
        let mut f = Fixture::new();
        let a = f.track("a", b"same");
        let b = f.track("b", b"same");
        f.track("c", b"same");
        f.scan();
        let hash = report(&f.conn, "library").unwrap().groups[0]
            .checksum
            .clone();
        merge_tracks(
            &mut f.conn,
            "library",
            "a",
            &["a".into(), "b".into()],
            &hash,
        )
        .unwrap();
        merge_tracks(
            &mut f.conn,
            "library",
            "c",
            &["a".into(), "c".into()],
            &hash,
        )
        .unwrap();
        assert_eq!(
            resolve_alias(&f.conn, "library", "b", Some(&b)).unwrap(),
            "c"
        );
        assert_eq!(
            resolve_alias(&f.conn, "library", "new-import-id", Some(&b)).unwrap(),
            "c"
        );
        assert_eq!(
            resolve_alias(&f.conn, "library", "a", Some(&a)).unwrap(),
            "c"
        );
        fs::remove_file(&b).unwrap();
        assert_eq!(
            resolve_alias(&f.conn, "library", "b", Some(&b)).unwrap(),
            "c"
        );
    }

    #[test]
    fn six_thousand_files_scan_and_resume_without_duplicate_nulls() {
        let f = Fixture::new();
        for index in 0_u64..6000 {
            f.track(&format!("track-{index:04}"), &(index / 2).to_le_bytes());
        }
        let start = Instant::now();
        let first = f.scan();
        assert_eq!(first.hashed, 6000);
        assert_eq!(first.processed, 6000);
        assert_eq!(report(&f.conn, "library").unwrap().groups.len(), 3000);
        let first_seconds = start.elapsed().as_secs_f64();
        let start = Instant::now();
        let second = f.scan();
        assert_eq!(second.hashed, 0);
        assert_eq!(second.cached, 6000);
        assert_eq!(second.bytes_read, 0);
        eprintln!("6,000 tiny-file fixture: first scan {first_seconds:.2}s; cached scan {:.2}s (not an audio-disk benchmark)", start.elapsed().as_secs_f64());
    }

    #[test]
    fn bulk_merge_respects_survivors_continues_after_errors_and_removes_only_committed_groups() {
        let mut f = Fixture::new();
        for (id, content) in [
            ("a", b"one"),
            ("b", b"one"),
            ("c", b"two"),
            ("d", b"two"),
            ("e", b"six"),
            ("f", b"six"),
        ] {
            f.track(id, content);
        }
        f.scan();
        let found = report(&f.conn, "library").unwrap();
        let groups: Vec<_> = ["b", "d", "f"]
            .into_iter()
            .map(|survivor| {
                let group = found
                    .groups
                    .iter()
                    .find(|g| g.tracks.iter().any(|t| t.track_id == survivor))
                    .unwrap();
                MergeBatchGroup {
                    checksum: group.checksum.clone(),
                    survivor_id: survivor.into(),
                    track_ids: group.tracks.iter().map(|t| t.track_id.clone()).collect(),
                    label: survivor.into(),
                }
            })
            .collect();
        validate_merge_batch(&groups).unwrap();
        assert!(validate_merge_batch(&[]).is_err());
        assert!(validate_merge_batch(&[groups[0].clone(), groups[0].clone()]).is_err());
        f.conn.execute_batch("CREATE TRIGGER fail_one_group BEFORE DELETE ON playlist_index_tracks WHEN OLD.track_id = 'c' BEGIN SELECT RAISE(ABORT, 'group failure'); END").unwrap();
        let manager = DuplicateManager::default();
        manager.0.lock().unwrap().merge_batch = Some(MergeBatchProgress {
            total: 3,
            ..Default::default()
        });
        run_merge_batch(&mut f.conn, "library", &groups, &manager);
        let state = manager.0.lock().unwrap().merge_batch.clone().unwrap();
        assert_eq!(state.processed, 3);
        assert_eq!(state.merged_groups, 2);
        assert_eq!(state.merged_records, 2);
        assert_eq!(state.removed_track_ids, ["a", "e"]);
        assert_eq!(state.errors.len(), 1);
        assert_eq!(state.errors[0].label, "d");
        let remaining = report(&f.conn, "library").unwrap();
        assert_eq!(remaining.groups.len(), 1);
        assert_eq!(
            remaining.groups[0]
                .tracks
                .iter()
                .map(|t| t.track_id.as_str())
                .collect::<Vec<_>>(),
            ["c", "d"]
        );
        assert_eq!(
            f.count(
                "SELECT COUNT(*) FROM playlist_index_tracks WHERE track_id IN ('b', 'c', 'd', 'f')"
            ),
            4
        );
        manager.0.lock().unwrap().cancel_merge_batch = true;
        run_merge_batch(&mut f.conn, "library", &groups[1..2], &manager);
        assert_eq!(
            manager
                .0
                .lock()
                .unwrap()
                .merge_batch
                .as_ref()
                .unwrap()
                .processed,
            3
        );
    }

    #[test]
    fn merge_failure_rolls_back_every_reference() {
        let mut f = Fixture::new();
        f.track("a", b"same");
        f.track("b", b"same");
        f.scan();
        let hash = report(&f.conn, "library").unwrap().groups[0]
            .checksum
            .clone();
        f.conn.execute_batch("CREATE TRIGGER reject_delete BEFORE DELETE ON playlist_index_tracks BEGIN SELECT RAISE(ABORT, 'test failure'); END;").unwrap();
        assert!(merge_tracks(
            &mut f.conn,
            "library",
            "a",
            &["a".into(), "b".into()],
            &hash
        )
        .is_err());
        assert_eq!(f.count("SELECT COUNT(*) FROM playlist_index_tracks"), 2);
        assert_eq!(
            f.count("SELECT COUNT(*) FROM playlist_track_merge_aliases"),
            0
        );
        assert_eq!(
            f.count("SELECT COUNT(*) FROM playlist_track_merge_history"),
            0
        );
    }
}
