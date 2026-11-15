---
name: salvo
description: Salvo (salvo.rs) Rust web framework usage guide for Basic-Studio — routing, handlers, middleware, SSE streaming, JSON extraction, and TestClient-based tests. Use when writing or reviewing the studio-gateway crate or any HTTP layer built on Salvo.
---

# Salvo 使用指南（Basic-Studio 专用）

来源：官方文档 https://salvo.rs/guide 、https://docs.rs/salvo （salvo 1.1.x）。
本项目网关 `crates/studio-gateway` 使用 Salvo，**不要**再引入 axum。

## 依赖

```toml
[dependencies]
salvo = { version = "1", features = ["sse"] }   # sse 需要显式开启
tokio = { version = "1", features = ["macros", "rt-multi-thread", "signal", "net"] }
futures-util = "0.3"
```

## 最小服务

```rust
use salvo::prelude::*;

#[handler]
async fn hello() -> &'static str { "Hello" }

#[tokio::main]
async fn main() {
    let acceptor = TcpListener::new("127.0.0.1:18596").bind().await.unwrap();
    Server::new(acceptor).serve(Router::new().get(hello)).await;
}
```

## 路由

```rust
let router = Router::new()
    .push(Router::with_path("healthz").get(healthz))
    .push(
        Router::with_path("v1")
            .push(Router::with_path("models").get(list_models))
            .push(Router::with_path("chat").push(Router::with_path("completions").post(chat)))
            .push(Router::with_path("responses").post(responses_create)
                .push(Router::with_path("{id}").get(responses_get).delete(responses_cancel))),
    );
```

- 路径参数：`Router::with_path("{id}")`，handler 内 `req.param::<String>("id")`。
- 通配：`{**rest}`。
- 中间件：`Router::hoop(mw)`，中间件本身也是 Handler（`ctrl.call_next(res).await` 放行）。
- 状态共享：把 `Arc<AppState>` 放进 **handler 结构体**（本项目做法），或 `affix-state` 特性 + `depot.obtain::<T>()`。

## Handler

```rust
#[handler]
async fn create(req: &mut Request, res: &mut Response) {
    let body: MyReq = match req.parse_json().await {
        Ok(v) => v,
        Err(e) => { res.status_code(StatusCode::BAD_REQUEST); res.render(Json(json!({"error": e.to_string()}))); return; }
    };
    res.render(Json(json!({"ok": true})));
}
```

带状态的写法（本项目 `Endpoint`）：

```rust
#[derive(Clone)]
struct Endpoint { route: Route, state: Arc<AppState> }

#[async_trait]
impl Handler for Endpoint {
    async fn handle(&self, req: &mut Request, _depot: &mut Depot, res: &mut Response, _ctrl: &mut FlowCtrl) {
        self.dispatch(req, res).await;
    }
}
```

## 响应

- JSON：`res.render(Json(value))`
- 文本：`res.render("text")`
- 状态码：`res.status_code(StatusCode::OK)`
- 自定义头：`res.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static("audio/pcm"))`
- 二进制体：`res.body(Vec::<u8>)`（`ResBody` 由 `Into` 转换）

## SSE（流式）

```rust
use salvo::sse::{self, SseEvent};
use futures_util::StreamExt;

let stream = rx.map(|event| Ok::<_, Infallible>(match event {
    Ev::Delta(t) => SseEvent::default().data(t),
    Ev::Done => SseEvent::default().event("done").data("{}"),
}));
sse::stream(res, stream);
```

- 事件名：`.event("response.output_text.delta")` → 线上为 `event: ...\ndata: ...\n\n`。
- 保持连接：`SseKeepAlive::new(stream).stream(res)`。
- 客户端断开时流自然结束（rx 关闭）。

## 测试（TestClient）

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use salvo::test::{ResponseExt, TestClient};

    #[tokio::test]
    async fn ok() {
        let service = Service::new(super::router(state()));
        let res = TestClient::post("http://127.0.0.1/v1/responses")
            .json(&json!({"model": "m", "input": "hi"}))
            .send(&service).await;
        assert_eq!(res.status_code, Some(StatusCode::OK));
        let body: serde_json::Value = res.take_json().await.unwrap();
        assert_eq!(body["status"], "completed");
    }
}
```

## 本项目约定

- 端点统一挂在 `crates/studio-gateway/src/lib.rs` 的 `router()`，路径以 `/v1` 为前缀。
- 错误统一用 `GatewayError` → `(StatusCode, JSON body)`，不要在 handler 里手写 500 文本。
- 流式一律用 `salvo::sse::stream`；音频流用 `res.body(...)` + `audio/pcm`。
- 状态只放 `AppState`（`Arc`），不要放全局 `static`。
