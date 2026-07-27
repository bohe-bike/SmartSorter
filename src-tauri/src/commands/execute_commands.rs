use crate::engine::{executor, hasher, undo};
use crate::models::log::{ExecutionLog, ExecutionSummary, Operation, OperationStatus, UndoStatus};
use crate::models::preview::{ExecuteTaskRequest, FileSnapshot, PlannedOperation, PreviewResult};
use crate::models::progress::ProgressPayload;
use crate::models::rule::ConflictStrategy;
use crate::storage::log_store;
use chrono::Local;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::command;
use tauri::{AppHandle, Emitter, Manager};
use uuid::Uuid;

/// 全局缓存：保存最近的预览结果，执行时从此取数据
pub static PREVIEW_CACHE: Mutex<Option<PreviewResult>> = Mutex::new(None);

#[command]
pub async fn execute_task(app: AppHandle, request: ExecuteTaskRequest) -> Result<String, String> {
    let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let task_id = request.task_id;

    // 从缓存中 clone 出所需数据后立即释放锁，避免在 async 中长期持有 Mutex
    let checked_set: HashSet<&str> = request.checked_ids.iter().map(|id| id.as_str()).collect();
    let confirmed_delete_set: HashSet<&str> = request
        .confirmed_delete_ids
        .iter()
        .map(|id| id.as_str())
        .collect();
    let (items, rule_set_name) = {
        let mut cache = PREVIEW_CACHE.lock().map_err(|e| e.to_string())?;
        let preview = cache
            .as_ref()
            .ok_or_else(|| "没有可用的预览结果，请先执行分析预览".to_string())?;
        if preview.task_id != task_id {
            return Err("任务 ID 不匹配，请重新分析预览".into());
        }
        let filtered = preview
            .items
            .iter()
            .filter(|item| checked_set.contains(item.id.as_str()))
            .cloned()
            .collect::<Vec<_>>();
        if let Some(item) = filtered.iter().find(|item| {
            requires_delete_confirmation(&item.operations)
                && !confirmed_delete_set.contains(item.id.as_str())
        }) {
            return Err(format!("删除操作需要用户确认: {}", item.source.path));
        }
        let rule_set_name = preview.rule_set_name.clone();
        if !filtered.is_empty() {
            // 一次执行尝试后即让预览失效，避免部分失败时重复运行旧计划。
            *cache = None;
        }
        (filtered, rule_set_name)
    }; // MutexGuard 在此释放

    let start = std::time::Instant::now();
    let mut operations: Vec<Operation> = Vec::new();
    let mut succeeded = 0u64;
    let mut failed = 0u64;
    let mut skipped = 0u64;
    let total_operations = items
        .iter()
        .map(|item| item.operations.len() as u64)
        .sum::<u64>();
    let mut current_operation = 0u64;

    for item in &items {
        if let Err(error) = validate_source_snapshot(&item.source) {
            for planned in &item.operations {
                current_operation += 1;
                let _ = app.emit(
                    "progress",
                    ProgressPayload {
                        task_id: task_id.clone(),
                        current: current_operation,
                        total: total_operations,
                        current_file: planned.source_path.clone(),
                        phase: "executing".into(),
                    },
                );
                skipped += 1;
                operations.push(Operation {
                    op_id: Uuid::new_v4().to_string(),
                    action: planned.action_type.clone(),
                    source_path: planned.source_path.clone(),
                    target_path: planned.target_path.clone(),
                    status: OperationStatus::Skipped,
                    error_message: Some(format!("预览已过期，已跳过: {}", error)),
                    reversible: false,
                    target_hash: None,
                });
            }
            continue;
        }

        let mut blocked = false;
        for planned in &item.operations {
            current_operation += 1;
            let _ = app.emit(
                "progress",
                ProgressPayload {
                    task_id: task_id.clone(),
                    current: current_operation,
                    total: total_operations,
                    current_file: planned.source_path.clone(),
                    phase: "executing".into(),
                },
            );

            if blocked {
                skipped += 1;
                operations.push(Operation {
                    op_id: Uuid::new_v4().to_string(),
                    action: planned.action_type.clone(),
                    source_path: planned.source_path.clone(),
                    target_path: planned.target_path.clone(),
                    status: OperationStatus::Skipped,
                    error_message: Some("前置操作未成功，已跳过".into()),
                    reversible: false,
                    target_hash: None,
                });
                continue;
            }

            let outcome = execute_planned_operation(planned);
            match &outcome.status {
                OperationStatus::Success => succeeded += 1,
                OperationStatus::Failed => {
                    failed += 1;
                    blocked = true;
                }
                OperationStatus::Skipped => {
                    skipped += 1;
                    blocked = true;
                }
            }
            let target_hash = if outcome.status == OperationStatus::Success && outcome.reversible {
                hasher::compute_sha256(Path::new(&outcome.target_path)).ok()
            } else {
                None
            };
            let reversible = outcome.reversible && target_hash.is_some();
            operations.push(Operation {
                op_id: Uuid::new_v4().to_string(),
                action: planned.action_type.clone(),
                source_path: planned.source_path.clone(),
                target_path: outcome.target_path,
                status: outcome.status,
                error_message: outcome.error_message,
                reversible,
                target_hash,
            });
        }
    }

    let duration_ms = start.elapsed().as_millis() as u64;

    // 在 operations 被 move 前收集失败详情
    let fail_details: Vec<String> = operations
        .iter()
        .filter(|op| op.status == OperationStatus::Failed)
        .map(|op| {
            let err = op.error_message.as_deref().unwrap_or("未知错误");
            format!(
                "  {} → {}\n    原因: {}",
                op.source_path, op.target_path, err
            )
        })
        .collect();

    let undo_status = if operations
        .iter()
        .any(|operation| operation.status == OperationStatus::Success && !operation.reversible)
    {
        UndoStatus::Partial
    } else {
        UndoStatus::Available
    };

    let log = ExecutionLog {
        log_id: Uuid::new_v4().to_string(),
        task_id,
        rule_set_name,
        executed_at: Local::now().to_rfc3339(),
        duration_ms,
        summary: ExecutionSummary {
            total: operations.len() as u64,
            succeeded,
            failed,
            skipped,
        },
        operations,
        undo_status,
    };

    log_store::append(&data_dir, &log)?;

    if failed > 0 {
        let msg = format!(
            "执行完成：{} 成功, {} 失败, {} 跳过\n\n失败详情:\n{}",
            succeeded,
            failed,
            skipped,
            fail_details.join("\n")
        );
        Err(msg)
    } else {
        Ok(format!("执行完成：{} 成功，{} 跳过", succeeded, skipped))
    }
}

