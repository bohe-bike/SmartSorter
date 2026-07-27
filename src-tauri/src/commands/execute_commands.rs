use crate::engine::{executor, undo};
use crate::models::log::{ExecutionLog, ExecutionSummary, Operation, OperationStatus, UndoStatus};
use crate::models::preview::{PlannedOperation, PreviewResult};
use crate::models::progress::ProgressPayload;
use crate::models::rule::ConflictStrategy;
use crate::storage::log_store;
use chrono::Local;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::command;
use tauri::{AppHandle, Emitter, Manager};
use uuid::Uuid;

/// 全局缓存：保存最近的预览结果，执行时从此取数据
pub static PREVIEW_CACHE: Mutex<Option<PreviewResult>> = Mutex::new(None);

#[command]
pub async fn execute_task(
    app: AppHandle,
    task_id: String,
    checked_ids: Vec<String>,
) -> Result<String, String> {
    let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;

    // 从缓存中 clone 出所需数据后立即释放锁，避免在 async 中长期持有 Mutex
    let checked_set: HashSet<&str> = checked_ids.iter().map(|s| s.as_str()).collect();
    let (items, rule_set_name) = {
        let cache = PREVIEW_CACHE.lock().map_err(|e| e.to_string())?;
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
        (filtered, preview.rule_set_name.clone())
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
            operations.push(Operation {
                op_id: Uuid::new_v4().to_string(),
                action: planned.action_type.clone(),
                source_path: planned.source_path.clone(),
                target_path: outcome.target_path,
                status: outcome.status,
                error_message: outcome.error_message,
                reversible: outcome.reversible,
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
    let mut overwrote = false;

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
                if let Err(error) = std::fs::remove_file(&target) {
                    return ExecutionOutcome {
                        status: OperationStatus::Failed,
                        target_path: target.to_string_lossy().into_owned(),
                        error_message: Some(format!("删除冲突目标失败: {}", error)),
                        reversible: false,
                    };
                }
                overwrote = true;
            }
            ConflictStrategy::AutoRename => target = next_available_target(&target),
        }
    }

    let result = match planned.action_type.as_str() {
        "rename" => executor::safe_rename(source, &target),
        "move" => executor::safe_move(source, &target),
        "copy" => executor::safe_copy(source, &target),
        _ => Err("未知操作类型".into()),
    };

    match result {
        Ok(()) => ExecutionOutcome {
            status: OperationStatus::Success,
            target_path: target.to_string_lossy().into_owned(),
            error_message: None,
            reversible: !overwrote,
        },
        Err(error) => ExecutionOutcome {
            status: OperationStatus::Failed,
            target_path: target.to_string_lossy().into_owned(),
            error_message: Some(error),
            reversible: false,
        },
    }
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
        };
        let move_file = PlannedOperation {
            action_type: "move".into(),
            source_path: renamed.to_string_lossy().into_owned(),
            target_path: moved.to_string_lossy().into_owned(),
            conflict_strategy: Some(ConflictStrategy::Skip),
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
        };
        let outcome = execute_planned_operation(&planned);

        assert!(matches!(outcome.status, OperationStatus::Skipped));
        assert_eq!(std::fs::read_to_string(&source).unwrap(), "source");
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "existing");

        std::fs::remove_dir_all(root).unwrap();
    }
}
