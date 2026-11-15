use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::Error;

/// 任务类型。引擎是否"能跑某任务"由能力矩阵决定，不由引擎名决定。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskKind {
    Chat,
    Completion,
    Embed,
    Rerank,
    Tts,
    Transcribe,
    Decision,
}

impl TaskKind {
    pub fn as_str(self) -> &'static str {
        match self {
            TaskKind::Chat => "chat",
            TaskKind::Completion => "completion",
            TaskKind::Embed => "embed",
            TaskKind::Rerank => "rerank",
            TaskKind::Tts => "tts",
            TaskKind::Transcribe => "transcribe",
            TaskKind::Decision => "decision",
        }
    }
}

impl fmt::Display for TaskKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for TaskKind {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "chat" => Ok(TaskKind::Chat),
            "completion" => Ok(TaskKind::Completion),
            "embed" => Ok(TaskKind::Embed),
            "rerank" => Ok(TaskKind::Rerank),
            "tts" => Ok(TaskKind::Tts),
            "transcribe" => Ok(TaskKind::Transcribe),
            "decision" => Ok(TaskKind::Decision),
            other => Err(Error::TaskKind(other.to_string())),
        }
    }
}
