//! Additive USB updates. Existing media are immutable; metadata publication has
//! a recoverable journal and keeps the previous databases/report as a backup.
use super::*;
use std::time::UNIX_EPOCH;

const JOURNAL: &str = ".rau-update-journal.json";
const METADATA: [&str; 3] = [
    "PIONEER/rekordbox/export.pdb",
    "PIONEER/rekordbox/exportLibrary.db",
    "rau-export-report.json",
];

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileSignature {
    pub size: u64,
    pub modified: String,
    #[serde(default)]
    pub identity: Option<String>,
}
impl FileSignature {
    pub fn read(path: &Path) -> Result<Self> {
        let m = fs::metadata(path)?;
        if !m.is_file() {
            return Err(invalid("expected a regular media file"));
        }
        #[cfg(unix)]
        let identity = {
            use std::os::unix::fs::MetadataExt;
            Some(format!(
                "{}:{}:{}:{}",
                m.dev(),
                m.ino(),
                m.ctime(),
                m.ctime_nsec()
            ))
        };
        #[cfg(not(unix))]
        let identity = None;
        Ok(Self {
            size: m.len(),
            modified: format!(
                "{:?}",
                m.modified()?
                    .duration_since(UNIX_EPOCH)
                    .map_err(|_| invalid("invalid file timestamp"))?
            ),
            identity,
        })
    }
}

