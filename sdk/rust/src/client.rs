use std::time::Duration;

use futures_util::Stream;
use futures_util::StreamExt;
use reqwest::{Method, StatusCode};
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::error::{Error, Result};
use crate::types::{
    ChatRequest, ChatResponse, DecideRequest, DecideResponse, EmbeddingRequest, ModelInfo,
    ModelList, ResponsesRequest, ResponsesResponse, SpeechRequest,
};

/// Basic-Studio 网关客户端。
#[derive(Clone)]
pub struct Client {
    base_url: String,
    api_key: Option<String>,
    http: reqwest::Client,
}

impl Default for Client {
    fn default() -> Self {
        Self::new(crate::DEFAULT_BASE_URL)
    }
}

impl Client {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            api_key: None,
            http: reqwest::Client::new(),
        }
    }

    /// 设置 Bearer Token（网关启用 `[gateway].api_keys` 时必填）。
    pub fn with_api_key(mut self, api_key: impl Into<String>) -> Self {
        self.api_key = Some(api_key.into());
        self
    }

    /// 设置整体超时（默认 60s）。
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.http = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .unwrap_or_default();
        self
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    // ------------------------------------------------------------- 基础设施

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    async fn send(
        &self,
        method: Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<reqwest::Response> {
        let mut request = self
            .http
            .request(method, self.url(path))
            .header(reqwest::header::ACCEPT, "application/json");

        if let Some(api_key) = &self.api_key {
            request = request.bearer_auth(api_key);
        }
        if let Some(body) = body {
            request = request.json(&body);
        }

        let response = request.send().await?;
        if !response.status().is_success() {
            return Err(api_error(response).await);
        }
        Ok(response)
    }

    async fn json<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<T> {
        let response = self.send(method, path, body).await?;
        response
            .json::<T>()
            .await
            .map_err(|e| Error::Decode(e.to_string()))
    }

    // ------------------------------------------------------------- 基础接口

    pub async fn health(&self) -> Result<Value> {
        self.json(Method::GET, "/healthz", None).await
    }

    pub async fn ready(&self) -> Result<Value> {
        self.json(Method::GET, "/readyz", None).await
    }

    pub async fn models(&self) -> Result<Vec<ModelInfo>> {
        let list: ModelList = self.json(Method::GET, "/v1/models", None).await?;
        Ok(list.data)
    }

    // ------------------------------------------------------------- 对话

    pub async fn chat(&self, request: ChatRequest) -> Result<ChatResponse> {
        self.json(
            Method::POST,
            "/v1/chat/completions",
            Some(request.to_body()),
        )
        .await
    }

    /// 流式对话：返回文本增量流。
    pub async fn chat_stream(
        &self,
        mut request: ChatRequest,
    ) -> Result<impl Stream<Item = Result<String>>> {
        request.stream = true;

        let response = self
            .send(
                Method::POST,
                "/v1/chat/completions",
                Some(request.to_body()),
            )
            .await?;

        let stream = response.bytes_stream();
        Ok(futures_util::stream::unfold(
            (stream, String::new()),
            |(mut stream, mut buffer)| async move {
                loop {
                    if let Some(pos) = buffer.find('\n') {
                        let line = buffer[..pos].trim().to_string();
                        buffer.drain(..=pos);

                        if line.contains("[DONE]") {
                            return None;
                        }
                        if let Some(delta) = parse_chat_delta(&line) {
                            return Some((Ok(delta), (stream, buffer)));
                        }
                        continue;
                    }

                    match stream.next().await {
                        Some(Ok(chunk)) => buffer.push_str(&String::from_utf8_lossy(&chunk)),
                        Some(Err(err)) => {
                            return Some((Err(Error::Http(err)), (stream, buffer)));
                        }
                        None => return None,
                    }
                }
            },
        ))
    }

    // ------------------------------------------------------------- Responses

    pub async fn responses(&self, request: ResponsesRequest) -> Result<ResponsesResponse> {
        self.json(Method::POST, "/v1/responses", Some(request.to_body()))
            .await
    }

    /// Responses 流式接口：返回原始 JSON 事件流（每项为 `{"type": ..., ...}`）。
    pub async fn responses_stream(
        &self,
        mut request: ResponsesRequest,
    ) -> Result<impl Stream<Item = Result<Value>>> {
        request.stream = true;

        let response = self
            .send(Method::POST, "/v1/responses", Some(request.to_body()))
            .await?;

        let stream = response.bytes_stream();
        Ok(futures_util::stream::unfold(
            (stream, String::new()),
            |(mut stream, mut buffer)| async move {
                loop {
                    if let Some(pos) = buffer.find('\n') {
                        let line = buffer[..pos].trim().to_string();
                        buffer.drain(..=pos);
                        if let Some(value) = parse_sse_json(&line) {
                            return Some((Ok(value), (stream, buffer)));
                        }
                        continue;
                    }

                    match stream.next().await {
                        Some(Ok(chunk)) => buffer.push_str(&String::from_utf8_lossy(&chunk)),
                        Some(Err(err)) => return Some((Err(Error::Http(err)), (stream, buffer))),
                        None => return None,
                    }
                }
            },
        ))
    }

    // ------------------------------------------------------------- 其它任务

    pub async fn embeddings(&self, request: EmbeddingRequest) -> Result<Value> {
        self.json(
            Method::POST,
            "/v1/embeddings",
            Some(serde_json::json!({ "model": request.model, "input": request.input })),
        )
        .await
    }

    /// 语音合成，返回完整 PCM（16bit little-endian）。
    pub async fn tts(&self, request: SpeechRequest) -> Result<Vec<u8>> {
        let response = self
            .send(Method::POST, "/v1/audio/speech", Some(request.to_body()))
            .await?;

        let bytes = response.bytes().await?;
        Ok(bytes.to_vec())
    }

    pub async fn decide(&self, request: DecideRequest) -> Result<DecideResponse> {
        self.json(Method::POST, "/v1/decide", Some(request.to_body()))
            .await
    }
}

