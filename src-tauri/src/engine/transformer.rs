use std::path::{Path, PathBuf};
use std::fs;
use chrono::{DateTime, Local};
use crate::models::rule::{Action, RenameParams, RenameMode, RouteParams};

/// 序列号上下文，用于 {seq} 变量展开
pub struct TransformContext {
    pub sequence: u32,
    pub seq_padding: u32,
}

impl Default for TransformContext {
    fn default() -> Self {
        Self { sequence: 1, seq_padding: 3 }
    }
}

/// 根据动作计算文件的新路径（纯计算，不触碰文件系统）
pub fn compute_target(source: &Path, action: &Action, ctx: &mut TransformContext) -> Option<PathBuf> {
    match action {
        Action::Rename(params) => compute_rename(source, params, ctx),
        Action::Move(params) => compute_route(source, params, ctx),
        Action::Copy(params) => compute_route(source, params, ctx),
        Action::Delete(_) => None,
    }
}

fn compute_rename(source: &Path, params: &RenameParams, ctx: &mut TransformContext) -> Option<PathBuf> {
    let parent = source.parent()?;
    let stem = source.file_stem()?.to_string_lossy().into_owned();
    let ext = source.extension().map(|e| e.to_string_lossy().into_owned());

    let new_stem = match params.mode {
        RenameMode::Replace => {
            let find = params.detail.get("find").and_then(|v| v.as_str()).unwrap_or("");
            let replace_with = params.detail.get("replace").and_then(|v| v.as_str()).unwrap_or("");
            stem.replace(find, replace_with)
        }
        RenameMode::Prefix => {
            let prefix = params.detail.get("text").and_then(|v| v.as_str()).unwrap_or("");
            let expanded = expand_template(prefix, source, ctx);
            format!("{}{}", expanded, stem)
        }
        RenameMode::Suffix => {
            let suffix = params.detail.get("text").and_then(|v| v.as_str()).unwrap_or("");
            let expanded = expand_template(suffix, source, ctx);
            format!("{}{}", stem, expanded)
        }
        RenameMode::Sequence => {
            let template = params.detail.get("template").and_then(|v| v.as_str())
                .unwrap_or("{filename}_{seq}");
            let padding = params.detail.get("padding").and_then(|v| v.as_u64()).unwrap_or(3) as u32;
            ctx.seq_padding = padding;
            let result = expand_template(template, source, ctx);
            ctx.sequence += 1;
            result
        }
    };

    let new_name = match ext {
        Some(e) if !e.is_empty() => format!("{}.{}", new_stem, e),
        _ => new_stem,
    };
    Some(parent.join(new_name))
}

fn compute_route(source: &Path, params: &RouteParams, ctx: &mut TransformContext) -> Option<PathBuf> {
    let dest_dir = expand_template(&params.dest_pattern, source, ctx);
    let dest_dir = sanitize_path(&dest_dir);
    let file_name = source.file_name()?;
    Some(PathBuf::from(dest_dir).join(file_name))
}

/// 清理路径字符串：去除前后空白，移除路径各段中的非法字符
fn sanitize_path(path_str: &str) -> String {
    let trimmed = path_str.trim();
    // 分离驱动器前缀（如 D:\uff09和其余部分
    let (prefix, rest) = if trimmed.len() >= 2 && trimmed.as_bytes()[1] == b':' {
        (&trimmed[..2], &trimmed[2..])
    } else {
        ("", trimmed)
    };
    // 对每一段路径去除前后空白和尾部点号（Windows 不允许）
    let cleaned: Vec<&str> = rest.split(['\\', '/'])
        .map(|seg| seg.trim().trim_end_matches('.'))
        .filter(|seg| !seg.is_empty())
        .collect();
    if cleaned.is_empty() {
        prefix.to_string()
    } else {
        format!("{}\\{}", prefix, cleaned.join("\\"))
    }
}

/// 展开魔法变量模板，将 {filename}, {created_year} 等替换为实际值
pub fn expand_template(template: &str, source: &Path, ctx: &TransformContext) -> String {
    let mut result = template.to_string();
    let metadata = fs::metadata(source).ok();

    let stem = source.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let ext = source.extension().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let full_name = source.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let parent = source.parent().and_then(|p| p.file_name())
        .map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();

    result = result.replace("{filename}", &stem);
    result = result.replace("{extension}", &ext);
    result = result.replace("{full_name}", &full_name);
    result = result.replace("{parent_dir}", &parent);

    if let Some(ref m) = metadata {
        let size_mb = m.len() as f64 / (1024.0 * 1024.0);
        result = result.replace("{size_mb}", &format!("{:.1}", size_mb));
    }

    if let Some(created) = metadata.as_ref().and_then(|m| m.created().ok()) {
        let dt: DateTime<Local> = created.into();
        result = result.replace("{created_year}", &dt.format("%Y").to_string());
        result = result.replace("{created_month}", &dt.format("%m").to_string());
        result = result.replace("{created_day}", &dt.format("%d").to_string());
    }

    if let Some(modified) = metadata.as_ref().and_then(|m| m.modified().ok()) {
        let dt: DateTime<Local> = modified.into();
        result = result.replace("{modified_year}", &dt.format("%Y").to_string());
        result = result.replace("{modified_month}", &dt.format("%m").to_string());
        result = result.replace("{modified_day}", &dt.format("%d").to_string());
    }

    let seq = format!("{:0>width$}", ctx.sequence, width = ctx.seq_padding as usize);
    result = result.replace("{seq}", &seq);

    result.trim().to_string()
}
