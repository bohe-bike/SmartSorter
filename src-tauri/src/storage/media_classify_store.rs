use crate::models::media_classify::{
    AliasLearningHint, KeywordAlias, KeywordGroupSaveRequest, MediaKeywordGroup,
};
use chrono::Local;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

const KNOWLEDGE_FILE: &str = "media_classify_aliases.json";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MediaClassifyKnowledge {
    pub aliases: Vec<KeywordAlias>,
    #[serde(default)]
    pub creator_exclusions: Vec<String>,
    #[serde(default)]
    pub keyword_groups: Vec<MediaKeywordGroup>,
}

pub fn load(data_dir: &Path) -> Result<MediaClassifyKnowledge, String> {
    let path = data_dir.join(KNOWLEDGE_FILE);
    if !path.exists() {
        return Ok(MediaClassifyKnowledge::default());
    }
    let content =
        fs::read_to_string(&path).map_err(|error| format!("读取归类知识库失败: {}", error))?;
    serde_json::from_str(&content).map_err(|error| format!("解析归类知识库失败: {}", error))
}

pub fn record_confirmations(data_dir: &Path, hints: &[AliasLearningHint]) -> Result<(), String> {
    if hints.is_empty() {
        return Ok(());
    }

    let mut knowledge = load(data_dir)?;
    let now = Local::now().to_rfc3339();
    for hint in hints {
        let alias = hint.alias.trim();
        let canonical = hint.canonical.trim();
        if alias.is_empty() || canonical.is_empty() || normalize(alias) == normalize(canonical) {
            continue;
        }
        if let Some(existing) = knowledge.aliases.iter_mut().find(|entry| {
            normalize(&entry.alias) == normalize(alias)
                && normalize(&entry.canonical) == normalize(canonical)
        }) {
            existing.confirmations += 1;
            existing.updated_at = now.clone();
        } else {
            knowledge.aliases.push(KeywordAlias {
                alias: alias.to_string(),
                canonical: canonical.to_string(),
                confirmations: 1,
                updated_at: now.clone(),
            });
        }
    }
    knowledge
        .aliases
        .sort_by(|left, right| normalize(&left.alias).cmp(&normalize(&right.alias)));
    write(data_dir, &knowledge)
}

pub fn load_creator_exclusions(data_dir: &Path) -> Result<Vec<String>, String> {
    Ok(load(data_dir)?.creator_exclusions)
}

pub fn save_creator_exclusions(
    data_dir: &Path,
    keywords: Vec<String>,
) -> Result<Vec<String>, String> {
    let mut knowledge = load(data_dir)?;
    let mut seen = std::collections::HashSet::new();
    let mut cleaned = Vec::new();
    for keyword in keywords {
        let value = keyword.trim();
        let normalized = normalize_for_match(value);
        if value.chars().count() >= 2 && seen.insert(normalized) {
            cleaned.push(value.to_string());
        }
    }
    cleaned.sort_by(|left, right| normalize_for_match(left).cmp(&normalize_for_match(right)));
    knowledge.creator_exclusions = cleaned.clone();
    write(data_dir, &knowledge)?;
    Ok(cleaned)
}

pub fn load_keyword_groups(data_dir: &Path) -> Result<Vec<MediaKeywordGroup>, String> {
    let mut groups = load(data_dir)?.keyword_groups;
    groups.sort_by(|left, right| left.name.to_lowercase().cmp(&right.name.to_lowercase()));
    Ok(groups)
}

