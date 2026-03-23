use serde::{Deserialize, Serialize};
use super::rule::ConflictStrategy;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreviewRequest {
    pub source_paths: Vec<String>,
    pub rule_set_id: String,
    pub recursive: bool,
    pub max_depth: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreviewResult {
    pub task_id: String,
    pub generated_at: String,
    pub summary: PreviewSummary,
    pub items: Vec<PreviewItem>,
    pub errors: Vec<PreviewError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreviewSummary {
    pub total_scanned: u64,
    pub matched: u64,
    pub to_rename: u64,
    pub to_move: u64,
    pub to_copy: u64,
    pub to_delete: u64,
    pub conflicts: u64,
    pub errors: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreviewItem {
    pub id: String,
    pub checked: bool,
    pub source: FileSnapshot,
    pub target: FileTarget,
    pub changes: Vec<ChangeDetail>,
    pub conflict: Option<Conflict>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileSnapshot {
    pub path: String,
    pub name: String,
    pub size_bytes: u64,
    pub created_at: String,
    pub modified_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileTarget {
    pub path: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeDetail {
    pub rule_id: String,
    pub rule_name: String,
    pub action_type: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conflict {
    pub conflict_type: String,
    pub existing_file: Option<FileSnapshot>,
    pub resolution: ConflictStrategy,
    pub resolved_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreviewError {
    pub path: String,
    pub error: String,
    pub message: String,
}
