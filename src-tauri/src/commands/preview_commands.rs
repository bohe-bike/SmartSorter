use crate::engine::{matcher, scanner, transformer};
use crate::models::preview::{
    ChangeDetail, Conflict, FileSnapshot, FileTarget, PlannedOperation, PreviewError, PreviewItem,
    PreviewRequest, PreviewResult, PreviewSummary,
};
use crate::models::progress::ProgressPayload;
use crate::models::rule::{Action, ConflictStrategy};
use crate::storage::rule_store;
use chrono::Local;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
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
        let mut reserved_targets: HashSet<String> = HashSet::new();
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
                let mut operations = Vec::new();
                let mut target_path = file_path.clone();
                let mut conflict = None;

                for action in &rule.actions {
                    let action_type = match action {
                        Action::Rename(_) => "rename",
                        Action::Move(_) => "move",
                        Action::Copy(_) => "copy",
                        Action::Delete(_) => "delete",
                    };

                    if matches!(action, Action::Delete(_)) {
                        changes.push(ChangeDetail {
                            rule_id: rule.id.clone(),
                            rule_name: rule.name.clone(),
                            action_type: "delete".into(),
                            description: format!("删除 {}", target_path.display()),
                        });
                        operations.push(PlannedOperation {
                            action_type: "delete".into(),
                            source_path: target_path.to_string_lossy().into_owned(),
                            target_path: String::new(),
                            conflict_strategy: None,
                        });
                        break; // 删除后没有可继续操作的文件状态。
                    }

                    let Some(base_target) =
                        transformer::compute_target(&target_path, action, &mut ctx)
                    else {
                        continue;
                    };
                    if paths_equal(&base_target, &target_path) {
                        continue;
                    }

                    let strategy = match action {
                        Action::Move(params) | Action::Copy(params) => {
                            params.conflict_strategy.clone()
                        }
                        Action::Rename(_) => ConflictStrategy::Skip,
                        Action::Delete(_) => unreachable!(),
                    };
                    let (resolved_target, action_conflict) =
                        resolve_target(base_target, &strategy, &mut reserved_targets);
                    if conflict.is_none() {
                        conflict = action_conflict;
                    }

                    changes.push(ChangeDetail {
                        rule_id: rule.id.clone(),
                        rule_name: rule.name.clone(),
                        action_type: action_type.into(),
                        description: format!(
                            "{} → {}",
                            target_path.display(),
                            resolved_target.display()
                        ),
                    });
                    operations.push(PlannedOperation {
                        action_type: action_type.into(),
                        source_path: target_path.to_string_lossy().into_owned(),
                        target_path: resolved_target.to_string_lossy().into_owned(),
                        conflict_strategy: Some(strategy),
                    });
                    target_path = resolved_target;
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
                        conflict,
                        operations,
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
    let conflicts = items.iter().filter(|item| item.conflict.is_some()).count() as u64;

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
            conflicts,
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

fn resolve_target(
    initial: PathBuf,
    strategy: &ConflictStrategy,
    reserved: &mut HashSet<String>,
) -> (PathBuf, Option<Conflict>) {
    let initial_key = path_key(&initial);
    let occupied_on_disk = initial.exists();
    let occupied_in_batch = reserved.contains(&initial_key);
    if !occupied_on_disk && !occupied_in_batch {
        reserved.insert(initial_key);
        return (initial, None);
    }

    let existing_file = occupied_on_disk.then(|| file_snapshot(&initial));
    let conflict_type = if occupied_on_disk {
        "name_collision"
    } else {
        "batch_name_collision"
    };
    let resolved = if *strategy == ConflictStrategy::AutoRename {
        next_available_target(&initial, reserved)
    } else {
        initial
    };
    reserved.insert(path_key(&resolved));

    (
        resolved.clone(),
        Some(Conflict {
            conflict_type: conflict_type.into(),
            existing_file,
            resolution: strategy.clone(),
            resolved_path: resolved.to_string_lossy().into_owned(),
        }),
    )
}

fn next_available_target(initial: &Path, reserved: &HashSet<String>) -> PathBuf {
    let stem = initial
        .file_stem()
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_default();
    let extension = initial
        .extension()
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_default();
    let parent = initial.parent().unwrap_or(Path::new("."));

    for counter in 2u32..=9999 {
        let name = if extension.is_empty() {
            format!("{} ({})", stem, counter)
        } else {
            format!("{} ({}).{}", stem, counter, extension)
        };
        let candidate = parent.join(name);
        if !candidate.exists() && !reserved.contains(&path_key(&candidate)) {
            return candidate;
        }
    }

    initial.to_path_buf()
}

fn file_snapshot(path: &Path) -> FileSnapshot {
    let metadata = std::fs::metadata(path).ok();
    FileSnapshot {
        path: path.to_string_lossy().into_owned(),
        name: path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default(),
        size_bytes: metadata.as_ref().map(|meta| meta.len()).unwrap_or(0),
        created_at: metadata
            .as_ref()
            .and_then(|meta| meta.created().ok())
            .map(|time| chrono::DateTime::<Local>::from(time).to_rfc3339())
            .unwrap_or_default(),
        modified_at: metadata
            .as_ref()
            .and_then(|meta| meta.modified().ok())
            .map(|time| chrono::DateTime::<Local>::from(time).to_rfc3339())
            .unwrap_or_default(),
    }
}

#[cfg(target_os = "windows")]
fn path_key(path: &Path) -> String {
    path.to_string_lossy().replace('/', "\\").to_lowercase()
}

#[cfg(not(target_os = "windows"))]
fn path_key(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn paths_equal(left: &Path, right: &Path) -> bool {
    path_key(left) == path_key(right)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_rename_resolves_a_target_reserved_by_an_earlier_file() {
        let base =
            std::env::temp_dir().join(format!("smart-sorter-preview-{}", uuid::Uuid::new_v4()));
        let target = base.join("report.txt");
        let mut reserved = HashSet::new();

        let (first, first_conflict) =
            resolve_target(target.clone(), &ConflictStrategy::AutoRename, &mut reserved);
        let (second, second_conflict) =
            resolve_target(target, &ConflictStrategy::AutoRename, &mut reserved);

        assert_eq!(first, base.join("report.txt"));
        assert!(first_conflict.is_none());
        assert_eq!(second, base.join("report (2).txt"));
        assert_eq!(
            second_conflict.map(|conflict| conflict.conflict_type),
            Some("batch_name_collision".into())
        );
    }
}
