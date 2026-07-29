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
    /// 成功执行后写入目标文件的哈希，撤销前必须匹配，避免误处理后来替换的文件。
    #[serde(default)]
    pub target_hash: Option<String>,
    /// 需要额外备份才能撤销的操作所使用的备份文件路径。
    #[serde(default)]
    pub backup_path: Option<String>,
    /// 备份文件的哈希，恢复前必须匹配。
    #[serde(default)]
    pub backup_hash: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationStatus {
    Success,
    Failed,
    Skipped,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UndoStatus {
    Available,
    Partial,
    Expired,
}
