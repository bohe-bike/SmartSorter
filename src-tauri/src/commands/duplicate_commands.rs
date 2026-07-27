use crate::engine::{executor, hasher, scanner};
use crate::models::duplicate::{
    DuplicateDeleteRequest, DuplicateFile, DuplicateGroup, DuplicateResult, DuplicateScanError,
};
use crate::models::log::{ExecutionLog, ExecutionSummary, Operation, OperationStatus, UndoStatus};
use crate::models::progress::ProgressPayload;
use crate::storage::log_store;
use chrono::{DateTime, Local};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::{command, AppHandle, Emitter, Manager};
use uuid::Uuid;

/// 仅保留最近一次扫描，删除必须基于同一份扫描快照执行。
pub static DUPLICATE_SCAN_CACHE: Mutex<Option<DuplicateResult>> = Mutex::new(None);

#[command]
pub async fn scan_duplicates(
    app: AppHandle,
    paths: Vec<String>,
    recursive: bool,
) -> Result<DuplicateResult, String> {
    // async 使 Tauri 在后台线程池执行，不阻塞主窗口
    let task_id = Uuid::new_v4().to_string();

    // 第一步：收集所有文件及大小，并按文件大小预筛（spawn_blocking 避免阻塞 tokio 线程）
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

    let (scanned_count, candidates, mut errors) = tauri::async_runtime::spawn_blocking({
        let paths = paths.clone();
        move || {
            let mut all_files: Vec<(std::path::PathBuf, u64)> = Vec::new();
            let mut seen_paths = HashSet::new();
            let mut errors = Vec::new();
            for p in &paths {
                let root = std::path::Path::new(p);
                if !root.exists() {
                    errors.push(scan_error(p, "root_not_found", "扫描目录不存在"));
                    continue;
                }
                let files = scanner::scan_directory(root, recursive, None);
                for f in files {
                    if !seen_paths.insert(path_key(&f)) {
                        continue;
                    }
                    match std::fs::metadata(&f) {
                        Ok(meta) => all_files.push((f, meta.len())),
                        Err(error) => errors.push(scan_error(
                            &f.to_string_lossy(),
                            "metadata_failed",
                            &format!("读取文件信息失败: {}", error),
                        )),
                    }
                }
            }
            let scanned_count = all_files.len() as u64;
            // 第二步：按文件大小分组（快速预筛）
            let candidates = build_size_candidates(&all_files);
            (scanned_count, candidates, errors)
        }
    })
    .await
    .map_err(|e| e.to_string())?;

    // 第三步：对候选文件计算 SHA-256 哈希，精确去重（在 spawn_blocking 中执行，避免阻塞 tokio 线程）
    let total_candidates: u64 = candidates.iter().map(|(_, files)| files.len() as u64).sum();
    let app_clone = app.clone();
    let task_id_clone = task_id.clone();
    let (hash_groups, hash_errors): (
        HashMap<String, Vec<(std::path::PathBuf, u64)>>,
        Vec<DuplicateScanError>,
    ) = tauri::async_runtime::spawn_blocking(move || {
        let mut map: HashMap<String, Vec<(std::path::PathBuf, u64)>> = HashMap::new();
        let mut errors = Vec::new();
        let mut hashed_count: u64 = 0;
        for (size, files) in &candidates {
            for file in files {
                hashed_count += 1;
                let _ = app_clone.emit(
                    "progress",
                    ProgressPayload {
                        task_id: task_id_clone.clone(),
                        current: hashed_count,
                        total: total_candidates,
                        current_file: file.to_string_lossy().into_owned(),
                        phase: "hashing".into(),
                    },
                );
                match hasher::compute_sha256(file) {
                    Ok(hash) => {
                        map.entry(hash).or_default().push((file.clone(), *size));
                    }
                    Err(error) => errors.push(scan_error(
                        &file.to_string_lossy(),
                        "hash_failed",
                        &format!("计算 SHA-256 失败: {}", error),
                    )),
                }
            }
        }
        (map, errors)
    })
    .await
    .map_err(|e| e.to_string())?;

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

    errors.extend(hash_errors);
    let result = DuplicateResult {
        task_id: task_id.clone(),
        scanned_count,
        total_groups: groups.len() as u64,
        total_wasted_bytes,
        groups,
        errors,
    };
    let mut cache = DUPLICATE_SCAN_CACHE.lock().map_err(|e| e.to_string())?;
    *cache = Some(result.clone());

    Ok(result)
}

