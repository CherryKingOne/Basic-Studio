//! OpenAI 兼容 HTTP 网关。
//!
//! 对外只暴露一套协议；引擎差异（vLLM / SGLang / sidecar）由适配器消化。
//! 所有推理都经过：`鉴权 → 准入控制 → 路由选引擎 → 引擎调用（含失败转移）`。

use std::convert::Infallible;
use std::sync::Arc;
use std::time::Instant;

use axum::body::{Body, Bytes};
use axum::extract::State;
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use studio_auth::ApiKeys;
use studio_config::Config;
use studio_core::{InferenceOutcome, InferenceRequest, ModelId, Sampling, StreamEvent, TaskKind};
use studio_engine::{Engine, EngineRegistry};
use studio_obs::Counters;
use studio_registry::ModelIndex;
use studio_scheduler::Scheduler;
use tokio::sync::mpsc::{self, UnboundedSender};

/// 网关共享状态。
#[derive(Clone)]
pub struct AppState {
    pub cfg: Arc<Config>,
    pub index: Arc<ModelIndex>,
    pub registry: Arc<EngineRegistry>,
    pub scheduler: Arc<Scheduler>,
    pub auth: Arc<ApiKeys>,
    pub metrics: Arc<Counters>,
}

impl AppState {
    pub fn new(
        cfg: Config,
        index: ModelIndex,
        registry: EngineRegistry,
        scheduler: Scheduler,
        auth: ApiKeys,
    ) -> Self {
        Self {
            cfg: Arc::new(cfg),
            index: Arc::new(index),
            registry: Arc::new(registry),
            scheduler: Arc::new(scheduler),
            auth: Arc::new(auth),
            metrics: Arc::new(Counters::default()),
        }
    }
}

/// 网关错误 → HTTP 响应。
pub enum GatewayError {
    Unauthorized,
    BadRequest(String),
    ModelNotFound(String),
    Saturated(String),
    NoEngine(String),
    Upstream(String),
    Canceled,
}

impl IntoResponse for GatewayError {
    fn into_response(self) -> Response {
        let (status, code, message, retryable) = match &self {
            GatewayError::Unauthorized => (
                StatusCode::UNAUTHORIZED,
                "unauthorized",
                "missing or invalid API key".to_string(),
                false,
            ),
            GatewayError::BadRequest(message) => (
                StatusCode::BAD_REQUEST,
                "bad_request",
                message.clone(),
                false,
            ),
            GatewayError::ModelNotFound(model) => (
                StatusCode::NOT_FOUND,
                "model_not_found",
                format!("model not found: {model}"),
                false,
            ),
            GatewayError::Saturated(message) => (
                StatusCode::TOO_MANY_REQUESTS,
                "rate_limited",
                message.clone(),
                true,
            ),
            GatewayError::NoEngine(detail) => (
                StatusCode::SERVICE_UNAVAILABLE,
                "no_capable_engine",
                format!("no engine could serve this request: {detail}"),
                true,
            ),
            GatewayError::Upstream(detail) => (
                StatusCode::BAD_GATEWAY,
                "upstream_error",
                detail.clone(),
                true,
            ),
            GatewayError::Canceled => (
                StatusCode::from_u16(499).unwrap_or(StatusCode::BAD_REQUEST),
                "canceled",
                "client canceled the request".to_string(),
                false,
            ),
        };

        let mut body = json!({
            "error": {
                "type": code,
                "message": message,
                "retryable": retryable,
            }
        });

        if retryable {
            body["error"]["retry_after_s"] = json!(5);
        }

        (status, Json(body)).into_response()
    }
}

/// 构造路由。
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz))
        .route("/metrics", get(metrics))
        .route("/v1/models", get(list_models))
        .route("/v1/chat/completions", post(chat_completions))
        .route("/v1/completions", post(completions))
        .route("/v1/embeddings", post(embeddings))
        .route("/v1/audio/speech", post(speech))
        .route("/v1/decide", post(decide))
        .with_state(state)
}

async fn healthz() -> impl IntoResponse {
    Json(json!({ "status": "ok" }))
}

