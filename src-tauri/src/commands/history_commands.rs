use tauri::command;
use tauri::{AppHandle, Manager};
use crate::models::log::ExecutionLog;
use crate::storage::log_store;

#[command]
pub fn load_history(app: AppHandle) -> Result<Vec<ExecutionLog>, String> {
    let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    log_store::load_all(&data_dir)
}
