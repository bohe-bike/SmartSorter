use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chrono::{DateTime, Local};
use tauri::{command, AppHandle, Emitter, Manager};
use uuid::Uuid;

use crate::engine::{executor, metadata, scanner};
use crate::models::log::{ExecutionLog, ExecutionSummary, Operation, OperationStatus, UndoStatus};
use crate::models::media_classify::{
    ClassifyExecuteRequest, ClassifyPreviewItem, ClassifyPreviewResult, KeywordGroup, KeywordInfo,
    MediaClassifyResult, MediaFile,
};
use crate::models::progress::ProgressPayload;
use crate::storage::log_store;

pub static MEDIA_SCAN_CACHE: Mutex<Option<MediaClassifyResult>> = Mutex::new(None);
pub static MEDIA_PREVIEW_CACHE: Mutex<Option<ClassifyPreviewResult>> = Mutex::new(None);

#[command]
pub async fn scan_media_authors(
    app: AppHandle,
    paths: Vec<String>,
    recursive: bool,
    media_types: Vec<String>,
    keyword_sources: Vec<String>,
) -> Result<MediaClassifyResult, String> {
    let task_id = Uuid::new_v4().to_string();
    let filters = normalize_media_filters(&media_types);
    let sources: HashSet<String> = keyword_sources.iter().map(|s| s.to_lowercase()).collect();

    // ① 收集当前文件夹下的子文件夹名作为关键字
    let mut folder_keywords: Vec<String> = Vec::new();
    if sources.contains("folder_name") {
        for root_path in &paths {
            let root = Path::new(root_path);
            if !root.is_dir() {
                continue;
            }
            if let Ok(entries) = std::fs::read_dir(root) {
                for entry in entries.flatten() {
                    if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                        let name = entry.file_name().to_string_lossy().trim().to_string();
                        // 过滤空名称及极短名称（避免单字符误匹配大量文件）
                        if name.chars().count() >= 2 && !folder_keywords.contains(&name) {
                            folder_keywords.push(name);
                        }
                    }
                }
            }
        }
    }

    // ② 先在阻塞线程中快速扫描文件路径（不提取元数据）
    let (paths_for_scan, filters_for_scan) = (paths.clone(), filters.clone());
    let raw_files: Vec<(PathBuf, u64)> = tauri::async_runtime::spawn_blocking(move || {
        let mut files = Vec::new();
        for root_path in &paths_for_scan {
            let root = Path::new(root_path);
            if !root.exists() {
                continue;
            }
            for file in scanner::scan_directory(root, recursive, None) {
                let Some(mt) = metadata::get_media_type(&file) else {
                    continue;
                };
                let mt_name = metadata::media_type_name(mt);
                if !filters_for_scan.is_empty() && !filters_for_scan.iter().any(|v| v == mt_name) {
                    continue;
                }
                if let Ok(file_meta) = std::fs::metadata(&file) {
                    files.push((file, file_meta.len()));
                }
            }
        }
        files
    })
    .await
    .map_err(|e| e.to_string())?;

    // 去重：多源目录嵌套时同一文件可能被扫描多次
    let mut seen: HashSet<PathBuf> = HashSet::new();
    let raw_files: Vec<(PathBuf, u64)> = raw_files
        .into_iter()
        .filter(|(p, _)| seen.insert(p.clone()))
        .collect();

    let total = raw_files.len() as u64;

    // 在单个阻塞线程中批量提取元数据并发送进度事件（避免逐文件 spawn 开销）
    let app_clone = app.clone();
    let task_id_clone = task_id.clone();
    let media_files: Vec<(PathBuf, u64, metadata::MediaMetadata)> =
        tauri::async_runtime::spawn_blocking(move || {
            let mut files = Vec::new();
            for (index, (path, size_bytes)) in raw_files.into_iter().enumerate() {
                let _ = app_clone.emit(
                    "progress",
                    ProgressPayload {
                        task_id: task_id_clone.clone(),
                        current: index as u64 + 1,
                        total,
                        current_file: path.to_string_lossy().into_owned(),
                        phase: "scanning".into(),
                    },
                );
                let meta = metadata::extract_all_metadata(&path);
                files.push((path, size_bytes, meta));
            }
            files
        })
        .await
        .map_err(|e| e.to_string())?;

    // ③ 从元数据中收集关键字
    let mut metadata_keywords: HashSet<String> = HashSet::new();
    for (_, _, meta) in &media_files {
        if sources.contains("artist") {
            if let Some(ref v) = meta.artist {
                metadata_keywords.insert(v.clone());
            }
        }
        if sources.contains("album_artist") {
            if let Some(ref v) = meta.album_artist {
                metadata_keywords.insert(v.clone());
            }
        }
        if sources.contains("album") {
            if let Some(ref v) = meta.album {
                metadata_keywords.insert(v.clone());
            }
        }
        if sources.contains("composer") {
            if let Some(ref v) = meta.composer {
                metadata_keywords.insert(v.clone());
            }
        }
    }

    // ④ 合并所有关键字（用 HashSet 去重，避免 O(n²) 线性查找）
    // 同时过滤极短关键字（< 2 个字符），防止误匹配
    let all_keywords: Vec<String> = {
        let mut seen: HashSet<String> = HashSet::new();
        let mut combined: Vec<String> = Vec::new();
        for kw in folder_keywords.iter().chain(metadata_keywords.iter()) {
            if kw.chars().count() >= 2 && seen.insert(kw.clone()) {
                combined.push(kw.clone());
            }
        }
        combined
    };

    // ⑤ 关键字包含关系合并：若 A 包含 B 的文本，合并为 B（最短关键字）
    let merged_map = merge_containing_keywords(&all_keywords);
    // merged_map: 原始关键字 → 合并后关键字（最短的那个）
    let final_keywords: Vec<String> = merged_map
        .values()
        .cloned()
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();

    // 构建 KeywordInfo 列表
    let mut keyword_infos: Vec<KeywordInfo> = Vec::new();
    for kw in &final_keywords {
        let merged_from: Vec<String> = merged_map
            .iter()
            .filter(|(k, v)| v.as_str() == kw.as_str() && k.as_str() != kw.as_str())
            .map(|(k, _)| k.clone())
            .collect();
        let source = if folder_keywords.contains(kw) && metadata_keywords.contains(kw) {
            "folder_name,metadata"
        } else if folder_keywords.contains(kw) {
            "folder_name"
        } else {
            "metadata"
        };
        keyword_infos.push(KeywordInfo {
            keyword: kw.clone(),
            source: source.to_string(),
            merged_from,
            file_count: 0, // 后面填充
        });
    }

    // ⑥ 匹配文件到关键字（文件名关键字优先）
    let mut no_match_count = 0u64;
    let mut unmatched_files: Vec<MediaFile> = Vec::new();
    let mut grouped: HashMap<String, Vec<MediaFile>> = HashMap::new();

    for (index, (path, size_bytes, _meta)) in media_files.iter().enumerate() {
        let _ = app.emit(
            "progress",
            ProgressPayload {
                task_id: task_id.clone(),
                current: index as u64 + 1,
                total,
                current_file: path.to_string_lossy().into_owned(),
                phase: "matching".into(),
            },
        );

        let file_name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let file_stem = path
            .file_stem()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();

        // 先从文件名匹配关键字（大小写不敏感）
        let mut matched: Vec<String> = Vec::new();
        let file_stem_lower = file_stem.to_lowercase();
        for kw in &final_keywords {
            if file_stem_lower.contains(&kw.to_lowercase()) {
                matched.push(kw.clone());
            }
        }

        // 如果文件名没匹配到，再从元数据尝试（大小写不敏感）
        if matched.is_empty() {
            let meta = &media_files[index].2;
            let meta_values: Vec<&str> = [
                meta.artist.as_deref(),
                meta.album_artist.as_deref(),
                meta.album.as_deref(),
                meta.composer.as_deref(),
            ]
            .into_iter()
            .flatten()
            .collect();

            for kw in &final_keywords {
                let kw_lower = kw.to_lowercase();
                if meta_values.iter().any(|v| {
                    let v_lower = v.to_lowercase();
                    v_lower.contains(&kw_lower) || kw_lower.contains(&v_lower)
                }) {
                    if !matched.contains(kw) {
                        matched.push(kw.clone());
                    }
                }
            }
        }

        if matched.is_empty() {
            no_match_count += 1;
            let modified_at = std::fs::metadata(path)
                .ok()
                .and_then(|m| m.modified().ok())
                .map(|t| DateTime::<Local>::from(t).to_rfc3339())
                .unwrap_or_default();

            let media_file = MediaFile {
                path: path.to_string_lossy().into_owned(),
                file_name,
                size_bytes: *size_bytes,
                media_type: metadata::media_type_label(path)
                    .unwrap_or("unknown")
                    .to_string(),
                matched_keywords: Vec::new(),
                modified_at,
                checked: true,
            };
            unmatched_files.push(media_file);
            continue;
        }

        let modified_at = std::fs::metadata(path)
            .ok()
            .and_then(|m| m.modified().ok())
            .map(|t| DateTime::<Local>::from(t).to_rfc3339())
            .unwrap_or_default();

        let media_file = MediaFile {
            path: path.to_string_lossy().into_owned(),
            file_name,
            size_bytes: *size_bytes,
            media_type: metadata::media_type_label(path)
                .unwrap_or("unknown")
                .to_string(),
            matched_keywords: matched.clone(),
            modified_at,
            checked: true,
        };

        // 归入第一个匹配的关键字组（多匹配在前端让用户选）
        let primary_keyword = matched.first().unwrap().clone();
        grouped.entry(primary_keyword).or_default().push(media_file);
    }

    // ⑦ 构建分组结果
    let mut groups: Vec<KeywordGroup> = grouped
        .into_iter()
        .map(|(keyword, mut files)| {
            files.sort_by(|a, b| a.file_name.cmp(&b.file_name));
            let total_size = files.iter().map(|f| f.size_bytes).sum();
            let file_count = files.len() as u64;
            KeywordGroup {
                keyword,
                file_count,
                total_size,
                files,
            }
        })
        .collect();
    groups.sort_by(|a, b| a.keyword.cmp(&b.keyword));

    // 未匹配文件排序
    unmatched_files.sort_by(|a, b| a.file_name.cmp(&b.file_name));

    // 更新 keyword_infos 中的 file_count
    for info in &mut keyword_infos {
        info.file_count = groups
            .iter()
            .find(|g| g.keyword == info.keyword)
            .map(|g| g.file_count)
            .unwrap_or(0);
    }
    keyword_infos.sort_by(|a, b| a.keyword.cmp(&b.keyword));

    let result = MediaClassifyResult {
        task_id,
        source_paths: paths.clone(),
        scanned_count: total,
        total_keywords: groups.len() as u64,
        no_match_count,
        unmatched_files,
        keywords: keyword_infos,
        groups,
    };

    let mut cache = MEDIA_SCAN_CACHE.lock().map_err(|e| e.to_string())?;
    *cache = Some(result.clone());

    Ok(result)
}

