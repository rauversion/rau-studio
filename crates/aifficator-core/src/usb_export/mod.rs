//! Experimental USB-library preparation. The common model is independent of a
//! player model and of the on-device database format. See docs/usb-export.md.

pub mod analysis;
pub mod anlz;
mod audio;
mod browse;
mod cache;
mod catalog;
pub mod devicesql;
mod engine;
pub mod incremental;
mod media;
pub mod onelibrary;
mod parallel;
mod paths;
mod settings;
mod source;
mod transfer;

use crate::rekordbox::{PlaylistNode, RekordboxLibrary};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use thiserror::Error;

pub use audio::read_pcm_wave;
pub use source::{check_source, SourceFailure, SourceIssueKind, UnavailableTrack};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AudioFormat {
    Mp3,
    Aiff,
    Wave,
}

impl AudioFormat {
    pub fn from_path(path: &Path) -> Option<Self> {
        match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
            "mp3" => Some(Self::Mp3),
            "aif" | "aiff" => Some(Self::Aiff),
            "wav" => Some(Self::Wave),
            _ => None,
        }
    }

    pub fn device_code(self) -> u16 {
        match self {
            Self::Mp3 => 1,
            Self::Aiff => 12,
            Self::Wave => 11,
        }
    }
}

pub fn preserves_source_audio(path: &Path) -> bool {
    matches!(
        AudioFormat::from_path(path),
        Some(AudioFormat::Mp3 | AudioFormat::Aiff)
    )
}

