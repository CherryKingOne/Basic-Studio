//! 模型注册表：扫描模型仓、解析 `model.toml`、建立索引。

mod index;
mod manifest;

pub use index::ModelIndex;
pub use manifest::{scan, Discovery, ManifestError};

use std::path::Path;
use studio_core::{ModelCard, Result};

/// 扫描模型仓并返回索引。
///
/// 单个模型清单损坏不会中断整体扫描，错误会记录在 `Discovery::errors` 中，
/// 由上层决定是警告还是拒绝启动。
pub fn discover(root: &Path) -> Result<(ModelIndex, Discovery)> {
    let discovery = scan(root)?;
    let index = ModelIndex::from_cards(discovery.cards.clone());
    Ok((index, discovery))
}

/// 从模型目录构造一张卡片（`dir/model.toml`）。
pub fn load_card(dir: &Path) -> Result<ModelCard> {
    ModelCard::from_toml(&dir.join("model.toml"))
}
