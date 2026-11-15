use std::fs::{self, File};
use std::path::{Path, PathBuf};

use studio_core::Result;

/// 打开（或创建）引擎日志文件，返回句柄与路径。
///
/// 文件名格式：`<label>-<unix-secs>.log`，按实例区分，便于对照重启记录。
pub fn open_log(dir: &Path, label: &str) -> Result<(File, PathBuf)> {
    fs::create_dir_all(dir)?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let path = dir.join(format!("{label}-{stamp}.log"));
    let file = File::create(&path)?;
    Ok((file, path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_log_file_in_nested_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("var/logs");
        let (file, path) = open_log(&dir, "vllm").expect("open log");
        drop(file);
        assert!(path.exists());
        assert!(path.to_string_lossy().contains("vllm-"));
    }
}
