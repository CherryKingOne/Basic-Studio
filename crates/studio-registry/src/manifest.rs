use std::fs;
use std::path::{Path, PathBuf};

use studio_core::ModelCard;

/// 扫描结果：成功解析的卡片 + 跳过的清单错误。
#[derive(Debug, Clone, Default)]
pub struct Discovery {
    pub cards: Vec<ModelCard>,
    pub errors: Vec<ManifestError>,
    pub root: Option<PathBuf>,
}

/// 单个清单的解析失败原因。
#[derive(Debug, Clone)]
pub struct ManifestError {
    pub path: PathBuf,
    pub reason: String,
}

impl ManifestError {
    pub fn new(path: impl Into<PathBuf>, reason: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            reason: reason.into(),
        }
    }
}

/// 扫描规则：
/// - 忽略 `.` 与 `_` 开头的目录（`_shared`、`.cache` 等）；
/// - 含 `model.toml` 的目录视为一个模型，不再向下递归；
/// - 其余目录继续递归，最大深度 6 层。
pub fn scan(root: &Path) -> std::io::Result<Discovery> {
    let mut discovery = Discovery {
        root: Some(root.to_path_buf()),
        ..Discovery::default()
    };

    if !root.is_dir() {
        return Ok(discovery);
    }

    walk(root, 0, &mut discovery)?;
    Ok(discovery)
}

fn walk(dir: &Path, depth: usize, out: &mut Discovery) -> std::io::Result<()> {
    if depth > 6 {
        return Ok(());
    }

    let manifest = dir.join("model.toml");
    if manifest.is_file() {
        match ModelCard::from_toml(&manifest) {
            Ok(card) => out.cards.push(card),
            Err(err) => out
                .errors
                .push(ManifestError::new(manifest, err.to_string())),
        }
        return Ok(());
    }

    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') || name.starts_with('_') {
            continue;
        }

        walk(&path, depth + 1, out)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use studio_core::TaskKind;

    fn write_card(dir: &Path, body: &str) {
        fs::create_dir_all(dir).unwrap();
        fs::write(dir.join("model.toml"), body).unwrap();
    }

    #[test]
    fn scans_nested_models_and_skips_hidden_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();

        write_card(
            &root.join("tts/index-tts2.5"),
            r#"
            id = "tts/index-tts2.5"
            family = "indextts"
            task = "tts"
            engines = ["transformers"]
            "#,
        );
        write_card(
            &root.join("DM/laya"),
            r#"
            id = "dm/laya"
            family = "laya"
            task = "decision"
            engines = ["vllm"]
            "#,
        );
        fs::create_dir_all(root.join(".cache")).unwrap();
        fs::create_dir_all(root.join("_shared")).unwrap();

        let discovery = scan(root).unwrap();
        assert_eq!(discovery.cards.len(), 2, "two models discovered");
        assert!(discovery.errors.is_empty());
    }

    #[test]
    fn broken_manifest_is_reported_not_fatal() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write_card(&root.join("tts/broken"), "id = 1");

        let discovery = scan(root).unwrap();
        assert!(discovery.cards.is_empty());
        assert_eq!(discovery.errors.len(), 1);
        assert!(discovery.errors[0].path.ends_with("model.toml"));
    }

    #[test]
    fn missing_root_is_not_an_error() {
        let tmp = tempfile::tempdir().unwrap();
        let discovery = scan(&tmp.path().join("does-not-exist")).unwrap();
        assert!(discovery.cards.is_empty());
        assert!(discovery.errors.is_empty());
    }

    #[test]
    fn card_task_roundtrip() {
        assert_eq!("tts".parse::<TaskKind>().unwrap(), TaskKind::Tts);
    }
}