#[command]
pub fn preview_media_classify(
    request: ClassifyExecuteRequest,
) -> Result<ClassifyPreviewResult, String> {
    let scan_result = {
        let cache = MEDIA_SCAN_CACHE.lock().map_err(|e| e.to_string())?;
        let result = cache
            .as_ref()
            .ok_or_else(|| "没有可用的媒体扫描结果，请先执行扫描".to_string())?;
        if result.task_id != request.task_id {
            return Err("任务 ID 不匹配，请重新扫描".into());
        }
        result.clone()
    };

    let selected: HashSet<&str> = request.selected_paths.iter().map(|s| s.as_str()).collect();
    // 追踪本批次已分配的目标路径，防止同名覆盖
    let mut used_targets: HashSet<PathBuf> = HashSet::new();

    let mut items = Vec::new();
    for group in &scan_result.groups {
        for file in &group.files {
            if !selected.contains(file.path.as_str()) {
                continue; // 用户未勾选，跳过
            }

            // 获取用户指定的关键字（多匹配时由前端选择），否则取默认所属组
            let keyword = request
                .keyword_assignments
                .get(&file.path)
                .cloned()
                .unwrap_or_else(|| group.keyword.clone());

            let source = Path::new(&file.path);
            let root_dir = find_root_dir(source, &scan_result.source_paths);
            let base_target = build_target_path(source, &keyword, &root_dir)?;
            let target = resolve_unique_target(base_target, &mut used_targets);

            if paths_equal(target.as_path(), source) {
                continue; // 已在正确位置，跳过
            }

            items.push(ClassifyPreviewItem {
                source_path: file.path.clone(),
                target_path: target.to_string_lossy().into_owned(),
                action_desc: format!("移动到 {} 并重命名", keyword),
                size_bytes: file.size_bytes,
            });
        }
    }

    // 处理未匹配的文件（需要用户手动指定关键字）
    for file in &scan_result.unmatched_files {
        if !selected.contains(file.path.as_str()) {
            continue;
        }
        if let Some(keyword) = request.keyword_assignments.get(&file.path) {
            let source = Path::new(&file.path);
            let root_dir = find_root_dir(source, &scan_result.source_paths);
            let base_target = build_target_path(source, keyword, &root_dir)?;
            let target = resolve_unique_target(base_target, &mut used_targets);

            if paths_equal(target.as_path(), source) {
                continue; // 已在正确位置，跳过
            }

            items.push(ClassifyPreviewItem {
                source_path: file.path.clone(),
                target_path: target.to_string_lossy().into_owned(),
                action_desc: format!("移动到 {} 并重命名", keyword),
                size_bytes: file.size_bytes,
            });
        }
    }

    let preview = ClassifyPreviewResult {
        task_id: scan_result.task_id,
        total: items.len() as u64,
        items,
    };

    let mut cache = MEDIA_PREVIEW_CACHE.lock().map_err(|e| e.to_string())?;
    *cache = Some(preview.clone());

    Ok(preview)
}

