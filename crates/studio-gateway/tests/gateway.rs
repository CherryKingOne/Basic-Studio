use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use studio_auth::ApiKeys;
use studio_config::{Config, EngineConfig};
use studio_core::{
    EngineCaps, EngineKind, InferenceOutcome, InferenceRequest, ModelCard, Result, StreamEvent,
    TaskKind, Usage,
};
use studio_engine::{Engine, EngineRegistry, EngineState, Health};
use studio_gateway::{router, AppState};
use studio_registry::ModelIndex;
use studio_scheduler::Scheduler;
use tokio::sync::mpsc::UnboundedSender;
use tower::ServiceExt;

struct MockEngine {
    kind: EngineKind,
    label: String,
    card: ModelCard,
}

static CAPS: EngineCaps = EngineCaps {
    streaming: true,
    max_ctx: None,
    tensor_parallel: false,
};

#[async_trait]
impl Engine for MockEngine {
    fn kind(&self) -> EngineKind {
        self.kind
    }
    fn label(&self) -> &str {
        &self.label
    }
    fn state(&self) -> EngineState {
        EngineState::Ready
    }
    fn caps(&self) -> &EngineCaps {
        &CAPS
    }
    fn model_path(&self) -> Option<&std::path::Path> {
        None
    }
    fn card(&self) -> Option<&ModelCard> {
        Some(&self.card)
    }
    async fn start(&self) -> Result<()> {
        Ok(())
    }
    async fn health(&self) -> Health {
        Health::ready()
    }
    async fn shutdown(&self, _grace: Duration) -> Result<()> {
        Ok(())
    }
    async fn infer(
        &self,
        req: &InferenceRequest,
        tx: UnboundedSender<StreamEvent>,
    ) -> Result<InferenceOutcome> {
        let mut outcome = InferenceOutcome::default();

        let delta = StreamEvent::Delta(format!("echo:{}", req.prompt));
        outcome.record(&delta);
        let _ = tx.send(delta);

        let done = StreamEvent::Done(Usage {
            prompt_tokens: 3,
            completion_tokens: 5,
        });
        outcome.record(&done);
        let _ = tx.send(done);
        Ok(outcome)
    }
}

fn state(api_keys: Vec<&str>) -> AppState {
    let card = ModelCard::parse(
        r#"
        id = "tts/mock"
        family = "mock"
        task = "chat"
        engines = ["vllm"]
        "#,
    )
    .expect("card");

    let decision = ModelCard::parse(
        r#"
        id = "dm/mock"
        family = "mock"
        task = "decision"
        engines = ["vllm"]
        "#,
    )
    .expect("decision card");

    let mut cfg = Config::default();
    cfg.gateway.api_keys = api_keys.into_iter().map(str::to_string).collect();
    cfg.engine_mut(EngineKind::Vllm).base_url = Some("http://mock:8000".to_string());

    let mut registry = EngineRegistry::new();
    registry.register(Arc::new(MockEngine {
        kind: EngineKind::Vllm,
        label: "vllm/tts-mock".to_string(),
        card: card.clone(),
    }));
    registry.register(Arc::new(MockEngine {
        kind: EngineKind::Vllm,
        label: "vllm/dm-mock".to_string(),
        card: decision.clone(),
    }));

    let index = ModelIndex::from_cards(vec![card, decision]);
    let scheduler = Scheduler::new(&cfg.scheduler);
    let auth = ApiKeys::new(cfg.gateway.api_keys.clone());
    AppState::new(cfg, index, registry, scheduler, auth)
}

fn app(api_keys: Vec<&str>) -> Router {
    router(state(api_keys))
}

async fn send(app: Router, req: Request<Body>) -> (StatusCode, String) {
    let response = app.oneshot(req).await.expect("router should respond");
    let status = response.status();
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("body should be readable")
        .to_bytes();
    (status, String::from_utf8_lossy(&bytes).to_string())
}

fn post_body(path: &str, body: Value) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(path)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            serde_json::to_vec(&body).expect("serialize body"),
        ))
        .expect("build request")
}

fn post_json_with_key(path: &str, body: Value, key: Option<&str>) -> Request<Body> {
    let mut builder = Request::builder()
        .method("POST")
        .uri(path)
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(key) = key {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {key}"));
    }
    builder
        .body(Body::from(
            serde_json::to_vec(&body).expect("serialize body"),
        ))
        .expect("build request")
}

async fn post_json(app: Router, path: &str, body: Value) -> (StatusCode, Value) {
    let (status, text) = send(app, post_body(path, body)).await;
    let body = serde_json::from_str(&text).unwrap_or(Value::Null);
    (status, body)
}

async fn get_text(app: Router, path: &str) -> (StatusCode, String) {
    let req = Request::builder()
        .method("GET")
        .uri(path)
        .body(Body::empty())
        .expect("build request");
    send(app, req).await
}

// ------------------------------------------------------------------ 基础端点

#[tokio::test]
async fn lists_models() {
    let (status, text) = get_text(app(vec![]), "/v1/models").await;

    assert_eq!(status, StatusCode::OK);
    let body: Value = serde_json::from_str(&text).unwrap();
    let ids: Vec<&str> = body["data"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|m| m["id"].as_str())
        .collect();
    assert!(ids.contains(&"tts/mock"));
}

