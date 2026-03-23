use std::path::Path;
use std::sync::Mutex;
use tauri::command;
use tauri::{AppHandle, Manager, Emitter};
use uuid::Uuid;
use chrono::Local;
use crate::models::preview::PreviewResult;
use crate::models::progress::ProgressPayload;
use crate::models::log::{ExecutionLog, ExecutionSummary, Operation, OperationStatus, UndoStatus};
use crate::engine::{executor, undo};
use crate::storage::log_store;

/// 全局缓存：保存最近的预览结果，执行时从此取数据
pub static PREVIEW_CACHE: Mutex<Option<PreviewResult>> = Mutex::new(None);

#[command]
pub async fn execute_task(app: AppHandle, task_id: String, checked_ids: Vec<String>) -> Result<String, String> {
    let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;

    // 从缓存中 clone 出所需数据后立即释放锁，避免在 async 中长期持有 Mutex
    let items = {
        let cache = PREVIEW_CACHE.lock().map_err(|e| e.to_string())?;
        let preview = cache.as_ref()
            .ok_or_else(|| "没有可用的预览结果，请先执行分析预览".to_string())?;
        if preview.task_id != task_id {
            return Err("任务 ID 不匹配，请重新分析预览".into());
        }
        preview.items.iter()
            .filter(|item| checked_ids.contains(&item.id))
            .cloned()
            .collect::<Vec<_>>()
    }; // MutexGuard 在此释放

    let start = std::time::Instant::now();
    let mut operations: Vec<Operation> = Vec::new();
    let mut succeeded = 0u64;
    let mut failed = 0u64;
    let mut skipped = 0u64;
    let total_items = items.len() as u64;

    for (idx, item) in items.iter().enumerate() {
        let _ = app.emit("progress", ProgressPayload {
            task_id: task_id.clone(), current: idx as u64, total: total_items,
            current_file: item.source.path.clone(), phase: "executing".into(),
        });

        let source = Path::new(&item.source.path);
        let target = Path::new(&item.target.path);

        // 确保目标目录存在（兜底，executor 内也有此逻辑）
        if let Some(parent) = target.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        if item.changes.is_empty() {
            skipped += 1;
            continue;
        }

        let action_type = item.changes.first()
            .map(|c| c.action_type.as_str())
            .unwrap_or("unknown");

        let result = match action_type {
            "rename" => executor::safe_rename(source, target),
            "move" => executor::safe_move(source, target),
            "copy" => executor::safe_copy(source, target),
            "delete" => executor::safe_delete(source),
            _ => Err("未知操作类型".into()),
        };

        let (status, error_message) = match &result {
            Ok(()) => { succeeded += 1; (OperationStatus::Success, None) }
            Err(e) => { failed += 1; (OperationStatus::Failed, Some(e.clone())) }
        };

        operations.push(Operation {
            op_id: Uuid::new_v4().to_string(),
            action: action_type.to_string(),
            source_path: item.source.path.clone(),
            target_path: item.target.path.clone(),
            status,
            error_message,
            reversible: action_type != "delete",
        });
    }

    let duration_ms = start.elapsed().as_millis() as u64;

    // 在 operations 被 move 前收集失败详情
    let fail_details: Vec<String> = operations.iter()
        .filter(|op| op.status == OperationStatus::Failed)
        .map(|op| {
            let err = op.error_message.as_deref().unwrap_or("未知错误");
            format!("  {} → {}\n    原因: {}", op.source_path, op.target_path, err)
        })
        .collect();

    let log = ExecutionLog {
        log_id: Uuid::new_v4().to_string(),
        task_id,
        rule_set_name: String::new(),
        executed_at: Local::now().to_rfc3339(),
        duration_ms,
        summary: ExecutionSummary { total: operations.len() as u64, succeeded, failed, skipped },
        operations,
        undo_status: UndoStatus::Available,
    };

    log_store::append(&data_dir, &log)?;

    if failed > 0 {
        let msg = format!(
            "执行完成：{} 成功, {} 失败, {} 跳过\n\n失败详情:\n{}",
            succeeded, failed, skipped, fail_details.join("\n")
        );
        Err(msg)
    } else {
        Ok(format!("执行完成：{} 个操作全部成功", succeeded))
    }
}

#[command]
pub fn undo_task(app: AppHandle, log_id: String) -> Result<(), String> {
    let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let logs = log_store::load_all(&data_dir)?;
    let log = logs.iter()
        .find(|l| l.log_id == log_id)
        .ok_or_else(|| "未找到对应的执行日志".to_string())?;

    let _new_status = undo::undo_operations(log)?;

    // 更新日志中的 undo_status
    let mut all_logs = log_store::load_all(&data_dir)?;
    if let Some(entry) = all_logs.iter_mut().find(|l| l.log_id == log_id) {
        entry.undo_status = UndoStatus::Expired;
    }
    // 重写日志文件
    let path = data_dir.join("execution_logs.json");
    let content = serde_json::to_string_pretty(&all_logs)
        .map_err(|e| format!("序列化失败: {}", e))?;
    std::fs::write(&path, content)
        .map_err(|e| format!("写入日志失败: {}", e))?;

    Ok(())
}
