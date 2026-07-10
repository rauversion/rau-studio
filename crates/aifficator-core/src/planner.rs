use crate::rekordbox::{PlaylistSummary, RekordboxLibrary, Track};
use crate::validation::{
    default_target_path, track_action, validate_track, IssueCode, IssueSeverity, TrackAction,
    TrackIssue,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PlanOptions {
    pub playlist_paths: Vec<String>,
    pub reuse_existing: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConversionPlan {
    pub selected_playlist_paths: Vec<String>,
    pub playlists_total: usize,
    pub referenced_tracks_total: usize,
    pub unique_tracks_total: usize,
    pub convert_total: usize,
    pub reuse_existing_total: usize,
    pub skipped_total: usize,
    pub blocked_total: usize,
    pub items: Vec<ConversionPlanItem>,
    pub issues: Vec<TrackIssue>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConversionPlanItem {
    pub track_id: String,
    pub name: Option<String>,
    pub artist: Option<String>,
    pub kind: Option<String>,
    pub source_path: Option<PathBuf>,
    pub target_path: Option<PathBuf>,
    pub action: PlanAction,
    pub issues: Vec<TrackIssue>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PlanAction {
    Convert,
    ReuseExisting,
    SkipAlreadyAiff,
    Blocked,
}

pub fn build_conversion_plan(library: &RekordboxLibrary, options: PlanOptions) -> ConversionPlan {
    let playlists = library.playlists_flat();
    let selected = selected_playlists(&playlists, &options.playlist_paths);
    let track_index = library.track_by_id();

    let mut referenced_track_ids = Vec::new();
    let mut issues = Vec::new();

    for playlist in &selected {
        for key in &playlist.track_keys {
            if track_index.contains_key(key) {
                referenced_track_ids.push(key.clone());
            } else {
                issues.push(TrackIssue {
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

    let mut seen = BTreeSet::new();
    let unique_track_ids = referenced_track_ids
        .iter()
        .filter(|track_id| seen.insert((*track_id).clone()))
        .cloned()
        .collect::<Vec<_>>();

    let mut items = Vec::new();
    for track_id in &unique_track_ids {
        if let Some(track) = track_index.get(track_id) {
            items.push(plan_item_for_track(track, options.reuse_existing));
        }
    }

    let mut target_to_track = BTreeMap::<PathBuf, String>::new();
    for item in &mut items {
        if item.action == PlanAction::Convert || item.action == PlanAction::ReuseExisting {
            if let Some(target_path) = &item.target_path {
                if let Some(previous_track_id) =
                    target_to_track.insert(target_path.clone(), item.track_id.clone())
                {
                    let issue = TrackIssue {
                        severity: IssueSeverity::Error,
                        code: IssueCode::TargetCollision,
                        track_id: Some(item.track_id.clone()),
                        playlist_path: None,
                        source_path: item.source_path.clone(),
                        message: format!(
                            "Colision de salida: tracks {} y {} apuntan a {}",
                            previous_track_id,
                            item.track_id,
                            target_path.display()
                        ),
                    };
                    item.action = PlanAction::Blocked;
                    item.issues.push(issue.clone());
                    issues.push(issue);
                }
            }
        }
    }

    let convert_total = items
        .iter()
        .filter(|item| item.action == PlanAction::Convert)
        .count();
    let reuse_existing_total = items
        .iter()
        .filter(|item| item.action == PlanAction::ReuseExisting)
        .count();
    let skipped_total = items
        .iter()
        .filter(|item| item.action == PlanAction::SkipAlreadyAiff)
        .count();
    let blocked_total = items
        .iter()
        .filter(|item| item.action == PlanAction::Blocked)
        .count();

    ConversionPlan {
        selected_playlist_paths: selected
            .iter()
            .map(|playlist| playlist.path.clone())
            .collect(),
        playlists_total: selected.len(),
        referenced_tracks_total: referenced_track_ids.len(),
        unique_tracks_total: unique_track_ids.len(),
        convert_total,
        reuse_existing_total,
        skipped_total,
        blocked_total,
        items,
        issues,
    }
}

fn selected_playlists<'a>(
    playlists: &'a [PlaylistSummary],
    requested_paths: &[String],
) -> Vec<&'a PlaylistSummary> {
    let requested = requested_paths.iter().collect::<BTreeSet<_>>();

    playlists
        .iter()
        .filter(|playlist| {
            playlist.node_type.as_deref() == Some("1")
                && (requested.is_empty() || requested.contains(&playlist.path))
        })
        .collect()
}

fn plan_item_for_track(track: &Track, reuse_existing: bool) -> ConversionPlanItem {
    let track_issues = validate_track(track);
    let has_blocking_issue = track_issues
        .iter()
        .any(|issue| issue.severity == IssueSeverity::Error);
    let target_path = track.file_path.as_deref().map(default_target_path);

    let action = if has_blocking_issue {
        PlanAction::Blocked
    } else {
        match track_action(track) {
            TrackAction::Convert => {
                if reuse_existing && target_path.as_ref().is_some_and(|path| path.exists()) {
                    PlanAction::ReuseExisting
                } else {
                    PlanAction::Convert
                }
            }
            TrackAction::AlreadyAiff => PlanAction::SkipAlreadyAiff,
            TrackAction::Unsupported => PlanAction::Blocked,
        }
    };

    ConversionPlanItem {
        track_id: track.track_id.clone(),
        name: track.name.clone(),
        artist: track.artist.clone(),
        kind: track.kind.clone(),
        source_path: track.file_path.clone(),
        target_path,
        action,
        issues: track_issues,
    }
}