pub fn save_keyword_group(
    data_dir: &Path,
    request: KeywordGroupSaveRequest,
) -> Result<MediaKeywordGroup, String> {
    let name = request.name.trim();
    if name.is_empty() {
        return Err("关键词组名称不能为空".into());
    }
    let dimension = request.classification_dimension.trim().to_lowercase();
    if !matches!(dimension.as_str(), "all" | "creator" | "album" | "folder") {
        return Err("关键词组归类维度无效".into());
    }
    let sources = normalize_sources(&request.keyword_sources, &dimension);
    if sources.is_empty() {
        return Err("关键词组至少需要一个有效的关键词来源".into());
    }

    let mut knowledge = load(data_dir)?;
    let normalized_name = normalize_for_match(name);
    let editing_id = request.id.as_deref().filter(|id| !id.trim().is_empty());
    if knowledge.keyword_groups.iter().any(|group| {
        Some(group.id.as_str()) != editing_id && normalize_for_match(&group.name) == normalized_name
    }) {
        return Err("关键词组名称已存在，请使用其他名称".into());
    }
    let exclusions: std::collections::HashSet<String> = knowledge
        .creator_exclusions
        .iter()
        .map(|keyword| normalize_for_match(keyword))
        .collect();
    let keywords = normalize_keywords(
        request.keywords,
        if matches!(dimension.as_str(), "all" | "creator") {
            Some(&exclusions)
        } else {
            None
        },
    );
    if keywords.is_empty() {
        return Err("关键词组至少需要一个有效关键词".into());
    }

    let now = Local::now().to_rfc3339();
    let group = if let Some(id) = request.id.filter(|id| !id.trim().is_empty()) {
        let existing = knowledge
            .keyword_groups
            .iter_mut()
            .find(|group| group.id == id)
            .ok_or_else(|| "要编辑的关键词组不存在".to_string())?;
        existing.name = name.to_string();
        existing.classification_dimension = dimension;
        existing.keyword_sources = sources;
        existing.keywords = keywords;
        existing.updated_at = now;
        existing.clone()
    } else {
        let group = MediaKeywordGroup {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.to_string(),
            classification_dimension: dimension,
            keyword_sources: sources,
            keywords,
            created_at: now.clone(),
            updated_at: now,
        };
        knowledge.keyword_groups.push(group.clone());
        group
    };
    knowledge
        .keyword_groups
        .sort_by(|left, right| left.name.to_lowercase().cmp(&right.name.to_lowercase()));
    write(data_dir, &knowledge)?;
    Ok(group)
}

pub fn delete_keyword_group(data_dir: &Path, id: &str) -> Result<(), String> {
    let mut knowledge = load(data_dir)?;
    let original_len = knowledge.keyword_groups.len();
    knowledge.keyword_groups.retain(|group| group.id != id);
    if knowledge.keyword_groups.len() == original_len {
        return Err("要删除的关键词组不存在".into());
    }
    write(data_dir, &knowledge)
}

fn write(data_dir: &Path, knowledge: &MediaClassifyKnowledge) -> Result<(), String> {
    fs::create_dir_all(data_dir).map_err(|error| format!("创建归类知识库目录失败: {}", error))?;
    let content = serde_json::to_string_pretty(knowledge)
        .map_err(|error| format!("序列化归类知识库失败: {}", error))?;
    fs::write(data_dir.join(KNOWLEDGE_FILE), content)
        .map_err(|error| format!("写入归类知识库失败: {}", error))
}

fn normalize(value: &str) -> String {
    value.trim().to_lowercase()
}

fn normalize_for_match(value: &str) -> String {
    value
        .trim()
        .to_lowercase()
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect()
}

fn normalize_keywords(
    keywords: Vec<String>,
    exclusions: Option<&std::collections::HashSet<String>>,
) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut result = Vec::new();
    for keyword in keywords {
        let value = keyword.trim();
        let normalized = normalize_for_match(value);
        if value.chars().count() >= 2
            && !normalized.is_empty()
            && exclusions.map_or(true, |items| !items.contains(&normalized))
            && seen.insert(normalized)
        {
            result.push(value.to_string());
        }
    }
    result.sort_by(|left, right| normalize_for_match(left).cmp(&normalize_for_match(right)));
    result
}

