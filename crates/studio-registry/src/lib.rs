//! 模型注册表：扫描模型仓、解析 `model.toml`、建立索引。

mod index;
mod manifest;

pub use index::ModelIndex;
pub use manifest::{scan, Discovery, ManifestError};

use std::path::Path;
use studio_core::{EngineKind, ModelCard, Result};

/// 扫描模型仓并返回索引。
///
/// 单个模型清单损坏不会中断整体扫描，错误会记录在 `Discovery::errors` 中，
/// 由上层决定是警告还是拒绝启动。
pub fn discover(root: &Path) -> Result<(ModelIndex, Discovery)> {
    discover_with_default(root, EngineKind::Transformers)
}

/// 与 [`discover`] 相同，但显式指定"清单未声明引擎时"使用的默认引擎。
pub fn discover_with_default(
    root: &Path,
    default_engine: EngineKind,
) -> Result<(ModelIndex, Discovery)> {
    let mut discovery = scan(root)?;
    for card in &mut discovery.cards {
        card.apply_default_engine(default_engine);
    }
    let index = ModelIndex::from_cards(discovery.cards.clone());
    Ok((index, discovery))
}

/// 从模型目录构造一张卡片（`dir/model.toml`）。
pub fn load_card(dir: &Path) -> Result<ModelCard> {
    ModelCard::from_toml(&dir.join("model.toml"))
}