async fn readyz(State(state): State<AppState>) -> impl IntoResponse {
    let engines = state.registry.list();
    let ready = engines.iter().filter(|e| e.state.is_ready()).count();
    let total = engines.len();
    if total == 0 || ready == 0 {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "ready": false, "ready_engines": ready, "total_engines": total })),
        )
            .into_response()
    } else {
        Json(json!({ "ready": true, "ready_engines": ready, "total_engines": total }))
            .into_response()
    }
}

async fn metrics(State(state): State<AppState>) -> impl IntoResponse {
    let mut text = state.metrics.render();
    for (kind, count) in state.registry.counts_by_kind() {
        text.push_str(&format!(
            "# HELP studio_engine_instances Engine instances by kind\n# TYPE studio_engine_instances gauge\nstudio_engine_instances{{kind=\"{kind}\"}} {count}\n"
        ));
    }
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/plain; version=0.0.4")],
        text,
    )
        .into_response()
}

async fn list_models(State(state): State<AppState>) -> impl IntoResponse {
    let data: Vec<Value> = state
        .index
        .iter()
        .map(|card| {
            json!({
                "id": card.id.to_string(),
                "object": "model",
                "task": card.task.to_string(),
                "engines": card.engines.iter().map(|e| e.to_string()).collect::<Vec<_>>(),
                "adapter": card.adapter,
                "api_version": card.api_version,
                "streaming": card.streaming,
            })
        })
        .collect();

    Json(json!({ "object": "list", "data": data }))
}

// ---------------------------------------------------------------- 请求体定义

#[derive(Debug, Deserialize)]
pub struct ChatRequest {
    pub model: String,
    #[serde(default)]
    pub messages: Vec<ChatMessage>,
    #[serde(default)]
    pub prompt: Option<String>,
    #[serde(default)]
    pub stream: bool,
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default)]
    pub top_p: Option<f32>,
    #[serde(default)]
    pub max_tokens: Option<u32>,
}

#[derive(Debug, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: Value,
}

