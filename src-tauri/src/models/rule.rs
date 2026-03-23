use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleSet {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub rules: Vec<Rule>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rule {
    pub id: String,
    pub enabled: bool,
    pub name: String,
    pub condition_group: ConditionGroup,
    pub actions: Vec<Action>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConditionGroup {
    pub logic: Logic,
    pub conditions: Vec<Condition>,
    pub sub_groups: Vec<ConditionGroup>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Logic {
    And,
    Or,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Condition {
    pub field: ConditionField,
    pub operator: Operator,
    pub value: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConditionField {
    Filename,
    Extension,
    FullName,
    SizeBytes,
    CreatedAt,
    ModifiedAt,
    ParentDir,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Operator {
    Equals,
    NotEquals,
    Contains,
    NotContains,
    StartsWith,
    EndsWith,
    Regex,
    In,
    NotIn,
    Gt,
    Gte,
    Lt,
    Lte,
    Between,
    Before,
    After,
    WithinDays,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "params")]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Rename(RenameParams),
    Move(RouteParams),
    Copy(RouteParams),
    Delete(DeleteParams),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenameParams {
    pub mode: RenameMode,
    #[serde(flatten)]
    pub detail: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RenameMode {
    Replace,
    Prefix,
    Suffix,
    Sequence,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteParams {
    pub dest_pattern: String,
    pub conflict_strategy: ConflictStrategy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictStrategy {
    Skip,
    Overwrite,
    AutoRename,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteParams {
    pub confirm_required: bool,
}
