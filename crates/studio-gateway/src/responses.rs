//! OpenAI Responses API（`/v1/responses`）。
//!
//! 与 Chat Completions 并存，不复用其请求体：
//! - `input` 支持字符串或消息数组；
//! - `instructions` 等价于 system prompt；
//! - 非流式返回 `output[]` 结构，流式返回带 `event:` 的类型化事件；
//! - `previous_response_id` 通过内置存储实现多轮（有 TTL，默认 1 小时）。

use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::response::sse::Event;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};
use studio_core::{InferenceRequest, StreamEvent, TaskKind, Usage};
use tokio::sync::mpsc;

use crate::{
    authorize, collect_text, dispatch, drain, request, sse_response, track, AppState, GatewayError,
};

/// 单条已完成的响应，供 `previous_response_id` 使用。
#[derive(Debug, Clone)]
pub struct StoredResponse {
    pub id: String,
    pub model: String,
    pub text: String,
    pub created_at: u64,
    pub status: String,
}

/// 响应存储：有界 + TTL，避免无限增长。
#[derive(Debug)]
pub struct ResponseStore {
    inner: Mutex<HashMap<String, StoredResponse>>,
    ttl: Duration,
}

impl Default for ResponseStore {
    fn default() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
            ttl: Duration::from_secs(3600),
        }
    }
}

impl ResponseStore {
    pub fn with_ttl(ttl_s: u64) -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
            ttl: Duration::from_secs(ttl_s.max(60)),
        }
    }

    pub fn put(&self, response: StoredResponse) {
        let mut map = self.inner.lock().expect("response store lock");
        map.insert(response.id.clone(), response);
        self.evict_expired(&mut map);
    }

    pub fn get(&self, id: &str) -> Option<StoredResponse> {
        let mut map = self.inner.lock().expect("response store lock");
        self.evict_expired(&mut map);
        map.get(id).cloned()
    }

    pub fn remove(&self, id: &str) -> bool {
        self.inner
            .lock()
            .expect("response store lock")
            .remove(id)
            .is_some()
    }

    pub fn len(&self) -> usize {
        self.inner.lock().expect("response store lock").len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn evict_expired(&self, map: &mut HashMap<String, StoredResponse>) {
        let now = now_secs();
        map.retain(|_, stored| now.saturating_sub(stored.created_at) < self.ttl.as_secs());
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn new_id(prefix: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    format!("{prefix}_{:08x}{:04x}", now_secs(), nanos & 0xffff)
}

// ------------------------------------------------------------------ 请求体

#[derive(Debug, Deserialize)]
pub struct ResponsesRequest {
    pub model: String,
    /// 字符串或消息数组（`[{role, content}]`）。
    pub input: Value,
    #[serde(default)]
    pub instructions: Option<String>,
    #[serde(default)]
    pub stream: bool,
    #[serde(default)]
    pub max_output_tokens: Option<u32>,
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default)]
    pub top_p: Option<f32>,
    #[serde(default)]
    pub previous_response_id: Option<String>,
}

/// 解析 `input` 字段，返回 (system, prompt)。
pub fn parse_input(input: &Value) -> (Option<String>, String) {
    match input {
        Value::String(text) => (None, text.clone()),
        Value::Array(items) => {
            let mut system_parts: Vec<String> = Vec::new();
            let mut dialogue: Vec<String> = Vec::new();

            for item in items {
                let role = item
                    .get("role")
                    .and_then(|v| v.as_str())
                    .unwrap_or("user")
                    .to_string();
                let text = content_to_text(item.get("content").unwrap_or(&Value::Null));

                if role == "system" || role == "developer" {
                    system_parts.push(text);
                } else {
                    dialogue.push(format!("{role}: {text}"));
                }
            }

            let system = (!system_parts.is_empty()).then(|| system_parts.join("\n"));
            (system, dialogue.join("\n"))
        }
        other => (None, other.to_string()),
    }
}

