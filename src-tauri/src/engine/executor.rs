use std::path::Path;
use std::fs;
use crate::engine::hasher;

/// 安全移动文件：复制 → 校验哈希 → 删除源文件
pub fn safe_move(src: &Path, dest: &Path) -> Result<(), String> {
    if !src.exists() {
        return Err(format!("源文件不存在: {}", src.display()));
    }
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("创建目标目录失败: {}", e))?;
    }
    // 计算源文件哈希
    let src_hash = hasher::compute_sha256(src)
        .map_err(|e| format!("计算源文件哈希失败: {}", e))?;
    // 复制文件
    fs::copy(src, dest)
        .map_err(|e| format!("复制文件失败: {}", e))?;
    // 校验目标文件哈希
    let dest_hash = hasher::compute_sha256(dest)
        .map_err(|e| format!("校验目标文件哈希失败: {}", e))?;
    if src_hash != dest_hash {
        let _ = fs::remove_file(dest);
        return Err("文件复制后哈希校验失败，操作已回滚".into());
    }
    // 删除源文件
    fs::remove_file(src)
        .map_err(|e| format!("删除源文件失败: {}", e))?;
    Ok(())
}

/// 安全复制文件：复制 → 校验哈希
pub fn safe_copy(src: &Path, dest: &Path) -> Result<(), String> {
    if !src.exists() {
        return Err(format!("源文件不存在: {}", src.display()));
    }
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("创建目标目录失败: {}", e))?;
    }
    let src_hash = hasher::compute_sha256(src)
        .map_err(|e| format!("计算源文件哈希失败: {}", e))?;
    fs::copy(src, dest)
        .map_err(|e| format!("复制文件失败: {}", e))?;
    let dest_hash = hasher::compute_sha256(dest)
        .map_err(|e| format!("校验目标文件哈希失败: {}", e))?;
    if src_hash != dest_hash {
        let _ = fs::remove_file(dest);
        return Err("文件复制后哈希校验失败，操作已回滚".into());
    }
    Ok(())
}

/// 安全重命名文件
pub fn safe_rename(src: &Path, dest: &Path) -> Result<(), String> {
    if !src.exists() {
        return Err(format!("源文件不存在: {}", src.display()));
    }
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("创建目标目录失败: {}", e))?;
    }
    fs::rename(src, dest)
        .map_err(|e| {
            // 跨卷时 rename 会失败，回退到 safe_move
            format!("重命名失败({})，尝试 safe_move", e)
        })
        .or_else(|_| safe_move(src, dest))
}

/// 安全删除文件
pub fn safe_delete(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Err(format!("文件不存在: {}", path.display()));
    }
    fs::remove_file(path)
        .map_err(|e| format!("删除文件失败: {}", e))
}
