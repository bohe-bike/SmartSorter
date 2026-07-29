use crate::models::log::ExecutionLog;
use std::fs;
use std::path::{Path, PathBuf};

const LOGS_FILE: &str = "execution_logs.json";

const MAX_LOG_ENTRIES: usize = 200;

pub fn append(data_dir: &Path, log: &ExecutionLog) -> Result<(), String> {
    let mut all = load_all(data_dir)?;
    all.push(log.clone());
    let mut removed = Vec::new();
    // 超过最大条数时，移除最旧的条目
    if all.len() > MAX_LOG_ENTRIES {
        removed = all
            .drain(0..all.len() - MAX_LOG_ENTRIES)
            .collect::<Vec<_>>();
    }
    write_all(data_dir, &all)?;
    cleanup_tag_backups(data_dir, &removed);
    Ok(())
}

fn cleanup_tag_backups(data_dir: &Path, logs: &[ExecutionLog]) {
    let backup_root = data_dir.join("media_tag_backups");
    for backup in logs
        .iter()
        .flat_map(|log| log.operations.iter())
        .filter_map(|operation| operation.backup_path.as_deref())
        .map(PathBuf::from)
        .filter(|backup| backup.starts_with(&backup_root))
    {
        let _ = fs::remove_file(&backup);
        if let Some(parent) = backup.parent() {
            let _ = fs::remove_dir(parent);
        }
    }
}

pub fn load_all(data_dir: &Path) -> Result<Vec<ExecutionLog>, String> {
    let path = data_dir.join(LOGS_FILE);
    if !path.exists() {
        return Ok(vec![]);
    }
    let content = fs::read_to_string(&path).map_err(|e| format!("读取日志文件失败: {}", e))?;
    serde_json::from_str(&content).map_err(|e| format!("解析日志文件失败: {}", e))
}

fn write_all(data_dir: &Path, logs: &[ExecutionLog]) -> Result<(), String> {
    fs::create_dir_all(data_dir).map_err(|e| format!("创建数据目录失败: {}", e))?;
    let path = data_dir.join(LOGS_FILE);
    let content = serde_json::to_string_pretty(logs).map_err(|e| format!("序列化失败: {}", e))?;
    fs::write(&path, content).map_err(|e| format!("写入日志文件失败: {}", e))
}
