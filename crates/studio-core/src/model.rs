use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{EngineKind, Error, Result, TaskKind};

/// 稳定的模型标识，形如 `tts/index-tts2.5` 或 `dm/laya`。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ModelId(String);

impl ModelId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ModelId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// 模型级限流/资源约束。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ModelLimits {
    pub max_text_chars: Option<u32>,
    pub sample_rate: Option<u32>,
    pub max_concurrency: Option<u32>,
}

/// 模型清单，对应 `models/<path>/model.toml`。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ModelCard {
    pub id: ModelId,
    pub family: String,
    pub task: TaskKind,
    /// 首选引擎；与 `engines` 首项一致时冗余，但便于阅读。
    pub engine: Option<EngineKind>,
    /// 引擎降级顺序，不能为空。
    pub engines: Vec<EngineKind>,
    /// sidecar 内使用的适配器，例如 `indextts_v25`。
    pub adapter: Option<String>,
    /// sidecar 协议版本，例如 `v25`。
    pub api_version: Option<String>,
    pub streaming: bool,
    pub min_vram_gb: Option<u32>,
    pub params_b: Option<f32>,
    pub dtype: Option<String>,
    pub limits: ModelLimits,
    /// 磁盘路径，扫描时填充，不参与序列化。
    #[serde(skip)]
    pub path: Option<PathBuf>,
}

impl Default for ModelCard {
    fn default() -> Self {
        Self {
            id: ModelId::new(""),
            family: String::new(),
            task: TaskKind::Chat,
            engine: None,
            engines: Vec::new(),
            adapter: None,
            api_version: None,
            streaming: false,
            min_vram_gb: None,
            params_b: None,
            dtype: None,
            limits: ModelLimits::default(),
            path: None,
        }
    }
}

impl ModelCard {
    /// 从 `model.toml` 读取清单，并把所在目录回填到 `path`。
    pub fn from_toml(path: &Path) -> Result<Self> {
        let raw = fs::read_to_string(path).map_err(|e| Error::Manifest {
            path: path.to_path_buf(),
            reason: e.to_string(),
        })?;
        let mut card: ModelCard = toml::from_str(&raw).map_err(|e| Error::Manifest {
            path: path.to_path_buf(),
            reason: e.to_string(),
        })?;
        card.path = path.parent().map(Path::to_path_buf);
        Self::validate(card, path)
    }

    /// 从 TOML 文本解析清单（测试与内置样例使用）。
    pub fn parse(raw: &str) -> Result<Self> {
        let card: ModelCard = toml::from_str(raw).map_err(|e| Error::Manifest {
            path: PathBuf::from("<memory>"),
            reason: e.to_string(),
        })?;
        Self::validate(card, Path::new("<memory>"))
    }

    fn validate(card: Self, origin: &Path) -> Result<Self> {
        if card.id.as_str().is_empty() {
            return Err(Error::Manifest {
                path: origin.to_path_buf(),
                reason: "field `id` is required".to_string(),
            });
        }
        if card.engines.is_empty() {
            return Err(Error::Manifest {
                path: origin.to_path_buf(),
                reason: "field `engines` must list at least one engine".to_string(),
            });
        }
        Ok(card)
    }

    /// 首选引擎：优先取显式 `engine`，否则取 `engines` 首项。
    pub fn preferred_engine(&self) -> Option<EngineKind> {
        self.engine.or_else(|| self.engines.first().copied())
    }

    pub fn supports(&self, task: TaskKind) -> bool {
        self.task == task
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_card() {
        let card = ModelCard::parse(
            r#"
            id = "tts/index-tts2.5"
            family = "indextts"
            task = "tts"
            engine = "transformers"
            adapter = "indextts_v25"
            api_version = "v25"
            engines = ["transformers"]
            "#,
        )
        .expect("card should parse");

        assert_eq!(card.id.as_str(), "tts/index-tts2.5");
        assert_eq!(card.preferred_engine(), Some(EngineKind::Transformers));
        assert!(card.supports(TaskKind::Tts));
        assert!(!card.supports(TaskKind::Chat));
        assert_eq!(card.engines.len(), 1);
    }

    #[test]
    fn rejects_card_without_engines() {
        let err = ModelCard::parse("id = \"dm/laya\"\nfamily = \"laya\"\ntask = \"decision\"")
            .expect_err("engines is required");
        assert!(matches!(err, Error::Manifest { .. }));
        assert!(err.to_string().contains("engines"));
    }

    #[test]
    fn rejects_card_without_id() {
        let err = ModelCard::parse("family = \"laya\"\ntask = \"decision\"\nengines = [\"vllm\"]")
            .expect_err("id is required");
        assert!(matches!(err, Error::Manifest { .. }));
    }
}