struct ExecutionOutcome {
    status: OperationStatus,
    target_path: String,
    error_message: Option<String>,
    reversible: bool,
}

fn execute_planned_operation(planned: &PlannedOperation) -> ExecutionOutcome {
    let source = Path::new(&planned.source_path);
    if planned.action_type == "delete" {
        return match executor::safe_delete(source) {
            Ok(()) => ExecutionOutcome {
                status: OperationStatus::Success,
                target_path: String::new(),
                error_message: None,
                reversible: false,
            },
            Err(error) => ExecutionOutcome {
                status: OperationStatus::Failed,
                target_path: String::new(),
                error_message: Some(error),
                reversible: false,
            },
        };
    }

    let mut target = PathBuf::from(&planned.target_path);
    let strategy = planned
        .conflict_strategy
        .clone()
        .unwrap_or(ConflictStrategy::Skip);
    if strategy == ConflictStrategy::Overwrite && !target.exists() && planned.expected_target_exists
    {
        return ExecutionOutcome {
            status: OperationStatus::Skipped,
            target_path: target.to_string_lossy().into_owned(),
            error_message: Some("目标文件在预览后已不存在，已跳过覆盖".into()),
            reversible: false,
        };
    }

    if target.exists() {
        match strategy {
            ConflictStrategy::Skip => {
                return ExecutionOutcome {
                    status: OperationStatus::Skipped,
                    target_path: target.to_string_lossy().into_owned(),
                    error_message: Some("目标路径已存在，按冲突策略跳过".into()),
                    reversible: false,
                }
            }
            ConflictStrategy::Overwrite => {
                return execute_overwrite(planned, source, &target);
            }
            ConflictStrategy::AutoRename => target = next_available_target(&target),
        }
    }

    let result = run_file_operation(&planned.action_type, source, &target);

    match result {
        Ok(()) => ExecutionOutcome {
            status: OperationStatus::Success,
            target_path: target.to_string_lossy().into_owned(),
            error_message: None,
            reversible: true,
        },
        Err(error) => ExecutionOutcome {
            status: OperationStatus::Failed,
            target_path: target.to_string_lossy().into_owned(),
            error_message: Some(error),
            reversible: false,
        },
    }
}

