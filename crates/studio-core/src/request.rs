use crate::{ModelId, TaskKind};

/// 统一的推理请求抽象。
///
/// 引擎适配器负责把它翻译成各引擎的私有协议；对外 API 只认这个结构。
#[derive(Debug, Clone, PartialEq)]
pub struct InferenceRequest {
    pub model: ModelId,
    pub task: TaskKind,
    pub prompt: String,
    pub stream: bool,
    /// 优先级，调度层使用；数值越小越优先。
    pub priority: u8,
}

impl InferenceRequest {
    pub fn new(model: ModelId, task: TaskKind, prompt: impl Into<String>) -> Self {
        Self {
            model,
            task,
            prompt: prompt.into(),
            stream: false,
            priority: 128,
        }
    }

    pub fn with_stream(mut self, stream: bool) -> Self {
        self.stream = stream;
        self
    }

    pub fn with_priority(mut self, priority: u8) -> Self {
        self.priority = priority;
        self
    }
}
