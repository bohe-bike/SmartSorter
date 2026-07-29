use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chrono::{DateTime, Local};
use tauri::{command, AppHandle, Emitter, Manager};
use uuid::Uuid;

use crate::engine::{executor, hasher, metadata, scanner};
use crate::models::log::{ExecutionLog, ExecutionSummary, Operation, OperationStatus, UndoStatus};
use crate::models::media_classify::{
    AliasLearningHint, ClassifyExecuteRequest, ClassifyPreviewItem, ClassifyPreviewResult,
    KeywordAlias, KeywordGroup, KeywordGroupSaveRequest, KeywordInfo, MediaClassifyResult,
    MediaFile, MediaKeywordGroup,
};
use crate::models::progress::ProgressPayload;
use crate::storage::{log_store, media_classify_store};

pub static MEDIA_SCAN_CACHE: Mutex<Option<MediaClassifyResult>> = Mutex::new(None);
pub static MEDIA_PREVIEW_CACHE: Mutex<Option<ClassifyPreviewResult>> = Mutex::new(None);

const AUTO_CLASSIFY_THRESHOLD: u8 = 80;
const AUTO_CLASSIFY_MARGIN: u8 = 15;
// 封面不能单独证明作者归属，仅用于加强已经存在的文本候选。
const EXACT_COVER_ART_SCORE: u8 = 22;

#[derive(Debug, Clone, Copy)]
enum ClassificationDimension {
    All,
    Creator,
    Album,
    Folder,
}

impl ClassificationDimension {
    fn parse(value: &str) -> Result<Self, String> {
        match value.trim().to_lowercase().as_str() {
            "all" => Ok(Self::All),
            "creator" => Ok(Self::Creator),
            "album" => Ok(Self::Album),
            "folder" => Ok(Self::Folder),
            _ => Err("归类维度无效，应为 all、creator、album 或 folder".into()),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Creator => "creator",
            Self::Album => "album",
            Self::Folder => "folder",
        }
    }

    fn allows_source(self, source: &str) -> bool {
        match self {
            Self::All => matches!(
                source,
                "folder_name" | "artist" | "album_artist" | "album" | "composer"
            ),
            Self::Creator => matches!(
                source,
                "folder_name" | "artist" | "album_artist" | "composer"
            ),
            Self::Album => matches!(source, "folder_name" | "album"),
            Self::Folder => source == "folder_name",
        }
    }
}

#[derive(Debug, Default)]
struct CandidateScore {
    score: u8,
    evidence: Vec<String>,
}

#[derive(Debug, Clone)]
struct CoverArtEvidence {
    keyword: String,
    anchor_count: u64,
}

#[command]
pub async fn scan_media_authors(
    app: AppHandle,
    paths: Vec<String>,
    recursive: bool,
    media_types: Vec<String>,
    keyword_sources: Vec<String>,
    classification_dimension: String,
) -> Result<MediaClassifyResult, String> {
    scan_media_with_keywords(
        app,
        paths,
        recursive,
        media_types,
        keyword_sources,
        classification_dimension,
        None,
    )
    .await
}