/// Last observed local metadata supports a three-way merge with edits on USB.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TrackMetadata {
    title: String,
    artist: String,
    album: String,
    genre: String,
    label: String,
    key: String,
    comment: String,
    bpm_x100: u32,
    rating: u8,
    year: u16,
    track_number: u32,
}
impl TrackMetadata {
    pub(super) fn from_track(track: &ExportTrack) -> Self {
        Self {
            title: track.title.clone(),
            artist: track.artist.clone(),
            album: track.album.clone(),
            genre: track.genre.clone(),
            label: track.label.clone(),
            key: track.key.clone(),
            comment: track.comment.clone(),
            bpm_x100: track.bpm_x100,
            rating: track.rating,
            year: track.year,
            track_number: track.track_number,
        }
    }
    fn merge(&self, baseline: &Self, recorded: &Self, selected: &mut ExportTrack) -> bool {
        let mut preserved = false;
        macro_rules! keep_edit { ($($field:ident),+) => { $(
            if self.$field != recorded.$field || (selected.$field == baseline.$field && self.$field != selected.$field) {
                selected.$field = self.$field.clone(); preserved = true;
            }
        )+ }; }
        keep_edit!(
            title,
            artist,
            album,
            genre,
            label,
            key,
            comment,
            bpm_x100,
            rating,
            year,
            track_number
        );
        preserved
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct DevicePlaylist {
    pub id: u32,
    pub name: String,
    pub path: String,
    pub track_count: usize,
    pub source_id: Option<String>,
}
#[derive(Clone, Debug, Serialize)]
pub struct DestinationInfo {
    pub path: String,
    pub existing: bool,
    pub can_update: bool,
    pub reason: Option<String>,
    pub playlists: Vec<DevicePlaylist>,
    pub track_count: usize,
    pub revision: String,
    pub recovery_available: bool,
}
pub struct Existing {
    pub root: PathBuf,
    pub report: ExportReport,
    pub revision: String,
    changed_analysis: BTreeSet<u32>,
    recorded_metadata: BTreeMap<u32, TrackMetadata>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Work {
    Reuse,
    Analyze,
    Export,
}
#[derive(Clone, Debug, Default, Serialize)]
pub struct SyncCounts {
    pub new_tracks: usize,
    pub reused_tracks: usize,
    pub updated_tracks: usize,
    pub analysis_tracks: usize,
    pub preserved_playlists: usize,
}
pub struct Update {
    pub plan: ExportPlan,
    pub existing: Existing,
    pub work: BTreeMap<u32, Work>,
    pub counts: SyncCounts,
    pub notices: Vec<String>,
    source_metadata: BTreeMap<u32, TrackMetadata>,
    source_preparation: BTreeMap<u32, analysis::TrackAnalysis>,
    source_aliases: BTreeMap<u32, BTreeSet<String>>,
}

/// Refuse symlinks and traversal in device-owned paths, including parents.
pub fn device_file(root: &Path, relative: &str) -> Result<PathBuf> {
    let relative = relative.strip_prefix('/').unwrap_or(relative);
    let mut out = root.to_owned();
    for component in relative.split('/') {
        if component.is_empty()
            || matches!(component, "." | "..")
            || component.contains(['\\', '\0', ':'])
        {
            return Err(invalid("unsafe USB path"));
        }
        out.push(component);
        match fs::symlink_metadata(&out) {
            Ok(m) if m.file_type().is_symlink() => {
                return Err(invalid("USB update refuses symbolic links"))
            }
            Ok(_) => (),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(e.into()),
        }
    }
    Ok(out)
}
fn bounded(path: &Path, limit: u64) -> Result<Vec<u8>> {
    if fs::metadata(path)?.len() > limit {
        return Err(invalid("USB metadata exceeds size limit"));
    }
    Ok(fs::read(path)?)
}
fn revision(root: &Path) -> Result<String> {
    let mut digest = Sha256::new();
    digest.update(root.as_os_str().as_encoded_bytes());
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let m = fs::metadata(root)?;
        digest.update(m.dev().to_le_bytes());
        // Some removable filesystems assign a synthetic root inode that
        // changes between lookups, even though the mounted device and all
        // library bytes are unchanged. The canonical path, device and content
        // hashes identify this snapshot without relying on that unstable ID.
    }
    for name in METADATA.into_iter().chain(["PIONEER/DEVSETTING.DAT"]) {
        let p = device_file(root, name)?;
        digest.update(name.as_bytes());
        if p.exists() {
            digest.update(hash_file(&p)?);
        }
    }
    Ok(format!("{:x}", digest.finalize()))
}

pub fn inspect_destination(root: &Path) -> Result<DestinationInfo> {
    let root = root.canonicalize()?;
    if !root.is_dir() {
        return Err(invalid("Elige una carpeta de destino."));
    }
    let recovery = device_file(&root, JOURNAL)?.exists();
    // Publication may have stopped between two renames. Recovery must remain
    // available even when one database is temporarily absent or unreadable.
    if recovery {
        return Ok(DestinationInfo { path: root.to_string_lossy().into_owned(), existing: true,
            can_update: false, reason: Some("Hay una actualización interrumpida. Restaura la biblioteca anterior antes de continuar.".into()),
            playlists: Vec::new(), track_count: 0, revision: String::new(), recovery_available: true });
    }
    let pdb = device_file(&root, METADATA[0])?;
    let plus = device_file(&root, METADATA[1])?;
    let existing = pdb.exists() || plus.exists();
    let reason = if existing {
        match load_existing(&root) {
            Ok(old) => return Ok(DestinationInfo { path: root.to_string_lossy().into_owned(), existing: true,
                can_update: true, reason: None, playlists: display_playlists(&old.report.playlists, &old.report.playlist_sources),
                track_count: old.report.tracks.len(), revision: old.revision, recovery_available: false }),
            Err(error) => Some(format!("La biblioteca se puede visualizar, pero no actualizar de forma incremental: {error}")),
        }
    } else {
        None
    };
    let mut playlists = Vec::new();
    let mut track_count = 0;
    if pdb.exists() {
        let db = devicesql::inspect(&bounded(&pdb, 128 * 1024 * 1024)?)?;
        track_count = db.tracks.len();
        for p in db.playlists.iter().filter(|p| !p.is_folder) {
            let mut names = vec![p.name.clone()];
            let mut parent = p.parent_id;
            let mut seen = BTreeSet::new();
            while parent != 0 && seen.insert(parent) {
                let Some(node) = db.playlists.iter().find(|p| p.id == parent) else {
                    break;
                };
                names.push(node.name.clone());
                parent = node.parent_id;
            }
            names.reverse();
            playlists.push(DevicePlaylist {
                id: p.id,
                name: p.name.clone(),
                path: names.join(" / "),
                track_count: db.entries.iter().filter(|e| e[2] == p.id).count(),
                source_id: None,
            });
        }
    } else if plus.exists() {
        let db = onelibrary::inspect(&plus)?;
        track_count = db.tracks.len();
        playlists = db
            .playlists
            .iter()
            .filter(|p| !p.is_folder)
            .map(|p| DevicePlaylist {
                id: p.id,
                name: p.name.clone(),
                path: p.name.clone(),
                track_count: p.tracks.len(),
                source_id: None,
            })
            .collect();
    }
    let mut reason = reason;
    let mut can_update = !existing;
    if !existing {
        // Retain the original refusal for unrelated Contents/PIONEER trees.
        if let Err(e) = validate_classic_destination(&root) {
            can_update = false;
            reason = Some(e.to_string());
        }
    }
    Ok(DestinationInfo {
        path: root.to_string_lossy().into_owned(),
        existing,
        can_update,
        reason,
        playlists,
        track_count,
        revision: revision(&root)?,
        recovery_available: recovery,
    })
}

pub fn load_existing(root: &Path) -> Result<Existing> {
    let root = root.canonicalize()?;
    if device_file(&root, "RAU_EXPORT_INCOMPLETE.txt")?.exists() {
        return Err(invalid("La exportación inicial quedó incompleta. Se necesita un destino con una exportación de Rau terminada para actualizarla."));
    }
    let data = bounded(&device_file(&root, METADATA[2])?, 64 * 1024 * 1024)?;
    let mut report: ExportReport = serde_json::from_slice(&data)?;
    if !(5..=8).contains(&report.schema_version)
        || report.tracks.is_empty()
        || report.tracks.len() > 10_000
    {
        return Err(invalid(
            "Se necesita un informe de exportación de Rau compatible.",
        ));
    }
    let classic = devicesql::inspect(&bounded(
        &device_file(&root, METADATA[0])?,
        128 * 1024 * 1024,
    )?)?;
    let plus = onelibrary::inspect(&device_file(&root, METADATA[1])?)?;
    if report.schema_version < 8 && !plus.cues.is_empty() {
        return Err(invalid("La biblioteca antigua contiene cues de OneLibrary que Rau no puede migrar sin perder datos. Solo se permite visualizarla."));
    }
    if [
        "history",
        "history_content",
        "hotCueBankList",
        "hotCueBankList_cue",
        "image",
        "myTag",
        "myTag_content",
        "recommendedLike",
    ]
    .iter()
    .any(|table| plus.table_rows.get(*table).copied().unwrap_or(0) != 0)
        || classic
            .table_rows
            .iter()
            .any(|(table, rows)| [9, 10, 11, 12, 13, 14, 15].contains(table) && *rows != 0)
    {
        return Err(invalid("La biblioteca contiene historial, imágenes u otras tablas que Rau todavía no puede conservar al actualizar. Solo se permite visualizarla."));
    }
    if classic.tracks.len() != report.tracks.len()
        || plus.tracks.len() != report.tracks.len()
        || report.library_identity.master_database_id != plus.identity.master_database_id
    {
        return Err(invalid("La biblioteca cambió fuera de Rau y contiene pistas que no están registradas en su informe."));
    }
    report.library_identity = plus.identity;
    let mut seen = BTreeSet::new();
    let mut recorded_metadata = BTreeMap::new();
    for item in &mut report.tracks {
        recorded_metadata.insert(item.track.id, TrackMetadata::from_track(&item.track));
        if item.source_metadata.is_none() {
            item.source_metadata = Some(TrackMetadata::from_track(&item.track));
        }
        if item.source_preparation.is_none() {
            item.source_preparation = Some(item.track.analysis.clone());
        }
        if item.source_aliases.len() > 10_000
            || !seen.insert(item.track.source_key.clone())
            || item
                .source_aliases
                .iter()
                .any(|alias| alias == &item.track.source_key || !seen.insert(alias.clone()))
        {
            return Err(invalid("duplicate source identity in USB report"));
        }
        let c = classic
            .tracks
            .iter()
            .find(|t| t.id == item.track.id)
            .ok_or_else(|| invalid("USB track identity changed"))?;
        let p = plus
            .tracks
            .iter()
            .find(|t| t.id == item.track.id)
            .ok_or_else(|| invalid("USB track identity changed"))?;
        if c.path != item.track.device_path
            || p.path != c.path
            || c.analysis_path != p.analysis_path
            || c.size_bytes != item.audio.size_bytes
        {
            return Err(invalid(
                "Las rutas o los archivos del USB cambiaron fuera de Rau.",
            ));
        }
        item.track.analysis_path = Some(c.analysis_path.clone());
        item.analysis.dat_path = c.analysis_path.clone();
        item.analysis.ext_path = c.analysis_path.replace(".DAT", ".EXT");
        item.analysis.three_band_path = c.analysis_path.replace(".DAT", ".2EX");
        // A classic player may update only export.pdb; retain an unambiguous
        // title edit there as well as supported OneLibrary metadata edits.
        item.track.title = if c.title == p.title || c.title == item.track.title {
            p.title.clone()
        } else if p.title == item.track.title {
            c.title.clone()
        } else {
            return Err(invalid(
                "Las dos bibliotecas tienen cambios incompatibles en el título de una pista.",
            ));
        };
        item.track.artist = p.artist.clone();
        item.track.album = p.album.clone();
        item.track.genre = p.genre.clone();
        item.track.label = p.label.clone();
        item.track.key = p.key.clone();
        item.track.bpm_x100 = p.bpm_x100;
        item.track.rating = p.rating;
        item.track.year = p.year;
        item.track.track_number = p.track_number;
        item.track.comment = p.comment.clone();
        let cues = plus.cues.get(&item.track.id).cloned().unwrap_or_default();
        #[cfg(unix)]
        if item.analysis.renderer == super::engine::RENDERER && cues != item.analysis.plus_cues {
            super::engine::validate_cues(
                &bounded(
                    &device_file(&root, &item.analysis.dat_path)?,
                    anlz::MAX_FILE_BYTES as u64,
                )?,
                &bounded(
                    &device_file(&root, &item.analysis.ext_path)?,
                    anlz::MAX_FILE_BYTES as u64,
                )?,
                &cues,
            )?;
        }
        item.analysis.plus_cues = cues;
    }
    let classic_playlists = classic
        .playlists
        .iter()
        .map(|p| {
            let mut entries: Vec<_> = classic.entries.iter().filter(|e| e[2] == p.id).collect();
            entries.sort_by_key(|e| e[0]);
            ExportPlaylist {
                id: p.id,
                parent_id: p.parent_id,
                position: p.position,
                name: p.name.clone(),
                is_folder: p.is_folder,
                tracks: entries.iter().map(|e| e[1]).collect(),
            }
        })
        .collect::<Vec<_>>();
    report.playlists = merge_playlists(&report.playlists, &classic_playlists, &plus.playlists)?;
    report
        .playlist_sources
        .retain(|id, _| report.playlists.iter().any(|p| p.id == *id && !p.is_folder));
    let mut sources = BTreeSet::new();
    if report
        .playlist_sources
        .values()
        .any(|source| !sources.insert(source))
    {
        return Err(invalid("duplicate playlist source identity in USB report"));
    }
    let model = ExportPlan {
        tracks: report.tracks.iter().map(|t| t.track.clone()).collect(),
        playlists: report.playlists.clone(),
        source_bytes: 0,
        omitted_tracks: Vec::new(),
        playlist_sources: report.playlist_sources.clone(),
    };
    devicesql::validate_plan(&model)?;
    Ok(Existing {
        revision: revision(&root)?,
        root,
        report,
        changed_analysis: BTreeSet::new(),
        recorded_metadata,
    })
}

fn display_playlists(
    playlists: &[ExportPlaylist],
    sources: &BTreeMap<u32, String>,
) -> Vec<DevicePlaylist> {
    playlists
        .iter()
        .filter(|p| !p.is_folder)
        .map(|p| {
            let mut names = vec![p.name.clone()];
            let mut parent = p.parent_id;
            let mut seen = BTreeSet::new();
            while parent != 0 && seen.insert(parent) {
                let Some(node) = playlists.iter().find(|p| p.id == parent) else {
                    break;
                };
                names.push(node.name.clone());
                parent = node.parent_id;
            }
            names.reverse();
            DevicePlaylist {
                id: p.id,
                name: p.name.clone(),
                path: names.join(" / "),
                track_count: p.tracks.len(),
                source_id: sources.get(&p.id).cloned(),
            }
        })
        .collect()
}

fn merge_playlists(
    baseline: &[ExportPlaylist],
    classic: &[ExportPlaylist],
    plus: &[ExportPlaylist],
) -> Result<Vec<ExportPlaylist>> {
    let indexed = |nodes: &[ExportPlaylist]| {
        nodes
            .iter()
            .map(|p| (p.id, p.clone()))
            .collect::<BTreeMap<_, _>>()
    };
    let baseline = indexed(baseline);
    let classic = indexed(classic);
    let plus = indexed(plus);
    let ids: BTreeSet<_> = baseline
        .keys()
        .chain(classic.keys())
        .chain(plus.keys())
        .copied()
        .collect();
    let mut result = Vec::new();
    for id in ids {
        let b = baseline.get(&id);
        let c = classic.get(&id);
        let p = plus.get(&id);
        let chosen = if c == p || p == b {
            c
        } else if c == b {
            p
        } else {
            return Err(invalid("Device Library y OneLibrary tienen cambios incompatibles en una playlist. Reconcílialos antes de actualizar desde Rau."));
        };
        if let Some(node) = chosen {
            result.push(node.clone());
        }
    }
    Ok(result)
}

fn hydrate(old: &mut Existing) -> Result<()> {
    for item in &mut old.report.tracks {
        let audio = device_file(&old.root, &item.track.device_path)?;
        let signature = FileSignature::read(&audio)?;
        if signature.size != u64::from(item.audio.size_bytes) {
            return Err(invalid(format!(
                "El archivo del USB cambió: {}",
                item.track.title
            )));
        }
        if item.device_signature.as_ref() != Some(&signature) && hash_file(&audio)? != item.sha256 {
            return Err(invalid(format!(
                "El audio del USB no coincide con el export registrado: {}",
                item.track.title
            )));
        }
        item.device_signature = Some(signature);
        let paths = [
            &item.analysis.dat_path,
            &item.analysis.ext_path,
            &item.analysis.three_band_path,
        ];
        let signatures = paths
            .iter()
            .map(|p| FileSignature::read(&device_file(&old.root, p)?))
            .collect::<Result<Vec<_>>>()?;
        if item.analysis_signatures.as_ref() == Some(&signatures) {
            continue;
        }
        let mut hashes = Vec::new();
        for path in paths {
            hashes.push(hash_file(&device_file(&old.root, path)?)?);
        }
        if hashes
            != [
                item.dat_sha256.clone(),
                item.ext_sha256.clone(),
                item.three_band_sha256.clone(),
            ]
        {
            #[cfg(unix)]
            if item.analysis.renderer == super::engine::RENDERER {
                super::engine::validate_cues(
                    &bounded(
                        &device_file(&old.root, &item.analysis.dat_path)?,
                        anlz::MAX_FILE_BYTES as u64,
                    )?,
                    &bounded(
                        &device_file(&old.root, &item.analysis.ext_path)?,
                        anlz::MAX_FILE_BYTES as u64,
                    )?,
                    &item.analysis.plus_cues,
                )?;
            }
            let dat = anlz::inspect(&bounded(
                &device_file(&old.root, &item.analysis.dat_path)?,
                anlz::MAX_FILE_BYTES as u64,
            )?)?;
            let ext = anlz::inspect(&bounded(
                &device_file(&old.root, &item.analysis.ext_path)?,
                anlz::MAX_FILE_BYTES as u64,
            )?)?;
            let three_band = anlz::inspect(&bounded(
                &device_file(&old.root, &item.analysis.three_band_path)?,
                anlz::MAX_FILE_BYTES as u64,
            )?)?;
            if dat.audio_path != item.track.device_path
                || ext.audio_path != item.track.device_path
                || three_band.audio_path != item.track.device_path
            {
                return Err(invalid("USB analysis points to another audio file"));
            }
            let mut tempos = Vec::new();
            for beat in dat.beats.unwrap_or_default() {
                let seconds = f64::from(beat.time_ms) / 1000.0;
                let needs_anchor = tempos.last().is_none_or(|p: &analysis::TempoAnchor| {
                    let steps = ((seconds - p.seconds) * p.bpm / 60.0).round();
                    (seconds - p.seconds - steps * 60.0 / p.bpm).abs() > 0.002
                        || (f64::from(beat.bpm_x100) / 100.0 - p.bpm).abs() > 0.005
                        || (p.beat_in_bar - 1 + (steps as u64 % 4) as u16) % 4 + 1
                            != beat.beat_in_bar
                });
                if needs_anchor {
                    tempos.push(analysis::TempoAnchor {
                        seconds,
                        bpm: f64::from(beat.bpm_x100) / 100.0,
                        beat_in_bar: beat.beat_in_bar,
                    });
                }
            }
            let cues = ext
                .cue_lists
                .iter()
                .filter(|c| c.extended)
                .flat_map(|c| c.cues.clone())
                .collect();
            item.track.analysis.tempos = tempos;
            item.track.analysis.cues = cues;
            analysis::validate(&item.track.analysis)?;
            item.analysis.beat_count = analysis::beats(&item.track.analysis, &item.audio)
                .map_err(|error| invalid(format!("«{}»: {error}", item.track.title)))?
                .len();
            item.analysis.hot_cues = item
                .track
                .analysis
                .cues
                .iter()
                .filter(|c| c.hot_cue > 0)
                .count();
            item.analysis.memory_cues = item
                .track
                .analysis
                .cues
                .iter()
                .filter(|c| c.hot_cue == 0)
                .count();
            old.changed_analysis.insert(item.track.id);
        }
        item.dat_sha256 = hashes[0].clone();
        item.ext_sha256 = hashes[1].clone();
        item.three_band_sha256 = hashes[2].clone();
        item.analysis_signatures = Some(signatures);
    }
    Ok(())
}

pub fn prepare(
    mut existing: Existing,
    incoming: ExportPlan,
    bindings: BTreeMap<u32, String>,
) -> Result<Update> {
    if device_file(&existing.root, JOURNAL)?.exists() {
        return Err(invalid("Hay una actualización interrumpida. Restaura la biblioteca anterior antes de continuar."));
    }
    hydrate(&mut existing)?;
    let mut tracks: BTreeMap<_, _> = existing
        .report
        .tracks
        .iter()
        .map(|t| (t.track.id, t.track.clone()))
        .collect();
    let mut work: BTreeMap<_, _> = tracks.keys().map(|id| (*id, Work::Reuse)).collect();
    let mut allocator = paths::Allocator::default();
    for item in &existing.report.tracks {
        allocator.reserve(
            &item.track.device_path,
            &item.track.artist,
            &item.analysis.dat_path,
        )?;
    }
    reserve_contents(&existing.root, &mut allocator)?;
    let mut next_id = tracks.keys().max().copied().unwrap_or(0);
    let mut mapping = BTreeMap::new();
    let mut notices = Vec::new();
    let mut counts = SyncCounts::default();
    let mut source_metadata = BTreeMap::new();
    let mut source_preparation = BTreeMap::new();
    let mut source_aliases: BTreeMap<_, BTreeSet<_>> = existing
        .report
        .tracks
        .iter()
        .map(|item| {
            (
                item.track.id,
                item.source_aliases
                    .iter()
                    .cloned()
                    .chain([item.track.source_key.clone()])
                    .collect(),
            )
        })
        .collect();
    for selected in incoming.tracks {
        let exact = existing
            .report
            .tracks
            .iter()
            .find(|p| p.matches_source(&selected.source_key))
            .cloned();
        // Different catalogs can refer to the same original file. Only use an
        // unambiguous original-path match; the content check below still decides
        // whether audio is reused or must be exported again.
        let previous = exact.or_else(|| {
            let selected_path = selected
                .source_path
                .canonicalize()
                .unwrap_or_else(|_| selected.source_path.clone());
            let mut candidates = existing.report.tracks.iter().filter(|item| {
                item.track.source_path == selected.source_path
                    || item
                        .track
                        .source_path
                        .canonicalize()
                        .is_ok_and(|path| path == selected_path)
            });
            let candidate = candidates.next().cloned();
            if candidates.next().is_some() {
                None
            } else {
                candidate
            }
        });
        let mut track = selected.clone();
        if let Some(previous) = previous {
            track.id = previous.track.id;
            track.source_key = previous.track.source_key.clone();
            source_aliases
                .entry(track.id)
                .or_default()
                .insert(selected.source_key.clone());
            if selected.source_key != previous.track.source_key {
                notices.push(format!("«{}» corresponde al mismo archivo original de otra biblioteca; se reutiliza su identidad en el USB.", track.title));
            }
            let usb_source = device_file(&existing.root, &previous.track.device_path)?;
            let from_usb = track.source_path == usb_source;
            let signature = if from_usb {
                None
            } else {
                Some(FileSignature::read(&track.source_path)?)
            };
            let expected = previous
                .source_sha256
                .as_ref()
                .or(previous.track.analysis.source_sha256.as_ref())
                .or(if previous.track.copy_source {
                    Some(&previous.sha256)
                } else {
                    None
                });
            let same = from_usb
                || (track.source_path == previous.track.source_path
                    && signature.is_some()
                    && previous.source_signature == signature)
                || expected.is_some_and(|hash| {
                    hash_file(&track.source_path).is_ok_and(|actual| &actual == hash)
                });
            if same {
                source_metadata.insert(track.id, TrackMetadata::from_track(&selected));
                track.device_path = previous.track.device_path.clone();
                track.analysis_path = previous.track.analysis_path.clone();
                if from_usb {
                    track.source_path = previous.track.source_path.clone();
                }
                track.copy_source = previous.track.copy_source;
                if !from_usb {
                    if let Some(cached) = existing
                        .report
                        .tracks
                        .iter_mut()
                        .find(|p| p.track.id == track.id)
                    {
                        cached.source_signature = signature;
                        cached.source_sha256 = expected.cloned();
                    }
                }
                if track.analysis.tempos.is_empty() {
                    track.analysis.tempos = previous.track.analysis.tempos.clone();
                    if !track.analysis.tempos.is_empty() {
                        track
                            .analysis
                            .warnings
                            .retain(|warning| !warning.starts_with("No XML beat grid"));
                    }
                }
                if track.analysis.cues.is_empty() {
                    track.analysis.cues = previous.track.analysis.cues.clone();
                }
                if !from_usb {
                    source_preparation.insert(track.id, track.analysis.clone());
                }
                let analysis_key =
                    |a: &analysis::TrackAnalysis| serde_json::to_vec(&(&a.tempos, &a.cues));
                let changed =
                    analysis_key(&track.analysis)? != analysis_key(&previous.track.analysis)?;
                let source_unchanged =
                    previous
                        .source_preparation
                        .as_ref()
                        .is_some_and(|baseline| {
                            analysis_key(baseline).ok() == analysis_key(&track.analysis).ok()
                        });
                let metadata = TrackMetadata::from_track(&previous.track);
                let recorded = &existing.recorded_metadata[&track.id];
                let source_baseline = previous.source_metadata.as_ref().unwrap_or(recorded);
                let keep_usb_bpm = metadata.bpm_x100 != recorded.bpm_x100
                    || (track.bpm_x100 == source_baseline.bpm_x100
                        && metadata.bpm_x100 != track.bpm_x100);
                if metadata.merge(source_baseline, recorded, &mut track) {
                    notices.push(format!(
                        "Se conservan los cambios de metadata del USB para «{}».",
                        track.title
                    ));
                }
                if existing.changed_analysis.contains(&track.id)
                    || from_usb
                    || (changed && source_unchanged)
                    || keep_usb_bpm
                {
                    track.analysis = previous.track.analysis.clone();
                    track.bpm_x100 = previous.track.bpm_x100;
                    if changed {
                        notices.push(format!(
                            "Se conserva el análisis existente del USB para «{}».",
                            track.title
                        ));
                    }
                } else if changed {
                    let nonce = LibraryIdentity::new().master_database_id;
                    track.analysis_path = Some(format!(
                        "/PIONEER/USBANLZ/RAU/{nonce:08X}{:08X}/ANLZ0000.DAT",
                        track.id
                    ));
                    work.insert(track.id, Work::Analyze);
                    counts.analysis_tracks += 1;
                }
            } else {
                source_metadata.insert(track.id, TrackMetadata::from_track(&selected));
                source_preparation.insert(track.id, track.analysis.clone());
                track.device_path =
                    allocator.assign(&track.source_path, &track.title, &track.artist)?;
                track.analysis_path = None;
                work.insert(track.id, Work::Export);
                counts.updated_tracks += 1;
            }
        } else {
            next_id = next_id
                .checked_add(1)
                .ok_or_else(|| invalid("USB track ID limit"))?;
            track.id = next_id;
            track.device_path =
                allocator.assign(&track.source_path, &track.title, &track.artist)?;
            track.analysis_path = None;
            source_metadata.insert(track.id, TrackMetadata::from_track(&selected));
            source_preparation.insert(track.id, track.analysis.clone());
            work.insert(track.id, Work::Export);
            counts.new_tracks += 1;
        }
        mapping.insert(selected.id, track.id);
        tracks.insert(track.id, track);
    }
    let mut playlists = existing.report.playlists.clone();
    let mut sources = existing.report.playlist_sources.clone();
    sources.extend(bindings);
    let mut next_playlist = playlists.iter().map(|p| p.id).max().unwrap_or(0);
    let mut updated = BTreeSet::new();
    for p in incoming.playlists {
        let source = incoming
            .playlist_sources
            .get(&p.id)
            .ok_or_else(|| invalid("playlist source identity is missing"))?;
        let members = p
            .tracks
            .iter()
            .map(|id| {
                mapping
                    .get(id)
                    .copied()
                    .ok_or_else(|| invalid("missing incoming playlist track"))
            })
            .collect::<Result<Vec<_>>>()?;
        if let Some(old) = playlists
            .iter_mut()
            .find(|old| sources.get(&old.id) == Some(source))
        {
            old.name = p.name;
            old.tracks = members;
            updated.insert(old.id);
        } else {
            next_playlist = next_playlist
                .checked_add(1)
                .ok_or_else(|| invalid("playlist ID limit"))?;
            let position = playlists.iter().filter(|p| p.parent_id == 0).count() as u32;
            playlists.push(ExportPlaylist {
                id: next_playlist,
                parent_id: 0,
                position,
                name: p.name,
                is_folder: false,
                tracks: members,
            });
            sources.insert(next_playlist, source.clone());
            updated.insert(next_playlist);
        }
    }
    counts.preserved_playlists = playlists
        .iter()
        .filter(|p| !p.is_folder && !updated.contains(&p.id))
        .count();
    // Old analysis must not masquerade as output of the new renderer. Prepare
    // fresh companions from USB audio, at fresh paths, preserving rollback.
    #[cfg(unix)]
    for previous in &existing.report.tracks {
        if previous.analysis.renderer != super::engine::RENDERER
            && work.get(&previous.track.id) == Some(&Work::Reuse)
        {
            let track = tracks
                .get_mut(&previous.track.id)
                .ok_or_else(|| invalid("missing migration track"))?;
            track.analysis_path = Some(format!(
                "/PIONEER/USBANLZ/RAU/{:08X}{:08X}/ANLZ0000.DAT",
                LibraryIdentity::new().master_database_id,
                track.id
            ));
            work.insert(track.id, Work::Analyze);
            counts.analysis_tracks += 1;
        }
    }
    counts.reused_tracks = work.values().filter(|w| **w != Work::Export).count();
    let included: BTreeSet<_> = tracks
        .values()
        .map(|t| t.source_key.clone())
        .chain(
            source_aliases
                .values()
                .flat_map(|aliases| aliases.iter().cloned()),
        )
        .collect();
    let plan = ExportPlan {
        tracks: tracks.into_values().collect(),
        playlists,
        source_bytes: incoming.source_bytes,
        omitted_tracks: incoming
            .omitted_tracks
            .into_iter()
            .filter(|t| !included.contains(&t.source_key))
            .collect(),
        playlist_sources: sources,
    };
    devicesql::validate_plan(&plan)?;
    Ok(Update {
        plan,
        existing,
        work,
        counts,
        notices,
        source_metadata,
        source_preparation,
        source_aliases,
    })
}

fn reserve_contents(root: &Path, allocator: &mut paths::Allocator) -> Result<()> {
    fn walk(
        root: &Path,
        dir: &Path,
        depth: usize,
        allocator: &mut paths::Allocator,
        count: &mut usize,
    ) -> Result<()> {
        if depth > 8 {
            return Err(invalid("Contents directory nesting limit"));
        }
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let kind = entry.file_type()?;
            if kind.is_symlink() {
                return Err(invalid("USB update refuses symbolic links"));
            }
            *count += 1;
            if *count > 100_000 {
                return Err(invalid("USB file inventory limit"));
            }
            if kind.is_dir() {
                walk(root, &entry.path(), depth + 1, allocator, count)?;
            } else if kind.is_file() {
                let relative = format!(
                    "/{}",
                    entry
                        .path()
                        .strip_prefix(root)
                        .map_err(|_| invalid("unsafe device path"))?
                        .to_string_lossy()
                );
                allocator.reserve_file(&relative)?;
            }
        }
        Ok(())
    }
    let contents = device_file(root, "Contents")?;
    if contents.exists() {
        walk(root, &contents, 0, allocator, &mut 0)?;
    }
    Ok(())
}

#[derive(Serialize, Deserialize)]
struct Journal {
    backup: String,
    originals: BTreeMap<String, String>,
}
pub fn recover(root: &Path) -> Result<()> {
    let root = root.canonicalize()?;
    let journal_path = device_file(&root, JOURNAL)?;
    let journal: Journal = serde_json::from_slice(&bounded(&journal_path, 1024 * 1024)?)?;
    let backup_name = journal.backup.strip_prefix(".rau-backups/").filter(|name| {
        !name.is_empty()
            && !name.contains('/')
            && name.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
    });
    if backup_name.is_none()
        || journal.originals.len() != METADATA.len()
        || journal
            .originals
            .keys()
            .any(|p| !METADATA.contains(&p.as_str()))
    {
        return Err(invalid("invalid update journal"));
    }
    // Verify every available backup before moving any metadata.
    for (relative, expected) in &journal.originals {
        let backup = device_file(&root, &format!("{}/{relative}", journal.backup))?;
        let original = if backup.exists() {
            backup
        } else {
            device_file(&root, relative)?
        };
        if !original.exists() || hash_file(&original)? != *expected {
            return Err(invalid(
                "El respaldo del USB no supera la comprobación de integridad.",
            ));
        }
    }
    for (relative, expected) in &journal.originals {
        let backup = device_file(&root, &format!("{}/{relative}", journal.backup))?;
        let current = device_file(&root, relative)?;
        if backup.exists() {
            if current.exists() {
                let interrupted =
                    device_file(&root, &format!("{}/interrupted/{relative}", journal.backup))?;
                fs::create_dir_all(interrupted.parent().unwrap())?;
                if interrupted.exists() {
                    return Err(invalid("recovery conflict already exists"));
                }
                fs::rename(&current, interrupted)?;
            }
            fs::rename(&backup, &current)?;
        }
        if !current.exists() || hash_file(&current)? != *expected {
            return Err(invalid(
                "No se pudo restaurar la biblioteca anterior completamente.",
            ));
        }
    }
    fs::remove_file(journal_path)?;
    Ok(())
}

fn verify_existing_media(old: &Existing) -> Result<()> {
    for item in &old.report.tracks {
        let audio = FileSignature::read(&device_file(&old.root, &item.track.device_path)?)?;
        let analysis = [
            &item.analysis.dat_path,
            &item.analysis.ext_path,
            &item.analysis.three_band_path,
        ]
        .into_iter()
        .map(|p| FileSignature::read(&device_file(&old.root, p)?))
        .collect::<Result<Vec<_>>>()?;
        if item.device_signature.as_ref() != Some(&audio)
            || item.analysis_signatures.as_ref() != Some(&analysis)
        {
            return Err(invalid(format!("El audio o análisis del USB cambió después de la revisión: «{}». Vuelve a revisar.", item.track.title)));
        }
    }
    Ok(())
}

fn rewrite_staged(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

pub fn write(
    update: &Update,
    ffmpeg: &Path,
    ffprobe: &Path,
    workers: usize,
    cache: Option<&Path>,
    mut progress: impl FnMut(ExportProgress),
) -> Result<ExportReport> {
    let root = &update.existing.root;
    if revision(root)? != update.existing.revision {
        return Err(invalid(
            "El USB cambió desde la revisión. Vuelve a revisar antes de actualizar.",
        ));
    }
    if device_file(root, JOURNAL)?.exists() {
        return Err(invalid("Hay una actualización interrumpida. Restaura la biblioteca anterior antes de continuar."));
    }
    verify_existing_media(&update.existing)?;
    let local = tempfile::Builder::new()
        .prefix("rau-usb-prepare-")
        .tempdir()?;
    let stage = local.path();
    let old: BTreeMap<_, _> = update
        .existing
        .report
        .tracks
        .iter()
        .map(|t| (t.track.id, t))
        .collect();

    let mut reused_records = BTreeMap::new();
    let apply_source = |item: &mut ExportedTrack| {
        if let Some(metadata) = update.source_metadata.get(&item.track.id) {
            item.source_metadata = Some(metadata.clone());
        }
        if let Some(preparation) = update.source_preparation.get(&item.track.id) {
            item.source_preparation = Some(preparation.clone());
        }
        if let Some(aliases) = update.source_aliases.get(&item.track.id) {
            item.source_aliases = aliases
                .iter()
                .filter(|key| **key != item.track.source_key)
                .cloned()
                .collect();
        }
    };
    for track in &update.plan.tracks {
        if update.work[&track.id] == Work::Reuse {
            let mut item = (*old
                .get(&track.id)
                .ok_or_else(|| invalid("cached USB track missing"))?)
            .clone();
            item.track = track.clone();
            apply_source(&mut item);
            reused_records.insert(track.id, item);
            progress(ExportProgress::event(
                ExportStage::Audio,
                0,
                0,
                Some(track.title.clone()),
                "reuse",
                "Ya está en el pendrive; no se vuelve a procesar ni copiar",
            ));
        }
    }
    let pending: Vec<_> = update
        .plan
        .tracks
        .iter()
        .filter(|track| update.work[&track.id] != Work::Reuse)
        .cloned()
        .collect();
    let files = pending
        .iter()
        .map(|track| {
            if update.work[&track.id] == Work::Export {
                4usize
            } else {
                3
            }
        })
        .sum();
    let prepared = super::parallel::run(
        &pending,
        workers,
        reused_records.len(),
        files,
        root,
        |track, notify| {
            let action = update.work.get(&track.id).copied().unwrap_or(Work::Export);
            let mut item = match action {
                Work::Export => {
                    notify(
                        if track.copy_source { "copy" } else { "convert" },
                        &format!("{} → {}", track.source_path.display(), track.device_path),
                    );
                    super::prepare_track(track, stage, ffmpeg, ffprobe, cache, notify)
                }
                Work::Reuse => return Err(invalid("reused track entered the preparation queue")),
                Work::Analyze => {
                    notify(
                        "analysis",
                        "Actualizando el análisis; se conserva el audio del USB",
                    );
                    let mut item = (*old
                        .get(&track.id)
                        .ok_or_else(|| invalid("cached USB track missing"))?)
                    .clone();
                    let source = device_file(root, &track.device_path)?;
                    let (audio, bundle) =
                        super::cache::prepare(track, &source, ffmpeg, ffprobe, cache, notify)?;
                    for (relative, bytes) in [
                        (&bundle.summary.dat_path, &bundle.dat),
                        (&bundle.summary.ext_path, &bundle.ext),
                        (&bundle.summary.three_band_path, &bundle.three_band),
                    ] {
                        let path = device_file(stage, relative)?;
                        fs::create_dir_all(path.parent().unwrap())?;
                        write_new(&path, bytes)?;
                    }
                    item.dat_sha256 = hash_file(&device_file(stage, &bundle.summary.dat_path)?)?;
                    item.ext_sha256 = hash_file(&device_file(stage, &bundle.summary.ext_path)?)?;
                    item.three_band_sha256 =
                        hash_file(&device_file(stage, &bundle.summary.three_band_path)?)?;
                    item.analysis = bundle.summary;
                    item.audio = audio;
                    item.track = track.clone();
                    Ok(item)
                }
            }?;
            apply_source(&mut item);
            super::transfer::PreparedTrack::new(item, stage, root, action == Work::Export)
        },
        &mut progress,
    )?;
    for item in prepared {
        reused_records.insert(item.track.id, item);
    }
    let records = update
        .plan
        .tracks
        .iter()
        .map(|track| {
            reused_records
                .remove(&track.id)
                .ok_or_else(|| invalid("USB pipeline result missing"))
        })
        .collect::<Result<Vec<_>>>()?;
    let staging = tempfile::Builder::new()
        .prefix(".rau-stage-")
        .tempdir_in(root)?;
    let stage = staging.path();
    fs::create_dir_all(stage.join("PIONEER/rekordbox"))?;
    progress(ExportProgress::event(
        ExportStage::Library,
        update.plan.tracks.len(),
        update.plan.tracks.len(),
        None,
        "library",
        "Generando y comprobando ambas bibliotecas",
    ));
    let mut report = super::finish_report(
        &update.plan,
        records,
        update.existing.report.library_identity.clone(),
        stage,
    )?;
    let settings = device_file(root, "PIONEER/DEVSETTING.DAT")?;
    if settings.exists() {
        rewrite_staged(
            &stage.join("PIONEER/DEVSETTING.DAT"),
            &bounded(&settings, 1024 * 1024)?,
        )?;
        report.device_settings_sha256 = hash_file(&stage.join("PIONEER/DEVSETTING.DAT"))?;
    }
    verify_existing_media(&update.existing)?;
    progress(ExportProgress::event(
        ExportStage::Library,
        update.plan.tracks.len(),
        update.plan.tracks.len(),
        None,
        "publish",
        "Todas las copias están verificadas; preparando las bibliotecas finales",
    ));
    rewrite_staged(
        &stage.join(METADATA[2]),
        &serde_json::to_vec_pretty(&report)?,
    )?;
    progress(ExportProgress::event(
        ExportStage::Library,
        update.plan.tracks.len(),
        update.plan.tracks.len(),
        None,
        "publish",
        "Guardando la biblioteca con respaldo de la versión anterior",
    ));
    let backup = format!(
        ".rau-backups/{}-{:08X}",
        chrono::Utc::now().format("%Y%m%d%H%M%S"),
        LibraryIdentity::new().master_database_id
    );
    let backup_dir = device_file(root, &backup)?;
    fs::create_dir_all(backup_dir.parent().unwrap())?;
    fs::create_dir(backup_dir)?;
    let originals = METADATA
        .iter()
        .map(|p| Ok((p.to_string(), hash_file(&device_file(root, p)?)?)))
        .collect::<Result<BTreeMap<_, _>>>()?;
    let journal = Journal { backup, originals };
    let journal_path = device_file(root, JOURNAL)?;
    write_new(&journal_path, &serde_json::to_vec_pretty(&journal)?)?;
    let publish = (|| -> Result<()> {
        if revision(root)? != update.existing.revision {
            return Err(invalid(
                "El USB cambió durante la preparación. Se conserva la biblioteca anterior.",
            ));
        }
        verify_existing_media(&update.existing)?;
        for relative in METADATA {
            let current = device_file(root, relative)?;
            if hash_file(&current)? != journal.originals[relative] {
                return Err(invalid("USB metadata changed during publication"));
            }
            let previous = device_file(root, &format!("{}/{relative}", journal.backup))?;
            fs::create_dir_all(previous.parent().unwrap())?;
            fs::rename(&current, previous)?;
            fs::rename(device_file(stage, relative)?, current)?;
        }
        Ok(())
    })();
    if let Err(error) = publish {
        return match recover(root) {
            Ok(()) => Err(error),
            Err(recovery) => Err(invalid(format!(
                "{error}. La restauración requiere atención: {recovery}"
            ))),
        };
    }
    fs::remove_file(journal_path)?;
    progress(ExportProgress::event(
        ExportStage::Library,
        update.plan.tracks.len(),
        update.plan.tracks.len(),
        None,
        "cleanup",
        "Limpiando archivos temporales de la actualización",
    ));
    if let Err(error) = staging.close() {
        progress(ExportProgress::event(ExportStage::Library, update.plan.tracks.len(), update.plan.tracks.len(), None, "cleanup_error", &format!("La biblioteca quedó publicada, pero no se pudo completar la limpieza temporal: {error}")));
    }
    progress(ExportProgress::event(
        ExportStage::Library,
        update.plan.tracks.len(),
        update.plan.tracks.len(),
        None,
        "complete",
        "Exportación completada; bibliotecas e informe guardados",
    ));
    Ok(report)
}
