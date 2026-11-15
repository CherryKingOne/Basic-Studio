use serde::{Deserialize, Serialize};

/// token 用量。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
}

impl Usage {
    pub fn total(&self) -> u64 {
        self.prompt_tokens + self.completion_tokens
    }
}

/// 音频块：PCM 小端 16bit。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioChunk {
    pub pcm: Vec<u8>,
    pub sample_rate: u32,
    pub channels: u16,
}

impl AudioChunk {
    pub fn new(pcm: Vec<u8>, sample_rate: u32) -> Self {
        Self {
            pcm,
            sample_rate,
            channels: 1,
        }
    }

    pub fn duration_ms(&self) -> f64 {
        if self.sample_rate == 0 {
            return 0.0;
        }
        let bytes_per_sample = 2 * self.channels as u64;
        let samples = self.pcm.len() as u64 / bytes_per_sample.max(1);
        samples as f64 * 1000.0 / self.sample_rate as f64
    }
}

/// 引擎产出的流式事件。
///
/// 无论请求是否 `stream=true`，引擎都通过事件汇报；由网关决定是否实时下发。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamEvent {
    /// 文本增量。
    Delta(String),
    /// 音频块（TTS）。
    Audio(AudioChunk),
    /// 结束，附用量。
    Done(Usage),
    /// 流内错误（可恢复，由网关决定是否重试）。
    Error(String),
}

/// 单次推理的汇总结果。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InferenceOutcome {
    pub usage: Option<Usage>,
    pub events: u64,
    /// 音频总采样率（TTS）。
    pub sample_rate: Option<u32>,
}

impl InferenceOutcome {
    pub fn record(&mut self, event: &StreamEvent) {
        self.events += 1;
        match event {
            StreamEvent::Done(usage) => self.usage = Some(*usage),
            StreamEvent::Audio(chunk) => self.sample_rate = Some(chunk.sample_rate),
            _ => {}
        }
    }
}
