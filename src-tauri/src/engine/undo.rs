use crate::engine::executor;
use crate::engine::hasher;
use crate::models::log::{ExecutionLog, Operation, OperationStatus, UndoStatus};
use std::fs;
use std::path::Path;
use uuid::Uuid;

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
            "tag_cleanup" => undo_tag_cleanup(op),
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

fn undo_tag_cleanup(operation: &Operation) -> Result<(), String> {
    let original = Path::new(&operation.source_path);
    let backup_path = operation
        .backup_path
        .as_deref()
        .ok_or_else(|| "标签清洗记录缺少备份路径".to_string())?;
    let backup = Path::new(backup_path);
    if !original.exists() {
        return Err(format!("已清洗文件不存在: {}", original.display()));
    }
    if !backup.exists() {
        return Err(format!("标签备份文件不存在: {}", backup.display()));
    }
    verify_path_hash(
        original,
        operation.target_hash.as_deref(),
        "已清洗文件已被修改，拒绝用旧备份覆盖",
    )?;
    verify_path_hash(
        backup,
        operation.backup_hash.as_deref(),
        "标签备份文件已损坏或被替换",
    )?;

    let parent = original
        .parent()
        .ok_or_else(|| "无法确定媒体文件所在目录".to_string())?;
    let file_name = original
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "media".into());
    let restore_stage = parent.join(format!(
        ".smartsorter-restore-{}-{}",
        Uuid::new_v4(),
        file_name
    ));
    let cleaned_rollback = parent.join(format!(
        ".smartsorter-cleaned-{}-{}",
        Uuid::new_v4(),
        file_name
    ));

    executor::safe_copy(backup, &restore_stage)?;
    if let Err(error) = fs::rename(original, &cleaned_rollback) {
        let _ = fs::remove_file(&restore_stage);
        return Err(format!("暂存已清洗文件失败: {}", error));
    }
    if let Err(error) = fs::rename(&restore_stage, original) {
        let rollback_error = fs::rename(&cleaned_rollback, original).err();
        let _ = fs::remove_file(&restore_stage);
        return Err(match rollback_error {
            Some(rollback_error) => format!(
                "恢复标签备份失败: {}；恢复清洗后文件也失败: {}",
                error, rollback_error
            ),
            None => format!("恢复标签备份失败，已保留清洗后文件: {}", error),
        });
    }

    let _ = fs::remove_file(&cleaned_rollback);
    let _ = fs::remove_file(backup);
    if let Some(parent) = backup.parent() {
        let _ = fs::remove_dir(parent);
    }
    Ok(())
}

fn verify_path_hash(
    path: &Path,
    expected: Option<&str>,
    mismatch_message: &str,
) -> Result<(), String> {
    let expected = expected.ok_or_else(|| "操作记录缺少文件哈希，拒绝恢复".to_string())?;
    let actual = hasher::compute_sha256(path)
        .map_err(|error| format!("计算文件 SHA-256 失败: {}", error))?;
    if actual != expected {
        return Err(mismatch_message.into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::log::{ExecutionSummary, Operation};
    use chrono::Local;

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
                backup_path: None,
                backup_hash: None,
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

    #[test]
    fn undo_tag_cleanup_restores_the_verified_backup() {
        let root = std::env::temp_dir().join(format!("smart-sorter-tag-undo-{}", Uuid::new_v4()));
        let original = root.join("track.mp3");
        let backup_dir = root.join("media_tag_backups").join("task");
        let backup = backup_dir.join("backup");
        std::fs::create_dir_all(&backup_dir).unwrap();
        std::fs::write(&original, "cleaned media").unwrap();
        std::fs::write(&backup, "original media").unwrap();
        let cleaned_hash = hasher::compute_sha256(&original).unwrap();
        let backup_hash = hasher::compute_sha256(&backup).unwrap();

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
                action: "tag_cleanup".into(),
                source_path: original.to_string_lossy().into_owned(),
                target_path: "Artist / AlbumArtist = 作者A".into(),
                status: OperationStatus::Success,
                error_message: None,
                reversible: true,
                target_hash: Some(cleaned_hash),
                backup_path: Some(backup.to_string_lossy().into_owned()),
                backup_hash: Some(backup_hash),
            }],
            undo_status: UndoStatus::Available,
        };

        assert_eq!(undo_operations(&log).unwrap(), UndoStatus::Expired);
        assert_eq!(
            std::fs::read_to_string(&original).unwrap(),
            "original media"
        );
        assert!(!backup.exists());

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn undo_tag_cleanup_refuses_to_replace_a_modified_file() {
        let root = std::env::temp_dir().join(format!("smart-sorter-tag-guard-{}", Uuid::new_v4()));
        let original = root.join("track.mp3");
        let backup = root.join("backup");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(&original, "cleaned media").unwrap();
        let cleaned_hash = hasher::compute_sha256(&original).unwrap();
        std::fs::write(&backup, "original media").unwrap();
        let backup_hash = hasher::compute_sha256(&backup).unwrap();
        std::fs::write(&original, "later edit").unwrap();

        let operation = Operation {
            op_id: Uuid::new_v4().to_string(),
            action: "tag_cleanup".into(),
            source_path: original.to_string_lossy().into_owned(),
            target_path: "Artist / AlbumArtist = 作者A".into(),
            status: OperationStatus::Success,
            error_message: None,
            reversible: true,
            target_hash: Some(cleaned_hash),
            backup_path: Some(backup.to_string_lossy().into_owned()),
            backup_hash: Some(backup_hash),
        };

        assert!(undo_tag_cleanup(&operation).is_err());
        assert_eq!(std::fs::read_to_string(&original).unwrap(), "later edit");
        assert!(backup.exists());

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
