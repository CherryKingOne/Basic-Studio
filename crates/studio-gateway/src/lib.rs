//! OpenAI 兼容 HTTP 网关（基于 Salvo）。
//!
//! 对外暴露 Chat Completions、Responses、Embeddings、Audio Speech 等接口；
//! 引擎差异（vLLM / SGLang / Transformers / llama.cpp）由适配器消化。
//! 所有推理都经过：`鉴权 → 准入控制 → 路由选引擎 → 引擎调用（含失败转移）`。

use std::convert::Infallible;
use std::sync::Arc;
use std::time::Instant;

use salvo::http::{header, HeaderValue};
use salvo::prelude::*;
use salvo::sse::{self, SseEvent};
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

mod responses;

pub use responses::{ResponseStore, StoredResponse, PROTOCOL_NOTE};

/// 网关共享状态。
#[derive(Clone)]
pub struct AppState {
    pub cfg: Arc<Config>,
    pub index: Arc<ModelIndex>,
    pub registry: Arc<EngineRegistry>,
    pub scheduler: Arc<Scheduler>,
    pub auth: Arc<ApiKeys>,
    pub metrics: Arc<Counters>,
    /// Responses API 的多轮存储（`previous_response_id`）。
    pub responses: Arc<ResponseStore>,
}

impl AppState {
    pub fn new(
        cfg: Config,
        index: ModelIndex,
        registry: EngineRegistry,
        scheduler: Scheduler,
        auth: ApiKeys,
    ) -> Self {
        Self::with_ttl(cfg, index, registry, scheduler, auth, 3600)
    }

    /// 同 [`AppState::new`]，但可指定 Responses 存储的 TTL（秒）。
    #[allow(clippy::too_many_arguments)]
    pub fn with_ttl(
        cfg: Config,
        index: ModelIndex,
        registry: EngineRegistry,
        scheduler: Scheduler,
        auth: ApiKeys,
        ttl_s: u64,
    ) -> Self {
        Self {
            cfg: Arc::new(cfg),
            index: Arc::new(index),
            registry: Arc::new(registry),
            scheduler: Arc::new(scheduler),
            auth: Arc::new(auth),
            metrics: Arc::new(Counters::default()),
            responses: Arc::new(ResponseStore::with_ttl(ttl_s)),
        }
    }
}

/// 网关错误：统一转成 (状态码, JSON 体)。
pub enum GatewayError {
    Unauthorized,
    BadRequest(String),
    ModelNotFound(String),
    Saturated(String),
    NoEngine(String),
    Upstream(String),
    Canceled,
}

impl GatewayError {
    pub fn into_parts(&self) -> (StatusCode, Value) {
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
                StatusCode::BAD_REQUEST,
                "canceled",
                "client canceled the request".to_string(),
                false,
            ),
        };

        let mut body = json!({
            "error": { "type": code, "message": message, "retryable": retryable }
        });
        if retryable {
            body["error"]["retry_after_s"] = json!(5);
        }
        (status, body)
    }

    fn render(&self, res: &mut Response) {
        let (status, body) = self.into_parts();
        res.status_code(status);
        res.render(Json(body));
    }
}

// ------------------------------------------------------------------ 路由

#[derive(Debug, Clone, Copy)]
enum Route {
    Health,
    Ready,
    Metrics,
    Models,
    Chat,
    Completions,
    Embeddings,
    Speech,
    Decide,
    ResponsesCreate,
    ResponsesRetrieve,
    ResponsesCancel,
}

/// 带状态的 Handler：把 `Arc<AppState>` 放进结构体，避免全局变量。
#[derive(Clone)]
struct Endpoint {
    route: Route,
    state: Arc<AppState>,
}

