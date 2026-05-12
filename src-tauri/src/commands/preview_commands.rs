use crate::engine::{matcher, scanner, transformer};
use crate::models::preview::{
    ChangeDetail, FileSnapshot, FileTarget, PreviewError, PreviewItem, PreviewRequest,
    PreviewResult, PreviewSummary,
};
use crate::models::progress::ProgressPayload;
use crate::models::rule::Action;
use crate::storage::rule_store;
use chrono::Local;
use tauri::{command, AppHandle, Emitter, Manager};
use uuid::Uuid;

#[command]
pub async fn analyze_preview(
    app: AppHandle,
    request: PreviewRequest,
) -> Result<PreviewResult, String> {
    let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let all_rules = rule_store::load_all(&data_dir)?;
    let rule_set = all_rules
        .iter()
        .find(|rs| rs.id == request.rule_set_id)
        .ok_or_else(|| format!("规则方案 {} 不存在", request.rule_set_id))?;

    let task_id = Uuid::new_v4().to_string();

    // 第一步：扫描所有数据源（spawn_blocking 避免阻塞 tokio 线程）
    let _ = app.emit(
        "progress",
        ProgressPayload {
            task_id: task_id.clone(),
            current: 0,
            total: 0,
            current_file: String::new(),
            phase: "scanning".into(),
        },
    );

    let source_paths_clone = request.source_paths.clone();
    let (all_scanned_files, mut scan_errors) = tauri::async_runtime::spawn_blocking(move || {
        let mut files: Vec<(String, std::path::PathBuf)> = Vec::new();
        let mut errs: Vec<PreviewError> = Vec::new();
        for source in &source_paths_clone {
            let root = std::path::Path::new(source);
            if !root.exists() {
                errs.push(PreviewError {
                    path: source.clone(),
                    error: "not_found".into(),
                    message: "路径不存在".into(),
                });
                continue;
            }
            let scanned = scanner::scan_directory(root, request.recursive, request.max_depth);
            for f in scanned {
                files.push((source.clone(), f));
            }
        }
        (files, errs)
    })
    .await
    .map_err(|e| e.to_string())?;

    let mut errors: Vec<PreviewError> = Vec::new();
    errors.append(&mut scan_errors);
    let total_scanned = all_scanned_files.len() as u64;

    let _ = app.emit(
        "progress",
        ProgressPayload {
            task_id: task_id.clone(),
            current: 0,
            total: total_scanned,
            current_file: String::new(),
            phase: "matching".into(),
        },
    );

    // 第二步：匹配规则（同样在 spawn_blocking 中执行）
    let rule_set_clone = rule_set.clone();
    let app_clone = app.clone();
    let task_id_clone = task_id.clone();
    let (items, match_errors) = tauri::async_runtime::spawn_blocking(move || {
        let mut items: Vec<PreviewItem> = Vec::new();
        let errs: Vec<PreviewError> = Vec::new();
        for (idx, (_source, file_path)) in all_scanned_files.iter().enumerate() {
            if idx % 50 == 0 {
                let _ = app_clone.emit(
                    "progress",
                    ProgressPayload {
                        task_id: task_id_clone.clone(),
                        current: idx as u64,
                        total: total_scanned,
                        current_file: file_path.to_string_lossy().into_owned(),
                        phase: "matching".into(),
                    },
                );
            }

            for rule in &rule_set_clone.rules {
                if !rule.enabled {
                    continue;
                }
                if !matcher::matches(file_path, &rule.condition_group) {
                    continue;
                }

                let meta = std::fs::metadata(file_path).ok();
                let source_snapshot = FileSnapshot {
                    path: file_path.to_string_lossy().into_owned(),
                    name: file_path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                    size_bytes: meta.as_ref().map(|m| m.len()).unwrap_or(0),
                    created_at: meta
                        .as_ref()
                        .and_then(|m| m.created().ok())
                        .map(|t| chrono::DateTime::<Local>::from(t).to_rfc3339())
                        .unwrap_or_default(),
                    modified_at: meta
                        .as_ref()
                        .and_then(|m| m.modified().ok())
                        .map(|t| chrono::DateTime::<Local>::from(t).to_rfc3339())
                        .unwrap_or_default(),
                };

                let mut ctx = transformer::TransformContext::default();
                let mut changes = Vec::new();
                let mut target_path = file_path.clone();

                for action in &rule.actions {
                    if let Some(new_path) = transformer::compute_target(file_path, action, &mut ctx)
                    {
                        let action_type = match action {
                            Action::Rename(_) => "rename",
                            Action::Move(_) => "move",
                            Action::Copy(_) => "copy",
                            Action::Delete(_) => "delete",
                        };
                        changes.push(ChangeDetail {
                            rule_id: rule.id.clone(),
                            rule_name: rule.name.clone(),
                            action_type: action_type.into(),
                            description: format!(
                                "{} → {}",
                                file_path.display(),
                                new_path.display()
                            ),
                        });
                        target_path = new_path;
                    } else if matches!(action, Action::Delete(_)) {
                        changes.push(ChangeDetail {
                            rule_id: rule.id.clone(),
                            rule_name: rule.name.clone(),
                            action_type: "delete".into(),
                            description: format!("删除 {}", file_path.display()),
                        });
                    }
                }

                if !changes.is_empty() {
                    items.push(PreviewItem {
                        id: Uuid::new_v4().to_string(),
                        checked: true,
                        source: source_snapshot,
                        target: FileTarget {
                            path: target_path.to_string_lossy().into_owned(),
                            name: target_path
                                .file_name()
                                .map(|n| n.to_string_lossy().into_owned())
                                .unwrap_or_default(),
                        },
                        changes,
                        conflict: None,
                    });
                }
                break; // 每个文件只匹配第一条规则
            }
        }
        (items, errs)
    })
    .await
    .map_err(|e| e.to_string())?;

    errors.extend(match_errors);
    // match_errors 将来可用于收集匹配阶段错误，目前未使用
    let _ = &errors;

    let mut to_rename = 0u64;
    let mut to_move = 0u64;
    let mut to_copy = 0u64;
    let mut to_delete = 0u64;
    for item in &items {
        for c in &item.changes {
            match c.action_type.as_str() {
                "rename" => to_rename += 1,
                "move" => to_move += 1,
                "copy" => to_copy += 1,
                "delete" => to_delete += 1,
                _ => {}
            }
        }
    }

    let result = PreviewResult {
        task_id,
        rule_set_name: rule_set.name.clone(),
        generated_at: Local::now().to_rfc3339(),
        summary: PreviewSummary {
            total_scanned,
            matched: items.len() as u64,
            to_rename,
            to_move,
            to_copy,
            to_delete,
            conflicts: 0,
            errors: errors.len() as u64,
        },
        items,
        errors,
    };

    // 缓存预览结果供 execute_task 使用
    if let Ok(mut cache) = super::execute_commands::PREVIEW_CACHE.lock() {
        *cache = Some(result.clone());
    }

    Ok(result)
}