impl ChatMessage {
    fn text(&self) -> String {
        match &self.content {
            Value::String(text) => text.clone(),
            other => other.to_string(),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct CompletionRequest {
    pub model: String,
    pub prompt: String,
    #[serde(default)]
    pub stream: bool,
    #[serde(default)]
    pub max_tokens: Option<u32>,
}

#[derive(Debug, Deserialize)]
pub struct EmbeddingRequest {
    pub model: String,
    pub input: String,
}

#[derive(Debug, Deserialize)]
pub struct SpeechRequest {
    pub model: String,
    pub input: String,
    #[serde(default)]
    pub voice: Option<String>,
    #[serde(default)]
    pub stream: bool,
    #[serde(default)]
    pub emotion: Option<String>,
    #[serde(default)]
    pub speed: Option<f32>,
}

#[derive(Debug, Deserialize)]
pub struct DecideRequest {
    pub model: String,
    pub state: Value,
    #[serde(default)]
    pub context: Option<String>,
}

// ---------------------------------------------------------------- 通用逻辑

fn authorize(state: &AppState, headers: &HeaderMap) -> Result<(), GatewayError> {
    let header = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok());
    if state.auth.check(header) {
        Ok(())
    } else {
        Err(GatewayError::Unauthorized)
    }
}

fn request(model: &str, task: TaskKind, prompt: String) -> InferenceRequest {
    InferenceRequest::new(ModelId::new(model), task, prompt)
}

/// 依次尝试候选引擎；只有在"尚未向客户端输出任何内容"时才允许换引擎。
///
/// 返回成功的引擎与结果。
async fn dispatch(
    state: &AppState,
    req: InferenceRequest,
    client_tx: UnboundedSender<StreamEvent>,
) -> Result<(Arc<dyn Engine>, InferenceOutcome, Option<u64>), GatewayError> {
    let available = state.cfg.enabled_engines();
    let selections =
        studio_router::select_for(&state.index, Some(req.model.as_str()), req.task, &available);

    if selections.is_empty() {
        return Err(GatewayError::ModelNotFound(req.model.to_string()));
    }

    let mut last_error = String::from("no engine available");

    for selection in selections {
        let engine = match state
            .registry
            .for_model(selection.engine, &selection.model_id)
        {
            Some(engine) => engine,
            None => continue,
        };

        let permit = match state.scheduler.acquire(&selection.model_id).await {
            Ok(permit) => permit,
            Err(err) => {
                state
                    .metrics
                    .queue_rejects
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                return Err(GatewayError::Saturated(err.to_string()));
            }
        };

        let outcome = call_engine(&engine, &req, &client_tx, permit).await;
        match outcome {
            Ok(outcome) => return Ok((engine, outcome.0, outcome.1)),
            Err(err) => {
                state.metrics.inc_failed();
                if err.forwarded {
                    return Err(GatewayError::Upstream(err.message));
                }
                tracing::warn!(
                    engine = %engine.label(),
                    error = %err.message,
                    "engine attempt failed, trying next candidate"
                );
                last_error = err.message;
            }
        }
    }

    Err(GatewayError::NoEngine(last_error))
}

struct AttemptError {
    message: String,
    forwarded: bool,
}

/// 调用单个引擎并把事件转发给客户端。
async fn call_engine(
    engine: &Arc<dyn Engine>,
    req: &InferenceRequest,
    client_tx: &UnboundedSender<StreamEvent>,
    _permit: studio_scheduler::Permit,
) -> Result<(InferenceOutcome, Option<u64>), AttemptError> {
    let (tx, mut rx) = mpsc::unbounded_channel::<StreamEvent>();
    let engine_clone = engine.clone();
    let req_clone = req.clone();

    let handle = tokio::spawn(async move {
        let result = if req_clone.task == TaskKind::Tts {
            engine_clone.tts(&req_clone, tx).await
        } else {
            engine_clone.infer(&req_clone, tx).await
        };
        result
    });

    let mut outcome = InferenceOutcome::default();
    let mut forwarded = false;
    let start = Instant::now();
    let mut first_token_ms: Option<u64> = None;

    while let Some(event) = rx.recv().await {
        match &event {
            StreamEvent::Error(message) => {
                return Err(AttemptError {
                    message: message.clone(),
                    forwarded,
                });
            }
            StreamEvent::Delta(_) | StreamEvent::Audio(_) => {
                if first_token_ms.is_none() {
                    first_token_ms = Some(start.elapsed().as_millis() as u64);
                }
            }
            StreamEvent::Done(_) => {}
        }

        outcome.record(&event);
        if client_tx.send(event).is_ok() {
            forwarded = true;
        } else {
            // 客户端已断开，停止转发但让引擎跑完（避免资源泄漏）。
            break;
        }
    }

    let joined = handle.await.map_err(|e| AttemptError {
        message: format!("engine task failed: {e}"),
        forwarded,
    })?;

    let ttft = first_token_ms.or_else(|| Some(start.elapsed().as_millis() as u64));

    match joined {
        Ok(final_outcome) => Ok((final_outcome, ttft)),
        Err(err) => Err(AttemptError {
            message: err.to_string(),
            forwarded,
        }),
    }
}

fn sse_stream(
    rx: mpsc::UnboundedReceiver<StreamEvent>,
) -> impl futures_util::Stream<Item = Result<Bytes, Infallible>> {
    futures_util::stream::unfold(rx, |mut rx| async move {
        rx.recv().await.map(|event| {
            let payload = match event {
                StreamEvent::Delta(text) => json!({
                    "choices": [{ "delta": { "content": text } }]
                }),
                StreamEvent::Done(usage) => json!({
                    "choices": [],
                    "usage": {
                        "prompt_tokens": usage.prompt_tokens,
                        "completion_tokens": usage.completion_tokens,
                    }
                }),
                StreamEvent::Audio(_) => json!({ "ignored": true }),
                StreamEvent::Error(message) => json!({ "error": message }),
            };
            let chunk = format!("data: {payload}\n\n");
            (Ok::<_, Infallible>(Bytes::from(chunk)), rx)
        })
    })
}

/// 把文本事件聚合成完整回复。
fn collect_text(events: Vec<StreamEvent>) -> (String, InferenceOutcome) {
    let mut text = String::new();
    let mut outcome = InferenceOutcome::default();
    for event in events {
        if let StreamEvent::Delta(delta) = &event {
            text.push_str(delta);
        }
        outcome.record(&event);
    }
    (text, outcome)
}

fn drain(rx: &mut mpsc::UnboundedReceiver<StreamEvent>) -> Vec<StreamEvent> {
    let mut events = Vec::new();
    while let Ok(event) = rx.try_recv() {
        let done = matches!(event, StreamEvent::Done(_));
        events.push(event);
        if done {
            break;
        }
    }
    events
}

fn track(state: &AppState, outcome: &InferenceOutcome, first_ms: Option<u64>) {
    state.metrics.inc_requests();
    if let Some(usage) = outcome.usage {
        state
            .metrics
            .add_usage(usage.prompt_tokens, usage.completion_tokens);
    }
    if let Some(ms) = first_ms {
        state.metrics.record_ttft(ms);
    }
}

// ---------------------------------------------------------------- 处理器

async fn chat_completions(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<ChatRequest>,
) -> Result<Response, GatewayError> {
    authorize(&state, &headers)?;

    let prompt = if !body.messages.is_empty() {
        body.messages
            .iter()
            .rev()
            .find(|m| m.role == "user")
            .map(ChatMessage::text)
            .or_else(|| body.prompt.clone())
            .unwrap_or_default()
    } else {
        body.prompt.clone().unwrap_or_default()
    };

    let system = body
        .messages
        .iter()
        .find(|m| m.role == "system")
        .map(ChatMessage::text);

    let mut req = request(&body.model, TaskKind::Chat, prompt)
        .with_stream(body.stream)
        .with_sampling(Sampling {
            temperature: body.temperature,
            top_p: body.top_p,
            max_tokens: body.max_tokens,
            emotion: None,
        });
    req.system = system;

    run_request(state, req).await
}

async fn completions(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<CompletionRequest>,
) -> Result<Response, GatewayError> {
    authorize(&state, &headers)?;

    let req = request(&body.model, TaskKind::Completion, body.prompt)
        .with_stream(body.stream)
        .with_sampling(Sampling {
            max_tokens: body.max_tokens,
            ..Sampling::default()
        });

    run_request(state, req).await
}

async fn embeddings(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<EmbeddingRequest>,
) -> Result<Response, GatewayError> {
    authorize(&state, &headers)?;
    let req = request(&body.model, TaskKind::Embed, body.input);
    run_request(state, req).await
}

async fn decide(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<DecideRequest>,
) -> Result<Response, GatewayError> {
    authorize(&state, &headers)?;

    let prompt = match (&body.context, &body.state) {
        (Some(context), state_value) => {
            json!({ "context": context, "state": state_value }).to_string()
        }
        (None, state_value) => state_value.to_string(),
    };

    let req = request(&body.model, TaskKind::Decision, prompt);
    let (client_tx, mut rx) = mpsc::unbounded_channel::<StreamEvent>();
    let (engine, outcome, ttft) = dispatch(&state, req, client_tx).await?;
    let events = drain(&mut rx);
    let (text, ev_outcome) = collect_text(events);
    track(&state, &ev_outcome, ttft);

    Ok(Json(json!({
        "model": body.model,
        "output": text,
        "engine": engine.label(),
        "usage": outcome.usage,
    }))
    .into_response())
}

async fn speech(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<SpeechRequest>,
) -> Result<Response, GatewayError> {
    authorize(&state, &headers)?;

    let mut req = request(&body.model, TaskKind::Tts, body.input).with_stream(body.stream);
    req.voice = body.voice;
    req.sampling.emotion = body.emotion;
    req.extra = json!({ "speed": body.speed });

    if !body.stream {
        let (client_tx, mut rx) = mpsc::unbounded_channel::<StreamEvent>();
        let (engine, outcome, ttft) = dispatch(&state, req, client_tx).await?;
        let events = drain(&mut rx);

        let mut pcm = Vec::new();
        let mut rate = None;
        for event in events {
            if let StreamEvent::Audio(chunk) = event {
                rate.get_or_insert(chunk.sample_rate);
                pcm.extend_from_slice(&chunk.pcm);
            }
        }

        state.metrics.add_audio(pcm.len() as u64);
        track(&state, &outcome, ttft);

        let mut response = Response::new(Body::from(pcm));
        response.headers_mut().insert(
            header::CONTENT_TYPE,
            header::HeaderValue::from_static("audio/pcm"),
        );
        if let Some(rate) = rate {
            if let Ok(value) = header::HeaderValue::from_str(&rate.to_string()) {
                response.headers_mut().insert("x-audio-rate", value);
            }
        }
        let _ = engine;
        return Ok(response);
    }

    let (client_tx, rx) = mpsc::unbounded_channel::<StreamEvent>();
    let state_clone = state.clone();
    tokio::spawn(async move {
        let outcome = dispatch(&state_clone, req, client_tx)
            .await
            .map(|(_, outcome, ttft)| (outcome, ttft));
        if let Ok((outcome, ttft)) = outcome {
            track(&state_clone, &outcome, ttft);
        }
    });

    let stream = futures_util::stream::unfold(rx, |mut rx| async move {
        rx.recv().await.map(|event| match event {
            StreamEvent::Audio(chunk) => {
                let bytes = chunk.pcm;
                (Ok::<_, Infallible>(Bytes::from(bytes)), rx)
            }
            StreamEvent::Error(message) => {
                (Ok::<_, Infallible>(Bytes::from(message.into_bytes())), rx)
            }
            _ => (Ok::<_, Infallible>(Bytes::from_static(b"")), rx),
        })
    });

    let mut response = Response::new(Body::from_stream(stream));
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        header::HeaderValue::from_static("audio/pcm"),
    );
    Ok(response)
}

/// 文本类请求的统一入口：流式返回 SSE，非流式聚合为 JSON。
async fn run_request(state: AppState, req: InferenceRequest) -> Result<Response, GatewayError> {
    let model = req.model.to_string();
    let stream = req.stream;

    if stream {
        let (client_tx, rx) = mpsc::unbounded_channel::<StreamEvent>();
        let state_clone = state.clone();
        tokio::spawn(async move {
            let outcome = dispatch(&state_clone, req, client_tx)
                .await
                .map(|(_, outcome, ttft)| (outcome, ttft));
            if let Ok((outcome, ttft)) = outcome {
                track(&state_clone, &outcome, ttft);
            }
        });

        let response = Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "text/event-stream")
            .header(header::CACHE_CONTROL, "no-cache")
            .body(Body::from_stream(sse_stream(rx)))
            .map_err(|e| GatewayError::Upstream(e.to_string()))?;
        return Ok(response);
    }