impl Endpoint {
    async fn dispatch(&self, req: &mut Request, res: &mut Response) -> Result<(), GatewayError> {
        match self.route {
            Route::Health => {
                res.render(Json(json!({ "status": "ok" })));
                Ok(())
            }
            Route::Ready => ready(&self.state, res),
            Route::Metrics => metrics(&self.state, res),
            Route::Models => models(&self.state, res),
            Route::Chat => chat(&self.state, req, res).await,
            Route::Completions => completions(&self.state, req, res).await,
            Route::Embeddings => embeddings(&self.state, req, res).await,
            Route::Speech => speech(&self.state, req, res).await,
            Route::Decide => decide(&self.state, req, res).await,
            Route::ResponsesCreate => responses::create(&self.state, req, res).await,
            Route::ResponsesRetrieve => responses::retrieve(&self.state, req, res).await,
            Route::ResponsesCancel => responses::cancel(&self.state, req, res).await,
        }
    }
}

#[async_trait]
impl Handler for Endpoint {
    async fn handle(
        &self,
        req: &mut Request,
        _depot: &mut Depot,
        res: &mut Response,
        _ctrl: &mut FlowCtrl,
    ) {
        if let Err(err) = self.dispatch(req, res).await {
            if !res.is_stamped() {
                err.render(res);
            }
        }
    }
}

fn endpoint(route: Route, state: &Arc<AppState>) -> Endpoint {
    Endpoint {
        route,
        state: state.clone(),
    }
}

/// 构造路由。所有路径以 `/v1` 为前缀。
pub fn router(state: AppState) -> Router {
    let state = Arc::new(state);

    let v1 = Router::with_path("v1")
        .push(Router::with_path("models").get(endpoint(Route::Models, &state)))
        .push(
            Router::with_path("chat")
                .push(Router::with_path("completions").post(endpoint(Route::Chat, &state))),
        )
        .push(Router::with_path("completions").post(endpoint(Route::Completions, &state)))
        .push(Router::with_path("embeddings").post(endpoint(Route::Embeddings, &state)))
        .push(
            Router::with_path("audio")
                .push(Router::with_path("speech").post(endpoint(Route::Speech, &state))),
        )
        .push(Router::with_path("decide").post(endpoint(Route::Decide, &state)))
        .push(
            Router::with_path("responses")
                .post(endpoint(Route::ResponsesCreate, &state))
                .push(
                    Router::with_path("{id}")
                        .get(endpoint(Route::ResponsesRetrieve, &state))
                        .delete(endpoint(Route::ResponsesCancel, &state)),
                ),
        );

    Router::new()
        .push(Router::with_path("healthz").get(endpoint(Route::Health, &state)))
        .push(Router::with_path("readyz").get(endpoint(Route::Ready, &state)))
        .push(Router::with_path("metrics").get(endpoint(Route::Metrics, &state)))
        .push(v1)
}

// ------------------------------------------------------------------ 基础端点

fn ready(state: &AppState, res: &mut Response) -> Result<(), GatewayError> {
    let engines = state.registry.list();
    let ready = engines.iter().filter(|e| e.state.is_ready()).count();
    let total = engines.len();

    let payload =
        json!({ "ready": total > 0 && ready > 0, "ready_engines": ready, "total_engines": total });
    res.render(Json(payload));
    Ok(())
}

fn metrics(state: &AppState, res: &mut Response) -> Result<(), GatewayError> {
    let mut text = state.metrics.render();
    for (kind, count) in state.registry.counts_by_kind() {
        text.push_str(&format!(
            "# HELP studio_engine_instances Engine instances by kind\n# TYPE studio_engine_instances gauge\nstudio_engine_instances{{kind=\"{kind}\"}} {count}\n"
        ));
    }

    res.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/plain; version=0.0.4"),
    );
    res.body(text);
    Ok(())
}

fn models(state: &AppState, res: &mut Response) -> Result<(), GatewayError> {
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

    res.render(Json(json!({ "object": "list", "data": data })));
    Ok(())
}

// ------------------------------------------------------------------ 请求体

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

// ------------------------------------------------------------------ 通用逻辑

pub(crate) fn authorize(state: &AppState, req: &Request) -> Result<(), GatewayError> {
    let header = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok());
    if state.auth.check(header) {
        Ok(())
    } else {
        Err(GatewayError::Unauthorized)
    }
}