fn normalize_sources(sources: &[String], dimension: &str) -> Vec<String> {
    let mut result = Vec::new();
    for source in sources {
        let source = source.trim().to_lowercase();
        let valid = match dimension {
            "all" => matches!(
                source.as_str(),
                "folder_name" | "artist" | "album_artist" | "album" | "composer"
            ),
            "creator" => matches!(
                source.as_str(),
                "folder_name" | "artist" | "album_artist" | "composer"
            ),
            "album" => matches!(source.as_str(), "folder_name" | "album"),
            "folder" => source == "folder_name",
            _ => false,
        };
        if valid && !result.contains(&source) {
            result.push(source);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn persists_and_counts_confirmed_aliases() {
        let directory =
            std::env::temp_dir().join(format!("smart-sorter-aliases-{}", Uuid::new_v4()));
        let hint = AliasLearningHint {
            source_path: "D:\\media\\file.mp3".into(),
            alias: "Jay Chou".into(),
            canonical: "周杰伦".into(),
        };

        record_confirmations(&directory, &[hint.clone(), hint]).unwrap();
        let knowledge = load(&directory).unwrap();

        assert_eq!(knowledge.aliases.len(), 1);
        assert_eq!(knowledge.aliases[0].confirmations, 2);
        assert_eq!(knowledge.aliases[0].canonical, "周杰伦");
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn persists_creator_exclusions_without_case_or_space_duplicates() {
        let directory =
            std::env::temp_dir().join(format!("smart-sorter-exclusions-{}", Uuid::new_v4()));

        let saved = save_creator_exclusions(
            &directory,
            vec!["音乐频道".into(), " 音乐 频道 ".into(), "A".into()],
        )
        .unwrap();

        assert_eq!(saved, vec!["音乐频道"]);
        assert_eq!(
            load_creator_exclusions(&directory).unwrap(),
            vec!["音乐频道"]
        );
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn all_dimension_accepts_every_keyword_source() {
        let sources = normalize_sources(
            &[
                "folder_name".into(),
                "artist".into(),
                "album_artist".into(),
                "album".into(),
                "composer".into(),
            ],
            "all",
        );

        assert_eq!(
            sources,
            vec!["folder_name", "artist", "album_artist", "album", "composer",]
        );
    }

    #[test]
    fn saves_named_keyword_groups_and_filters_creator_exclusions() {
        let directory =
            std::env::temp_dir().join(format!("smart-sorter-keyword-groups-{}", Uuid::new_v4()));
        save_creator_exclusions(&directory, vec!["频道名".into()]).unwrap();

        let group = save_keyword_group(
            &directory,
            KeywordGroupSaveRequest {
                id: None,
                name: "常用作者".into(),
                classification_dimension: "creator".into(),
                keyword_sources: vec!["artist".into(), "folder_name".into()],
                keywords: vec!["作者A".into(), "频道名".into(), "作者A".into()],
            },
        )
        .unwrap();

        assert_eq!(group.keywords, vec!["作者A"]);
        assert_eq!(load_keyword_groups(&directory).unwrap().len(), 1);

        let duplicate = save_keyword_group(
            &directory,
            KeywordGroupSaveRequest {
                id: None,
                name: " 常用 作者 ".into(),
                classification_dimension: "creator".into(),
                keyword_sources: vec!["artist".into()],
                keywords: vec!["作者B".into()],
            },
        );
        assert_eq!(duplicate.unwrap_err(), "关键词组名称已存在，请使用其他名称");

        let updated = save_keyword_group(
            &directory,
            KeywordGroupSaveRequest {
                id: Some(group.id.clone()),
                name: "常用作者（已编辑）".into(),
                classification_dimension: "creator".into(),
                keyword_sources: vec!["artist".into()],
                keywords: vec!["作者B".into()],
            },
        )
        .unwrap();

        assert_eq!(updated.id, group.id);
        assert_eq!(
            load_keyword_groups(&directory).unwrap()[0].keywords,
            vec!["作者B"]
        );
        std::fs::remove_dir_all(directory).unwrap();
    }
}