fn execute_overwrite(planned: &PlannedOperation, source: &Path, target: &Path) -> ExecutionOutcome {
    if let Err(error) = validate_overwrite_target(planned, target) {
        return ExecutionOutcome {
            status: OperationStatus::Skipped,
            target_path: target.to_string_lossy().into_owned(),
            error_message: Some(error),
            reversible: false,
        };
    }

    let backup = match move_target_to_backup(target) {
        Ok(backup) => backup,
        Err(error) => {
            return ExecutionOutcome {
                status: OperationStatus::Failed,
                target_path: target.to_string_lossy().into_owned(),
                error_message: Some(error),
                reversible: false,
            }
        }
    };
    let result = run_file_operation(&planned.action_type, source, target);
    match result {
        Ok(()) => {
            let cleanup_warning = fs::remove_file(&backup).err().map(|error| {
                format!(
                    "覆盖已完成，但未能清理临时备份 {}: {}",
                    backup.display(),
                    error
                )
            });
            ExecutionOutcome {
                status: OperationStatus::Success,
                target_path: target.to_string_lossy().into_owned(),
                error_message: cleanup_warning,
                reversible: false,
            }
        }
        Err(error) => {
            let rollback = restore_overwritten_target(target, &backup);
            let message = match rollback {
                Ok(()) => format!("{}；原目标文件已恢复", error),
                Err(rollback_error) => format!("{}；恢复原目标文件失败: {}", error, rollback_error),
            };
            ExecutionOutcome {
                status: OperationStatus::Failed,
                target_path: target.to_string_lossy().into_owned(),
                error_message: Some(message),
                reversible: false,
            }
        }
    }
}

fn run_file_operation(action_type: &str, source: &Path, target: &Path) -> Result<(), String> {
    match action_type {
        "rename" => executor::safe_rename(source, target),
        "move" => executor::safe_move(source, target),
        "copy" => executor::safe_copy(source, target),
        _ => Err("未知操作类型".into()),
    }
}

fn validate_source_snapshot(snapshot: &FileSnapshot) -> Result<(), String> {
    if snapshot.sha256.is_empty() {
        return Err("预览未能生成源文件哈希".into());
    }
    let path = Path::new(&snapshot.path);
    let actual_size = fs::metadata(path)
        .map_err(|error| format!("读取源文件信息失败: {}", error))?
        .len();
    if actual_size != snapshot.size_bytes {
        return Err(format!(
            "文件大小已变更（预览时 {} 字节，当前 {} 字节）",
            snapshot.size_bytes, actual_size
        ));
    }
    let actual_hash = hasher::compute_sha256(path)
        .map_err(|error| format!("计算源文件 SHA-256 失败: {}", error))?;
    if actual_hash != snapshot.sha256 {
        return Err("文件内容已变更".into());
    }
    Ok(())
}

fn requires_delete_confirmation(operations: &[PlannedOperation]) -> bool {
    operations
        .iter()
        .any(|operation| operation.action_type == "delete" && operation.requires_confirmation)
}

fn validate_overwrite_target(planned: &PlannedOperation, target: &Path) -> Result<(), String> {
    if !planned.expected_target_exists {
        return Err("目标文件在预览后新增，已跳过覆盖以保护该文件".into());
    }
    let expected_hash = planned
        .expected_target_hash
        .as_deref()
        .ok_or_else(|| "预览时未能校验目标文件，已跳过覆盖".to_string())?;
    let actual_hash = hasher::compute_sha256(target)
        .map_err(|error| format!("计算目标文件 SHA-256 失败: {}", error))?;
    if actual_hash != expected_hash {
        return Err("目标文件在预览后已变更，已跳过覆盖以保护该文件".into());
    }
    Ok(())
}

fn move_target_to_backup(target: &Path) -> Result<PathBuf, String> {
    if !target.is_file() {
        return Err(format!("冲突目标不是普通文件: {}", target.display()));
    }
    let parent = target.parent().unwrap_or(Path::new("."));
    let name = target
        .file_name()
        .map(|name| name.to_string_lossy())
        .unwrap_or_default();
    let backup = parent.join(format!(".{}.smartsorter-backup-{}", name, Uuid::new_v4()));
    fs::rename(target, &backup).map_err(|error| format!("创建冲突目标备份失败: {}", error))?;
    Ok(backup)
}

fn restore_overwritten_target(target: &Path, backup: &Path) -> Result<(), String> {
    if target.exists() {
        fs::remove_file(target).map_err(|error| format!("清理未完成的新目标失败: {}", error))?;
    }
    fs::rename(backup, target).map_err(|error| format!("恢复备份失败: {}", error))
}

fn next_available_target(initial: &Path) -> PathBuf {
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
        if !candidate.exists() {
            return candidate;
        }
    }

    initial.to_path_buf()
}