#[command]
pub async fn execute_media_classify(app: AppHandle, task_id: String) -> Result<String, String> {
    let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let preview = {
        let cache = MEDIA_PREVIEW_CACHE.lock().map_err(|e| e.to_string())?;
        let preview = cache
            .as_ref()
            .ok_or_else(|| "没有可用的归类预览结果，请先生成预览".to_string())?;
        if preview.task_id != task_id {
            return Err("任务 ID 不匹配，请重新生成预览".into());
        }
        preview.clone()
    };

    // 获取扫描根目录，用于限制空目录清理不越过根目录
    let source_roots: HashSet<PathBuf> = {
        let cache = MEDIA_SCAN_CACHE.lock().map_err(|e| e.to_string())?;
        cache
            .as_ref()
            .map(|r| r.source_paths.iter().map(PathBuf::from).collect())
            .unwrap_or_default()
    };

    let start = std::time::Instant::now();
    let mut operations = Vec::new();
    let mut succeeded = 0u64;
    let mut failed = 0u64;
    let total = preview.items.len() as u64;

    // 收集源文件的父目录，用于后续清理空文件夹
    let mut source_parents: HashSet<PathBuf> = HashSet::new();

    for (index, item) in preview.items.iter().enumerate() {
        let _ = app.emit(
            "progress",
            ProgressPayload {
                task_id: task_id.clone(),
                current: index as u64 + 1,
                total,
                current_file: item.source_path.clone(),
                phase: "executing".into(),
            },
        );

        let source = Path::new(&item.source_path);
        let target = Path::new(&item.target_path);

        if let Some(parent) = source.parent() {
            source_parents.insert(parent.to_path_buf());
        }

        let result = executor::safe_move(source, target);

        let (status, error_message) = match &result {
            Ok(()) => {
                succeeded += 1;
                (OperationStatus::Success, None)
            }
            Err(err) => {
                failed += 1;
                (OperationStatus::Failed, Some(err.clone()))
            }
        };

        operations.push(Operation {
            op_id: Uuid::new_v4().to_string(),
            action: "move".to_string(),
            source_path: item.source_path.clone(),
            target_path: item.target_path.clone(),
            status,
            error_message,
            reversible: true,
        });
    }

    // 清理空文件夹（向上递归，但不越过扫描根目录）
    for parent in &source_parents {
        let _ = remove_empty_dir_recursive(parent, &source_roots);
    }

    let log = ExecutionLog {
        log_id: Uuid::new_v4().to_string(),
        task_id,
        rule_set_name: "媒体关键字归类".to_string(),
        executed_at: Local::now().to_rfc3339(),
        duration_ms: start.elapsed().as_millis() as u64,
        summary: ExecutionSummary {
            total: operations.len() as u64,
            succeeded,
            failed,
            skipped: 0,
        },
        operations,
        undo_status: UndoStatus::Available,
    };

    log_store::append(&data_dir, &log)?;

    if failed > 0 {
        return Err(format!("执行完成：{} 成功，{} 失败", succeeded, failed));
    }

    Ok(format!("执行完成：{} 个文件已归类", succeeded))
}

