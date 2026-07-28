use crate::models::media_classify::{AliasLearningHint, KeywordAlias};
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
}
