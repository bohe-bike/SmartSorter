use std::path::Path;
use std::collections::HashMap;
use tauri::{command, AppHandle, Emitter};
use uuid::Uuid;
use chrono::{DateTime, Local};
use crate::models::duplicate::{DuplicateResult, DuplicateGroup, DuplicateFile};
use crate::models::progress::ProgressPayload;
use crate::engine::{scanner, hasher};

#[command]
pub async fn scan_duplicates(app: AppHandle, paths: Vec<String>, recursive: bool) -> Result<DuplicateResult, String> {
    // async 使 Tauri 在后台线程池执行，不阻塞主窗口
    let task_id = Uuid::new_v4().to_string();

    // 第一步：收集所有文件及大小
    let _ = app.emit("progress", ProgressPayload {
        task_id: task_id.clone(), current: 0, total: 0,
        current_file: String::new(), phase: "scanning".into(),
    });

    let mut all_files: Vec<(std::path::PathBuf, u64)> = Vec::new();
    for p in &paths {
        let root = Path::new(p);
        if !root.exists() { continue; }
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
        if *size == 0 { continue; } // 跳过空文件
        size_groups.entry(*size).or_default().push(path.clone());
    }

    // 只保留 size 相同 >= 2 个的文件
    let candidates: Vec<(u64, Vec<std::path::PathBuf>)> = size_groups.into_iter()
        .filter(|(_, files)| files.len() >= 2)
        .collect();

    // 第三步：对候选文件计算 SHA-256 哈希，精确去重
    let mut hash_groups: HashMap<String, Vec<(std::path::PathBuf, u64)>> = HashMap::new();
    let total_candidates: u64 = candidates.iter().map(|(_, files)| files.len() as u64).sum();
    let mut hashed_count: u64 = 0;
    for (size, files) in &candidates {
        for file in files {
            hashed_count += 1;
            let _ = app.emit("progress", ProgressPayload {
                task_id: task_id.clone(), current: hashed_count, total: total_candidates,
                current_file: file.to_string_lossy().into_owned(), phase: "hashing".into(),
            });
            match hasher::compute_sha256(file) {
                Ok(hash) => {
                    hash_groups.entry(hash).or_default().push((file.clone(), *size));
                }
                Err(_) => continue,
            }
        }
    }

    // 第四步：构建结果
    let mut groups: Vec<DuplicateGroup> = Vec::new();
    let mut total_wasted_bytes: u64 = 0;

    for (hash, files) in &hash_groups {
        if files.len() < 2 { continue; }

        let file_size = files[0].1;
        // 浪费空间 = (份数 - 1) × 文件大小
        total_wasted_bytes += (files.len() as u64 - 1) * file_size;

        let mut dup_files: Vec<DuplicateFile> = files.iter().enumerate().map(|(i, (path, _))| {
            let meta = std::fs::metadata(path).ok();
            DuplicateFile {
                path: path.to_string_lossy().into_owned(),
                created_at: meta.as_ref()
                    .and_then(|m| m.created().ok())
                    .map(|t| DateTime::<Local>::from(t).to_rfc3339())
                    .unwrap_or_default(),
                modified_at: meta.as_ref()
                    .and_then(|m| m.modified().ok())
                    .map(|t| DateTime::<Local>::from(t).to_rfc3339())
                    .unwrap_or_default(),
                keep: i == 0, // 默认保留第一个
            }
        }).collect();

        // 按修改日期降序排，最新的标记 keep
        dup_files.sort_by(|a, b| b.modified_at.cmp(&a.modified_at));
        for f in dup_files.iter_mut() { f.keep = false; }
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