#[derive(Debug, Error)]
pub enum UsbExportError {
    #[error("{0}")]
    Invalid(String),
    #[error("export destination already contains {0}")]
    DestinationConflict(String),
    #[error("filesystem error: {0}")]
    Io(#[from] std::io::Error),
    #[error("report error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("library database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("Rekordbox USB: {0}")]
    Rekordbox(#[from] rekordbox_usb::Error),
}
pub type Result<T> = std::result::Result<T, UsbExportError>;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct LibraryIdentity {
    pub name: String,
    pub master_database_id: u32,
}

impl LibraryIdentity {
    pub fn new() -> Self {
        let digest = Sha256::digest(format!(
            "{}:{}",
            chrono::Utc::now().to_rfc3339(),
            std::process::id()
        ));
        Self {
            name: "Rau Studio".into(),
            master_database_id: u32::from_le_bytes(digest[..4].try_into().unwrap()).max(1),
        }
    }
}

impl Default for LibraryIdentity {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AudioInfo {
    pub format: AudioFormat,
    pub bitrate_kbps: u32,
    pub sample_rate: u32,
    pub channels: u16,
    pub bit_depth: u16,
    pub frames: u64,
    pub size_bytes: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExportTrack {
    pub id: u32,
    pub source_key: String,
    pub source_path: PathBuf,
    pub device_path: String,
    #[serde(default)]
    pub copy_source: bool,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub genre: String,
    pub label: String,
    pub key: String,
    pub comment: String,
    pub bpm_x100: u32,
    pub rating: u8,
    pub year: u16,
    pub track_number: u32,
    #[serde(default)]
    pub analysis: analysis::TrackAnalysis,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub analysis_path: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExportPlaylist {
    pub id: u32,
    pub parent_id: u32,
    pub position: u32,
    pub name: String,
    pub is_folder: bool,
    pub tracks: Vec<u32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExportPlan {
    pub tracks: Vec<ExportTrack>,
    pub playlists: Vec<ExportPlaylist>,
    pub source_bytes: u64,
    /// Excluded from all media/playlist references after explicit UI consent.
    #[serde(default)]
    pub omitted_tracks: Vec<UnavailableTrack>,
    #[serde(default)]
    pub playlist_sources: BTreeMap<u32, String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExportReport {
    pub schema_version: u32,
    #[serde(default)]
    pub engine: String,
    pub format: String,
    pub hardware_validated: bool,
    pub created_at: String,
    pub database_sha256: String,
    pub one_library_sha256: String,
    pub library_identity: LibraryIdentity,
    pub device_settings_sha256: String,
    pub tracks: Vec<ExportedTrack>,
    pub playlists: Vec<ExportPlaylist>,
    #[serde(default)]
    pub omitted_tracks: Vec<UnavailableTrack>,
    #[serde(default)]
    pub playlist_sources: BTreeMap<u32, String>,
    pub limitations: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExportedTrack {
    pub track: ExportTrack,
    pub audio: AudioInfo,
    pub sha256: String,
    pub analysis: anlz::AnalysisSummary,
    pub dat_sha256: String,
    pub ext_sha256: String,
    pub three_band_sha256: String,
    #[serde(default)]
    pub source_sha256: Option<String>,
    #[serde(default)]
    pub source_signature: Option<incremental::FileSignature>,
    #[serde(default)]
    pub device_signature: Option<incremental::FileSignature>,
    #[serde(default)]
    pub analysis_signatures: Option<Vec<incremental::FileSignature>>,
    #[serde(default)]
    pub source_metadata: Option<incremental::TrackMetadata>,
    #[serde(default)]
    pub source_preparation: Option<analysis::TrackAnalysis>,
    #[serde(default)]
    pub source_aliases: Vec<String>,
}

impl ExportedTrack {
    pub fn matches_source(&self, key: &str) -> bool {
        self.track.source_key == key || self.source_aliases.iter().any(|alias| alias == key)
    }
}

pub fn available_export_workers() -> usize {
    transfer::available_workers()
}

fn invalid(message: impl Into<String>) -> UsbExportError {
    UsbExportError::Invalid(message.into())
}

/// Snapshot a selection without writing media. Empty selection is an error;
/// callers must name a playlist or folder explicitly (including ROOT for all).
pub fn plan_library(library: &RekordboxLibrary, selected: &[String]) -> Result<ExportPlan> {
    if selected.is_empty() {
        return Err(invalid("select at least one playlist or folder"));
    }
    let available = library.playlists_flat();
    for path in selected {
        if !available.iter().any(|p| p.path == *path) {
            return Err(invalid(format!("playlist not found: {path}")));
        }
    }
    let mut tracks_by_key = BTreeMap::new();
    for track in &library.tracks {
        if tracks_by_key
            .insert(track.track_id.as_str(), track)
            .is_some()
        {
            return Err(invalid(format!(
                "duplicate source TrackID: {}",
                track.track_id
            )));
        }
    }
    let mut nodes = Vec::new();
    fn visit<'a>(
        node: &'a PlaylistNode,
        parent_path: &str,
        selected: &[String],
        inherited: bool,
        parent_id: u32,
        nodes: &mut Vec<(&'a PlaylistNode, u32, u32)>,
    ) {
        let path = if parent_path.is_empty() {
            node.name.clone()
        } else {
            format!("{parent_path}/{}", node.name)
        };
        let include = inherited || selected.contains(&path);
        let id = if include {
            let id = nodes.len() as u32 + 1;
            nodes.push((node, id, parent_id));
            id
        } else {
            parent_id
        };
        for child in &node.children {
            visit(child, &path, selected, include, id, nodes);
        }
    }
    for node in &library.playlists {
        visit(node, "", selected, false, 0, &mut nodes);
    }
    if nodes.len() > 10_000 {
        return Err(invalid("prototype limit: 10000 playlist nodes"));
    }
    let keys: BTreeSet<_> = nodes
        .iter()
        .flat_map(|(n, _, _)| n.track_keys.iter())
        .collect();
    if keys.is_empty() || keys.len() > 10_000 {
        return Err(invalid(
            "select between 1 and 10000 distinct tracks for this prototype",
        ));
    }
    let mut tracks = Vec::new();
    let mut ids = BTreeMap::new();
    let mut source_bytes = 0u64;
    let mut device_paths = paths::Allocator::default();
    for key in keys {
        let source = tracks_by_key
            .get(key.as_str())
            .ok_or_else(|| invalid(format!("missing playlist TrackID: {key}")))?;
        let source_path = source
            .file_path
            .as_ref()
            .ok_or_else(|| invalid(format!("track {key} has no local file")))?
            .canonicalize()?;
        let metadata = fs::metadata(&source_path)?;
        if !metadata.is_file() || metadata.len() == 0 {
            return Err(invalid(format!(
                "not a nonempty audio file: {}",
                source_path.display()
            )));
        }
        File::open(&source_path)?;
        source_bytes = source_bytes
            .checked_add(metadata.len())
            .ok_or_else(|| invalid("source byte count overflow"))?;
        let id = tracks.len() as u32 + 1;
        ids.insert(key.clone(), id);
        let attr = |key: &str| source.attributes.get(key).cloned().unwrap_or_default();
        let bpm = attr("AverageBpm").parse::<f64>().unwrap_or(0.0);
        if !bpm.is_finite() || !(0.0..=999.99).contains(&bpm) {
            return Err(invalid(format!("invalid BPM on track {key}")));
        }
        tracks.push(ExportTrack {
            id,
            source_key: key.clone(),
            copy_source: preserves_source_audio(
                source.file_path.as_deref().unwrap_or(&source_path),
            ),
            device_path: device_paths.assign(
                source.file_path.as_deref().unwrap_or(&source_path),
                source.name.as_deref().unwrap_or(key),
                source.artist.as_deref().unwrap_or_default(),
            )?,
            source_path,
            title: source.name.clone().unwrap_or_else(|| key.clone()),
            artist: source.artist.clone().unwrap_or_default(),
            album: source.album.clone().unwrap_or_default(),
            genre: attr("Genre"),
            label: attr("Label"),
            key: attr("Tonality"),
            comment: attr("Comments"),
            bpm_x100: (bpm * 100.0).round() as u32,
            rating: (attr("Rating").parse::<u16>().unwrap_or(0).min(255) / 51) as u8,
            year: attr("Year").parse().unwrap_or(0),
            track_number: attr("TrackNumber").parse().unwrap_or(0),
            analysis: analysis::from_xml(source)
                .map_err(|e| invalid(format!("track {key}: {e}")))?,
            analysis_path: None,
        });
    }
    let mut sibling_counts = BTreeMap::<u32, u32>::new();
    let playlists = nodes
        .into_iter()
        .map(|(node, id, parent_id)| {
            let position = sibling_counts.entry(parent_id).or_default();
            let playlist = ExportPlaylist {
                id,
                parent_id,
                position: *position,
                name: node.name.clone(),
                is_folder: node.node_type.as_deref() == Some("0"),
                tracks: node.track_keys.iter().map(|key| ids[key]).collect(),
            };
            *position += 1;
            playlist
        })
        .collect();
    let plan = ExportPlan {
        tracks,
        playlists,
        source_bytes,
        omitted_tracks: Vec::new(),
        playlist_sources: BTreeMap::new(),
    };
    devicesql::validate_plan(&plan)?;
    Ok(plan)
}

/// CLI convenience wrapper that creates a NEW directory. For an existing folder
/// or USB root, use write_classic_destination_with_progress instead.
pub fn write_classic_folder(
    plan: &ExportPlan,
    output: &Path,
    ffmpeg: &Path,
) -> Result<ExportReport> {
    write_classic_folder_with_progress(plan, output, ffmpeg, |_| {})
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExportStage {
    Audio,
    Analysis,
    Library,
}

#[derive(Clone, Debug, Serialize)]
pub struct PublicationProgress {
    pub completed_files: usize,
    pub total_files: usize,
    /// Payload bytes whose destination copy has been flushed and verified.
    pub completed_bytes: u64,
    pub total_bytes: u64,
    pub current_file_bytes: u64,
    /// Bytes copied or read back in the current phase of the current file.
    pub current_file_processed_bytes: u64,
    pub total_bytes_known: bool,
    pub current_track: Option<String>,
    pub current_file: Option<String>,
    pub phase: String,
    pub queued_tracks: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct PreparationProgress {
    pub completed_tracks: usize,
    pub total_tracks: usize,
    pub reused_tracks: usize,
    pub workers: usize,
    pub active_tracks: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ExportProgress {
    pub stage: ExportStage,
    pub completed_tracks: usize,
    pub total_tracks: usize,
    pub current_track: Option<String>,
    pub action: Option<String>,
    pub detail: Option<String>,
    pub publication: Option<PublicationProgress>,
    pub preparation: Option<PreparationProgress>,
}

impl ExportProgress {
    pub fn event(
        stage: ExportStage,
        completed_tracks: usize,
        total_tracks: usize,
        current_track: Option<String>,
        action: &str,
        detail: &str,
    ) -> Self {
        Self {
            stage,
            completed_tracks,
            total_tracks,
            current_track,
            action: Some(action.into()),
            detail: Some(detail.into()),
            publication: None,
            preparation: None,
        }
    }

    pub fn with_publication(mut self, publication: PublicationProgress) -> Self {
        self.publication = Some(publication);
        self
    }
}

/// Track-level progress; callers must not interpret this as byte/time progress.
pub fn write_classic_folder_with_progress(
    plan: &ExportPlan,
    output: &Path,
    ffmpeg: &Path,
    progress: impl FnMut(ExportProgress),
) -> Result<ExportReport> {
    devicesql::validate_plan(plan)?;
    fs::create_dir(output)?; // Atomic refusal of existing directories/symlinks.
    let ffprobe = std::env::var_os("RAU_FFPROBE")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            ffmpeg.with_file_name(if cfg!(windows) {
                "ffprobe.exe"
            } else {
                "ffprobe"
            })
        });
    write_classic_destination_with_progress(plan, output, ffmpeg, &ffprobe, progress)
}

/// Read-only preflight. Other files (including volume metadata) may coexist;
/// export-owned paths must be absent, including symlinks and case variants.
pub fn validate_classic_destination(output: &Path) -> Result<PathBuf> {
    let output = output.canonicalize()?;
    if !output.is_dir() {
        return Err(invalid("export destination must be a directory"));
    }
    let reserved = [
        "PIONEER",
        "Contents",
        "rau-export-report.json",
        "RAU_EXPORT_INCOMPLETE.txt",
    ];
    for entry in fs::read_dir(&output)? {
        let name = entry?.file_name().to_string_lossy().into_owned();
        if reserved.iter().any(|r| name.eq_ignore_ascii_case(r)) {
            return Err(UsbExportError::DestinationConflict(name));
        }
    }
    Ok(output)
}

/// Export directly into an existing destination. Never replaces a library or
/// follows pre-existing export subdirectories. The marker remains on failure;
/// the database is written only after audio and analysis are complete.
pub fn write_classic_destination_with_progress(
    plan: &ExportPlan,
    output: &Path,
    ffmpeg: &Path,
    ffprobe: &Path,
    progress: impl FnMut(ExportProgress),
) -> Result<ExportReport> {
    write_destination_parallel(
        plan,
        output,
        ffmpeg,
        ffprobe,
        available_export_workers(),
        progress,
    )
}

pub fn write_destination_parallel(
    plan: &ExportPlan,
    output: &Path,
    ffmpeg: &Path,
    ffprobe: &Path,
    workers: usize,
    progress: impl FnMut(ExportProgress),
) -> Result<ExportReport> {
    write_destination_cached(plan, output, ffmpeg, ffprobe, workers, None, progress)
}

pub fn write_destination_cached(
    plan: &ExportPlan,
    output: &Path,
    ffmpeg: &Path,
    ffprobe: &Path,
    workers: usize,
    cache: Option<&Path>,
    mut progress: impl FnMut(ExportProgress),
) -> Result<ExportReport> {
    devicesql::validate_plan(plan)?;
    let output = validate_classic_destination(output)?;
    let marker = output.join("RAU_EXPORT_INCOMPLETE.txt");
    write_new(
        &marker,
        b"Incomplete experimental export. Do not use this library on a player.\n",
    )?;
    // Reserve each top-level directory atomically; create_dir_all here could
    // silently merge into another export created after the preflight.
    fs::create_dir(output.join("Contents"))?;
    fs::create_dir(output.join("PIONEER"))?;
    fs::create_dir(output.join("PIONEER/rekordbox"))?;
    let local = tempfile::Builder::new()
        .prefix("rau-usb-prepare-")
        .tempdir()?;
    let exported = parallel::run(
        &plan.tracks,
        workers,
        0,
        plan.tracks.len() * 4,
        &output,
        |track, notify| {
            notify(
                if track.copy_source { "copy" } else { "convert" },
                &format!("{} → {}", track.source_path.display(), track.device_path),
            );
            let record = prepare_track(track, local.path(), ffmpeg, ffprobe, cache, notify)?;
            transfer::PreparedTrack::new(record, local.path(), &output, true)
        },
        &mut progress,
    )?;
    progress(ExportProgress::event(
        ExportStage::Library,
        plan.tracks.len(),
        plan.tracks.len(),
        None,
        "library",
        "Generando y comprobando ambas bibliotecas",
    ));
    let report = finish_report(plan, exported, LibraryIdentity::new(), &output)?;
    fs::remove_file(marker)?;
    progress(ExportProgress::event(
        ExportStage::Library,
        plan.tracks.len(),
        plan.tracks.len(),
        None,
        "complete",
        "Exportación completada; bibliotecas e informe guardados",
    ));
    Ok(report)
}

pub(super) fn prepare_track(
    track: &ExportTrack,
    output: &Path,
    ffmpeg: &Path,
    ffprobe: &Path,
    cache: Option<&Path>,
    notify: &dyn Fn(&str, &str),
) -> Result<ExportedTrack> {
    let destination = output.join(track.device_path.trim_start_matches('/'));
    fs::create_dir_all(destination.parent().unwrap())?;
    let source_signature = incremental::FileSignature::read(&track.source_path)?;
    let before = hash_file(&track.source_path)?;
    if track
        .analysis
        .source_sha256
        .as_ref()
        .is_some_and(|expected| expected != &before)
    {
        return Err(invalid(format!("El audio de «{}» cambió desde su preparación musical. Vuelve a analizar y guardar la pista.", track.title)));
    }
    media::export(track, &destination, ffmpeg)?;
    if before != hash_file(&track.source_path)?
        || source_signature != incremental::FileSignature::read(&track.source_path)?
    {
        return Err(invalid(format!(
            "source changed during export: {}",
            track.source_path.display()
        )));
    }
    if track.copy_source && before != hash_file(&destination)? {
        return Err(invalid(format!(
            "copied audio differs from source: {}",
            track.title
        )));
    }
    File::open(&destination)?.sync_all()?;
    let (audio, bundle) = cache::prepare(track, &destination, ffmpeg, ffprobe, cache, notify)?;
    if !track.copy_source
        && (audio.channels != 2 || audio.sample_rate != 44_100 || audio.bit_depth != 16)
    {
        return Err(invalid(
            "unexpected conversion output; expected stereo PCM16 at 44100 Hz",
        ));
    }
    let dat_path = output.join(bundle.summary.dat_path.trim_start_matches('/'));
    let ext_path = output.join(bundle.summary.ext_path.trim_start_matches('/'));
    let three_band_path = output.join(bundle.summary.three_band_path.trim_start_matches('/'));
    fs::create_dir_all(dat_path.parent().unwrap())?;
    for (path, bytes) in [
        (&dat_path, &bundle.dat),
        (&ext_path, &bundle.ext),
        (&three_band_path, &bundle.three_band),
    ] {
        write_new(path, bytes)?;
        let reread = fs::read(path)?;
        if &reread != bytes {
            return Err(invalid("analysis read-back differs from prepared bytes"));
        }
        anlz::inspect(&reread)?;
    }
    Ok(ExportedTrack {
        track: track.clone(),
        audio,
        sha256: hash_file(&destination)?,
        source_sha256: Some(before),
        source_signature: Some(source_signature),
        device_signature: Some(incremental::FileSignature::read(&destination)?),
        analysis_signatures: Some(vec![
            incremental::FileSignature::read(&dat_path)?,
            incremental::FileSignature::read(&ext_path)?,
            incremental::FileSignature::read(&three_band_path)?,
        ]),
        source_metadata: Some(incremental::TrackMetadata::from_track(track)),
        source_preparation: Some(track.analysis.clone()),
        source_aliases: Vec::new(),
        analysis: bundle.summary,
        dat_sha256: hash_file(&dat_path)?,
        ext_sha256: hash_file(&ext_path)?,
        three_band_sha256: hash_file(&three_band_path)?,
    })
}

pub(super) fn finish_report(
    plan: &ExportPlan,
    exported: Vec<ExportedTrack>,
    identity: LibraryIdentity,
    output: &Path,
) -> Result<ExportReport> {
    let created = chrono::Utc::now();
    let audio: Vec<_> = exported.iter().map(|track| track.audio.clone()).collect();
    let date = created.format("%Y-%m-%d").to_string();
    let database = engine::database(plan, &audio, &date, &identity)?;
    let inspected = devicesql::inspect(&database)?;
    devicesql::compare(plan, &audio, &inspected)?;
    // Finish and close SQLCipher locally before publishing either library.
    // The USB gets a self-contained encrypted file without WAL/journal files.
    let staging = tempfile::tempdir()?;
    let prepared_plus = staging.path().join("exportLibrary.db");
    let cues = exported
        .iter()
        .map(|t| (t.track.id, t.analysis.plus_cues.clone()))
        .collect();
    onelibrary::write_with_cues(&prepared_plus, plan, &audio, &date, &identity, &cues)?;
    onelibrary::compare_classic(&inspected, &onelibrary::inspect(&prepared_plus)?)?;
    let plus_path = output.join("PIONEER/rekordbox/exportLibrary.db");
    write_new(&plus_path, &fs::read(&prepared_plus)?)?;
    let plus_inspected = onelibrary::inspect(&plus_path)?;
    onelibrary::compare(plan, &audio, &identity, &plus_inspected)?;
    let db_path = output.join("PIONEER/rekordbox/export.pdb");
    write_new(&db_path, &database)?;
    let reread = fs::read(&db_path)?;
    if reread != database {
        return Err(invalid("database read-back differs from prepared bytes"));
    }
    onelibrary::compare_classic(&devicesql::inspect(&reread)?, &plus_inspected)?;
    let settings_path = output.join("PIONEER/DEVSETTING.DAT");
    write_new(&settings_path, &settings::rgb_device_settings())?;
    settings::verify_rgb_device_settings(&fs::read(&settings_path)?)?;
    let report = ExportReport {
        schema_version: 8, engine: engine::RENDERER.into(), format: "devicesql-and-onelibrary-experimental".into(), hardware_validated: false,
        created_at: created.to_rfc3339(), database_sha256: hash_file(&db_path)?,
        one_library_sha256: hash_file(&plus_path)?, library_identity: identity,
        device_settings_sha256: hash_file(&settings_path)?,
        tracks: exported, playlists: plan.playlists.clone(), omitted_tracks: plan.omitted_tracks.clone(), playlist_sources: plan.playlist_sources.clone(),
        limitations: vec!["Classic Device Library and SQLCipher 4 OneLibrary are generated by Rau from the same export plan; OneLibrary hardware acceptance remains pending".into(),
            if cfg!(unix) { "Cues and loops are written in ANLZ and OneLibrary; hot-cue bank lists, My Tags, histories and recommendations are not exported" } else { "Windows retains the legacy ANLZ renderer; OneLibrary cue rows are not generated for new analysis" }.into(),
            "ANLZ DAT/EXT/2EX contain monochrome, RGB and three-band waveforms plus XML or explicitly saved Rau grids and imported cues; no PQT2, phrase/vocal analysis or artwork".into(),
            "On macOS/Linux, classic, RGB and three-band waveforms and MP3 seek use rekordbox-usb; integration acceptance on players remains pending".into(),
            "Saved Rau analysis uses a manually reviewable constant-tempo 4/4 grid; tempo changes need imported XML grids. Ten memory cues and sixteen hot slots are accepted; each player's available slots differ".into(),
            "On macOS/Linux, cue comments and explicit RGB are stored in EXT; palette indices, active-loop flags and quantized-loop fractions are not inferred from XML".into(),
            "MP3 uses the bundled FFmpeg decoder with gapless trimming disabled, not rekordbox-usb's optional mpg123 loader; source/player alignment still needs validation".into(),
            "MP3 and AIFF sources are copied unchanged; other sources are converted to stereo PCM16 AIFF at 44100 Hz; player support for original audio parameters remains unverified".into(),
            "Previous Rau diagnostic acceptance does not validate this new engine; full export and player acceptance remain unverified".into(),
            "Classic browse category/sort defaults are generated independently; player acceptance is pending".into()],
    };
    write_new(
        &output.join("rau-export-report.json"),
        &serde_json::to_vec_pretty(&report)?,
    )?;
    Ok(report)
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = File::create_new(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

pub fn hash_file(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0; 65536];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}
