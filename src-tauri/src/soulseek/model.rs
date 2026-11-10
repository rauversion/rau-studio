use serde::{Deserialize, Serialize};
use soulseek_client::soulseek_core::{FileAttribute, FileEntry};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct RemoteFile {
    pub path: String,
    pub size: u64,
    pub extension: String,
    pub attributes: Vec<Attribute>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Attribute {
    pub kind: u32,
    pub value: u32,
}

impl From<&FileEntry> for RemoteFile {
    fn from(file: &FileEntry) -> Self {
        Self {
            path: file.name.clone(),
            size: file.size,
            extension: file.extension.clone(),
            attributes: file
                .attributes
                .iter()
                .map(|a| Attribute {
                    kind: a.kind,
                    value: a.value,
                })
                .collect(),
        }
    }
}

impl From<&RemoteFile> for FileEntry {
    fn from(file: &RemoteFile) -> Self {
        Self {
            name: file.path.clone(),
            size: file.size,
            extension: file.extension.clone(),
            attributes: file
                .attributes
                .iter()
                .map(|a| FileAttribute {
                    kind: a.kind,
                    value: a.value,
                })
                .collect(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Search {
    pub id: String,
    pub server: String,
    pub account: String,
    pub query: String,
    pub state: String,
    pub result_count: u64,
    pub created_at: String,
    pub last_error: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SearchHit {
    pub id: String,
    pub search_id: String,
    pub username: String,
    pub file: RemoteFile,
    pub slots_free: bool,
    pub average_speed: u32,
    pub queue_length: u32,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DownloadState {
    Queued,
    Running,
    RetryWaiting,
    Paused,
    Interrupted,
    Completed,
    Cancelled,
    Failed,
}

impl DownloadState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::RetryWaiting => "retry_waiting",
            Self::Paused => "paused",
            Self::Interrupted => "interrupted",
            Self::Completed => "completed",
            Self::Cancelled => "cancelled",
            Self::Failed => "failed",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Download {
    pub id: String,
    pub batch_id: Option<String>,
    pub server: String,
    pub account: String,
    pub username: String,
    pub file: RemoteFile,
    pub destination: String,
    pub output_root: String,
    pub remote_root: String,
    pub state: DownloadState,
    pub bytes: u64,
    pub attempts: u32,
    pub last_error: Option<String>,
    #[serde(skip_serializing)]
    pub checkpoint: Option<super::partial::Checkpoint>,
    pub created_at: String,
    pub updated_at: String,
}
