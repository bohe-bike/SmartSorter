use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
pub struct ProgressPayload {
    pub task_id: String,
    pub current: u64,
    pub total: u64,
    pub current_file: String,
    pub phase: String,
}
