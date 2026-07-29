use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chrono::Local;
use tauri::{command, AppHandle, Emitter, Manager};
use uuid::Uuid;

use crate::engine::{metadata, scanner};
use crate::models::log::{ExecutionLog, ExecutionSummary, Operation, OperationStatus, UndoStatus};
use crate::models::media_tag_cleanup::{
    MediaTagCleanupExecuteRequest, MediaTagCleanupFile, MediaTagCleanupResult,
};
use crate::models::progress::ProgressPayload;
use crate::storage::{log_store, media_classify_store};

pub static MEDIA_TAG_CLEANUP_CACHE: Mutex<Option<MediaTagCleanupResult>> = Mutex::new(None);

#[command]
pub async fn scan_media_tag_cleanup(
    app: AppHandle,
    paths: Vec<String>,
    recursive: bool,
) -> Result<MediaTagCleanupResult, String> {
    if paths.is_empty() {
        return Err("请至少选择一个扫描目录".into());
    }

    let task_id = Uuid::new_v4().to_string();
    let _ = app.emit(
        "progress",
        ProgressPayload {
            task_id: task_id.clone(),
            current: 0,
            total: 0,
            current_file: String::new(),
            phase: "collecting".into(),
        },
    );
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?;
    let aliases = media_classify_store::load(&data_dir)?
        .aliases
        .into_iter()
        .map(|alias| (normalize_author_key(&alias.alias), alias.canonical))
        .filter(|(alias, canonical)| !alias.is_empty() && !canonical.trim().is_empty())
        .collect::<HashMap<_, _>>();

    let paths_for_scan = paths.clone();
    let app_for_scan = app.clone();
    let task_id_for_scan = task_id.clone();
    let raw_files = tauri::async_runtime::spawn_blocking(move || {
        let mut files = Vec::new();
        let mut visited_files = 0u64;
        for root_path in &paths_for_scan {
            let root = Path::new(root_path);
            if !root.is_dir() {
                continue;
            }
            let scanned_files =
                scanner::scan_directory_with_progress(root, recursive, None, |path| {
                    visited_files += 1;
                    if visited_files == 1 || visited_files % 100 == 0 {
                        let _ = app_for_scan.emit(
                            "progress",
                            ProgressPayload {
                                task_id: task_id_for_scan.clone(),
                                current: visited_files,
                                total: 0,
                                current_file: path.to_string_lossy().into_owned(),
                                phase: "collecting".into(),
                            },
                        );
                    }
                });
            for path in scanned_files {
                if matches!(
                    metadata::get_media_type(&path),
                    Some(metadata::MediaType::Audio | metadata::MediaType::Video)
                ) {
                    let size_bytes = std::fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0);
                    files.push((path, size_bytes));
                }
            }
        }
        files
    })
    .await
    .map_err(|error| error.to_string())?;

    let mut seen = HashSet::new();
    let raw_files = raw_files
        .into_iter()
        .filter(|(path, _)| seen.insert(path_key(path)))
        .collect::<Vec<_>>();
    let total = raw_files.len() as u64;
    let app_for_extract = app.clone();
    let task_id_for_extract = task_id.clone();
    let paths_for_author = paths.clone();
    let files = tauri::async_runtime::spawn_blocking(move || {
        let mut files = Vec::new();
        for (index, (path, size_bytes)) in raw_files.into_iter().enumerate() {
            let _ = app_for_extract.emit(
                "progress",
                ProgressPayload {
                    task_id: task_id_for_extract.clone(),
                    current: index as u64 + 1,
                    total,
                    current_file: path.to_string_lossy().into_owned(),
                    phase: "scanning".into(),
                },
            );
            let media = metadata::extract_all_metadata(&path);
            let (target_artist, author_source) =
                infer_target_artist(&path, &paths_for_author, &aliases, media.artist.as_deref());
            let supported = metadata::supports_tag_cleanup(&path);
            let skip_reason = if !supported {
                Some("该格式暂不支持安全标签清洗".to_string())
            } else if target_artist.is_none() {
                Some("未找到一级作者目录，且文件没有可用的 Artist 标签".to_string())
            } else {
                None
            };
            files.push(MediaTagCleanupFile {
                path: path.to_string_lossy().into_owned(),
                file_name: path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                size_bytes,
                media_type: metadata::media_type_label(&path)
                    .unwrap_or("unknown")
                    .to_string(),
                current_artist: media.artist,
                target_artist: target_artist.clone(),
                author_source,
                supported,
                skip_reason,
                checked: supported && target_artist.is_some(),
            });
        }
        files
    })
    .await
    .map_err(|error| error.to_string())?;

    let ready_count = files.iter().filter(|file| file.checked).count() as u64;
    let result = MediaTagCleanupResult {
        task_id,
        source_paths: paths,
        scanned_count: total,
        ready_count,
        files,
    };
    *MEDIA_TAG_CLEANUP_CACHE
        .lock()
        .map_err(|error| error.to_string())? = Some(result.clone());
    Ok(result)
}

