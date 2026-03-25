use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chrono::{DateTime, Local};
use tauri::{command, AppHandle, Emitter, Manager};
use uuid::Uuid;

use crate::engine::{executor, metadata, scanner};
use crate::models::log::{ExecutionLog, ExecutionSummary, Operation, OperationStatus, UndoStatus};
use crate::models::media_classify::{
    AuthorGroup, ClassifyAction, ClassifyExecuteRequest, ClassifyPreviewItem, ClassifyPreviewResult,
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
) -> Result<MediaClassifyResult, String> {
    let task_id = Uuid::new_v4().to_string();
    let mut media_files: Vec<(PathBuf, u64)> = Vec::new();
    let filters = normalize_media_filters(&media_types);

    for root_path in &paths {
        let root = Path::new(root_path);
        if !root.exists() {
            continue;
        }

        for file in scanner::scan_directory(root, recursive, None) {
            let Some(media_type) = metadata::get_media_type(&file) else { continue; };
            let media_type_name = metadata::media_type_name(media_type);
            if !filters.is_empty() && !filters.iter().any(|value| value == media_type_name) {
                continue;
            }

            if let Ok(file_meta) = std::fs::metadata(&file) {
                media_files.push((file, file_meta.len()));
            }
        }
    }

    let total = media_files.len() as u64;
    let mut no_author_count = 0u64;
    let mut grouped: HashMap<String, Vec<MediaFile>> = HashMap::new();

    let _ = app.emit(
        "progress",
        ProgressPayload {
            task_id: task_id.clone(),
            current: 0,
            total,
            current_file: String::new(),
            phase: "extracting".into(),
        },
    );

    for (index, (path, size_bytes)) in media_files.iter().enumerate() {
        let _ = app.emit(
            "progress",
            ProgressPayload {
                task_id: task_id.clone(),
                current: index as u64 + 1,
                total,
                current_file: path.to_string_lossy().into_owned(),
                phase: "extracting".into(),
            },
        );

        let Some(author) = metadata::extract_author(path) else {
            no_author_count += 1;
            continue;
        };

        let modified_at = std::fs::metadata(path)
            .ok()
            .and_then(|meta| meta.modified().ok())
            .map(|time| DateTime::<Local>::from(time).to_rfc3339())
            .unwrap_or_default();

        grouped.entry(author.clone()).or_default().push(MediaFile {
            path: path.to_string_lossy().into_owned(),
            file_name: path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default(),
            size_bytes: *size_bytes,
            media_type: metadata::media_type_label(path).unwrap_or("unknown").to_string(),
            author,
            modified_at,
            checked: true,
        });
    }

    let mut groups: Vec<AuthorGroup> = grouped
        .into_iter()
        .map(|(author, mut files)| {
            files.sort_by(|a, b| a.file_name.cmp(&b.file_name));
            let total_size = files.iter().map(|item| item.size_bytes).sum();
            let file_count = files.len() as u64;
            AuthorGroup {
                author,
                file_count,
                total_size,
                files,
            }
        })
        .collect();

    groups.sort_by(|a, b| a.author.cmp(&b.author));

    let result = MediaClassifyResult {
        task_id,
        scanned_count: total,
        total_authors: groups.len() as u64,
        no_author_count,
        groups,
    };

    let mut cache = MEDIA_SCAN_CACHE.lock().map_err(|e| e.to_string())?;
    *cache = Some(result.clone());

    Ok(result)
}

#[command]
pub fn preview_media_classify(request: ClassifyExecuteRequest) -> Result<ClassifyPreviewResult, String> {
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

    let rename_template = request
        .rename_template
        .clone()
        .unwrap_or_else(|| "{author} - {filename}".to_string());

    let mut items = Vec::new();
    for group in &scan_result.groups {
        for file in &group.files {
            if !request.checked_paths.contains(&file.path) {
                continue;
            }

            let source = Path::new(&file.path);
            let target = build_target_path(source, &group.author, &request.action, &rename_template)?;
            items.push(ClassifyPreviewItem {
                source_path: file.path.clone(),
                target_path: target.to_string_lossy().into_owned(),
                action_desc: describe_action(&request.action, &group.author),
            });
        }
    }

    let preview = ClassifyPreviewResult {
        task_id: scan_result.task_id,
        action: request.action,
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

    let start = std::time::Instant::now();
    let mut operations = Vec::new();
    let mut succeeded = 0u64;
    let mut failed = 0u64;
    let total = preview.items.len() as u64;

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

        let result = match preview.action {
            ClassifyAction::MoveToAuthorFolder => executor::safe_move(source, target),
            ClassifyAction::Rename => executor::safe_rename(source, target),
        };

        let action_name = match preview.action {
            ClassifyAction::MoveToAuthorFolder => "move",
            ClassifyAction::Rename => "rename",
        };

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
            action: action_name.to_string(),
            source_path: item.source_path.clone(),
            target_path: item.target_path.clone(),
            status,
            error_message,
            reversible: true,
        });
    }

    let log = ExecutionLog {
        log_id: Uuid::new_v4().to_string(),
        task_id,
        rule_set_name: "媒体作者归类".to_string(),
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

fn build_target_path(
    source: &Path,
    author: &str,
    action: &ClassifyAction,
    rename_template: &str,
) -> Result<PathBuf, String> {
    let parent = source
        .parent()
        .ok_or_else(|| format!("无法确定父目录: {}", source.display()))?;
    let stem = source
        .file_stem()
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_default();
    let extension = source
        .extension()
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_default();
    let safe_author = sanitize_segment(author);

    let target = match action {
        ClassifyAction::MoveToAuthorFolder => parent.join(safe_author).join(
            source
                .file_name()
                .map(|value| value.to_os_string())
                .unwrap_or_default(),
        ),
        ClassifyAction::Rename => {
            let file_name = rename_template
                .replace("{author}", author)
                .replace("{filename}", &stem)
                .replace("{extension}", &extension);
            let final_name = if extension.is_empty() {
                sanitize_segment(&file_name)
            } else {
                format!("{}.{}", sanitize_segment(&file_name), extension)
            };
            parent.join(final_name)
        }
    };

    if target == source {
        return Err(format!("目标路径与源路径相同: {}", source.display()));
    }

    Ok(target)
}

fn describe_action(action: &ClassifyAction, author: &str) -> String {
    match action {
        ClassifyAction::MoveToAuthorFolder => format!("移动到 {author} 文件夹"),
        ClassifyAction::Rename => format!("重命名为 {author} - 原文件名"),
    }
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

fn normalize_media_filters(filters: &[String]) -> Vec<String> {
    filters
        .iter()
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| matches!(value.as_str(), "image" | "audio" | "video" | "ebook"))
        .collect()
}