#[command]
pub async fn undo_task(app: AppHandle, log_id: String) -> Result<(), String> {
    let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;

    // 在 spawn_blocking 中执行文件 I/O，避免阻塞 tokio 线程
    tauri::async_runtime::spawn_blocking(move || {
        let logs = log_store::load_all(&data_dir)?;
        let log = logs
            .iter()
            .find(|l| l.log_id == log_id)
            .ok_or_else(|| "未找到对应的执行日志".to_string())?;

        let new_status = undo::undo_operations(log)?;

        // 更新日志中的 undo_status（根据实际结果写入，而非加确嵌入 Expired）
        let mut all_logs = log_store::load_all(&data_dir)?;
        if let Some(entry) = all_logs.iter_mut().find(|l| l.log_id == log_id) {
            entry.undo_status = new_status;
        }
        let path = data_dir.join("execution_logs.json");
        let content =
            serde_json::to_string_pretty(&all_logs).map_err(|e| format!("序列化失败: {}", e))?;
        std::fs::write(&path, content).map_err(|e| format!("写入日志失败: {}", e))?;

        Ok::<(), String>(())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn executes_a_rename_then_move_as_one_ordered_chain() {
        let root = std::env::temp_dir().join(format!("smart-sorter-execute-{}", Uuid::new_v4()));
        let source = root.join("source.txt");
        let renamed = root.join("renamed.txt");
        let moved = root.join("archive").join("renamed.txt");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(&source, "content").unwrap();

        let rename = PlannedOperation {
            action_type: "rename".into(),
            source_path: source.to_string_lossy().into_owned(),
            target_path: renamed.to_string_lossy().into_owned(),
            conflict_strategy: Some(ConflictStrategy::Skip),
            expected_target_exists: false,
            expected_target_hash: None,
            requires_confirmation: false,
        };
        let move_file = PlannedOperation {
            action_type: "move".into(),
            source_path: renamed.to_string_lossy().into_owned(),
            target_path: moved.to_string_lossy().into_owned(),
            conflict_strategy: Some(ConflictStrategy::Skip),
            expected_target_exists: false,
            expected_target_hash: None,
            requires_confirmation: false,
        };

        assert!(matches!(
            execute_planned_operation(&rename).status,
            OperationStatus::Success
        ));
        assert!(matches!(
            execute_planned_operation(&move_file).status,
            OperationStatus::Success
        ));
        assert!(!source.exists());
        assert!(!renamed.exists());
        assert_eq!(std::fs::read_to_string(&moved).unwrap(), "content");

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn skip_strategy_preserves_an_existing_target() {
        let root = std::env::temp_dir().join(format!("smart-sorter-conflict-{}", Uuid::new_v4()));
        let source = root.join("source.txt");
        let target = root.join("target.txt");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(&source, "source").unwrap();
        std::fs::write(&target, "existing").unwrap();

        let planned = PlannedOperation {
            action_type: "move".into(),
            source_path: source.to_string_lossy().into_owned(),
            target_path: target.to_string_lossy().into_owned(),
            conflict_strategy: Some(ConflictStrategy::Skip),
            expected_target_exists: true,
            expected_target_hash: None,
            requires_confirmation: false,
        };
        let outcome = execute_planned_operation(&planned);

        assert!(matches!(outcome.status, OperationStatus::Skipped));
        assert_eq!(std::fs::read_to_string(&source).unwrap(), "source");
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "existing");

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn failed_overwrite_restores_the_original_target() {
        let root = std::env::temp_dir().join(format!("smart-sorter-overwrite-{}", Uuid::new_v4()));
        let missing_source = root.join("missing.txt");
        let target = root.join("target.txt");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(&target, "original target").unwrap();
        let target_hash = hasher::compute_sha256(&target).unwrap();

        let planned = PlannedOperation {
            action_type: "move".into(),
            source_path: missing_source.to_string_lossy().into_owned(),
            target_path: target.to_string_lossy().into_owned(),
            conflict_strategy: Some(ConflictStrategy::Overwrite),
            expected_target_exists: true,
            expected_target_hash: Some(target_hash),
            requires_confirmation: false,
        };
        let outcome = execute_planned_operation(&planned);

        assert!(matches!(outcome.status, OperationStatus::Failed));
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "original target");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn detects_delete_operations_that_require_confirmation() {
        let delete = PlannedOperation {
            action_type: "delete".into(),
            source_path: "source.txt".into(),
            target_path: String::new(),
            conflict_strategy: None,
            expected_target_exists: false,
            expected_target_hash: None,
            requires_confirmation: true,
        };
        let unchecked_delete = PlannedOperation {
            requires_confirmation: false,
            ..delete.clone()
        };

        assert!(requires_delete_confirmation(&[delete]));
        assert!(!requires_delete_confirmation(&[unchecked_delete]));
    }
}