#[command]
pub async fn execute_media_tag_cleanup(
    app: AppHandle,
    request: MediaTagCleanupExecuteRequest,
) -> Result<String, String> {
    let scan_result = {
        let cache = MEDIA_TAG_CLEANUP_CACHE
            .lock()
            .map_err(|error| error.to_string())?;
        let result = cache
            .as_ref()
            .ok_or_else(|| "没有可用的标签清洗扫描结果，请先扫描".to_string())?;
        if result.task_id != request.task_id {
            return Err("任务 ID 不匹配，请重新扫描".into());
        }
        result.clone()
    };
    if request.selected_paths.is_empty() {
        return Err("请至少选择一个可清洗的媒体文件".into());
    }

    let selected_paths = request
        .selected_paths
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    let selected_files = scan_result
        .files
        .iter()
        .filter(|file| selected_paths.contains(file.path.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    if selected_files.len() != selected_paths.len() {
        return Err("包含不属于当前扫描结果的文件，请重新扫描".into());
    }

    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?;
    let task_id = request.task_id.clone();
    let assignments = request.author_assignments;
    let app_for_execute = app.clone();
    let (succeeded, failed, skipped, _operations) =
        tauri::async_runtime::spawn_blocking(move || {
            let start = std::time::Instant::now();
            let total = selected_files.len() as u64;
            let mut succeeded = 0u64;
            let mut failed = 0u64;
            let mut skipped = 0u64;
            let mut operations = Vec::new();

            for (index, file) in selected_files.iter().enumerate() {
                let _ = app_for_execute.emit(
                    "progress",
                    ProgressPayload {
                        task_id: task_id.clone(),
                        current: index as u64 + 1,
                        total,
                        current_file: file.path.clone(),
                        phase: "executing".into(),
                    },
                );
                let target_artist = assignments
                    .get(&file.path)
                    .map(String::as_str)
                    .or(file.target_artist.as_deref())
                    .map(str::trim)
                    .filter(|artist| !artist.is_empty());
                let (status, error_message, artist_label) = if !file.supported {
                    skipped += 1;
                    (
                        OperationStatus::Skipped,
                        file.skip_reason
                            .clone()
                            .or_else(|| Some("文件格式不支持".into())),
                        String::new(),
                    )
                } else if let Some(artist) = target_artist {
                    match metadata::clean_tags_and_set_artist(Path::new(&file.path), artist) {
                        Ok(()) => {
                            succeeded += 1;
                            (OperationStatus::Success, None, artist.to_string())
                        }
                        Err(error) => {
                            failed += 1;
                            (OperationStatus::Failed, Some(error), artist.to_string())
                        }
                    }
                } else {
                    skipped += 1;
                    (
                        OperationStatus::Skipped,
                        Some("缺少作者名称".into()),
                        String::new(),
                    )
                };
                operations.push(Operation {
                    op_id: Uuid::new_v4().to_string(),
                    action: "tag_cleanup".into(),
                    source_path: file.path.clone(),
                    target_path: if artist_label.is_empty() {
                        "未写入标签".into()
                    } else {
                        format!("Artist / AlbumArtist = {}", artist_label)
                    },
                    status,
                    error_message,
                    reversible: false,
                    target_hash: None,
                });
            }

            let log = ExecutionLog {
                log_id: Uuid::new_v4().to_string(),
                task_id,
                rule_set_name: "媒体标签清洗".into(),
                executed_at: Local::now().to_rfc3339(),
                duration_ms: start.elapsed().as_millis() as u64,
                summary: ExecutionSummary {
                    total,
                    succeeded,
                    failed,
                    skipped,
                },
                operations: operations.clone(),
                undo_status: UndoStatus::Expired,
            };
            log_store::append(&data_dir, &log)?;
            Ok::<_, String>((succeeded, failed, skipped, operations))
        })
        .await
        .map_err(|error| error.to_string())??;

    *MEDIA_TAG_CLEANUP_CACHE
        .lock()
        .map_err(|error| error.to_string())? = None;
    Ok(format!(
        "标签清洗完成：成功 {}，失败 {}，跳过 {}",
        succeeded, failed, skipped
    ))
}

fn infer_target_artist(
    path: &Path,
    roots: &[String],
    aliases: &HashMap<String, String>,
    current_artist: Option<&str>,
) -> (Option<String>, String) {
    if let Some(folder) = direct_child_folder_name(path, roots) {
        return (
            Some(canonical_author(&folder, aliases)),
            "一级作者目录".into(),
        );
    }
    if let Some(artist) = current_artist
        .map(str::trim)
        .filter(|artist| !artist.is_empty())
    {
        return (
            Some(canonical_author(artist, aliases)),
            "现有 Artist 标签".into(),
        );
    }
    (None, "未识别".into())
}

fn direct_child_folder_name(path: &Path, roots: &[String]) -> Option<String> {
    let root = roots
        .iter()
        .map(PathBuf::from)
        .filter(|root| path_is_within(path, root))
        .max_by_key(|root| root.components().count())?;
    let relative = path.strip_prefix(root).ok()?;
    let mut parts = relative
        .components()
        .filter_map(|component| match component {
            std::path::Component::Normal(part) => part.to_str(),
            _ => None,
        });
    let author = parts.next()?;
    parts.next()?;
    let author = author.trim();
    (!author.is_empty()).then(|| author.to_string())
}

fn canonical_author(author: &str, aliases: &HashMap<String, String>) -> String {
    aliases
        .get(&normalize_author_key(author))
        .cloned()
        .unwrap_or_else(|| author.trim().to_string())
}

fn normalize_author_key(value: &str) -> String {
    value
        .trim()
        .to_lowercase()
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect()
}

#[cfg(target_os = "windows")]
fn path_is_within(path: &Path, root: &Path) -> bool {
    let path = path.to_string_lossy().replace('/', "\\").to_lowercase();
    let root = root.to_string_lossy().replace('/', "\\").to_lowercase();
    path.starts_with(&format!("{}\\", root.trim_end_matches('\\')))
}

#[cfg(not(target_os = "windows"))]
fn path_is_within(path: &Path, root: &Path) -> bool {
    path.starts_with(root)
}

#[cfg(target_os = "windows")]
fn path_key(path: &Path) -> String {
    path.to_string_lossy().replace('/', "\\").to_lowercase()
}

#[cfg(not(target_os = "windows"))]
fn path_key(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefers_the_organized_author_folder_over_an_existing_tag() {
        let aliases = HashMap::from([("alias".to_string(), "标准作者".to_string())]);
        let (artist, source) = infer_target_artist(
            Path::new(r"D:\Media\Alias\nested\work.mp4"),
            &[r"D:\Media".to_string()],
            &aliases,
            Some("旧作者"),
        );

        assert_eq!(artist.as_deref(), Some("标准作者"));
        assert_eq!(source, "一级作者目录");
    }

    #[test]
    fn falls_back_to_the_existing_artist_for_files_at_the_scan_root() {
        let (artist, source) = infer_target_artist(
            Path::new(r"D:\Media\work.mp3"),
            &[r"D:\Media".to_string()],
            &HashMap::new(),
            Some("作者A"),
        );

        assert_eq!(artist.as_deref(), Some("作者A"));
        assert_eq!(source, "现有 Artist 标签");
    }
}
