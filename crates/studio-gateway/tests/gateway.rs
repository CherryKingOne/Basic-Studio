use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use salvo::prelude::*;
use salvo::test::{ResponseExt, TestClient};
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

fn service(api_keys: Vec<&str>) -> Service {
    Service::new(router(state(api_keys)))
}

async fn post_json(app: &Service, path: &str, body: Value) -> (Option<StatusCode>, Value) {
    let mut response = TestClient::post(format!("http://127.0.0.1{path}"))
        .json(&body)
        .send(app)
        .await;

    let status = response.status_code;
    let body: Value = response.take_json().await.unwrap_or(Value::Null);
    (status, body)
}

// ------------------------------------------------------------------ 基础端点

#[tokio::test]
async fn lists_models() {
    let mut response = TestClient::get("http://127.0.0.1/v1/models")
        .send(&service(vec![]))
        .await;

    assert_eq!(response.status_code, Some(StatusCode::OK));
    let body: Value = response.take_json().await.unwrap();
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
    let app = service(vec![]);

    let health = TestClient::get("http://127.0.0.1/healthz").send(&app).await;
    assert_eq!(health.status_code, Some(StatusCode::OK));

    let ready = TestClient::get("http://127.0.0.1/readyz").send(&app).await;
    assert_eq!(ready.status_code, Some(StatusCode::OK));

    let mut metrics = TestClient::get("http://127.0.0.1/metrics").send(&app).await;
    assert_eq!(metrics.status_code, Some(StatusCode::OK));
    let text = metrics.take_string().await.unwrap();
    assert!(text.contains("studio_requests_total"));
}

// ------------------------------------------------------------------ Chat

#[tokio::test]
async fn chat_returns_completion() {
    let (status, body) = post_json(
        &service(vec![]),
        "/v1/chat/completions",
        json!({ "model": "tts/mock", "messages": [{ "role": "user", "content": "hi" }] }),
    )
    .await;

    assert_eq!(status, Some(StatusCode::OK));
    assert_eq!(body["choices"][0]["message"]["content"], "echo:hi");
    assert_eq!(body["usage"]["total_tokens"], 8);
}

#[tokio::test]
async fn chat_streams_sse() {
    let mut response = TestClient::post("http://127.0.0.1/v1/chat/completions")
        .json(&json!({
            "model": "tts/mock",
            "messages": [{ "role": "user", "content": "hi" }],
            "stream": true
        }))
        .send(&service(vec![]))
        .await;

    assert_eq!(response.status_code, Some(StatusCode::OK));
    let text = response.take_string().await.unwrap();
    assert!(text.contains("data: "), "sse frames expected: {text}");
    assert!(text.contains("echo:hi"));
}

#[tokio::test]
async fn unknown_model_returns_404() {
    let (status, body) = post_json(
        &service(vec![]),
        "/v1/chat/completions",
        json!({ "model": "nope", "messages": [{ "role": "user", "content": "hi" }] }),
    )
    .await;

    assert_eq!(status, Some(StatusCode::NOT_FOUND));
    assert_eq!(body["error"]["type"], "model_not_found");
}

#[tokio::test]
async fn api_key_is_enforced_when_configured() {
    let app = service(vec!["secret"]);

    let unauthorized = TestClient::post("http://127.0.0.1/v1/chat/completions")
        .json(&json!({ "model": "tts/mock", "messages": [] }))
        .send(&app)
        .await;
    assert_eq!(unauthorized.status_code, Some(StatusCode::UNAUTHORIZED));

    let authorized = TestClient::post("http://127.0.0.1/v1/chat/completions")
        .add_header("Authorization", "Bearer secret", true)
        .json(&json!({
            "model": "tts/mock",
            "messages": [{ "role": "user", "content": "hi" }]
        }))
        .send(&app)
        .await;
    assert_eq!(authorized.status_code, Some(StatusCode::OK));
}

#[tokio::test]
async fn decision_endpoint_returns_json() {
    let (status, body) = post_json(
        &service(vec![]),
        "/v1/decide",
        json!({ "model": "dm/mock", "state": { "hp": 10 } }),
    )
    .await;

    assert_eq!(status, Some(StatusCode::OK));
    assert!(body["output"].as_str().unwrap().contains("echo:"));
}

// ------------------------------------------------------------------ Responses

#[tokio::test]
async fn responses_returns_structured_output() {
    let (status, body) = post_json(
        &service(vec![]),
        "/v1/responses",
        json!({ "model": "tts/mock", "input": "hello", "instructions": "be brief" }),
    )
    .await;

    assert_eq!(status, Some(StatusCode::OK));
    assert_eq!(body["object"], "response");
    assert_eq!(body["status"], "completed");
    assert_eq!(body["output"][0]["content"][0]["type"], "output_text");
    assert_eq!(body["output_text"], "echo:hello");
    assert_eq!(body["usage"]["total_tokens"], 8);
}

#[tokio::test]
async fn responses_accepts_message_array() {
    let (status, body) = post_json(
        &service(vec![]),
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

    assert_eq!(status, Some(StatusCode::OK));
    assert!(body["output_text"].as_str().unwrap().contains("ping"));
}

#[tokio::test]
async fn responses_supports_previous_response_id() {
    let app = service(vec![]);

    let (status, first) = post_json(
        &app,
        "/v1/responses",
        json!({ "model": "tts/mock", "input": "first" }),
    )
    .await;
    assert_eq!(status, Some(StatusCode::OK));
    let first_id = first["id"].as_str().unwrap().to_string();
    assert!(first_id.starts_with("resp_"));

    let (status, _second) = post_json(
        &app,
        "/v1/responses",
        json!({ "model": "tts/mock", "input": "second", "previous_response_id": first_id }),
    )
    .await;
    assert_eq!(status, Some(StatusCode::OK));
}

#[tokio::test]
async fn responses_stream_emits_typed_events() {
    let mut response = TestClient::post("http://127.0.0.1/v1/responses")
        .json(&json!({ "model": "tts/mock", "input": "hi", "stream": true }))
        .send(&service(vec![]))
        .await;

    assert_eq!(response.status_code, Some(StatusCode::OK));
    let text = response.take_string().await.unwrap();
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
        &service(vec![]),
        "/v1/responses",
        json!({ "model": "tts/mock", "input": "x", "previous_response_id": "resp_missing" }),
    )
    .await;

    assert_eq!(status, Some(StatusCode::BAD_REQUEST));
    assert_eq!(body["error"]["type"], "bad_request");
}

#[tokio::test]
async fn responses_require_auth_when_configured() {
    let response = TestClient::post("http://127.0.0.1/v1/responses")
        .json(&json!({ "model": "tts/mock", "input": "hi" }))
        .send(&service(vec!["secret"]))
        .await;
    assert_eq!(response.status_code, Some(StatusCode::UNAUTHORIZED));
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
