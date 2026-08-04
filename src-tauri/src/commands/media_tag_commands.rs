use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chrono::Local;
use tauri::{command, AppHandle, Emitter, Manager};
use uuid::Uuid;

use crate::engine::{executor, hasher, metadata, scanner};
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
    author_folder_mode: String,
    cleanup_mode: String,
    verify_content_hash: bool,
) -> Result<MediaTagCleanupResult, String> {
    if paths.is_empty() {
        return Err("请至少选择一个扫描目录".into());
    }

    let author_folder_mode = AuthorFolderMode::parse(&author_folder_mode)?;
    let cleanup_mode = metadata::TagCleanupMode::parse(&cleanup_mode)?;
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
            let (target_artist, author_source) = infer_target_artist(
                &path,
                &paths_for_author,
                &aliases,
                media.artist.as_deref(),
                author_folder_mode,
            );
            let write_check = metadata::validate_tag_cleanup_writable(&path);
            let supported = write_check.is_ok();
            let sha256 = if supported && verify_content_hash {
                hasher::compute_sha256(&path).unwrap_or_default()
            } else {
                String::new()
            };
            let skip_reason = if let Err(error) = write_check {
                Some(error)
            } else if verify_content_hash && sha256.is_empty() {
                Some("无法生成文件内容快照，请检查文件是否可读取".to_string())
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
                sha256: sha256.clone(),
                media_type: metadata::media_type_label(&path)
                    .unwrap_or("unknown")
                    .to_string(),
                current_artist: media.artist,
                target_artist: target_artist.clone(),
                author_source,
                supported,
                skip_reason,
                checked: supported
                    && target_artist.is_some()
                    && (!verify_content_hash || !sha256.is_empty()),
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
        author_folder_mode: author_folder_mode.as_str().into(),
        cleanup_mode: cleanup_mode.as_str().into(),
        verify_content_hash,
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
    let verify_content_hash = scan_result.verify_content_hash;
    let cleanup_mode = metadata::TagCleanupMode::parse(&scan_result.cleanup_mode)?;
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
                let mut backup_path = None;
                let mut backup_hash = None;
                let mut target_hash = None;
                let mut reversible = false;
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
                    let media_path = Path::new(&file.path);
                    if verify_content_hash && file.sha256.is_empty() {
                        failed += 1;
                        (
                            OperationStatus::Failed,
                            Some("文件缺少内容快照，请重新扫描后再执行标签清洗".into()),
                            artist.to_string(),
                        )
                    } else if let Err(error) =
                        validate_tag_cleanup_size_snapshot(media_path, file.size_bytes)
                    {
                        failed += 1;
                        (
                            OperationStatus::Failed,
                            Some(error),
                            artist.to_string(),
                        )
                    } else {
                        match create_tag_backup(&data_dir, &task_id, media_path) {
                        Ok((created_backup, created_hash)) => {
                            if verify_content_hash && created_hash != file.sha256 {
                                let _ = fs::remove_file(&created_backup);
                                if let Some(parent) = created_backup.parent() {
                                    let _ = fs::remove_dir(parent);
                                }
                                failed += 1;
                                (
                                    OperationStatus::Failed,
                                    Some(
                                        "文件内容在扫描后发生变化，已拒绝执行标签清洗"
                                            .into(),
                                    ),
                                    artist.to_string(),
                                )
                            } else {
                                backup_path = Some(created_backup.to_string_lossy().into_owned());
                                backup_hash = Some(created_hash.clone());
                                let expected_execution_hash = if verify_content_hash {
                                    file.sha256.as_str()
                                } else {
                                    created_hash.as_str()
                                };
                                match metadata::clean_tags_and_set_artist(
                                    media_path,
                                    &created_backup,
                                    artist,
                                    expected_execution_hash,
                                    cleanup_mode,
                                ) {
                                    Ok(()) => match hasher::compute_sha256(media_path) {
                                        Ok(cleaned_hash) => {
                                            succeeded += 1;
                                            target_hash = Some(cleaned_hash);
                                            reversible = true;
                                            (OperationStatus::Success, None, artist.to_string())
                                        }
                                        Err(error) => {
                                            failed += 1;
                                            (
                                                OperationStatus::Failed,
                                                Some(format!(
                                                    "标签已写入，但无法生成撤销校验哈希；原文件备份保留在 {}: {}",
                                                    created_backup.display(),
                                                    error
                                                )),
                                                artist.to_string(),
                                            )
                                        }
                                    },
                                    Err(error) => {
                                        failed += 1;
                                        (
                                            OperationStatus::Failed,
                                            Some(format!(
                                                "{}；原文件备份保留在 {}",
                                                error,
                                                created_backup.display(),
                                            )),
                                            artist.to_string(),
                                        )
                                    }
                                }
                            }
                        }
                        Err(error) => {
                            failed += 1;
                            (
                                OperationStatus::Failed,
                                Some(format!("创建原文件备份失败，未执行标签清洗: {}", error)),
                                artist.to_string(),
                            )
                        }
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
                    reversible,
                    target_hash,
                    backup_path,
                    backup_hash,
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
                undo_status: if operations
                    .iter()
                    .any(|operation| operation.status == OperationStatus::Success)
                {
                    UndoStatus::Available
                } else {
                    UndoStatus::Expired
                },
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

fn create_tag_backup(
    data_dir: &Path,
    task_id: &str,
    source: &Path,
) -> Result<(PathBuf, String), String> {
    let backup_dir = data_dir.join("media_tag_backups").join(task_id);
    fs::create_dir_all(&backup_dir).map_err(|error| format!("创建标签备份目录失败: {}", error))?;
    let extension = source
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| format!(".{}", extension))
        .unwrap_or_default();
    let backup = backup_dir.join(format!("{}{}", Uuid::new_v4(), extension));
    executor::safe_copy(source, &backup)?;
    match hasher::compute_sha256(&backup) {
        Ok(hash) => Ok((backup, hash)),
        Err(error) => {
            let _ = fs::remove_file(&backup);
            Err(format!("校验标签备份失败: {}", error))
        }
    }
}

fn infer_target_artist(
    path: &Path,
    roots: &[String],
    aliases: &HashMap<String, String>,
    current_artist: Option<&str>,
    folder_mode: AuthorFolderMode,
) -> (Option<String>, String) {
    let folder = match folder_mode {
        AuthorFolderMode::Children => direct_child_folder_name(path, roots),
        AuthorFolderMode::Selected => selected_root_folder_name(path, roots),
    };
    if let Some(folder) = folder {
        return (
            Some(canonical_author(&folder, aliases)),
            match folder_mode {
                AuthorFolderMode::Children => "一级作者目录".into(),
                AuthorFolderMode::Selected => "所选作者目录".into(),
            },
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AuthorFolderMode {
    Children,
    Selected,
}

impl AuthorFolderMode {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "children" => Ok(Self::Children),
            "selected" => Ok(Self::Selected),
            _ => Err("作者目录层级无效，请重新选择".into()),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Children => "children",
            Self::Selected => "selected",
        }
    }
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

fn selected_root_folder_name(path: &Path, roots: &[String]) -> Option<String> {
    roots
        .iter()
        .map(PathBuf::from)
        .filter(|root| path_is_within(path, root))
        .max_by_key(|root| root.components().count())?
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
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

fn validate_tag_cleanup_size_snapshot(path: &Path, expected_size: u64) -> Result<(), String> {
    let actual_size = fs::metadata(path)
        .map_err(|error| format!("读取待清洗文件失败: {}", error))?
        .len();
    if actual_size != expected_size {
        return Err(format!(
            "文件大小在扫描后发生变化（扫描时 {} 字节，当前 {} 字节），请重新扫描",
            expected_size, actual_size
        ));
    }
    Ok(())
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
    fn tag_backup_preserves_extension_and_content_hash() {
        let root = std::env::temp_dir().join(format!("smart-sorter-tag-backup-{}", Uuid::new_v4()));
        let data_dir = root.join("data");
        let source = root.join("track.mp3");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(&source, "media bytes").unwrap();
        let source_hash = hasher::compute_sha256(&source).unwrap();

        let (backup, backup_hash) = create_tag_backup(&data_dir, "task", &source).unwrap();

        assert_eq!(
            backup.extension().and_then(|value| value.to_str()),
            Some("mp3")
        );
        assert_eq!(backup_hash, source_hash);
        assert_eq!(std::fs::read(&backup).unwrap(), b"media bytes");

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn fast_tag_cleanup_snapshot_checks_size_without_requiring_hash() {
        let root = std::env::temp_dir().join(format!("smart-sorter-tag-size-{}", Uuid::new_v4()));
        let source = root.join("track.mp3");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(&source, "media bytes").unwrap();
        let size = std::fs::metadata(&source).unwrap().len();

        assert!(validate_tag_cleanup_size_snapshot(&source, size).is_ok());
        std::fs::write(&source, "different media bytes").unwrap();
        assert!(validate_tag_cleanup_size_snapshot(&source, size).is_err());

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn prefers_the_organized_author_folder_over_an_existing_tag() {
        let aliases = HashMap::from([("alias".to_string(), "标准作者".to_string())]);
        let (artist, source) = infer_target_artist(
            Path::new(r"D:\Media\Alias\nested\work.mp4"),
            &[r"D:\Media".to_string()],
            &aliases,
            Some("旧作者"),
            AuthorFolderMode::Children,
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
            AuthorFolderMode::Children,
        );

        assert_eq!(artist.as_deref(), Some("作者A"));
        assert_eq!(source, "现有 Artist 标签");
    }

    #[test]
    fn selected_author_folder_mode_uses_the_selected_root_name() {
        let (artist, source) = infer_target_artist(
            Path::new(r"D:\Media\作者A\专辑B\work.mp3"),
            &[r"D:\Media\作者A".to_string()],
            &HashMap::new(),
            Some("旧作者"),
            AuthorFolderMode::Selected,
        );

        assert_eq!(artist.as_deref(), Some("作者A"));
        assert_eq!(source, "所选作者目录");
    }
}