    let (client_tx, mut rx) = mpsc::unbounded_channel::<StreamEvent>();
    let (engine, outcome, ttft) = dispatch(&state, req, client_tx).await?;
    let events = drain(&mut rx);
    let (text, outcome_from_events) = collect_text(events);

    track(&state, &outcome_from_events, ttft);

    let mut payload = json!({
        "id": format!("chatcmpl-{}", &model.replace('/', "-")),
        "object": "chat.completion",
        "model": model,
        "choices": [{
            "index": 0,
            "message": { "role": "assistant", "content": text },
            "finish_reason": "stop",
        }],
        "engine": engine.label(),
    });

    if let Some(usage) = outcome.usage.or(outcome_from_events.usage) {
        payload["usage"] = json!({
            "prompt_tokens": usage.prompt_tokens,
            "completion_tokens": usage.completion_tokens,
            "total_tokens": usage.total(),
        });
    }

    Ok(Json(payload).into_response())
}

/// 启动网关并阻塞直到收到 Ctrl-C。
pub async fn serve(
    state: AppState,
    listen: &str,
    shutdown: impl std::future::Future<Output = ()> + Send + 'static,
) -> anyhow::Result<()> {
    let listener = tokio::net::TcpListener::bind(listen).await?;
    let local = listener.local_addr()?;
    tracing::info!(%local, "gateway listening");

    axum::serve(listener, router(state))
        .with_graceful_shutdown(shutdown)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sse_payload_shape() {
        let chunk = json!({ "choices": [{ "delta": { "content": "hi" } }] });
        assert_eq!(chunk["choices"][0]["delta"]["content"], "hi");
    }

    #[test]
    fn collect_text_joins_deltas() {
        let events = vec![
            StreamEvent::Delta("你".into()),
            StreamEvent::Delta("好".into()),
            StreamEvent::Done(Default::default()),
        ];
        let (text, outcome) = collect_text(events);
        assert_eq!(text, "你好");
        assert_eq!(outcome.events, 3);
    }

    #[test]
    fn request_builder_sets_stream() {
        let req = request("m", TaskKind::Chat, "hi".to_string()).with_stream(true);
        assert!(req.stream);
        assert_eq!(req.task, TaskKind::Chat);
    }
}
