use crate::rekordbox::{RekordboxLibrary, Track};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum IssueSeverity {
    Error,
    Warning,
    Info,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum IssueCode {
    MissingLocation,
    InvalidLocation,
    FileNotFound,
    CannotReadFile,
    UnsupportedFormat,
    AlreadyAiff,
    TargetAlreadyExists,
    DuplicateSource,
    MissingPlaylistTrack,
    TargetCollision,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TrackIssue {
    pub severity: IssueSeverity,
    pub code: IssueCode,
    pub track_id: Option<String>,
    pub playlist_path: Option<String>,
    pub source_path: Option<PathBuf>,
    pub message: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ValidationReport {
    pub tracks_total: usize,
    pub playlists_total: usize,
    pub convert_candidates: usize,
    pub already_aiff: usize,
    pub missing_files: usize,
    pub unreadable_files: usize,
    pub unsupported_tracks: usize,
    pub duplicate_sources: usize,
    pub playlist_reference_errors: usize,
    pub format_counts: BTreeMap<String, usize>,
    pub issues: Vec<TrackIssue>,
}

pub fn validate_library(library: &RekordboxLibrary) -> ValidationReport {
    let mut report = ValidationReport {
        tracks_total: library.tracks.len(),
        playlists_total: library.playlists_flat().len(),
        format_counts: library.format_counts(),
        ..ValidationReport::default()
    };
    let mut seen_sources = BTreeSet::new();
    let mut duplicate_sources = BTreeSet::new();

    for track in &library.tracks {
        let issues = validate_track(track);

        match track_action(track) {
            TrackAction::Convert => report.convert_candidates += 1,
            TrackAction::AlreadyAiff => report.already_aiff += 1,
            TrackAction::Unsupported => report.unsupported_tracks += 1,
        }

        if let Some(source_path) = &track.file_path {
            if !seen_sources.insert(source_path.clone()) {
                duplicate_sources.insert(source_path.clone());
            }
        }

        for issue in issues {
            match issue.code {
                IssueCode::FileNotFound => report.missing_files += 1,
                IssueCode::CannotReadFile => report.unreadable_files += 1,
                _ => {}
            }
            report.issues.push(issue);
        }
    }

    for duplicate in duplicate_sources {
        report.duplicate_sources += 1;
        report.issues.push(TrackIssue {
            severity: IssueSeverity::Warning,
            code: IssueCode::DuplicateSource,
            track_id: None,
            playlist_path: None,
            source_path: Some(duplicate.clone()),
            message: format!(
                "Mas de un track del XML apunta al mismo archivo: {}",
                duplicate.display()
            ),
        });
    }

    let track_ids: BTreeSet<_> = library
        .tracks
        .iter()
        .map(|track| track.track_id.as_str())
        .collect();
    for playlist in library.playlists_flat() {
        for key in playlist.track_keys {
            if !track_ids.contains(key.as_str()) {
                report.playlist_reference_errors += 1;
                report.issues.push(TrackIssue {
                    severity: IssueSeverity::Error,
                    code: IssueCode::MissingPlaylistTrack,
                    track_id: Some(key.clone()),
                    playlist_path: Some(playlist.path.clone()),
                    source_path: None,
                    message: format!(
                        "La playlist '{}' referencia un TrackID que no existe en COLLECTION: {}",
                        playlist.path, key
                    ),
                });
            }
        }
    }

    report.issues.sort_by(|left, right| {
        left.severity
            .cmp(&right.severity)
            .then_with(|| left.code.cmp(&right.code))
            .then_with(|| left.track_id.cmp(&right.track_id))
    });

    report
}

pub fn validate_track(track: &Track) -> Vec<TrackIssue> {
    let mut issues = Vec::new();

    let Some(location) = &track.location else {
        issues.push(TrackIssue {
            severity: IssueSeverity::Error,
            code: IssueCode::MissingLocation,
            track_id: Some(track.track_id.clone()),
            playlist_path: None,
            source_path: None,
            message: format!("El track {} no tiene Location en el XML", track.track_id),
        });
        return issues;
    };

    let Some(source_path) = &track.file_path else {
        issues.push(TrackIssue {
            severity: IssueSeverity::Error,
            code: IssueCode::InvalidLocation,
            track_id: Some(track.track_id.clone()),
            playlist_path: None,
            source_path: None,
            message: format!(
                "Location invalida para track {}: {}",
                track.track_id, location
            ),
        });
        return issues;
    };

    match fs::metadata(source_path) {
        Ok(metadata) => {
            if !metadata.is_file() {
                issues.push(TrackIssue {
                    severity: IssueSeverity::Error,
                    code: IssueCode::CannotReadFile,
                    track_id: Some(track.track_id.clone()),
                    playlist_path: None,
                    source_path: Some(source_path.clone()),
                    message: format!(
                        "El path no es un archivo regular: {}",
                        source_path.display()
                    ),
                });
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            issues.push(TrackIssue {
                severity: IssueSeverity::Error,
                code: IssueCode::FileNotFound,
                track_id: Some(track.track_id.clone()),
                playlist_path: None,
                source_path: Some(source_path.clone()),
                message: format!("Archivo no encontrado: {}", source_path.display()),
            });
        }
        Err(error) => {
            issues.push(TrackIssue {
                severity: IssueSeverity::Error,
                code: IssueCode::CannotReadFile,
                track_id: Some(track.track_id.clone()),
                playlist_path: None,
                source_path: Some(source_path.clone()),
                message: format!(
                    "No se pudo leer metadata de {}: {}",
                    source_path.display(),
                    error
                ),
            });
        }
    }

    match track_action(track) {
        TrackAction::AlreadyAiff => {
            issues.push(TrackIssue {
                severity: IssueSeverity::Info,
                code: IssueCode::AlreadyAiff,
                track_id: Some(track.track_id.clone()),
                playlist_path: None,
                source_path: Some(source_path.clone()),
                message: format!("El track ya es AIFF: {}", source_path.display()),
            });
        }
        TrackAction::Unsupported => {
            issues.push(TrackIssue {
                severity: IssueSeverity::Error,
                code: IssueCode::UnsupportedFormat,
                track_id: Some(track.track_id.clone()),
                playlist_path: None,
                source_path: Some(source_path.clone()),
                message: format!(
                    "Formato no soportado para conversion: kind={:?}, extension={:?}",
                    track.kind,
                    track.extension_lower()
                ),
            });
        }
        TrackAction::Convert => {
            let target_path = default_target_path(source_path);
            if target_path.exists() {
                issues.push(TrackIssue {
                    severity: IssueSeverity::Info,
                    code: IssueCode::TargetAlreadyExists,
                    track_id: Some(track.track_id.clone()),
                    playlist_path: None,
                    source_path: Some(source_path.clone()),
                    message: format!("Ya existe un AIFF convertido: {}", target_path.display()),
                });
            }
        }
    }

    issues
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrackAction {
    Convert,
    AlreadyAiff,
    Unsupported,
}

pub fn track_action(track: &Track) -> TrackAction {
    if is_aiff(track) {
        return TrackAction::AlreadyAiff;
    }

    if is_convertible(track) {
        return TrackAction::Convert;
    }

    TrackAction::Unsupported
}

pub fn is_aiff(track: &Track) -> bool {
    let kind = track
        .kind
        .as_deref()
        .unwrap_or_default()
        .to_ascii_lowercase();
    let extension = track.extension_lower().unwrap_or_default();

    kind.contains("aiff") || matches!(extension.as_str(), "aif" | "aiff")
}

pub fn is_convertible(track: &Track) -> bool {
    let kind = track
        .kind
        .as_deref()
        .unwrap_or_default()
        .to_ascii_lowercase();
    let extension = track.extension_lower().unwrap_or_default();

    matches!(
        extension.as_str(),
        "flac" | "mp3" | "wav" | "wave" | "m4a" | "alac" | "aac"
    ) || kind.contains("flac")
        || kind.contains("mp3")
        || kind.contains("wav")
        || kind.contains("wave")
        || kind.contains("alac")
        || kind.contains("m4a")
        || kind.contains("aac")
}

pub fn default_target_path(source_path: &Path) -> PathBuf {
    let parent = source_path.parent().unwrap_or_else(|| Path::new("."));
    let stem = source_path
        .file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("converted-track");

    parent.join("converted").join(format!("{stem}.aiff"))
}