/// 构建目标路径：移动到关键字子文件夹 + 重命名为 "关键字-主题.后缀"
fn build_target_path(source: &Path, keyword: &str, root_dir: &Path) -> Result<PathBuf, String> {
    let stem = source
        .file_stem()
        .map(|v| v.to_string_lossy().into_owned())
        .unwrap_or_default();
    let extension = source
        .extension()
        .map(|v| v.to_string_lossy().into_owned())
        .unwrap_or_default();

    let safe_keyword = sanitize_segment(keyword);

    // 构建主题：大小写不敏感地从文件名中移除第一次出现的关键字
    let topic = ci_remove_first(&stem, keyword);
    // 清理主题中的前后分隔符
    let topic = topic
        .trim_matches(|c: char| c == '-' || c == '_' || c == ' ' || c == '　')
        .to_string();
    let topic = if topic.is_empty() {
        stem.clone()
    } else {
        topic
    };

    // 新文件名: 关键字-主题.后缀
    let new_name = if extension.is_empty() {
        format!("{}-{}", safe_keyword, sanitize_segment(&topic))
    } else {
        format!(
            "{}-{}.{}",
            safe_keyword,
            sanitize_segment(&topic),
            extension
        )
    };

    let target = root_dir.join(&safe_keyword).join(&new_name);
    Ok(target)
}

