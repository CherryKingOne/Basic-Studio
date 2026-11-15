//! 分层配置：默认值 → 配置文件 → 环境变量 → CLI。
//!
//! 默认值来自 `config/default.toml`（唯一来源），调用方只需传入可选的覆盖文件。

mod loader;
mod schema;

pub use loader::{load, load_from_str, render, validate};
pub use schema::{
    ChecksumPolicy, Config, EngineConfig, GatewayConfig, ModelsConfig, SchedulerConfig,
    SidecarConfig, SidecarsConfig,
};
