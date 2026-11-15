use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{ModelId, TaskKind};

/// 采样参数。缺省值贴近各引擎的默认行为。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Sampling {
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
    pub max_tokens: Option<u32>,
    /// TTS 情感/风格等引擎私有参数。
    pub emotion: Option<String>,
}

/// 统一的推理请求抽象。
///
/// 引擎适配器负责把它翻译成各引擎的私有协议；对外 API 只认这个结构。
#[derive(Debug, Clone, PartialEq)]
pub struct InferenceRequest {
    pub model: ModelId,
    pub task: TaskKind,
    pub prompt: String,
    /// 可选 system prompt（chat 类任务）。
    pub system: Option<String>,
    pub stream: bool,
    /// 优先级，调度层使用；数值越小越优先。
    pub priority: u8,
    /// TTS 音色。
    pub voice: Option<String>,
    pub sampling: Sampling,
    /// 透传给引擎的额外字段（如 vLLM 的 `guided_json`）。
    pub extra: Value,
}

impl InferenceRequest {
    pub fn new(model: ModelId, task: TaskKind, prompt: impl Into<String>) -> Self {
        Self {
            model,
            task,
            prompt: prompt.into(),
            system: None,
            stream: false,
            priority: 128,
            voice: None,
            sampling: Sampling::default(),
            extra: Value::Null,
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

    pub fn with_system(mut self, system: impl Into<String>) -> Self {
        self.system = Some(system.into());
        self
    }

    pub fn with_voice(mut self, voice: impl Into<String>) -> Self {
        self.voice = Some(voice.into());
        self
    }

    pub fn with_sampling(mut self, sampling: Sampling) -> Self {
        self.sampling = sampling;
        self
    }

    pub fn with_extra(mut self, extra: Value) -> Self {
        self.extra = extra;
        self
    }
}
