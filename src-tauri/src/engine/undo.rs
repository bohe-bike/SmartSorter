use std::path::Path;
use std::fs;
use crate::models::log::{ExecutionLog, OperationStatus, UndoStatus};
use crate::engine::executor;

/// 根据执行日志中的映射表，将文件恢复到原始位置
pub fn undo_operations(log: &ExecutionLog) -> Result<UndoStatus, String> {
    if log.undo_status == UndoStatus::Expired {
        return Err("此操作已过期，无法撤销".into());
    }

    let mut success_count = 0u32;
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
                if target.exists() {
                    executor::safe_move(target, source)
                } else {
                    Err(format!("目标文件已不存在: {}", op.target_path))
                }
            }
            "rename" => {
                let target = Path::new(&op.target_path);
                let source = Path::new(&op.source_path);
                if target.exists() {
                    executor::safe_rename(target, source)
                } else {
                    Err(format!("目标文件已不存在: {}", op.target_path))
                }
            }
            "copy" => {
                let target = Path::new(&op.target_path);
                if target.exists() {
                    fs::remove_file(target)
                        .map_err(|e| format!("删除复制文件失败: {}", e))
                } else {
                    Ok(())
                }
            }
            "delete" => {
                Err("删除操作不可撤销".into())
            }
            _ => Ok(()),
        };

        match result {
            Ok(()) => success_count += 1,
            Err(_) => fail_count += 1,
        }
    }

    if fail_count == 0 {
        Ok(UndoStatus::Available)
    } else if success_count > 0 {
        Ok(UndoStatus::Partial)
    } else {
        Err("撤销全部失败".into())
    }
}
