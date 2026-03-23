use tauri::command;
use tauri::{AppHandle, Manager};
use crate::models::rule::RuleSet;
use crate::storage::rule_store;

#[command]
pub fn load_rule_sets(app: AppHandle) -> Result<Vec<RuleSet>, String> {
    let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    rule_store::load_all(&data_dir)
}

#[command]
pub fn save_rule_set(app: AppHandle, rule_set: RuleSet) -> Result<(), String> {
    let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    rule_store::save(&data_dir, &rule_set)
}

#[command]
pub fn delete_rule_set(app: AppHandle, id: String) -> Result<(), String> {
    let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    rule_store::delete(&data_dir, &id)
}
