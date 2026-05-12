use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaClassifyResult {
    pub task_id: String,
    pub source_paths: Vec<String>,
    pub scanned_count: u64,
    pub total_keywords: u64,
    pub no_match_count: u64,
    pub unmatched_files: Vec<MediaFile>,
    pub keywords: Vec<KeywordInfo>,
    pub groups: Vec<KeywordGroup>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeywordInfo {
    pub keyword: String,
    pub source: String,
    pub merged_from: Vec<String>,
    pub file_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeywordGroup {
    pub keyword: String,
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
    pub matched_keywords: Vec<String>,
    pub modified_at: String,
    pub checked: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassifyExecuteRequest {
    pub task_id: String,
    pub keyword_assignments: HashMap<String, String>,
    pub selected_paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassifyPreviewItem {
    pub source_path: String,
    pub target_path: String,
    pub action_desc: String,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassifyPreviewResult {
    pub task_id: String,
    pub items: Vec<ClassifyPreviewItem>,
    pub total: u64,
}