async fn scan_media_with_keywords(
    app: AppHandle,
    paths: Vec<String>,
    recursive: bool,
    media_types: Vec<String>,
    keyword_sources: Vec<String>,
    classification_dimension: String,
    saved_keywords: Option<Vec<String>>,
) -> Result<MediaClassifyResult, String> {
    let task_id = Uuid::new_v4().to_string();
    let filters = normalize_media_filters(&media_types);
    let dimension = ClassificationDimension::parse(&classification_dimension)?;
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?;
    let knowledge = media_classify_store::load(&data_dir)?;
    let alias_map = build_alias_map(&knowledge.aliases);
    let creator_exclusions: HashSet<String> = knowledge
        .creator_exclusions
        .iter()
        .map(|keyword| normalize_match_text(keyword))
        .filter(|keyword| !keyword.is_empty())
        .collect();
    let sources: HashSet<String> = keyword_sources
        .iter()
        .map(|source| source.trim().to_lowercase())
        .filter(|source| dimension.allows_source(source))
        .collect();
    if sources.is_empty() {
        return Err(format!(
            "归类维度“{}”没有启用可用的关键字来源",
            dimension.as_str()
        ));
    }

    // ① 收集当前文件夹下的子文件夹名作为关键字
    let mut folder_keywords: Vec<String> = Vec::new();
    if saved_keywords.is_none() && sources.contains("folder_name") {
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

    // ④ 合并所有关键字。大小写不同的同一关键字只保留首次出现的写法，
    // 并排序以确保多匹配列表和默认分组的顺序稳定。
    let is_saved_keyword_group = saved_keywords.is_some();
    let all_keywords: Vec<String> = if let Some(saved_keywords) = saved_keywords {
        let mut seen: HashSet<String> = HashSet::new();
        let mut combined = Vec::new();
        for keyword in saved_keywords {
            if keyword.chars().count() >= 2
                && !(matches!(
                    dimension,
                    ClassificationDimension::Creator | ClassificationDimension::All
                ) && is_creator_keyword_excluded(&keyword, &alias_map, &creator_exclusions))
                && seen.insert(normalize_match_text(&keyword))
            {
                combined.push(keyword);
            }
        }
        combined.sort_by(|left, right| {
            left.to_lowercase()
                .cmp(&right.to_lowercase())
                .then_with(|| left.cmp(right))
        });
        combined
    } else {
        let mut seen: HashSet<String> = HashSet::new();
        let mut combined: Vec<String> = Vec::new();
        for kw in folder_keywords
            .iter()
            .chain(metadata_keywords.iter())
            .chain(
                knowledge
                    .aliases
                    .iter()
                    .flat_map(|alias| [&alias.alias, &alias.canonical]),
            )
        {
            if kw.chars().count() >= 2
                && !(matches!(
                    dimension,
                    ClassificationDimension::Creator | ClassificationDimension::All
                ) && is_creator_keyword_excluded(kw, &alias_map, &creator_exclusions))
                && seen.insert(normalize_match_text(kw))
            {
                combined.push(kw.clone());
            }
        }
        combined.sort_by(|a, b| {
            a.to_lowercase()
                .cmp(&b.to_lowercase())
                .then_with(|| a.cmp(b))
        });
        combined
    };

    // ⑤ 动态生成的关键字保持原有包含关系合并；已保存的关键词组按用户整理后的列表原样应用。
    let merged_map = if is_saved_keyword_group {
        all_keywords
            .iter()
            .map(|keyword| (keyword.clone(), keyword.clone()))
            .collect()
    } else {
        merge_containing_keywords(&all_keywords)
    };
    let normalized_folder_keywords = build_normalized_keyword_map(&merged_map);
    // merged_map: 原始关键字 → 合并后关键字（最短的那个）
    let mut final_keywords: Vec<String> = merged_map
        .values()
        .cloned()
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    final_keywords.sort_by(|a, b| {
        a.to_lowercase()
            .cmp(&b.to_lowercase())
            .then_with(|| a.cmp(b))
    });

    // 构建 KeywordInfo 列表
    let mut keyword_infos: Vec<KeywordInfo> = Vec::new();
    for kw in &final_keywords {
        let mut merged_from: Vec<String> = merged_map
            .iter()
            .filter(|(k, v)| v.as_str() == kw.as_str() && k.as_str() != kw.as_str())
            .map(|(k, _)| k.clone())
            .collect();
        merged_from.sort_by(|a, b| {
            a.to_lowercase()
                .cmp(&b.to_lowercase())
                .then_with(|| a.cmp(b))
        });
        let source = if is_saved_keyword_group {
            "keyword_group"
        } else if folder_keywords.contains(kw) && metadata_keywords.contains(kw) {
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

    // ⑥ 先用纯文本证据建立封面锚点。只有同一封面对应唯一的高置信度
    // 关键字时，才允许它参与后续的补强评分。
    let cover_art_evidence = build_cover_art_evidence(
        &media_files,
        &paths,
        &final_keywords,
        &normalized_folder_keywords,
        &alias_map,
        is_saved_keyword_group,
        &sources,
    );

    // ⑦ 汇集证据并评分。只有高置信度且显著领先的候选才自动归类。
    let mut no_match_count = 0u64;
    let mut unmatched_files: Vec<MediaFile> = Vec::new();
    let mut grouped: HashMap<String, Vec<MediaFile>> = HashMap::new();

    for (index, (path, size_bytes, meta)) in media_files.iter().enumerate() {
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
        let mut candidates = build_text_candidates(
            path,
            &paths,
            &final_keywords,
            &normalized_folder_keywords,
            &alias_map,
            is_saved_keyword_group,
            &sources,
            meta,
        );
        if let Some(cover_hash) = meta.cover_art_hash.as_deref() {
            if let Some(cover) = cover_art_evidence.get(cover_hash) {
                // 封面只加强已由文件名、目录或标签指向同一关键字的候选；
                // 没有文字证据时仍保持待确认，避免把专辑/频道封面误当作者。
                if candidates.contains_key(&cover.keyword) {
                    add_candidate(
                        &mut candidates,
                        &cover.keyword,
                        EXACT_COVER_ART_SCORE,
                        &format!(
                            "封面与“{}”的 {} 个高置信度文件完全一致",
                            cover.keyword, cover.anchor_count
                        ),
                    );
                }
            }
        }

        let ranked = rank_candidates(candidates);
        let matched_keywords: Vec<String> =
            ranked.iter().map(|(keyword, _)| keyword.clone()).collect();
        let confidence = ranked
            .first()
            .map(|(_, candidate)| candidate.score)
            .unwrap_or(0);
        let evidence = ranked
            .first()
            .map(|(_, candidate)| candidate.evidence.clone())
            .unwrap_or_default();
        let runner_up = ranked
            .get(1)
            .map(|(_, candidate)| candidate.score)
            .unwrap_or(0);
        let auto_classifiable = is_auto_classifiable(confidence, runner_up);

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
            matched_keywords,
            confidence,
            evidence,
            requires_confirmation: !auto_classifiable,
            modified_at,
            checked: auto_classifiable,
        };

        if !auto_classifiable {
            no_match_count += 1;
            unmatched_files.push(media_file);
            continue;
        }

        let primary_keyword = ranked
            .first()
            .map(|(keyword, _)| keyword.clone())
            .expect("自动归类必须存在候选关键字");
        grouped.entry(primary_keyword).or_default().push(media_file);
    }

    // ⑧ 构建分组结果
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
        classification_dimension: dimension.as_str().to_string(),
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
pub fn load_creator_exclusions(app: AppHandle) -> Result<Vec<String>, String> {
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?;
    media_classify_store::load_creator_exclusions(&data_dir)
}

#[command]
pub fn save_creator_exclusions(
    app: AppHandle,
    keywords: Vec<String>,
) -> Result<Vec<String>, String> {
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?;
    let saved = media_classify_store::save_creator_exclusions(&data_dir, keywords)?;
    *MEDIA_SCAN_CACHE.lock().map_err(|error| error.to_string())? = None;
    *MEDIA_PREVIEW_CACHE
        .lock()
        .map_err(|error| error.to_string())? = None;
    Ok(saved)
}

#[command]
pub fn load_media_keyword_groups(app: AppHandle) -> Result<Vec<MediaKeywordGroup>, String> {
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?;
    media_classify_store::load_keyword_groups(&data_dir)
}

#[command]
pub fn save_media_keyword_group(
    app: AppHandle,
    request: KeywordGroupSaveRequest,
) -> Result<MediaKeywordGroup, String> {
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?;
    let group = media_classify_store::save_keyword_group(&data_dir, request)?;
    *MEDIA_SCAN_CACHE.lock().map_err(|error| error.to_string())? = None;
    *MEDIA_PREVIEW_CACHE
        .lock()
        .map_err(|error| error.to_string())? = None;
    Ok(group)
}

#[command]
pub fn delete_media_keyword_group(app: AppHandle, id: String) -> Result<(), String> {
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?;
    media_classify_store::delete_keyword_group(&data_dir, &id)?;
    *MEDIA_SCAN_CACHE.lock().map_err(|error| error.to_string())? = None;
    *MEDIA_PREVIEW_CACHE
        .lock()
        .map_err(|error| error.to_string())? = None;
    Ok(())
}

#[command]
pub async fn apply_media_keyword_group(
    app: AppHandle,
    paths: Vec<String>,
    recursive: bool,
    media_types: Vec<String>,
    group_id: String,
) -> Result<MediaClassifyResult, String> {
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?;
    let group = media_classify_store::load_keyword_groups(&data_dir)?
        .into_iter()
        .find(|group| group.id == group_id)
        .ok_or_else(|| "关键词组不存在，请重新选择".to_string())?;
    scan_media_with_keywords(
        app,
        paths,
        recursive,
        media_types,
        group.keyword_sources,
        group.classification_dimension,
        Some(group.keywords),
    )
    .await
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
    let available_keywords: HashSet<&str> = scan_result
        .keywords
        .iter()
        .map(|keyword| keyword.keyword.as_str())
        .collect();
    // 追踪本批次已分配的目标路径，防止同名覆盖
    let mut used_targets: HashSet<String> = HashSet::new();

    let mut items = Vec::new();
    let mut learning_hints = Vec::new();
    for group in &scan_result.groups {
        for file in &group.files {
            if !selected.contains(file.path.as_str()) {
                continue; // 用户未勾选，跳过
            }

            // 多匹配必须由用户确认；单匹配可使用其唯一的归属关键字。
            let manually_assigned = request.keyword_assignments.contains_key(&file.path);
            let keyword = match request.keyword_assignments.get(&file.path) {
                Some(keyword) if file.matched_keywords.contains(keyword) => keyword.clone(),
                Some(_) => return Err(format!("文件 {} 的归属关键字无效", file.path)),
                None if file.matched_keywords.len() == 1 => group.keyword.clone(),
                None => {
                    return Err(format!(
                        "文件 {} 存在多个匹配关键字，请先选择归属",
                        file.path
                    ))
                }
            };

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
            if manually_assigned {
                append_learning_hints(&mut learning_hints, file, &keyword);
            }
        }
    }

    // 处理未匹配的文件（需要用户手动指定关键字）
    for file in &scan_result.unmatched_files {
        if !selected.contains(file.path.as_str()) {
            continue;
        }
        let keyword = request
            .keyword_assignments
            .get(&file.path)
            .ok_or_else(|| format!("未匹配文件 {} 尚未选择归属关键字", file.path))?;
        if file.matched_keywords.is_empty() {
            if !available_keywords.contains(keyword.as_str()) {
                return Err(format!("文件 {} 的归属关键字无效", file.path));
            }
        } else if !file.matched_keywords.contains(keyword) {
            return Err(format!("文件 {} 的归属关键字不在匹配候选中", file.path));
        }

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
        append_learning_hints(&mut learning_hints, file, keyword);
    }

    let preview = ClassifyPreviewResult {
        task_id: scan_result.task_id,
        total: items.len() as u64,
        items,
        learning_hints,
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
        let scan_result = cache
            .as_ref()
            .ok_or_else(|| "没有可用的媒体扫描结果，请重新扫描并生成预览".to_string())?;
        if scan_result.task_id != preview.task_id {
            return Err("扫描结果已更新，请重新生成归类预览".into());
        }
        scan_result.source_paths.iter().map(PathBuf::from).collect()
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

        let target_hash = if result.is_ok() {
            hasher::compute_sha256(target).ok()
        } else {
            None
        };
        operations.push(Operation {
            op_id: Uuid::new_v4().to_string(),
            action: "move".to_string(),
            source_path: item.source_path.clone(),
            target_path: item.target_path.clone(),
            status,
            error_message,
            reversible: target_hash.is_some(),
            target_hash,
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
    let succeeded_paths: HashSet<&str> = log
        .operations
        .iter()
        .filter(|operation| operation.status == OperationStatus::Success)
        .map(|operation| operation.source_path.as_str())
        .collect();
    let learned_hints: Vec<AliasLearningHint> = preview
        .learning_hints
        .iter()
        .filter(|hint| succeeded_paths.contains(hint.source_path.as_str()))
        .cloned()
        .collect();
    let learning_error =
        media_classify_store::record_confirmations(&data_dir, &learned_hints).err();

    if failed > 0 {
        return Err(format!("执行完成：{} 成功，{} 失败", succeeded, failed));
    }

    let learning_message = if let Some(error) = learning_error {
        format!("；别名学习未保存：{}", error)
    } else if learned_hints.is_empty() {
        String::new()
    } else {
        format!("；已学习 {} 条别名", learned_hints.len())
    };
    Ok(format!(
        "执行完成：{} 个文件已归类{}",
        succeeded, learning_message
    ))
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
    // 多扫描根目录可能存在嵌套关系，必须选择真正包含文件的最长根目录。
    // 不能使用裸字符串前缀：D:\\Media 不是 D:\\Media2 的父目录。
    if let Some(root) = source_paths
        .iter()
        .map(PathBuf::from)
        .filter(|root| path_is_within(source, root))
        .max_by_key(|root| root.components().count())
    {
        return root;
    }
    // fallback: 使用文件的直接父目录
    source.parent().unwrap_or(Path::new(".")).to_path_buf()
}

fn direct_child_folder_name(source: &Path, source_paths: &[String]) -> Option<String> {
    let root = find_root_dir(source, source_paths);
    let relative = source.strip_prefix(root).ok()?;
    let mut parts = relative
        .components()
        .filter_map(|component| match component {
            std::path::Component::Normal(name) => name.to_str(),
            _ => None,
        });
    let first = parts.next()?;
    // 根目录中的直接文件没有子文件夹可作为其归属关键字。
    parts.next()?;
    Some(first.trim().to_string())
}

#[cfg(target_os = "windows")]
fn path_is_within(source: &Path, root: &Path) -> bool {
    let source = source.to_string_lossy().replace('/', "\\").to_lowercase();
    let root = root.to_string_lossy().replace('/', "\\").to_lowercase();
    let root_with_separator = format!("{}\\", root.trim_end_matches('\\'));
    source == root || source.starts_with(&root_with_separator)
}

#[cfg(not(target_os = "windows"))]
fn path_is_within(source: &Path, root: &Path) -> bool {
    source.starts_with(root)
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
    if !path.is_dir() || roots.iter().any(|root| paths_equal(root, path)) {
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
fn resolve_unique_target(initial: PathBuf, used: &mut HashSet<String>) -> PathBuf {
    if !initial.exists() && !used.contains(&path_key(&initial)) {
        used.insert(path_key(&initial));
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
        if !candidate.exists() && !used.contains(&path_key(&candidate)) {
            used.insert(path_key(&candidate));
            return candidate;
        }
    }
    // 极端情况兜底
    used.insert(path_key(&initial));
    initial
}

#[cfg(target_os = "windows")]
fn path_key(path: &Path) -> String {
    path.to_string_lossy().replace('/', "\\").to_lowercase()
}

#[cfg(not(target_os = "windows"))]
fn path_key(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn build_alias_map(aliases: &[KeywordAlias]) -> HashMap<String, String> {
    let mut result = HashMap::new();
    for alias in aliases {
        let key = normalize_match_text(&alias.alias);
        let canonical = alias.canonical.trim();
        if !key.is_empty() && !canonical.is_empty() {
            result.insert(key, canonical.to_string());
        }
    }
    result
}

fn canonical_keyword(keyword: &str, aliases: &HashMap<String, String>) -> String {
    aliases
        .get(&normalize_match_text(keyword))
        .cloned()
        .unwrap_or_else(|| keyword.to_string())
}

fn match_target_keyword(
    keyword: &str,
    aliases: &HashMap<String, String>,
    is_saved_keyword_group: bool,
) -> String {
    if is_saved_keyword_group {
        keyword.to_string()
    } else {
        canonical_keyword(keyword, aliases)
    }
}

fn build_normalized_keyword_map(merged_map: &HashMap<String, String>) -> HashMap<String, String> {
    let mut entries: Vec<_> = merged_map.iter().collect();
    entries.sort_by(|(left, _), (right, _)| {
        normalize_match_text(left)
            .cmp(&normalize_match_text(right))
            .then_with(|| left.cmp(right))
    });
    entries
        .into_iter()
        .fold(HashMap::new(), |mut result, (source, target)| {
            result
                .entry(normalize_match_text(source))
                .or_insert_with(|| target.clone());
            result
        })
}

fn is_creator_keyword_excluded(
    keyword: &str,
    aliases: &HashMap<String, String>,
    exclusions: &HashSet<String>,
) -> bool {
    let canonical = canonical_keyword(keyword, aliases);
    exclusions.contains(&normalize_match_text(keyword))
        || exclusions.contains(&normalize_match_text(&canonical))
}

fn append_learning_hints(hints: &mut Vec<AliasLearningHint>, file: &MediaFile, canonical: &str) {
    let canonical_key = normalize_match_text(canonical);
    for alias in &file.matched_keywords {
        if normalize_match_text(alias) == canonical_key {
            continue;
        }
        hints.push(AliasLearningHint {
            source_path: file.path.clone(),
            alias: alias.clone(),
            canonical: canonical.to_string(),
        });
    }
}

fn build_cover_art_evidence(
    media_files: &[(PathBuf, u64, metadata::MediaMetadata)],
    scan_roots: &[String],
    keywords: &[String],
    normalized_folder_keywords: &HashMap<String, String>,
    aliases: &HashMap<String, String>,
    is_saved_keyword_group: bool,
    sources: &HashSet<String>,
) -> HashMap<String, CoverArtEvidence> {
    let mut owners_by_cover: HashMap<String, HashMap<String, u64>> = HashMap::new();

    for (path, _, meta) in media_files {
        let Some(cover_hash) = meta.cover_art_hash.as_ref() else {
            continue;
        };
        let ranked = rank_candidates(build_text_candidates(
            path,
            scan_roots,
            keywords,
            normalized_folder_keywords,
            aliases,
            is_saved_keyword_group,
            sources,
            meta,
        ));
        let confidence = ranked
            .first()
            .map(|(_, candidate)| candidate.score)
            .unwrap_or(0);
        let runner_up = ranked
            .get(1)
            .map(|(_, candidate)| candidate.score)
            .unwrap_or(0);
        if !is_auto_classifiable(confidence, runner_up) {
            continue;
        }
        if let Some((keyword, _)) = ranked.first() {
            *owners_by_cover
                .entry(cover_hash.clone())
                .or_default()
                .entry(keyword.clone())
                .or_default() += 1;
        }
    }

    owners_by_cover
        .into_iter()
        .filter_map(|(cover_hash, owners)| {
            if owners.len() != 1 {
                return None;
            }
            let (keyword, anchor_count) = owners.into_iter().next()?;
            Some((
                cover_hash,
                CoverArtEvidence {
                    keyword,
                    anchor_count,
                },
            ))
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn build_text_candidates(
    path: &Path,
    scan_roots: &[String],
    keywords: &[String],
    normalized_folder_keywords: &HashMap<String, String>,
    aliases: &HashMap<String, String>,
    is_saved_keyword_group: bool,
    sources: &HashSet<String>,
    meta: &metadata::MediaMetadata,
) -> HashMap<String, CandidateScore> {
    let mut candidates = HashMap::new();
    let file_stem = path
        .file_stem()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();

    for keyword in keywords {
        if let Some(score) = filename_match_score(&file_stem, keyword) {
            let target = match_target_keyword(keyword, aliases, is_saved_keyword_group);
            add_candidate(&mut candidates, &target, score, "文件名");
        }
    }

    if sources.contains("folder_name") {
        if let Some(folder_name) = direct_child_folder_name(path, scan_roots) {
            if let Some(keyword) =
                normalized_folder_keywords.get(&normalize_match_text(&folder_name))
            {
                let target = match_target_keyword(keyword, aliases, is_saved_keyword_group);
                add_candidate(&mut candidates, &target, 100, "所属子文件夹");
            }
        }
    }

    for (source, value, exact_score, partial_score) in [
        ("artist", meta.artist.as_deref(), 95, 60),
        ("album_artist", meta.album_artist.as_deref(), 92, 60),
        ("album", meta.album.as_deref(), 95, 60),
        ("composer", meta.composer.as_deref(), 80, 55),
    ] {
        if !sources.contains(source) {
            continue;
        }
        if let Some(value) = value {
            for keyword in keywords {
                if let Some(score) =
                    metadata_match_score(value, keyword, exact_score, partial_score)
                {
                    let target = match_target_keyword(keyword, aliases, is_saved_keyword_group);
                    add_candidate(&mut candidates, &target, score, source);
                }
            }
        }
    }

    candidates
}

fn rank_candidates(candidates: HashMap<String, CandidateScore>) -> Vec<(String, CandidateScore)> {
    let mut ranked: Vec<(String, CandidateScore)> = candidates.into_iter().collect();
    ranked.sort_by(|(left_keyword, left), (right_keyword, right)| {
        right.score.cmp(&left.score).then_with(|| {
            left_keyword
                .to_lowercase()
                .cmp(&right_keyword.to_lowercase())
        })
    });
    ranked
}

fn is_auto_classifiable(confidence: u8, runner_up: u8) -> bool {
    confidence >= AUTO_CLASSIFY_THRESHOLD
        && confidence.saturating_sub(runner_up) >= AUTO_CLASSIFY_MARGIN
}

fn add_candidate(
    candidates: &mut HashMap<String, CandidateScore>,
    keyword: &str,
    score: u8,
    evidence: &str,
) {
    let candidate = candidates.entry(keyword.to_string()).or_default();
    candidate.score = candidate.score.saturating_add(score).min(100);
    if !candidate.evidence.iter().any(|item| item == evidence) {
        candidate.evidence.push(evidence.to_string());
    }
}

fn filename_match_score(file_stem: &str, keyword: &str) -> Option<u8> {
    text_match_score(file_stem, keyword, 90, 82, 55)
}

fn metadata_match_score(
    value: &str,
    keyword: &str,
    exact_score: u8,
    partial_score: u8,
) -> Option<u8> {
    text_match_score(value, keyword, exact_score, partial_score, partial_score)
}

fn text_match_score(
    value: &str,
    keyword: &str,
    exact_score: u8,
    boundary_score: u8,
    partial_score: u8,
) -> Option<u8> {
    let value = normalize_match_text(value);
    let keyword = normalize_match_text(keyword);
    if value.is_empty() || keyword.is_empty() {
        return None;
    }
    if value == keyword {
        return Some(exact_score);
    }

    let mut best = None;
    for (start, _) in value.match_indices(&keyword) {
        let end = start + keyword.len();
        let before = value[..start].chars().next_back();
        let after = value[end..].chars().next();
        let has_boundaries =
            before.map_or(true, is_match_boundary) && after.map_or(true, is_match_boundary);
        let score = if has_boundaries {
            boundary_score
        } else {
            partial_score
        };
        best = Some(best.unwrap_or(0).max(score));
    }
    best
}

fn normalize_match_text(value: &str) -> String {
    value
        .trim()
        .to_lowercase()
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect()
}

fn is_match_boundary(ch: char) -> bool {
    !ch.is_alphanumeric()
}

fn normalize_media_filters(filters: &[String]) -> Vec<String> {
    filters
        .iter()
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| matches!(value.as_str(), "image" | "audio" | "video" | "ebook"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selects_the_longest_matching_scan_root() {
        let source = Path::new(r"D:\Media2\Author\work.mp4");
        let roots = vec![r"D:\Media".to_string(), r"D:\Media2".to_string()];

        assert_eq!(find_root_dir(source, &roots), PathBuf::from(r"D:\Media2"));
    }

    #[test]
    fn reads_direct_folder_as_a_file_keyword() {
        let source = Path::new(r"D:\Media\Author\nested\work.mp4");
        let roots = vec![r"D:\Media".to_string()];

        assert_eq!(
            direct_child_folder_name(source, &roots).as_deref(),
            Some("Author")
        );
    }

    #[test]
    fn merge_keywords_is_case_insensitive_and_uses_the_shortest_match() {
        let keywords = vec![
            "小凛蝶子".to_string(),
            "蝶子".to_string(),
            "Other".to_string(),
        ];
        let merged = merge_containing_keywords(&keywords);

        assert_eq!(merged.get("小凛蝶子"), Some(&"蝶子".to_string()));
        assert_eq!(merged.get("蝶子"), Some(&"蝶子".to_string()));
        assert_eq!(merged.get("Other"), Some(&"Other".to_string()));
    }

    #[test]
    fn classification_dimension_filters_incompatible_sources() {
        let all = ClassificationDimension::parse("all").unwrap();
        let creator = ClassificationDimension::parse("creator").unwrap();
        let album = ClassificationDimension::parse("album").unwrap();

        assert!(all.allows_source("artist"));
        assert!(all.allows_source("album"));
        assert!(all.allows_source("folder_name"));
        assert!(creator.allows_source("artist"));
        assert!(!creator.allows_source("album"));
        assert!(album.allows_source("album"));
        assert!(!album.allows_source("artist"));
    }

    #[test]
    fn scoring_requires_a_strong_and_clear_winner_for_auto_classification() {
        assert_eq!(filename_match_score("Alice - Live", "Alice"), Some(82));
        assert_eq!(filename_match_score("NotAliceLive", "Alice"), Some(55));
        assert_eq!(metadata_match_score("Alice", "Alice", 95, 60), Some(95));

        let mut candidates = HashMap::new();
        add_candidate(&mut candidates, "Alice", 95, "artist");
        add_candidate(&mut candidates, "Alice", 55, "文件名");
        add_candidate(&mut candidates, "Album", 60, "文件名");

        assert_eq!(candidates["Alice"].score, 100);
        assert!(candidates["Alice"].score >= AUTO_CLASSIFY_THRESHOLD);
        assert!(candidates["Alice"].score - candidates["Album"].score >= AUTO_CLASSIFY_MARGIN);
    }

    #[test]
    fn exact_cover_only_strengthens_an_existing_text_candidate() {
        let mut candidates = HashMap::new();
        add_candidate(&mut candidates, "Alice", 60, "artist");
        let cover = CoverArtEvidence {
            keyword: "Alice".into(),
            anchor_count: 3,
        };

        if candidates.contains_key(&cover.keyword) {
            add_candidate(
                &mut candidates,
                &cover.keyword,
                EXACT_COVER_ART_SCORE,
                "封面与“Alice”的 3 个高置信度文件完全一致",
            );
        }
        let ranked = rank_candidates(candidates);

        assert_eq!(ranked[0].0, "Alice");
        assert_eq!(ranked[0].1.score, 82);
        assert!(is_auto_classifiable(ranked[0].1.score, 0));
    }

    #[test]
    fn conflicting_cover_owners_are_not_used_as_evidence() {
        let owners = HashMap::from([("Alice".to_string(), 2u64), ("Bob".to_string(), 1u64)]);

        let evidence = if owners.len() == 1 {
            owners
                .into_iter()
                .next()
                .map(|(keyword, anchor_count)| CoverArtEvidence {
                    keyword,
                    anchor_count,
                })
        } else {
            None
        };

        assert!(evidence.is_none());
    }

    #[test]
    fn historical_aliases_resolve_to_the_canonical_keyword() {
        let aliases = vec![KeywordAlias {
            alias: "Jay Chou".into(),
            canonical: "周杰伦".into(),
            confirmations: 3,
            updated_at: String::new(),
        }];
        let alias_map = build_alias_map(&aliases);

        assert_eq!(canonical_keyword("jay chou", &alias_map), "周杰伦");
        assert_eq!(canonical_keyword("Other", &alias_map), "Other");
    }

    #[test]
    fn saved_keyword_group_preserves_its_curated_target_name() {
        let aliases = vec![KeywordAlias {
            alias: "Jay Chou".into(),
            canonical: "周杰伦".into(),
            confirmations: 3,
            updated_at: String::new(),
        }];
        let alias_map = build_alias_map(&aliases);

        assert_eq!(
            match_target_keyword("Jay Chou", &alias_map, true),
            "Jay Chou"
        );
        assert_eq!(
            match_target_keyword("Jay Chou", &alias_map, false),
            "周杰伦"
        );
    }

    #[test]
    fn folder_keyword_matching_ignores_case_and_whitespace() {
        let merged = HashMap::from([("频道 X".to_string(), "频道 X".to_string())]);
        let normalized = build_normalized_keyword_map(&merged);

        assert_eq!(
            normalized.get(&normalize_match_text(" 频道x ")),
            Some(&"频道 X".to_string())
        );
    }

    #[test]
    fn creator_exclusions_filter_the_keyword_and_its_aliases() {
        let aliases = vec![KeywordAlias {
            alias: "Channel X".into(),
            canonical: "频道X".into(),
            confirmations: 1,
            updated_at: String::new(),
        }];
        let alias_map = build_alias_map(&aliases);
        let exclusions = HashSet::from([normalize_match_text("频道X")]);

        assert!(is_creator_keyword_excluded(
            "Channel X",
            &alias_map,
            &exclusions
        ));
        assert!(is_creator_keyword_excluded(
            "频道 X",
            &alias_map,
            &exclusions
        ));
        assert!(!is_creator_keyword_excluded(
            "其他作者",
            &alias_map,
            &exclusions
        ));
    }
}
