use std::path::{Path, PathBuf};

/// 扫描指定目录，返回所有文件路径列表
pub fn scan_directory(root: &Path, recursive: bool, max_depth: Option<u32>) -> Vec<PathBuf> {
    scan_directory_with_progress(root, recursive, max_depth, |_| {})
}

/// 扫描指定目录，并在每发现一个文件时通知调用方。
pub fn scan_directory_with_progress<F>(
    root: &Path,
    recursive: bool,
    max_depth: Option<u32>,
    mut on_file: F,
) -> Vec<PathBuf>
where
    F: FnMut(&Path),
{
    let mut results = Vec::new();
    let walker = if recursive {
        let mut builder = walkdir::WalkDir::new(root);
        if let Some(depth) = max_depth {
            builder = builder.max_depth(depth as usize);
        }
        builder
    } else {
        walkdir::WalkDir::new(root).max_depth(1)
    };

    for entry in walker.into_iter().filter_map(|e| e.ok()) {
        if entry.file_type().is_file() {
            let path = entry.into_path();
            on_file(&path);
            results.push(path);
        }
    }
    results
}
