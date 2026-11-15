//! 引擎抽象：所有推理引擎适配器实现同一套生命周期与推理接口。
//!
//! 新增引擎 = 新增一个 `crates/engines/<name>` crate，实现 [`Engine`]，
//! 并在 `config/default.toml` 的 `[engines.<name>]` 段提供启动清单，无需改动其他层。

pub mod http;
mod registry;
mod traits;

pub use registry::{EngineInfo, EngineRegistry};
pub use traits::{Engine, EngineState, Health};
