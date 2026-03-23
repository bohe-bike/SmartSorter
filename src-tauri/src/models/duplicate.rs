use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DuplicateResult {
    pub task_id: String,
    pub scanned_count: u64,
    pub total_groups: u64,
    pub total_wasted_bytes: u64,
    pub groups: Vec<DuplicateGroup>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DuplicateGroup {
    pub group_id: String,
    pub hash: String,
    pub file_size: u64,
    pub files: Vec<DuplicateFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DuplicateFile {
    pub path: String,
    pub created_at: String,
    pub modified_at: String,
    pub keep: bool,
}
