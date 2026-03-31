use crate::engine::{executor, hasher, scanner};
use crate::models::duplicate::{DuplicateFile, DuplicateGroup, DuplicateResult};
use crate::models::log::{ExecutionLog, ExecutionSummary, Operation, OperationStatus, UndoStatus};
use crate::models::progress::ProgressPayload;
use crate::storage::log_store;
use chrono::{DateTime, Local};
use std::collections::HashMap;
use std::path::Path;
use tauri::{command, AppHandle, Emitter, Manager};
use uuid::Uuid;

#[command]
pub async fn scan_duplicates(
    app: AppHandle,
    paths: Vec<String>,
    recursive: bool,
) -> Result<DuplicateResult, String> {
    // async 使 Tauri 在后台线程池执行，不阻塞主窗口
    let task_id = Uuid::new_v4().to_string();

    // 第一步：收集所有文件及大小
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

    let mut all_files: Vec<(std::path::PathBuf, u64)> = Vec::new();
    for p in &paths {
        let root = Path::new(p);
        if !root.exists() {
            continue;
        }
        let files = scanner::scan_directory(root, recursive, None);
        for f in files {
            if let Ok(meta) = std::fs::metadata(&f) {
                all_files.push((f, meta.len()));
            }
        }
    }

    let scanned_count = all_files.len() as u64;

    // 第二步：按文件大小分组（快速预筛）
    let mut size_groups: HashMap<u64, Vec<std::path::PathBuf>> = HashMap::new();
    for (path, size) in &all_files {
        if *size == 0 {
            continue;
        } // 跳过空文件
        size_groups.entry(*size).or_default().push(path.clone());
    }

    // 只保留 size 相同 >= 2 个的文件
    let candidates: Vec<(u64, Vec<std::path::PathBuf>)> = size_groups
        .into_iter()
        .filter(|(_, files)| files.len() >= 2)
        .collect();

    // 第三步：对候选文件计算 SHA-256 哈希，精确去重
    let mut hash_groups: HashMap<String, Vec<(std::path::PathBuf, u64)>> = HashMap::new();
    let total_candidates: u64 = candidates.iter().map(|(_, files)| files.len() as u64).sum();
    let mut hashed_count: u64 = 0;
    for (size, files) in &candidates {
        for file in files {
            hashed_count += 1;
            let _ = app.emit(
                "progress",
                ProgressPayload {
                    task_id: task_id.clone(),
                    current: hashed_count,
                    total: total_candidates,
                    current_file: file.to_string_lossy().into_owned(),
                    phase: "hashing".into(),
                },
            );
            match hasher::compute_sha256(file) {
                Ok(hash) => {
                    hash_groups
                        .entry(hash)
                        .or_default()
                        .push((file.clone(), *size));
                }
                Err(_) => continue,
            }
        }
    }

    // 第四步：构建结果
    let mut groups: Vec<DuplicateGroup> = Vec::new();
    let mut total_wasted_bytes: u64 = 0;

    for (hash, files) in &hash_groups {
        if files.len() < 2 {
            continue;
        }

        let file_size = files[0].1;
        // 浪费空间 = (份数 - 1) × 文件大小
        total_wasted_bytes += (files.len() as u64 - 1) * file_size;

        let mut dup_files: Vec<DuplicateFile> = files
            .iter()
            .enumerate()
            .map(|(i, (path, _))| {
                let meta = std::fs::metadata(path).ok();
                DuplicateFile {
                    path: path.to_string_lossy().into_owned(),
                    created_at: meta
                        .as_ref()
                        .and_then(|m| m.created().ok())
                        .map(|t| DateTime::<Local>::from(t).to_rfc3339())
                        .unwrap_or_default(),
                    modified_at: meta
                        .as_ref()
                        .and_then(|m| m.modified().ok())
                        .map(|t| DateTime::<Local>::from(t).to_rfc3339())
                        .unwrap_or_default(),
                    keep: i == 0, // 默认保留第一个
                }
            })
            .collect();

        // 按修改日期降序排，最新的标记 keep
        dup_files.sort_by(|a, b| b.modified_at.cmp(&a.modified_at));
        for f in dup_files.iter_mut() {
            f.keep = false;
        }
        if let Some(first) = dup_files.first_mut() {
            first.keep = true;
        }

        groups.push(DuplicateGroup {
            group_id: Uuid::new_v4().to_string(),
            hash: hash.clone(),
            file_size,
            files: dup_files,
        });
    }

    // 按浪费空间降序
    groups.sort_by(|a, b| {
        let waste_a = (a.files.len() as u64 - 1) * a.file_size;
        let waste_b = (b.files.len() as u64 - 1) * b.file_size;
        waste_b.cmp(&waste_a)
    });

    Ok(DuplicateResult {
        task_id: task_id.clone(),
        scanned_count,
        total_groups: groups.len() as u64,
        total_wasted_bytes,
        groups,
    })
}

#[command]
pub async fn delete_duplicates(
    app: AppHandle,
    paths_to_delete: Vec<String>,
) -> Result<String, String> {
    let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let task_id = Uuid::new_v4().to_string();
    let start = std::time::Instant::now();
    let total = paths_to_delete.len() as u64;
    let mut operations: Vec<Operation> = Vec::new();
    let mut succeeded: u64 = 0;
    let mut failed: u64 = 0;

    for (idx, file_path) in paths_to_delete.iter().enumerate() {
        let _ = app.emit(
            "progress",
            ProgressPayload {
                task_id: task_id.clone(),
                current: idx as u64,
                total,
                current_file: file_path.clone(),
                phase: "deleting".into(),
            },
        );

        let path = Path::new(file_path);
        let result = executor::safe_delete(path);

        let (status, error_message) = match &result {
            Ok(()) => {
                succeeded += 1;
                (OperationStatus::Success, None)
            }
            Err(e) => {
                failed += 1;
                (OperationStatus::Failed, Some(e.clone()))
            }
        };

        operations.push(Operation {
            op_id: Uuid::new_v4().to_string(),
            action: "delete".to_string(),
            source_path: file_path.clone(),
            target_path: String::new(),
            status,
            error_message,
            reversible: false,
        });
    }

    let duration_ms = start.elapsed().as_millis() as u64;

    let log = ExecutionLog {
        log_id: Uuid::new_v4().to_string(),
        task_id: task_id.clone(),
        rule_set_name: "重复文件清理".to_string(),
        executed_at: Local::now().to_rfc3339(),
        duration_ms,
        summary: ExecutionSummary {
            total,
            succeeded,
            failed,
            skipped: 0,
        },
        operations,
        undo_status: UndoStatus::Expired,
    };
    let _ = log_store::append(&data_dir, &log);

    let message = if failed == 0 {
        format!("成功删除 {} 个重复文件", succeeded)
    } else {
        format!("删除完成：成功 {}，失败 {}", succeeded, failed)
    };

    Ok(message)
}
