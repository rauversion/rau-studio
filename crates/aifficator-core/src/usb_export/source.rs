//! Read-only source availability checks shared by selection review and start.
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::{Error, ErrorKind, Read};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SourceIssueKind {
    NoLocalPath,
    NotFound,
    PermissionDenied,
    NotAFile,
    EmptyFile,
    Unreadable,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SourceFailure {
    pub kind: SourceIssueKind,
    pub detail: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UnavailableTrack {
    pub source_key: String,
    pub title: String,
    pub artist: String,
    pub source_path: Option<PathBuf>,
    pub playlist_names: Vec<String>,
    pub problem: SourceFailure,
}

fn io_failure(error: Error) -> SourceFailure {
    SourceFailure {
        kind: match error.kind() {
            ErrorKind::NotFound => SourceIssueKind::NotFound,
            ErrorKind::PermissionDenied => SourceIssueKind::PermissionDenied,
            _ => SourceIssueKind::Unreadable,
        },
        detail: Some(error.to_string()),
    }
}

/// Return the actual byte size, or a structured reason the source cannot be
/// exported. Ignore cached catalog availability; a disk may have disconnected.
pub fn check_source(path: Option<&Path>) -> std::result::Result<u64, SourceFailure> {
    let path = path
        .filter(|path| !path.as_os_str().is_empty())
        .ok_or(SourceFailure {
            kind: SourceIssueKind::NoLocalPath,
            detail: None,
        })?;
    let metadata = fs::metadata(path).map_err(io_failure)?;
    if !metadata.is_file() {
        return Err(SourceFailure {
            kind: SourceIssueKind::NotAFile,
            detail: None,
        });
    }
    if metadata.len() == 0 {
        return Err(SourceFailure {
            kind: SourceIssueKind::EmptyFile,
            detail: None,
        });
    }
    let mut source = File::open(path).map_err(io_failure)?;
    fs::canonicalize(path).map_err(io_failure)?;
    let mut sample = [0];
    if source.read(&mut sample).map_err(io_failure)? == 0 {
        return Err(SourceFailure {
            kind: SourceIssueKind::EmptyFile,
            detail: None,
        });
    }
    Ok(metadata.len())
}
