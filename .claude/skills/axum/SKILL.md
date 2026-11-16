---
name: axum
description: Axum 0.8 Rust web framework usage guide for Basic-Studio — routing, extractors, shared state, SSE streaming, raw body streaming, and tower::ServiceExt::oneshot based tests. Use when writing or reviewing the studio-gateway crate or any HTTP layer built on Axum.
---

# Axum 使用指南（Basic-Studio 专用）

来源：https://docs.rs/axum （axum 0.8.x）。
本项目网关 `crates/studio-gateway` 使用 Axum，**不要**再引入 salvo / actix / rocket。
（迁移说明：原网关基于 Salvo，因 salvo 1.1.x 的 MSRV 为 1.95、高于生产工具链，已整体迁移到 axum 0.8。）

## 依赖

```toml
[dependencies]
axum = "0.8"
tokio = { version = "1", features = ["macros", "rt-multi-thread", "signal", "net"] }
futures-util = "0.3"
serde = { version = "1", features = ["derive"] }
serde_json = "1"

[dev-dependencies]
tower = { version = "0.5", features = ["util"] }
http-body-util = "0.1"
```

## 最小服务

```rust
use axum::{routing::get, Router};

#[tokio::main]
async fn main() {
    let app = Router::new().route("/healthz", get(healthz));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:18596").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
```

## 路由

```rust
use axum::{routing::{get, post}, Router};

let v1 = Router::new()
    .route("/models", get(models))
    .route("/chat/completions", post(chat))
    .route("/responses", post(responses::create))
    .route("/responses/{id}", get(responses::retrieve).delete(responses::cancel));

let app = Router::new()
    .route("/healthz", get(health))
    .route("/readyz", get(ready))
    .nest("/v1", v1)
    .with_state(state);
```

- 路径参数：`{id}`（0.8 起不再用 `:id`），handler 里用 `Path(id): Path<String>` 提取。
- 通配：`/files/{*rest}`。
- 中间件：`axum::middleware::from_fn_with_state`，或用 `tower_http`。
- 状态共享：`Router::with_state(Arc<AppState>)`，handler 取 `State(state): State<Arc<AppState>>`。
- 方法路由合并：`get(handler).delete(handler2)`。

## Handler

```rust
use axum::{extract::State, http::HeaderMap, Json};
use serde_json::Value;

async fn create(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    raw: Bytes,
) -> Result<Json<Value>, GatewayError> {
    authorize(&state, &headers)?;
    let body: MyReq = serde_json::from_slice(&raw)
        .map_err(|e| GatewayError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "ok": true })))
}
```

- 请求体：`Bytes`（原始字节）或 `Json<T>`；要自定义错误时用 `Bytes` + `serde_json`。
- 提取器顺序：body 提取器（`Bytes` / `Json`）必须放最后。
- 自定义错误：为错误实现 `IntoResponse`，返回 `(StatusCode, Json(body)).into_response()`。

## 响应

```rust
use axum::{response::{IntoResponse, Response}, http::{header, HeaderValue}, body::Body, Json};

async fn audio() -> Response {
    let mut res = Response::new(Body::from(pcm));
    res.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static("audio/pcm"));
    res
}

async fn metrics() -> Response {
    ([(header::CONTENT_TYPE, "text/plain; version=0.0.4")], text).into_response()
}
```

## SSE（流式）

```rust
use axum::response::sse::{Event, KeepAlive, Sse};
use futures_util::Stream;

let stream: S = rx.map(|ev| Ok::<_, Infallible>(Event::default().data(ev.to_json())));
let mut res = Sse::new(stream).keep_alive(KeepAlive::default()).into_response();
```

- 带事件名：`Event::default().event("response.output_text.delta").data(payload)` → `event: ...\ndata: ...\n\n`。
- 本项目用 `sse_response(stream)`（`lib.rs`）统一加 `Cache-Control: no-cache`。
- 原始字节流：`Body::from_stream(stream_of_bytes)` + `audio/pcm`。

## 测试（oneshot）

```rust
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;

async fn send(app: Router, req: Request<Body>) -> (StatusCode, String) {
    let res = app.oneshot(req).await.unwrap();
    let status = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8_lossy(&bytes).to_string())
}
```

- `Router` 是 `Clone` 的，每个用例 `router(state(...))`（或 `.clone()`）即可。
- SSE 断言直接看 body 文本里的 `data: ` / `event: ` 前缀。

## 本项目约定

- 端点统一挂在 `crates/studio-gateway/src/lib.rs` 的 `router()`，路径以 `/v1` 为前缀。
- 错误统一用 `GatewayError` → `(StatusCode, JSON body)`，不要在 handler 里手写 500 文本。
- 流式一律用 `sse_response`；音频流用 `Body::from_stream` + `audio/pcm`。
- 状态只放 `AppState`（`Arc`），不要放全局 `static`。
- 端口占用由 `TcpListener::bind` 报错并包装成友好提示。