/// content 可以是字符串，也可以是 `[{type:"input_text", text:"..."}]`。
pub fn content_to_text(content: &Value) -> String {
    match content {
        Value::String(text) => text.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|part| {
                part.get("text")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .or_else(|| part.as_str().map(str::to_string))
            })
            .collect::<Vec<_>>()
            .join(""),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn build_request(
    state: &AppState,
    body: &ResponsesRequest,
) -> Result<InferenceRequest, GatewayError> {
    let (system, prompt) = parse_input(&body.input);

    let prompt = match &body.previous_response_id {
        Some(id) => {
            let previous = state.responses.get(id).ok_or_else(|| {
                GatewayError::BadRequest(format!("previous_response_id not found: {id}"))
            })?;
            format!(
                "Previous response:\n{}\n\nCurrent input:\n{}",
                previous.text, prompt
            )
        }
        None => prompt,
    };

    let mut inference = request(&body.model, TaskKind::Chat, prompt)
        .with_stream(body.stream)
        .with_sampling(studio_core::Sampling {
            temperature: body.temperature,
            top_p: body.top_p,
            max_tokens: body.max_output_tokens,
            emotion: None,
        });

    inference.system = body.instructions.clone().or(system);
    Ok(inference)
}

// ------------------------------------------------------------------ 响应体

fn output_items(text: &str, item_id: &str) -> Value {
    json!([{
        "id": item_id,
        "type": "message",
        "status": "completed",
        "role": "assistant",
        "content": [{
            "type": "output_text",
            "text": text,
            "annotations": []
        }]
    }])
}

/// 非流式响应体。
pub fn response_payload(
    model: &str,
    text: &str,
    prompt_tokens: u64,
    completion_tokens: u64,
) -> Value {
    json!({
        "id": new_id("resp"),
        "object": "response",
        "created_at": now_secs(),
        "status": "completed",
        "model": model,
        "output": output_items(text, &new_id("msg")),
        "output_text": text,
        "usage": {
            "input_tokens": prompt_tokens,
            "output_tokens": completion_tokens,
            "total_tokens": prompt_tokens + completion_tokens,
        }
    })
}

/// 流式事件帧内容（不含 `event:` 前缀）。
pub fn stream_payload(event: &str, payload: &Value) -> String {
    let _ = event;
    payload.to_string()
}

struct StreamState {
    rx: mpsc::UnboundedReceiver<StreamEvent>,
    response_id: String,
    item_id: String,
    model: String,
    stage: u8,
    text: String,
    usage: Option<Usage>,
    pending: Vec<(String, String)>,
    finished: bool,
}

impl StreamState {
    async fn next_event(&mut self) -> Option<(String, String)> {
        loop {
            if !self.pending.is_empty() {
                return self.pending.remove(0).into();
            }

            match self.stage {
                0 => {
                    self.stage = 1;
                    return Some((
                        "response.created".to_string(),
                        json!({
                            "type": "response.created",
                            "response": {
                                "id": self.response_id,
                                "object": "response",
                                "status": "in_progress",
                                "model": self.model,
                                "output": []
                            }
                        })
                        .to_string(),
                    ));
                }
                1 => {
                    self.stage = 2;
                    return Some((
                        "response.in_progress".to_string(),
                        json!({
                            "type": "response.in_progress",
                            "response": { "id": self.response_id, "object": "response", "status": "in_progress" }
                        })
                        .to_string(),
                    ));
                }
                2 => {
                    self.stage = 3;
                    return Some((
                        "response.output_item.added".to_string(),
                        json!({
                            "type": "response.output_item.added",
                            "output_index": 0,
                            "item": {
                                "id": self.item_id,
                                "type": "message",
                                "status": "in_progress",
                                "role": "assistant",
                                "content": []
                            }
                        })
                        .to_string(),
                    ));
                }
                3 => {
                    self.stage = 4;
                    return Some((
                        "response.content_part.added".to_string(),
                        json!({
                            "type": "response.content_part.added",
                            "item_id": self.item_id,
                            "output_index": 0,
                            "content_index": 0,
                            "part": { "type": "output_text", "text": "", "annotations": [] }
                        })
                        .to_string(),
                    ));
                }
                _ => {}
            }

            let Some(event) = self.rx.recv().await else {
                if self.finished {
                    return None;
                }
                self.finished = true;
                if self.usage.is_none() {
                    self.usage = Some(Usage::default());
                }
                return Some(("response.completed".to_string(), self.completed_payload()));
            };

            match event {
                StreamEvent::Delta(text) => {
                    self.text.push_str(&text);
                    return Some((
                        "response.output_text.delta".to_string(),
                        json!({
                            "type": "response.output_text.delta",
                            "item_id": self.item_id,
                            "output_index": 0,
                            "content_index": 0,
                            "delta": text
                        })
                        .to_string(),
                    ));
                }
                StreamEvent::Done(usage) => {
                    self.usage = Some(usage);
                    self.pending.push((
                        "response.output_text.done".to_string(),
                        json!({
                            "type": "response.output_text.done",
                            "item_id": self.item_id,
                            "output_index": 0,
                            "content_index": 0,
                            "text": self.text
                        })
                        .to_string(),
                    ));
                    self.pending
                        .push(("response.completed".to_string(), self.completed_payload()));
                    return self.pending.remove(0).into();
                }
                StreamEvent::Audio(_) => continue,
                StreamEvent::Error(message) => {
                    return Some((
                        "response.failed".to_string(),
                        json!({
                            "type": "response.failed",
                            "response": {
                                "id": self.response_id,
                                "status": "failed",
                                "error": { "message": message }
                            }
                        })
                        .to_string(),
                    ));
                }
            }
        }
    }

    fn completed_payload(&self) -> String {
        let usage = self.usage.unwrap_or_default();
        json!({
            "type": "response.completed",
            "response": {
                "id": self.response_id,
                "object": "response",
                "status": "completed",
                "model": self.model,
                "output": output_items(&self.text, &self.item_id),
                "usage": {
                    "input_tokens": usage.prompt_tokens,
                    "output_tokens": usage.completion_tokens,
                    "total_tokens": usage.total(),
                }
            }
        })
        .to_string()
    }
}

fn responses_sse(model: String, rx: mpsc::UnboundedReceiver<StreamEvent>) -> Response {
    let state = StreamState {
        rx,
        response_id: new_id("resp"),
        item_id: new_id("msg"),
        model,
        stage: 0,
        text: String::new(),
        usage: None,
        pending: Vec::new(),
        finished: false,
    };

    let stream = futures_util::stream::unfold(state, |mut state| async move {
        state.next_event().await.map(|(name, data)| {
            (
                Ok::<_, Infallible>(Event::default().event(name).data(data)),
                state,
            )
        })
    });

    sse_response(stream)
}

// ------------------------------------------------------------------ 处理器

pub(crate) async fn create(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    raw: Bytes,
) -> Result<Response, GatewayError> {
    authorize(&state, &headers)?;

    let body: ResponsesRequest = serde_json::from_slice(raw.as_ref())
        .map_err(|e| GatewayError::BadRequest(format!("invalid request body: {e}")))?;

    let model = body.model.clone();
    let inference = build_request(&state, &body)?;

    if body.stream {
        let (client_tx, rx) = mpsc::unbounded_channel::<StreamEvent>();
        let state_clone = state.clone();

        tokio::spawn(async move {
            if let Ok((outcome, ttft)) = dispatch(&state_clone, inference, client_tx)
                .await
                .map(|(_, outcome, ttft)| (outcome, ttft))
            {
                track(&state_clone, &outcome, ttft);
            }
        });

        return Ok(responses_sse(model, rx));
    }

    let (client_tx, mut rx) = mpsc::unbounded_channel::<StreamEvent>();
    let (engine, outcome, ttft) = dispatch(&state, inference, client_tx).await?;
    let events = drain(&mut rx);
    let (text, ev_outcome) = collect_text(events);
    track(&state, &ev_outcome, ttft);

    let prompt_tokens = ev_outcome.usage.map(|u| u.prompt_tokens).unwrap_or(0);
    let completion_tokens = ev_outcome
        .usage
        .or(outcome.usage)
        .map(|u| u.completion_tokens)
        .unwrap_or(0);

    let payload = response_payload(&model, &text, prompt_tokens, completion_tokens);

    if let Some(id) = payload.get("id").and_then(|v| v.as_str()) {
        state.responses.put(StoredResponse {
            id: id.to_string(),
            model: format!("{} ({})", model, engine.label()),
            text,
            created_at: now_secs(),
            status: "completed".to_string(),
        });
    }

    Ok(Json(payload).into_response())
}

pub(crate) async fn retrieve(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, GatewayError> {
    authorize(&state, &headers)?;

    let stored = state
        .responses
        .get(&id)
        .ok_or_else(|| GatewayError::BadRequest(format!("response not found: {id}")))?;

    Ok(Json(json!({
        "id": stored.id,
        "object": "response",
        "created_at": stored.created_at,
        "status": stored.status,
        "model": stored.model,
        "output_text": stored.text,
    }))
    .into_response())
}

pub(crate) async fn cancel(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, GatewayError> {
    authorize(&state, &headers)?;

    let removed = state.responses.remove(&id);
    Ok(Json(json!({ "id": id, "cancelled": removed })).into_response())
}

/// 供文档引用的协议摘要。
pub const PROTOCOL_NOTE: &str =
    "Responses API: POST /v1/responses, GET /v1/responses/{id}, DELETE /v1/responses/{id}";

#[allow(dead_code)]
fn _unused(_: fn(&str) -> String) {
    let _ = stream_payload;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_string_input() {
        let (system, prompt) = parse_input(&json!("hello"));
        assert!(system.is_none());
        assert_eq!(prompt, "hello");
    }

    #[test]
    fn parses_message_array_with_system() {
        let (system, prompt) = parse_input(&json!([
            { "role": "system", "content": "be brief" },
            { "role": "user", "content": "hi" }
        ]));
        assert_eq!(system.as_deref(), Some("be brief"));
        assert_eq!(prompt, "user: hi");
    }

    #[test]
    fn parses_content_parts() {
        let text = content_to_text(&json!([
            { "type": "input_text", "text": "foo" },
            { "type": "input_text", "text": "bar" }
        ]));
        assert_eq!(text, "foobar");
    }

    #[test]
    fn payload_shape_matches_responses_schema() {
        let payload = response_payload("m", "hello", 3, 5);
        assert_eq!(payload["object"], "response");
        assert_eq!(payload["status"], "completed");
        assert_eq!(payload["output"][0]["content"][0]["type"], "output_text");
        assert_eq!(payload["usage"]["total_tokens"], 8);
    }

    #[test]
    fn store_evicts_after_ttl() {
        let store = ResponseStore::with_ttl(1);
        store.put(StoredResponse {
            id: "resp_old".to_string(),
            model: "m".to_string(),
            text: "t".to_string(),
            created_at: now_secs() - 120,
            status: "completed".to_string(),
        });
        assert!(store.get("resp_old").is_none());
        assert!(store.is_empty());
    }

    #[test]
    fn store_keeps_fresh_entries() {
        let store = ResponseStore::default();
        store.put(StoredResponse {
            id: "resp_1".to_string(),
            model: "m".to_string(),
            text: "t".to_string(),
            created_at: now_secs(),
            status: "completed".to_string(),
        });
        assert!(store.get("resp_1").is_some());
        assert!(store.remove("resp_1"));
        assert!(!store.remove("resp_1"));
    }
}
