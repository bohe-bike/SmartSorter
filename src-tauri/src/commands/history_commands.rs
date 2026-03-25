use crate::models::log::ExecutionLog;
use crate::storage::log_store;
use tauri::command;
use tauri::{AppHandle, Manager};

#[command]
pub fn load_history(app: AppHandle) -> Result<Vec<ExecutionLog>, String> {
    let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let mut logs = log_store::load_all(&data_dir)?;
    logs.sort_by(|a, b| b.executed_at.cmp(&a.executed_at));
    Ok(logs)
}
