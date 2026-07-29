use crate::engine::hasher;
use std::fs;
use std::fs::OpenOptions;
use std::io;
use std::path::Path;

/// 安全移动文件：同卷尝试原子 rename，跨卷或失败时回退到 copy → 校验哈希 → 删除源
pub fn safe_move(src: &Path, dest: &Path) -> Result<(), String> {
    if !src.exists() {
        return Err(format!("源文件不存在: {}", src.display()));
    }
    reject_existing_target(dest)?;
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("创建目标目录失败: {}", e))?;
    }
    // 尝试同卷原子 rename（O(1)，无需复制内容）
    if fs::rename(src, dest).is_ok() {
        return Ok(());
    }
    // 跨卷或 rename 失败 → 走复制流程
    let src_hash = hasher::compute_sha256(src).map_err(|e| format!("计算源文件哈希失败: {}", e))?;
    copy_without_overwrite(src, dest)?;
    let dest_hash =
        hasher::compute_sha256(dest).map_err(|e| format!("校验目标文件哈希失败: {}", e))?;
    if src_hash != dest_hash {
        let _ = fs::remove_file(dest);
        return Err("文件复制后哈希校验失败，操作已回滚".into());
    }
    fs::remove_file(src).map_err(|e| format!("删除源文件失败: {}", e))?;
    Ok(())
}

/// 安全复制文件：复制 → 校验哈希
pub fn safe_copy(src: &Path, dest: &Path) -> Result<(), String> {
    if !src.exists() {
        return Err(format!("源文件不存在: {}", src.display()));
    }
    reject_existing_target(dest)?;
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("创建目标目录失败: {}", e))?;
    }
    let src_hash = hasher::compute_sha256(src).map_err(|e| format!("计算源文件哈希失败: {}", e))?;
    copy_without_overwrite(src, dest)?;
    let dest_hash =
        hasher::compute_sha256(dest).map_err(|e| format!("校验目标文件哈希失败: {}", e))?;
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
    reject_existing_target(dest)?;
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("创建目标目录失败: {}", e))?;
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
    fs::remove_file(path).map_err(|e| format!("删除文件失败: {}", e))
}

fn reject_existing_target(dest: &Path) -> Result<(), String> {
    if dest.exists() {
        Err(format!("目标文件已存在，拒绝覆盖: {}", dest.display()))
    } else {
        Ok(())
    }
}

fn copy_without_overwrite(src: &Path, dest: &Path) -> Result<(), String> {
    let mut source = fs::File::open(src).map_err(|e| format!("打开源文件失败: {}", e))?;
    let mut target = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(dest)
        .map_err(|e| {
            if e.kind() == io::ErrorKind::AlreadyExists {
                format!("目标文件已存在，拒绝覆盖: {}", dest.display())
            } else {
                format!("创建目标文件失败: {}", e)
            }
        })?;
    if let Err(error) = io::copy(&mut source, &mut target) {
        drop(target);
        let _ = fs::remove_file(dest);
        return Err(format!("复制文件失败: {}", error));
    }
    if let Ok(metadata) = fs::metadata(src) {
        let _ = fs::set_permissions(dest, metadata.permissions());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn copy_refuses_to_overwrite_an_existing_target() {
        let root = std::env::temp_dir().join(format!("smart-sorter-copy-{}", Uuid::new_v4()));
        let source = root.join("source.txt");
        let target = root.join("target.txt");
        fs::create_dir_all(&root).unwrap();
        fs::write(&source, "new content").unwrap();
        fs::write(&target, "existing content").unwrap();

        assert!(safe_copy(&source, &target).is_err());
        assert_eq!(fs::read_to_string(&target).unwrap(), "existing content");

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn move_refuses_to_overwrite_an_existing_target() {
        let root = std::env::temp_dir().join(format!("smart-sorter-move-{}", Uuid::new_v4()));
        let source = root.join("source.txt");
        let target = root.join("target.txt");
        fs::create_dir_all(&root).unwrap();
        fs::write(&source, "new content").unwrap();
        fs::write(&target, "existing content").unwrap();

        assert!(safe_move(&source, &target).is_err());
        assert!(source.exists());
        assert_eq!(fs::read_to_string(&target).unwrap(), "existing content");

        fs::remove_dir_all(root).unwrap();
    }
}