#[command]
pub async fn delete_duplicates(
    app: AppHandle,
    request: DuplicateDeleteRequest,
) -> Result<String, String> {
    let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let files_to_delete = {
        let cache = DUPLICATE_SCAN_CACHE.lock().map_err(|e| e.to_string())?;
        let snapshot = cache
            .as_ref()
            .ok_or_else(|| "没有可用的重复文件扫描结果，请重新扫描".to_string())?;
        if snapshot.task_id != request.task_id {
            return Err("扫描任务已过期，请重新扫描后再删除".into());
        }
        validate_delete_request(snapshot, &request.paths_to_delete)?
    };

    let task_id = request.task_id;
    let start = std::time::Instant::now();
    let total = files_to_delete.len() as u64;
    let mut operations: Vec<Operation> = Vec::new();
    let mut succeeded: u64 = 0;
    let mut failed: u64 = 0;

    for (idx, file) in files_to_delete.iter().enumerate() {
        let _ = app.emit(
            "progress",
            ProgressPayload {
                task_id: task_id.clone(),
                current: idx as u64,
                total,
                current_file: file.path.clone(),
                phase: "deleting".into(),
            },
        );

        let path = Path::new(&file.path);
        let result = verify_snapshot(path, file.file_size, &file.hash)
            .and_then(|_| executor::safe_delete(path));

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
            source_path: file.path.clone(),
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

    if let Ok(mut cache) = DUPLICATE_SCAN_CACHE.lock() {
        *cache = None;
    }

    let message = if failed == 0 {
        format!("成功删除 {} 个重复文件", succeeded)
    } else {
        format!("删除完成：成功 {}，失败 {}", succeeded, failed)
    };

    Ok(message)
}

#[derive(Debug, Clone)]
struct FileDeletionSnapshot {
    path: String,
    file_size: u64,
    hash: String,
}

fn scan_error(
    path: impl AsRef<str>,
    error: impl Into<String>,
    message: impl Into<String>,
) -> DuplicateScanError {
    DuplicateScanError {
        path: path.as_ref().to_string(),
        error: error.into(),
        message: message.into(),
    }
}

fn path_key(path: &Path) -> String {
    #[cfg(windows)]
    {
        let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        path.to_string_lossy().replace('/', "\\").to_lowercase()
    }
    #[cfg(not(windows))]
    {
        path.to_string_lossy().into_owned()
    }
}

fn build_size_candidates(files: &[(PathBuf, u64)]) -> Vec<(u64, Vec<PathBuf>)> {
    let mut size_groups: HashMap<u64, Vec<PathBuf>> = HashMap::new();
    for (path, size) in files {
        size_groups.entry(*size).or_default().push(path.clone());
    }
    size_groups
        .into_iter()
        .filter(|(_, files)| files.len() >= 2)
        .collect()
}

fn validate_delete_request(
    snapshot: &DuplicateResult,
    paths_to_delete: &[String],
) -> Result<Vec<FileDeletionSnapshot>, String> {
    let mut paths_by_key: HashMap<String, (&DuplicateGroup, &DuplicateFile)> = HashMap::new();
    for group in &snapshot.groups {
        for file in &group.files {
            paths_by_key.insert(path_key(Path::new(&file.path)), (group, file));
        }
    }

    let mut selected_keys = HashSet::new();
    let mut selected_per_group: HashMap<&str, usize> = HashMap::new();
    let mut files = Vec::new();
    for requested_path in paths_to_delete {
        let key = path_key(Path::new(requested_path));
        if !selected_keys.insert(key.clone()) {
            continue;
        }
        let (group, file) = paths_by_key
            .get(&key)
            .ok_or_else(|| format!("待删除文件不属于当前扫描结果: {}", requested_path))?;
        *selected_per_group
            .entry(group.group_id.as_str())
            .or_default() += 1;
        files.push(FileDeletionSnapshot {
            path: file.path.clone(),
            file_size: group.file_size,
            hash: group.hash.clone(),
        });
    }

    if files.is_empty() {
        return Err("没有可删除的重复文件".into());
    }
    for group in &snapshot.groups {
        if selected_per_group
            .get(group.group_id.as_str())
            .copied()
            .unwrap_or_default()
            >= group.files.len()
        {
            return Err(format!("重复组至少需要保留一个文件: {}", group.group_id));
        }
    }
    Ok(files)
}

fn verify_snapshot(path: &Path, expected_size: u64, expected_hash: &str) -> Result<(), String> {
    let actual_size = std::fs::metadata(path)
        .map_err(|error| format!("读取文件信息失败: {}", error))?
        .len();
    if actual_size != expected_size {
        return Err(format!(
            "文件大小已变更（扫描时 {} 字节，当前 {} 字节），已跳过删除",
            expected_size, actual_size
        ));
    }
    let actual_hash = hasher::compute_sha256(path)
        .map_err(|error| format!("重新计算 SHA-256 失败: {}", error))?;
    if actual_hash != expected_hash {
        return Err("文件内容已变更，已跳过删除".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn group(group_id: &str, paths: &[&str]) -> DuplicateGroup {
        DuplicateGroup {
            group_id: group_id.into(),
            hash: "snapshot-hash".into(),
            file_size: 0,
            files: paths
                .iter()
                .map(|path| DuplicateFile {
                    path: (*path).into(),
                    created_at: String::new(),
                    modified_at: String::new(),
                    keep: false,
                })
                .collect(),
        }
    }

    fn snapshot(groups: Vec<DuplicateGroup>) -> DuplicateResult {
        DuplicateResult {
            task_id: "task-1".into(),
            scanned_count: 0,
            total_groups: groups.len() as u64,
            total_wasted_bytes: 0,
            groups,
            errors: Vec::new(),
        }
    }

    #[test]
    fn path_keys_deduplicate_equivalent_windows_paths() {
        #[cfg(windows)]
        assert_eq!(
            path_key(Path::new("C:/Files/Report.txt")),
            path_key(Path::new("c:\\files\\report.txt"))
        );
        #[cfg(not(windows))]
        assert_ne!(
            path_key(Path::new("/Files/Report.txt")),
            path_key(Path::new("/files/report.txt"))
        );
    }

    #[test]
    fn delete_request_cannot_remove_every_file_in_a_group() {
        let result = snapshot(vec![group("group-1", &["one.txt", "two.txt"])]);
        let error =
            validate_delete_request(&result, &["one.txt".into(), "two.txt".into()]).unwrap_err();
        assert!(error.contains("至少需要保留一个"));
    }

    #[test]
    fn delete_request_rejects_files_outside_the_scan_snapshot() {
        let result = snapshot(vec![group("group-1", &["one.txt", "two.txt"])]);
        let error = validate_delete_request(&result, &["outside.txt".into()]).unwrap_err();
        assert!(error.contains("不属于当前扫描结果"));
    }

    #[test]
    fn changed_file_fails_snapshot_verification() {
        let path = std::env::temp_dir().join(format!("smart-sorter-duplicate-{}", Uuid::new_v4()));
        std::fs::write(&path, "before").unwrap();
        let hash = hasher::compute_sha256(&path).unwrap();
        let size = std::fs::metadata(&path).unwrap().len();
        std::fs::write(&path, "after!").unwrap();

        assert!(verify_snapshot(&path, size, &hash).is_err());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn zero_byte_files_are_valid_duplicate_candidates() {
        let candidates =
            build_size_candidates(&[(PathBuf::from("empty-a"), 0), (PathBuf::from("empty-b"), 0)]);
        assert_eq!(
            candidates,
            vec![(0, vec![PathBuf::from("empty-a"), PathBuf::from("empty-b")])]
        );
    }
}
