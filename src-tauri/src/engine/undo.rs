use crate::engine::executor;
use crate::engine::hasher;
use crate::models::log::{ExecutionLog, Operation, OperationStatus, UndoStatus};
use std::fs;
use std::path::Path;

/// 根据执行日志中的映射表，将文件恢复到原始位置
pub fn undo_operations(log: &ExecutionLog) -> Result<UndoStatus, String> {
    if log.undo_status == UndoStatus::Expired {
        return Err("此操作已过期，无法撤销".into());
    }

    let mut fail_count = 0u32;
    let ops: Vec<_> = log.operations.iter().rev().collect();

    for op in &ops {
        if !op.reversible || op.status != OperationStatus::Success {
            continue;
        }
        let result = match op.action.as_str() {
            "move" => {
                let target = Path::new(&op.target_path);
                let source = Path::new(&op.source_path);
                if source.exists() {
                    Err(format!("原路径已有文件，拒绝覆盖: {}", op.source_path))
                } else if let Err(error) = verify_undo_target(op, target) {
                    Err(error)
                } else {
                    executor::safe_move(target, source)
                }
            }
            "rename" => {
                let target = Path::new(&op.target_path);
                let source = Path::new(&op.source_path);
                if source.exists() {
                    Err(format!("原路径已有文件，拒绝覆盖: {}", op.source_path))
                } else if let Err(error) = verify_undo_target(op, target) {
                    Err(error)
                } else {
                    executor::safe_rename(target, source)
                }
            }
            "copy" => {
                let target = Path::new(&op.target_path);
                if target.exists() {
                    if let Err(error) = verify_undo_target(op, target) {
                        Err(error)
                    } else {
                        fs::remove_file(target).map_err(|e| format!("删除复制文件失败: {}", e))
                    }
                } else {
                    Ok(())
                }
            }
            "delete" => Err("删除操作不可撤销".into()),
            _ => Ok(()),
        };

        match result {
            Ok(()) => {}
            Err(_) => fail_count += 1,
        }
    }

    if fail_count == 0 {
        Ok(UndoStatus::Expired)
    } else {
        Ok(UndoStatus::Partial)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::log::{ExecutionSummary, Operation};
    use chrono::Local;
    use uuid::Uuid;

    #[test]
    fn undo_refuses_to_delete_a_copy_that_was_replaced() {
        let root = std::env::temp_dir().join(format!("smart-sorter-undo-{}", Uuid::new_v4()));
        let source = root.join("source.txt");
        let target = root.join("target.txt");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(&source, "original").unwrap();
        std::fs::write(&target, "copied content").unwrap();
        let copied_hash = hasher::compute_sha256(&target).unwrap();
        std::fs::write(&target, "replacement content").unwrap();

        let log = ExecutionLog {
            log_id: Uuid::new_v4().to_string(),
            task_id: Uuid::new_v4().to_string(),
            rule_set_name: "test".into(),
            executed_at: Local::now().to_rfc3339(),
            duration_ms: 0,
            summary: ExecutionSummary {
                total: 1,
                succeeded: 1,
                failed: 0,
                skipped: 0,
            },
            operations: vec![Operation {
                op_id: Uuid::new_v4().to_string(),
                action: "copy".into(),
                source_path: source.to_string_lossy().into_owned(),
                target_path: target.to_string_lossy().into_owned(),
                status: OperationStatus::Success,
                error_message: None,
                reversible: true,
                target_hash: Some(copied_hash),
            }],
            undo_status: UndoStatus::Available,
        };

        assert_eq!(undo_operations(&log).unwrap(), UndoStatus::Partial);
        assert_eq!(
            std::fs::read_to_string(&target).unwrap(),
            "replacement content"
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}

fn verify_undo_target(operation: &Operation, target: &Path) -> Result<(), String> {
    if !target.exists() {
        return Err(format!("目标文件已不存在: {}", operation.target_path));
    }
    let expected_hash = operation
        .target_hash
        .as_deref()
        .ok_or_else(|| "操作记录缺少目标文件哈希，拒绝撤销以避免误删文件".to_string())?;
    let actual_hash = hasher::compute_sha256(target)
        .map_err(|error| format!("计算目标文件 SHA-256 失败: {}", error))?;
    if actual_hash != expected_hash {
        return Err("目标文件已被替换或修改，拒绝撤销以保护当前文件".into());
    }
    Ok(())
}