async fn api_error(response: reqwest::Response) -> Error {
    let status = response.status();
    let message = response
        .text()
        .await
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|value| {
            value
                .get("error")
                .and_then(|error| error.get("message"))
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_else(|| format!("http status {status}"));

    Error::Api {
        status: status.as_u16(),
        message,
    }
}

/// 从一行 SSE 中提取 chat 增量文本。
pub fn parse_chat_delta(line: &str) -> Option<String> {
    let value = parse_sse_json(line)?;
    value
        .pointer("/choices/0/delta/content")
        .and_then(Value::as_str)
        .map(str::to_string)
}

/// 解析 `data: {...}` 行；非数据行返回 `None`。
pub fn parse_sse_json(line: &str) -> Option<Value> {
    let payload = line.strip_prefix("data:")?.trim();
    if payload.is_empty() || payload == "[DONE]" {
        return None;
    }
    serde_json::from_str(payload).ok()
}

#[allow(dead_code)]
fn status_is_retryable(status: StatusCode) -> bool {
    status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_delta_lines() {
        assert_eq!(
            parse_chat_delta(r#"data: {"choices":[{"delta":{"content":"你"}}]}"#).as_deref(),
            Some("你")
        );
        assert_eq!(parse_chat_delta("event: response.created"), None);
        assert_eq!(parse_chat_delta("data: [DONE]"), None);
    }

    #[test]
    fn ignores_usage_only_frames() {
        let line = r#"data: {"choices":[],"usage":{"prompt_tokens":1,"completion_tokens":2}}"#;
        assert_eq!(parse_chat_delta(line), None);
    }

    #[test]
    fn builds_chat_body() {
        let body = ChatRequest::new("m", "hi")
            .with_system("be brief")
            .with_temperature(0.5)
            .with_stream(true)
            .to_body();
        assert_eq!(body["model"], "m");
        assert_eq!(body["messages"][0]["role"], "system");
        assert_eq!(body["stream"], true);
        assert_eq!(body["temperature"], 0.5);
    }

    #[test]
    fn builds_responses_body() {
        let body = ResponsesRequest::new("m", "hello")
            .with_instructions("be brief")
            .with_previous("resp_1")
            .to_body();
        assert_eq!(body["input"], "hello");
        assert_eq!(body["instructions"], "be brief");
        assert_eq!(body["previous_response_id"], "resp_1");
    }

    #[test]
    fn retryable_statuses() {
        assert!(status_is_retryable(StatusCode::TOO_MANY_REQUESTS));
        assert!(status_is_retryable(StatusCode::BAD_GATEWAY));
        assert!(!status_is_retryable(StatusCode::BAD_REQUEST));
    }

    #[test]
    fn chat_request_defaults_to_non_stream() {
        let request = ChatRequest::new("m", "hi");
        assert!(!request.stream);
        assert_eq!(request.to_body()["stream"], json!(false));
    }
}