/// 找到文件对应的扫描根目录（Windows 上大小写不敏感）
fn find_root_dir(source: &Path, source_paths: &[String]) -> PathBuf {
    for sp in source_paths {
        let root = Path::new(sp);
        #[cfg(target_os = "windows")]
        let matches = {
            let src_lower = source.to_string_lossy().to_lowercase();
            let root_lower = root.to_string_lossy().to_lowercase();
            src_lower.starts_with(&root_lower)
        };
        #[cfg(not(target_os = "windows"))]
        let matches = source.starts_with(root);
        if matches {
            return root.to_path_buf();
        }
    }
    // fallback: 使用文件的直接父目录
    source.parent().unwrap_or(Path::new(".")).to_path_buf()
}

/// 关键字包含关系合并（大小写不敏感）
/// 如果关键字 A 包含关键字 B 的文本（如 "小凛蝶子" 包含 "蝶子"），合并为最短的 B
fn merge_containing_keywords(keywords: &[String]) -> HashMap<String, String> {
    let mut result: HashMap<String, String> = HashMap::new();

    for kw in keywords {
        let kw_lower = kw.to_lowercase();
        let mut shortest = kw.clone();
        let mut shortest_len = kw.len();
        for other in keywords {
            if kw == other {
                continue;
            }
            // 大小写不敏感：若 kw 包含 other，则合并到更短的 other
            if kw_lower.contains(&other.to_lowercase()) && other.len() < shortest_len {
                shortest = other.clone();
                shortest_len = other.len();
            }
        }
        result.insert(kw.clone(), shortest);
    }

    result
}

