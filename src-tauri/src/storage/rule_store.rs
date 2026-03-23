use std::path::Path;
use std::fs;
use crate::models::rule::RuleSet;

const RULES_FILE: &str = "rule_sets.json";

pub fn load_all(data_dir: &Path) -> Result<Vec<RuleSet>, String> {
    let path = data_dir.join(RULES_FILE);
    if !path.exists() {
        return Ok(vec![]);
    }
    let content = fs::read_to_string(&path)
        .map_err(|e| format!("读取规则文件失败: {}", e))?;
    serde_json::from_str(&content)
        .map_err(|e| format!("解析规则文件失败: {}", e))
}

pub fn save(data_dir: &Path, rule_set: &RuleSet) -> Result<(), String> {
    let mut all = load_all(data_dir)?;
    if let Some(existing) = all.iter_mut().find(|r| r.id == rule_set.id) {
        *existing = rule_set.clone();
    } else {
        all.push(rule_set.clone());
    }
    write_all(data_dir, &all)
}

pub fn delete(data_dir: &Path, id: &str) -> Result<(), String> {
    let mut all = load_all(data_dir)?;
    all.retain(|r| r.id != id);
    write_all(data_dir, &all)
}

fn write_all(data_dir: &Path, rule_sets: &[RuleSet]) -> Result<(), String> {
    fs::create_dir_all(data_dir)
        .map_err(|e| format!("创建数据目录失败: {}", e))?;
    let path = data_dir.join(RULES_FILE);
    let content = serde_json::to_string_pretty(rule_sets)
        .map_err(|e| format!("序列化失败: {}", e))?;
    fs::write(&path, content)
        .map_err(|e| format!("写入规则文件失败: {}", e))
}
