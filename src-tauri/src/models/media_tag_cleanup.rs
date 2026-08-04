use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaTagCleanupResult {
    pub task_id: String,
    pub source_paths: Vec<String>,
    pub author_folder_mode: String,
    pub cleanup_mode: String,
    #[serde(default)]
    pub verify_content_hash: bool,
    pub scanned_count: u64,
    pub ready_count: u64,
    pub files: Vec<MediaTagCleanupFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaTagCleanupFile {
    pub path: String,
    pub file_name: String,
    pub size_bytes: u64,
    #[serde(default)]
    pub sha256: String,
    pub media_type: String,
    pub current_artist: Option<String>,
    pub target_artist: Option<String>,
    pub author_source: String,
    pub supported: bool,
    pub skip_reason: Option<String>,
    pub checked: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MediaTagCleanupExecuteRequest {
    pub task_id: String,
    pub selected_paths: Vec<String>,
    pub author_assignments: HashMap<String, String>,
}
