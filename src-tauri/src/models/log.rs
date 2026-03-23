use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionLog {
    pub log_id: String,
    pub task_id: String,
    pub rule_set_name: String,
    pub executed_at: String,
    pub duration_ms: u64,
    pub summary: ExecutionSummary,
    pub operations: Vec<Operation>,
    pub undo_status: UndoStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionSummary {
    pub total: u64,
    pub succeeded: u64,
    pub failed: u64,
    pub skipped: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Operation {
    pub op_id: String,
    pub action: String,
    pub source_path: String,
    pub target_path: String,
    pub status: OperationStatus,
    pub error_message: Option<String>,
    pub reversible: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationStatus {
    Success,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UndoStatus {
    Available,
    Partial,
    Expired,
}