/// 递归删除空目录（向上递归，但不删除扫描根目录及其祖先）
fn remove_empty_dir_recursive(path: &Path, roots: &HashSet<PathBuf>) -> Result<(), String> {
    if !path.is_dir() || roots.contains(path) {
        return Ok(());
    }
    let entries: Vec<_> = std::fs::read_dir(path)
        .map_err(|e| e.to_string())?
        .flatten()
        .collect();
    if entries.is_empty() {
        std::fs::remove_dir(path).map_err(|e| e.to_string())?;
        if let Some(parent) = path.parent() {
            let _ = remove_empty_dir_recursive(parent, roots);
        }
    }
    Ok(())
}

fn sanitize_segment(value: &str) -> String {
    let mut sanitized = String::with_capacity(value.len());
    for ch in value.chars() {
        if matches!(ch, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') {
            sanitized.push('_');
        } else {
            sanitized.push(ch);
        }
    }
    let trimmed = sanitized.trim().trim_end_matches('.').to_string();
    if trimmed.is_empty() {
        "未命名".to_string()
    } else {
        trimmed
    }
}

/// 在 Windows（大小写不敏感文件系统）上忽略大小写比较路径
#[cfg(target_os = "windows")]
fn paths_equal(a: &Path, b: &Path) -> bool {
    a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase()
}

#[cfg(not(target_os = "windows"))]
fn paths_equal(a: &Path, b: &Path) -> bool {
    a == b
}

/// 大小写不敏感地移除字符串中第一次出现的 pattern（仅处理 ASCII/CJK 等常见媒体文件名字符）
fn ci_remove_first(s: &str, pattern: &str) -> String {
    let s_lower = s.to_lowercase();
    let p_lower = pattern.to_lowercase();
    if let Some(start) = s_lower.find(&p_lower) {
        let end = start + p_lower.len();
        if s.is_char_boundary(start) && s.is_char_boundary(end) {
            return format!("{}{}", &s[..start], &s[end..]);
        }
    }
    s.to_string()
}

/// 若目标路径已被磁盘占用或本批次已分配，自动追加 (2)、(3)… 后缀避免覆盖
fn resolve_unique_target(initial: PathBuf, used: &mut HashSet<PathBuf>) -> PathBuf {
    if !initial.exists() && !used.contains(&initial) {
        used.insert(initial.clone());
        return initial;
    }
    let stem = initial
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ext = initial
        .extension()
        .map(|e| e.to_string_lossy().into_owned())
        .unwrap_or_default();
    let parent = initial.parent().unwrap_or(Path::new("."));
    for counter in 2u32..=9999 {
        let new_name = if ext.is_empty() {
            format!("{} ({})", stem, counter)
        } else {
            format!("{} ({}).{}", stem, counter, ext)
        };
        let candidate = parent.join(&new_name);
        if !candidate.exists() && !used.contains(&candidate) {
            used.insert(candidate.clone());
            return candidate;
        }
    }
    // 极端情况兜底
    used.insert(initial.clone());
    initial
}

fn normalize_media_filters(filters: &[String]) -> Vec<String> {
    filters
        .iter()
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| matches!(value.as_str(), "image" | "audio" | "video" | "ebook"))
        .collect()
}
