use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::json;
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

static CAPS: EngineCaps = EngineCaps {
    streaming: true,
    max_ctx: None,
    tensor_parallel: false,
};

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

    let mut cfg = Config::default();
    cfg.gateway.api_keys = api_keys.into_iter().map(str::to_string).collect();
    cfg.engine_mut(EngineKind::Vllm).base_url = Some("http://mock:8000".to_string());

    let mut registry = EngineRegistry::new();

    let decision = ModelCard::parse(
        r#"
        id = "dm/mock"
        family = "mock"
        task = "decision"
        engines = ["vllm"]
        "#,
    )
    .expect("decision card");

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

async fn post(
    app: axum::Router,
    path: &str,
    body: serde_json::Value,
) -> (StatusCode, serde_json::Value) {
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(path)
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let value = if bytes.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null)
    };
    (status, value)
}

#[tokio::test]
async fn lists_models() {
    let response = router(state(vec![]))
        .oneshot(
            Request::builder()
                .uri("/v1/models")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value["data"][0]["id"], "tts/mock");
}

#[tokio::test]
async fn chat_returns_completion() {
    let (status, body) = post(
        router(state(vec![])),
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
    let response = router(state(vec![]))
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/chat/completions")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "model": "tts/mock",
                        "messages": [{ "role": "user", "content": "hi" }],
                        "stream": true
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    assert!(content_type.contains("text/event-stream"));

    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains("data: "), "sse frames expected: {text}");
    assert!(text.contains("echo:hi"));
}

#[tokio::test]
async fn unknown_model_returns_404() {
    let (status, body) = post(
        router(state(vec![])),
        "/v1/chat/completions",
        json!({ "model": "nope", "messages": [{ "role": "user", "content": "hi" }] }),
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"]["type"], "model_not_found");
}

#[tokio::test]
async fn api_key_is_enforced_when_configured() {
    let app = router(state(vec!["secret"]));

    let unauthorized = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/chat/completions")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({ "model": "tts/mock", "messages": [] }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

    let authorized = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/chat/completions")
                .header("content-type", "application/json")
                .header("authorization", "Bearer secret")
                .body(Body::from(
                    json!({
                        "model": "tts/mock",
                        "messages": [{ "role": "user", "content": "hi" }]
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(authorized.status(), StatusCode::OK);
}

#[tokio::test]
async fn health_and_metrics_are_exposed() {
    let app = router(state(vec![]));

    let health = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/healthz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(health.status(), StatusCode::OK);

    let ready = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/readyz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(ready.status(), StatusCode::OK);

    let metrics = app
        .oneshot(
            Request::builder()
                .uri("/metrics")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(metrics.status(), StatusCode::OK);
    let bytes = metrics.into_body().collect().await.unwrap().to_bytes();
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains("studio_requests_total"));
}

#[tokio::test]
async fn decision_endpoint_returns_json() {
    let (status, body) = post(
        router(state(vec![])),
        "/v1/decide",
        json!({ "model": "dm/mock", "state": { "hp": 10 } }),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert!(body["output"].as_str().unwrap().contains("echo:"));
}

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

#[allow(dead_code)]
fn _assert_engine_config_defaults() {
    let mut cfg = EngineConfig::default_for(EngineKind::Vllm);
    cfg.base_url = Some("http://x".to_string());
    assert!(cfg.base_url.is_some());
}
