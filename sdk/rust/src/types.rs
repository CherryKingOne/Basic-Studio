use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChatMessage {
    pub role: String,
    pub content: Value,
}

impl ChatMessage {
    pub fn system(content: impl Into<Value>) -> Self {
        Self {
            role: "system".to_string(),
            content: content.into(),
        }
    }

    pub fn user(content: impl Into<Value>) -> Self {
        Self {
            role: "user".to_string(),
            content: content.into(),
        }
    }

    pub fn assistant(content: impl Into<Value>) -> Self {
        Self {
            role: "assistant".to_string(),
            content: content.into(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub stream: bool,
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
    pub max_tokens: Option<u32>,
}

impl ChatRequest {
    pub fn new(model: impl Into<String>, prompt: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            messages: vec![ChatMessage::user(prompt.into())],
            stream: false,
            temperature: None,
            top_p: None,
            max_tokens: None,
        }
    }

    pub fn with_system(mut self, content: impl Into<Value>) -> Self {
        self.messages.insert(0, ChatMessage::system(content));
        self
    }

    pub fn with_stream(mut self, stream: bool) -> Self {
        self.stream = stream;
        self
    }

    pub fn with_temperature(mut self, temperature: f32) -> Self {
        self.temperature = Some(temperature);
        self
    }

    pub fn with_max_tokens(mut self, max_tokens: u32) -> Self {
        self.max_tokens = Some(max_tokens);
        self
    }

    pub(crate) fn to_body(&self) -> Value {
        let mut body = serde_json::json!({
            "model": self.model,
            "messages": self.messages,
            "stream": self.stream,
        });
        if let Some(temperature) = self.temperature {
            body["temperature"] = serde_json::json!(temperature);
        }
        if let Some(top_p) = self.top_p {
            body["top_p"] = serde_json::json!(top_p);
        }
        if let Some(max_tokens) = self.max_tokens {
            body["max_tokens"] = serde_json::json!(max_tokens);
        }
        if self.stream {
            body["stream_options"] = serde_json::json!({ "include_usage": true });
        }
        body
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChatResponse {
    pub id: String,
    pub model: String,
    pub choices: Vec<ChatChoice>,
    #[serde(default)]
    pub engine: Option<String>,
    #[serde(default)]
    pub usage: Option<Usage>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChatChoice {
    #[serde(default)]
    pub message: Option<ChatMessageOut>,
    #[serde(default)]
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChatMessageOut {
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub content: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, Deserialize)]
pub struct Usage {
    #[serde(default)]
    pub prompt_tokens: u64,
    #[serde(default)]
    pub completion_tokens: u64,
}

impl ChatResponse {
    pub fn content(&self) -> String {
        self.choices
            .first()
            .and_then(|choice| choice.message.as_ref())
            .and_then(|message| message.content.clone())
            .unwrap_or_default()
    }
}

#[derive(Debug, Clone)]
pub struct ResponsesRequest {
    pub model: String,
    pub input: Value,
    pub stream: bool,
    pub instructions: Option<String>,
    pub max_output_tokens: Option<u32>,
    pub temperature: Option<f32>,
    pub previous_response_id: Option<String>,
}

impl ResponsesRequest {
    pub fn new(model: impl Into<String>, input: impl Into<Value>) -> Self {
        Self {
            model: model.into(),
            input: input.into(),
            stream: false,
            instructions: None,
            max_output_tokens: None,
            temperature: None,
            previous_response_id: None,
        }
    }

    pub fn with_instructions(mut self, instructions: impl Into<String>) -> Self {
        self.instructions = Some(instructions.into());
        self
    }

    pub fn with_stream(mut self, stream: bool) -> Self {
        self.stream = stream;
        self
    }

    pub fn with_previous(mut self, id: impl Into<String>) -> Self {
        self.previous_response_id = Some(id.into());
        self
    }

    pub fn to_body(&self) -> Value {
        let mut body = serde_json::json!({
            "model": self.model,
            "input": self.input,
            "stream": self.stream,
        });
        if let Some(instructions) = &self.instructions {
            body["instructions"] = serde_json::json!(instructions);
        }
        if let Some(max_output_tokens) = self.max_output_tokens {
            body["max_output_tokens"] = serde_json::json!(max_output_tokens);
        }
        if let Some(temperature) = self.temperature {
            body["temperature"] = serde_json::json!(temperature);
        }
        if let Some(previous) = &self.previous_response_id {
            body["previous_response_id"] = serde_json::json!(previous);
        }
        body
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ResponsesResponse {
    pub id: String,
    pub model: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub output_text: String,
    #[serde(default)]
    pub output: Vec<Value>,
}

#[derive(Debug, Clone)]
pub struct EmbeddingRequest {
    pub model: String,
    pub input: String,
}

impl EmbeddingRequest {
    pub fn new(model: impl Into<String>, input: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            input: input.into(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct SpeechRequest {
    pub model: String,
    pub input: String,
    pub voice: Option<String>,
    pub emotion: Option<String>,
}

impl SpeechRequest {
    pub fn new(model: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            input: text.into(),
            voice: None,
            emotion: None,
        }
    }

    pub fn with_voice(mut self, voice: impl Into<String>) -> Self {
        self.voice = Some(voice.into());
        self
    }

    pub fn with_emotion(mut self, emotion: impl Into<String>) -> Self {
        self.emotion = Some(emotion.into());
        self
    }

    pub fn to_body(&self) -> Value {
        let mut body = serde_json::json!({
            "model": self.model,
            "input": self.input,
            "stream": true,
        });
        if let Some(voice) = &self.voice {
            body["voice"] = serde_json::json!(voice);
        }
        if let Some(emotion) = &self.emotion {
            body["emotion"] = serde_json::json!(emotion);
        }
        body
    }
}

#[derive(Debug, Clone)]
pub struct DecideRequest {
    pub model: String,
    pub state: Value,
    pub context: Option<String>,
}

impl DecideRequest {
    pub fn new(model: impl Into<String>, state: impl Into<Value>) -> Self {
        Self {
            model: model.into(),
            state: state.into(),
            context: None,
        }
    }

    pub fn with_context(mut self, context: impl Into<String>) -> Self {
        self.context = Some(context.into());
        self
    }

    pub fn to_body(&self) -> Value {
        let mut body = serde_json::json!({ "model": self.model, "state": self.state });
        if let Some(context) = &self.context {
            body["context"] = serde_json::json!(context);
        }
        body
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct DecideResponse {
    pub model: String,
    pub output: String,
    #[serde(default)]
    pub engine: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ModelInfo {
    pub id: String,
    #[serde(default)]
    pub task: String,
    #[serde(default)]
    pub engines: Vec<String>,
    #[serde(default)]
    pub adapter: Option<String>,
    #[serde(default)]
    pub api_version: Option<String>,
    #[serde(default)]
    pub streaming: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ModelList {
    #[serde(default)]
    pub data: Vec<ModelInfo>,
}
