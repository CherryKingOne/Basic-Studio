use std::fmt;
use std::path::Path;
use std::time::Duration;

use async_trait::async_trait;
use studio_core::{EngineCaps, EngineKind, ModelCard, Result};

/// 引擎实例状态机。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineState {
    Starting,
    Ready,
    Unready,
    Draining,
    Dead,
}

impl EngineState {
    pub fn is_ready(self) -> bool {
        matches!(self, EngineState::Ready)
    }
}

impl fmt::Display for EngineState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            EngineState::Starting => "starting",
            EngineState::Ready => "ready",
            EngineState::Unready => "unready",
            EngineState::Draining => "draining",
            EngineState::Dead => "dead",
        };
        f.write_str(s)
    }
}

/// 健康检查结果。
#[derive(Debug, Clone, Default)]
pub struct Health {
    pub ready: bool,
    pub detail: String,
}

impl Health {
    pub fn ready() -> Self {
        Self {
            ready: true,
            detail: "ok".to_string(),
        }
    }

    pub fn unready(reason: impl Into<String>) -> Self {
        Self {
            ready: false,
            detail: reason.into(),
        }
    }
}

/// 引擎适配器统一接口。
///
/// 生命周期：`start` → `health`（轮询）→ `shutdown`。
/// 推理请求的 `infer` 在 M1 加入；当前 M0 只覆盖进程生命周期与能力声明。
#[async_trait]
pub trait Engine: Send {
    fn kind(&self) -> EngineKind;

    /// 实例标签，用于日志与指标，形如 `vllm/tts-index-tts2.5`。
    fn label(&self) -> &str;

    fn state(&self) -> EngineState;

    fn caps(&self) -> &EngineCaps;

    /// 当前实例绑定的模型目录（外部托管时可能为空）。
    fn model_path(&self) -> Option<&Path>;

    /// 模型卡片，便于路由层读取 `task` / `adapter` / `api_version`。
    fn card(&self) -> Option<&ModelCard>;

    async fn start(&mut self) -> Result<()>;

    async fn health(&self) -> Health;

    async fn shutdown(&mut self, grace: Duration) -> Result<()>;
}