pub(crate) fn request(model: &str, task: TaskKind, prompt: String) -> InferenceRequest {
    InferenceRequest::new(ModelId::new(model), task, prompt)
}

async fn parse<T: serde::de::DeserializeOwned>(req: &mut Request) -> Result<T, GatewayError> {
    let bytes = req
        .payload()
        .await
        .map_err(|e| GatewayError::BadRequest(format!("cannot read request body: {e}")))?;
    serde_json::from_slice(bytes.as_ref())
        .map_err(|e| GatewayError::BadRequest(format!("invalid request body: {e}")))
}

/// 依次尝试候选引擎；只有在"尚未向客户端输出任何内容"时才允许换引擎。
pub(crate) async fn dispatch(
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

        match call_engine(&engine, &req, &client_tx, permit).await {
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
        if req_clone.task == TaskKind::Tts {
            engine_clone.tts(&req_clone, tx).await
        } else {
            engine_clone.infer(&req_clone, tx).await
        }
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

pub(crate) fn collect_text(events: Vec<StreamEvent>) -> (String, InferenceOutcome) {
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

pub(crate) fn drain(rx: &mut mpsc::UnboundedReceiver<StreamEvent>) -> Vec<StreamEvent> {
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

pub(crate) fn track(state: &AppState, outcome: &InferenceOutcome, first_ms: Option<u64>) {
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

/// 把事件流转成 Salvo 的 SSE 响应。
fn sse_chat(
    res: &mut Response,
    rx: mpsc::UnboundedReceiver<StreamEvent>,
) -> Result<(), GatewayError> {
    res.headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));

    let stream = futures_util::stream::unfold(rx, |mut rx| async move {
        rx.recv().await.map(|event| {
            let payload = match event {
                StreamEvent::Delta(text) => {
                    json!({ "choices": [{ "delta": { "content": text } }] })
                }
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
            let event = SseEvent::default().text(payload.to_string());
            (Ok::<_, Infallible>(event), rx)
        })
    });

    sse::stream(res, stream);
    Ok(())
}

// ------------------------------------------------------------------ 端点实现

async fn chat(state: &AppState, req: &mut Request, res: &mut Response) -> Result<(), GatewayError> {
    authorize(state, req)?;
    let body: ChatRequest = parse(req).await?;

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

    let mut inference = request(&body.model, TaskKind::Chat, prompt)
        .with_stream(body.stream)
        .with_sampling(Sampling {
            temperature: body.temperature,
            top_p: body.top_p,
            max_tokens: body.max_tokens,
            emotion: None,
        });
    inference.system = system;

    run_text(state, inference, body.model, res).await
}

async fn completions(
    state: &AppState,
    req: &mut Request,
    res: &mut Response,
) -> Result<(), GatewayError> {
    authorize(state, req)?;
    let body: CompletionRequest = parse(req).await?;

    let inference = request(&body.model, TaskKind::Completion, body.prompt)
        .with_stream(body.stream)
        .with_sampling(Sampling {
            max_tokens: body.max_tokens,
            ..Sampling::default()
        });

    run_text(state, inference, body.model, res).await
}

async fn embeddings(
    state: &AppState,
    req: &mut Request,
    res: &mut Response,
) -> Result<(), GatewayError> {
    authorize(state, req)?;
    let body: EmbeddingRequest = parse(req).await?;
    let inference = request(&body.model, TaskKind::Embed, body.input);
    run_text(state, inference, body.model, res).await
}

async fn decide(
    state: &AppState,
    req: &mut Request,
    res: &mut Response,
) -> Result<(), GatewayError> {
    authorize(state, req)?;
    let body: DecideRequest = parse(req).await?;

    let prompt = match (&body.context, &body.state) {
        (Some(context), state_value) => {
            json!({ "context": context, "state": state_value }).to_string()
        }
        (None, state_value) => state_value.to_string(),
    };

    let inference = request(&body.model, TaskKind::Decision, prompt);
    let (client_tx, mut rx) = mpsc::unbounded_channel::<StreamEvent>();
    let (engine, outcome, ttft) = dispatch(state, inference, client_tx).await?;
    let events = drain(&mut rx);
    let (text, ev_outcome) = collect_text(events);
    track(state, &ev_outcome, ttft);

    res.render(Json(json!({
        "model": body.model,
        "output": text,
        "engine": engine.label(),
        "usage": outcome.usage,
    })));
    Ok(())
}

async fn speech(
    state: &AppState,
    req: &mut Request,
    res: &mut Response,
) -> Result<(), GatewayError> {
    authorize(state, req)?;
    let body: SpeechRequest = parse(req).await?;

    let mut inference = request(&body.model, TaskKind::Tts, body.input).with_stream(body.stream);
    inference.voice = body.voice;
    inference.sampling.emotion = body.emotion;
    inference.extra = json!({ "speed": body.speed });

    if !body.stream {
        let (client_tx, mut rx) = mpsc::unbounded_channel::<StreamEvent>();
        let (_engine, outcome, ttft) = dispatch(state, inference, client_tx).await?;
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
        track(state, &outcome, ttft);

        res.headers_mut()
            .insert(header::CONTENT_TYPE, HeaderValue::from_static("audio/pcm"));
        if let Some(rate) = rate {
            if let Ok(value) = HeaderValue::from_str(&rate.to_string()) {
                res.headers_mut().insert("x-audio-rate", value);
            }
        }
        res.body(pcm);
        return Ok(());
    }

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

    let stream = futures_util::stream::unfold(rx, |mut rx| async move {
        rx.recv().await.map(|event| match event {
            StreamEvent::Audio(chunk) => (
                Ok::<_, Infallible>(salvo::hyper::body::Bytes::from(chunk.pcm)),
                rx,
            ),
            _ => (Ok::<_, Infallible>(salvo::hyper::body::Bytes::new()), rx),
        })
    });

    let body = salvo::http::ResBody::stream(stream);
    res.headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static("audio/pcm"));
    res.body(body);
    Ok(())
}

/// 文本类请求的统一入口：流式返回 SSE，非流式聚合为 JSON。
async fn run_text(
    state: &AppState,
    inference: InferenceRequest,
    model: String,
    res: &mut Response,
) -> Result<(), GatewayError> {
    let stream = inference.stream;

    if stream {
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

        return sse_chat(res, rx);
    }

    let (client_tx, mut rx) = mpsc::unbounded_channel::<StreamEvent>();
    let (engine, outcome, ttft) = dispatch(state, inference, client_tx).await?;
    let events = drain(&mut rx);
    let (text, ev_outcome) = collect_text(events);
    track(state, &ev_outcome, ttft);

    let mut payload = json!({
        "id": format!("chatcmpl-{}", model.replace('/', "-")),
        "object": "chat.completion",
        "model": model,
        "choices": [{
            "index": 0,
            "message": { "role": "assistant", "content": text },
            "finish_reason": "stop",
        }],
        "engine": engine.label(),
    });

    if let Some(usage) = ev_outcome.usage.or(outcome.usage) {
        payload["usage"] = json!({
            "prompt_tokens": usage.prompt_tokens,
            "completion_tokens": usage.completion_tokens,
            "total_tokens": usage.total(),
        });
    }

    res.render(Json(payload));
    Ok(())
}

/// 启动网关并阻塞直到 `shutdown` 完成（通常是 Ctrl-C）。
pub async fn serve(
    state: AppState,
    listen: &str,
    shutdown: impl std::future::Future<Output = ()> + Send + 'static,
) -> anyhow::Result<()> {
    // salvo 的 bind() 在端口占用时会 panic，这里先做一次友好检查
    if std::net::TcpListener::bind(listen).is_err() {
        anyhow::bail!("gateway port {} is already in use", listen);
    }

    let acceptor = TcpListener::new(listen.to_string()).bind().await;
    let service = Service::new(router(state));
    let task = tokio::spawn(async move { Server::new(acceptor).serve(service).await });

    shutdown.await;
    task.abort();
    Ok(())
}
