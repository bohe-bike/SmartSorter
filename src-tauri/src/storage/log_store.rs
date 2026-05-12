use crate::models::log::ExecutionLog;
use std::fs;
use std::path::Path;

const LOGS_FILE: &str = "execution_logs.json";

const MAX_LOG_ENTRIES: usize = 200;

pub fn append(data_dir: &Path, log: &ExecutionLog) -> Result<(), String> {
    let mut all = load_all(data_dir)?;
    all.push(log.clone());
    // 超过最大条数时，移除最旧的条目
    if all.len() > MAX_LOG_ENTRIES {
        all.drain(0..all.len() - MAX_LOG_ENTRIES);
    }
    write_all(data_dir, &all)
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
