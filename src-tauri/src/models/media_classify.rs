use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaClassifyResult {
    pub task_id: String,
    pub scanned_count: u64,
    pub total_authors: u64,
    pub no_author_count: u64,
    pub groups: Vec<AuthorGroup>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthorGroup {
    pub author: String,
    pub file_count: u64,
    pub total_size: u64,
    pub files: Vec<MediaFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaFile {
    pub path: String,
    pub file_name: String,
    pub size_bytes: u64,
    pub media_type: String,
    pub author: String,
    pub modified_at: String,
    pub checked: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClassifyAction {
    MoveToAuthorFolder,
    Rename,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassifyExecuteRequest {
    pub task_id: String,
    pub action: ClassifyAction,
    pub rename_template: Option<String>,
    pub checked_paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassifyPreviewItem {
    pub source_path: String,
    pub target_path: String,
    pub action_desc: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassifyPreviewResult {
    pub task_id: String,
    pub action: ClassifyAction,
    pub items: Vec<ClassifyPreviewItem>,
    pub total: u64,
}