#[tokio::test]
async fn health_ready_and_metrics() {
    let (status, _) = get_text(app(vec![]), "/healthz").await;
    assert_eq!(status, StatusCode::OK);

    let (status, _) = get_text(app(vec![]), "/readyz").await;
    assert_eq!(status, StatusCode::OK);

    let (status, text) = get_text(app(vec![]), "/metrics").await;
    assert_eq!(status, StatusCode::OK);
    assert!(text.contains("studio_requests_total"));
}

// ------------------------------------------------------------------ Chat

#[tokio::test]
async fn chat_returns_completion() {
    let (status, body) = post_json(
        app(vec![]),
        "/v1/chat/completions",
        json!({ "model": "tts/mock", "messages": [{ "role": "user", "content": "hi" }] }),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["choices"][0]["message"]["content"], "echo:hi");
    assert_eq!(body["usage"]["total_tokens"], 8);
}

#[tokio::test]
async fn chat_streams_sse() {
    let (status, text) = send(
        app(vec![]),
        post_body(
            "/v1/chat/completions",
            json!({
                "model": "tts/mock",
                "messages": [{ "role": "user", "content": "hi" }],
                "stream": true
            }),
        ),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert!(text.contains("data: "), "sse frames expected: {text}");
    assert!(text.contains("echo:hi"));
}

#[tokio::test]
async fn unknown_model_returns_404() {
    let (status, body) = post_json(
        app(vec![]),
        "/v1/chat/completions",
        json!({ "model": "nope", "messages": [{ "role": "user", "content": "hi" }] }),
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"]["type"], "model_not_found");
}

#[tokio::test]
async fn api_key_is_enforced_when_configured() {
    let unauthorized = send(
        app(vec!["secret"]),
        post_json_with_key(
            "/v1/chat/completions",
            json!({ "model": "tts/mock", "messages": [] }),
            None,
        ),
    )
    .await;
    assert_eq!(unauthorized.0, StatusCode::UNAUTHORIZED);

    let authorized = send(
        app(vec!["secret"]),
        post_json_with_key(
            "/v1/chat/completions",
            json!({
                "model": "tts/mock",
                "messages": [{ "role": "user", "content": "hi" }]
            }),
            Some("secret"),
        ),
    )
    .await;
    assert_eq!(authorized.0, StatusCode::OK);
}

#[tokio::test]
async fn decision_endpoint_returns_json() {
    let (status, body) = post_json(
        app(vec![]),
        "/v1/decide",
        json!({ "model": "dm/mock", "state": { "hp": 10 } }),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert!(body["output"].as_str().unwrap().contains("echo:"));
}

// ------------------------------------------------------------------ Responses

#[tokio::test]
async fn responses_returns_structured_output() {
    let (status, body) = post_json(
        app(vec![]),
        "/v1/responses",
        json!({ "model": "tts/mock", "input": "hello", "instructions": "be brief" }),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["object"], "response");
    assert_eq!(body["status"], "completed");
    assert_eq!(body["output"][0]["content"][0]["type"], "output_text");
    assert_eq!(body["output_text"], "echo:hello");
    assert_eq!(body["usage"]["total_tokens"], 8);
}

#[tokio::test]
async fn responses_accepts_message_array() {
    let (status, body) = post_json(
        app(vec![]),
        "/v1/responses",
        json!({
            "model": "tts/mock",
            "input": [
                { "role": "system", "content": "be brief" },
                { "role": "user", "content": "ping" }
            ]
        }),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert!(body["output_text"].as_str().unwrap().contains("ping"));
}

#[tokio::test]
async fn responses_supports_previous_response_id() {
    let shared = app(vec![]);

    let (status, first) = post_json(
        shared.clone(),
        "/v1/responses",
        json!({ "model": "tts/mock", "input": "first" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let first_id = first["id"].as_str().unwrap().to_string();
    assert!(first_id.starts_with("resp_"));

    let (status, _second) = post_json(
        shared,
        "/v1/responses",
        json!({ "model": "tts/mock", "input": "second", "previous_response_id": first_id }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn responses_stream_emits_typed_events() {
    let (status, text) = send(
        app(vec![]),
        post_body(
            "/v1/responses",
            json!({ "model": "tts/mock", "input": "hi", "stream": true }),
        ),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert!(
        text.contains("event: response.created"),
        "missing created: {text}"
    );
    assert!(text.contains("event: response.output_text.delta"));
    assert!(text.contains("event: response.completed"));
}

#[tokio::test]
async fn responses_unknown_previous_id_is_rejected() {
    let (status, body) = post_json(
        app(vec![]),
        "/v1/responses",
        json!({ "model": "tts/mock", "input": "x", "previous_response_id": "resp_missing" }),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["type"], "bad_request");
}

#[tokio::test]
async fn responses_require_auth_when_configured() {
    let (status, _) = send(
        app(vec!["secret"]),
        post_json_with_key(
            "/v1/responses",
            json!({ "model": "tts/mock", "input": "hi" }),
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

// ------------------------------------------------------------------ 杂项

#[test]
fn task_kind_maps_to_engine_candidates() {
    let card = ModelCard::parse(
        r#"
        id = "tts/mock"
        family = "mock"
        task = "chat"
        engines = ["vllm"]
        "#,
    )
    .unwrap();
    assert_eq!(card.task, TaskKind::Chat);
    assert_eq!(card.preferred_engine(), Some(EngineKind::Vllm));
}

#[test]
fn engine_config_can_pin_backend() {
    let mut cfg = EngineConfig::default_for(EngineKind::Vllm);
    cfg.base_url = Some("http://x".to_string());
    assert!(cfg.base_url.is_some());
}
