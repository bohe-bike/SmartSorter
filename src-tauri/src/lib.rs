mod commands;
mod engine;
mod models;
mod storage;

use commands::{
    duplicate_commands::{delete_duplicates, scan_duplicates},
    execute_commands::{execute_task, undo_task},
    history_commands::load_history,
    media_commands::{execute_media_classify, preview_media_classify, scan_media_authors},
    preview_commands::analyze_preview,
    rule_commands::{delete_rule_set, load_rule_sets, save_rule_set},
};

#[tauri::command]
fn pick_folder() -> Result<Option<String>, String> {
    use std::path::PathBuf;
    let result = rfd::FileDialog::new().set_title("选择文件夹").pick_folder();
    Ok(result.map(|p: PathBuf| p.to_string_lossy().into_owned()))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            load_rule_sets,
            save_rule_set,
            delete_rule_set,
            analyze_preview,
            execute_task,
            undo_task,
            scan_duplicates,
            delete_duplicates,
            scan_media_authors,
            preview_media_classify,
            execute_media_classify,
            load_history,
            pick_folder,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
