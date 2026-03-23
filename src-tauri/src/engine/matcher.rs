use std::path::Path;
use std::fs;
use std::time::SystemTime;
use chrono::{DateTime, Local, NaiveDate, NaiveDateTime, TimeZone};
use regex::Regex;
use crate::models::rule::{Condition, ConditionGroup, ConditionField, Logic, Operator};

/// 判断一个文件是否满足条件组（支持 AND/OR 嵌套子组）
pub fn matches(path: &Path, group: &ConditionGroup) -> bool {
    let cond_results = group.conditions.iter().map(|c| evaluate_condition(path, c));
    let sub_results = group.sub_groups.iter().map(|sg| matches(path, sg));
    let all: Vec<bool> = cond_results.chain(sub_results).collect();
    if all.is_empty() {
        return true;
    }
    match group.logic {
        Logic::And => all.iter().all(|&r| r),
        Logic::Or => all.iter().any(|&r| r),
    }
}

fn evaluate_condition(path: &Path, cond: &Condition) -> bool {
    let meta = fs::metadata(path).ok();
    match cond.field {
        ConditionField::Filename => {
            let v = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            match_string(&v, &cond.operator, &cond.value)
        }
        ConditionField::Extension => {
            let v = path.extension().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            match_string(&v, &cond.operator, &cond.value)
        }
        ConditionField::FullName => {
            let v = path.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            match_string(&v, &cond.operator, &cond.value)
        }
        ConditionField::SizeBytes => {
            let v = meta.map(|m| m.len() as f64).unwrap_or(0.0);
            match_number(v, &cond.operator, &cond.value)
        }
        ConditionField::CreatedAt => {
            meta.and_then(|m| m.created().ok())
                .map(|t| match_timestamp(t, &cond.operator, &cond.value))
                .unwrap_or(false)
        }
        ConditionField::ModifiedAt => {
            meta.and_then(|m| m.modified().ok())
                .map(|t| match_timestamp(t, &cond.operator, &cond.value))
                .unwrap_or(false)
        }
        ConditionField::ParentDir => {
            let v = path.parent()
                .and_then(|p| p.file_name())
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            match_string(&v, &cond.operator, &cond.value)
        }
    }
}

fn match_string(value: &str, op: &Operator, target: &serde_json::Value) -> bool {
    let t = target.as_str().unwrap_or_default();
    match op {
        Operator::Equals => value.eq_ignore_ascii_case(t),
        Operator::NotEquals => !value.eq_ignore_ascii_case(t),
        Operator::Contains => value.to_lowercase().contains(&t.to_lowercase()),
        Operator::NotContains => !value.to_lowercase().contains(&t.to_lowercase()),
        Operator::StartsWith => value.to_lowercase().starts_with(&t.to_lowercase()),
        Operator::EndsWith => value.to_lowercase().ends_with(&t.to_lowercase()),
        Operator::Regex => Regex::new(t).map(|re| re.is_match(value)).unwrap_or(false),
        Operator::In => target.as_array()
            .map(|arr| arr.iter().any(|v| v.as_str()
                .map(|s| s.eq_ignore_ascii_case(value)).unwrap_or(false)))
            .unwrap_or(false),
        Operator::NotIn => target.as_array()
            .map(|arr| !arr.iter().any(|v| v.as_str()
                .map(|s| s.eq_ignore_ascii_case(value)).unwrap_or(false)))
            .unwrap_or(true),
        _ => false,
    }
}

fn match_number(value: f64, op: &Operator, target: &serde_json::Value) -> bool {
    match op {
        Operator::Equals => target.as_f64().map(|t| (value - t).abs() < f64::EPSILON).unwrap_or(false),
        Operator::NotEquals => target.as_f64().map(|t| (value - t).abs() >= f64::EPSILON).unwrap_or(false),
        Operator::Gt => target.as_f64().map(|t| value > t).unwrap_or(false),
        Operator::Gte => target.as_f64().map(|t| value >= t).unwrap_or(false),
        Operator::Lt => target.as_f64().map(|t| value < t).unwrap_or(false),
        Operator::Lte => target.as_f64().map(|t| value <= t).unwrap_or(false),
        Operator::Between => target.as_array().and_then(|arr| {
            let lo = arr.first()?.as_f64()?;
            let hi = arr.get(1)?.as_f64()?;
            Some(value >= lo && value <= hi)
        }).unwrap_or(false),
        _ => false,
    }
}

fn match_timestamp(value: SystemTime, op: &Operator, target: &serde_json::Value) -> bool {
    match op {
        Operator::Before => {
            parse_datetime(target).map(|t| DateTime::<Local>::from(value) < t).unwrap_or(false)
        }
        Operator::After => {
            parse_datetime(target).map(|t| DateTime::<Local>::from(value) > t).unwrap_or(false)
        }
        Operator::WithinDays => {
            target.as_u64().map(|days| {
                let now = SystemTime::now();
                let duration = now.duration_since(value).unwrap_or_default();
                duration.as_secs() <= days * 86400
            }).unwrap_or(false)
        }
        _ => false,
    }
}

fn parse_datetime(value: &serde_json::Value) -> Option<DateTime<Local>> {
    if let Some(s) = value.as_str() {
        if let Ok(nd) = NaiveDate::parse_from_str(s, "%Y-%m-%d") {
            return Local.from_local_datetime(&nd.and_hms_opt(0, 0, 0)?).single();
        }
        if let Ok(ndt) = NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S") {
            return Local.from_local_datetime(&ndt).single();
        }
    }
    if let Some(secs) = value.as_i64() {
        return Local.timestamp_opt(secs, 0).single();
    }
    None
}
