//! OpenAI 兼容协议的 HTTP 客户端工具。
//!
//! vLLM 与 SGLang 都提供 OpenAI 兼容接口，因此把"拼请求体 / 解析 SSE / 提取文本"收敛到这里，
//! 避免两个适配器各写一份。

use futures_util::StreamExt;
use serde_json::{json, Value};
use studio_core::{Error, InferenceOutcome, InferenceRequest, Result, StreamEvent, Usage};
use tokio::sync::mpsc::UnboundedSender;

/// 构造 `/v1/chat/completions` 请求体。
pub fn chat_body(req: &InferenceRequest) -> Value {
    let mut messages = Vec::new();
    if let Some(system) = &req.system {
        messages.push(json!({ "role": "system", "content": system }));
    }
    messages.push(json!({ "role": "user", "content": req.prompt }));

    let mut body = json!({
        "model": req.model.as_str(),
        "messages": messages,
        "stream": req.stream,
    });

    if req.stream {
        body["stream_options"] = json!({ "include_usage": true });
    }
    if let Some(temperature) = req.sampling.temperature {
        body["temperature"] = json!(temperature);
    }
    if let Some(top_p) = req.sampling.top_p {
        body["top_p"] = json!(top_p);
    }
    if let Some(max_tokens) = req.sampling.max_tokens {
        body["max_tokens"] = json!(max_tokens);
    }

    if let Value::Object(extra) = &req.extra {
        if let Value::Object(map) = &mut body {
            for (key, value) in extra {
                map.insert(key.clone(), value.clone());
            }
        }
    }

    body
}

/// 构造 `/v1/embeddings` 请求体。
pub fn embeddings_body(req: &InferenceRequest) -> Value {
    json!({
        "model": req.model.as_str(),
        "input": req.prompt,
        "encoding_format": "float",
    })
}

/// 解析 SSE 响应：逐块转发文本增量，结束时汇报用量。
///
/// `url` 只用于错误信息。
pub async fn post_chat(
    client: &reqwest::Client,
    url: &str,
    body: &Value,
    tx: &UnboundedSender<StreamEvent>,
) -> Result<InferenceOutcome> {
    let resp = client
        .post(url)
        .json(body)
        .send()
        .await
        .map_err(|e| Error::Process(format!("POST {url} failed: {e}")))?;

    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(Error::Process(format!("POST {url} -> {status}: {text}")));
    }

    let mut outcome = InferenceOutcome::default();
    let content_type = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    if !content_type.contains("event-stream") {
        let value: Value = resp
            .json()
            .await
            .map_err(|e| Error::Process(format!("invalid response from {url}: {e}")))?;
        if let Some(text) = extract_message(&value) {
            let event = StreamEvent::Delta(text);
            outcome.record(&event);
            let _ = tx.send(event);
        }
        if let Some(usage) = extract_usage(&value) {
            let event = StreamEvent::Done(usage);
            outcome.record(&event);
            let _ = tx.send(event);
        } else {
            let event = StreamEvent::Done(Usage::default());
            outcome.record(&event);
            let _ = tx.send(event);
        }
        return Ok(outcome);
    }

    let mut stream = resp.bytes_stream();
    let mut buffer = String::new();
    let mut finished = false;

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| Error::Process(format!("stream from {url} failed: {e}")))?;
        buffer.push_str(&String::from_utf8_lossy(&chunk));

        while let Some(pos) = buffer.find('\n') {
            let line = buffer[..pos].trim().to_string();
            buffer.drain(..=pos);

            if line.is_empty() || line.starts_with(':') {
                continue;
            }
            let payload = line.strip_prefix("data:").map(str::trim).unwrap_or(&line);
            if payload == "[DONE]" {
                finished = true;
                continue;
            }

            let value: Value = match serde_json::from_str(payload) {
                Ok(value) => value,
                Err(_) => continue,
            };

            if let Some(text) = extract_delta(&value) {
                let event = StreamEvent::Delta(text);
                outcome.record(&event);
                if tx.send(event).is_err() {
                    return Ok(outcome);
                }
            }
            if let Some(usage) = extract_usage(&value) {
                let event = StreamEvent::Done(usage);
                outcome.record(&event);
                let _ = tx.send(event);
                finished = true;
            }
        }
    }

    if !finished {
        let event = StreamEvent::Done(outcome.usage.unwrap_or_default());
        outcome.record(&event);
        let _ = tx.send(event);
    }

    Ok(outcome)
}

/// 取流式增量文本。
pub fn extract_delta(value: &Value) -> Option<String> {
    let choice = value.get("choices")?.as_array()?.first()?;
    let delta = choice.get("delta")?;
    if let Some(text) = delta.get("content").and_then(Value::as_str) {
        return Some(text.to_string());
    }
    // 兼容非标准字段
    delta
        .get("reasoning_content")
        .and_then(Value::as_str)
        .map(str::to_string)
}

/// 取非流式完整文本。
pub fn extract_message(value: &Value) -> Option<String> {
    let choice = value.get("choices")?.as_array()?.first()?;
    let message = choice.get("message")?;
    message
        .get("content")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            choice
                .get("text")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
}

/// 取 usage。
pub fn extract_usage(value: &Value) -> Option<Usage> {
    let usage = value.get("usage")?;
    Some(Usage {
        prompt_tokens: usage
            .get("prompt_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        completion_tokens: usage
            .get("completion_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0),
    })
}

/// 语音合成直通：返回音频字节与采样率（若响应头提供 `x-audio-rate`）。
pub async fn post_speech(
    client: &reqwest::Client,
    url: &str,
    body: &Value,
) -> Result<(Vec<u8>, Option<u32>)> {
    let resp = client
        .post(url)
        .json(body)
        .send()
        .await
        .map_err(|e| Error::Process(format!("POST {url} failed: {e}")))?;

    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(Error::Process(format!("POST {url} -> {status}: {text}")));
    }

    let rate = resp
        .headers()
        .get("x-audio-rate")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u32>().ok());

    let bytes = resp
        .bytes()
        .await
        .map_err(|e| Error::Process(format!("read audio from {url} failed: {e}")))?;
    Ok((bytes.to_vec(), rate))
}

/// 简单的 JSON POST，用于 sidecar 的非流式接口。
pub async fn post_json(client: &reqwest::Client, url: &str, body: &Value) -> Result<Value> {
    let resp = client
        .post(url)
        .json(body)
        .send()
        .await
        .map_err(|e| Error::Process(format!("POST {url} failed: {e}")))?;

    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(Error::Process(format!("POST {url} -> {status}: {text}")));
    }

    resp.json()
        .await
        .map_err(|e| Error::Process(format!("invalid json from {url}: {e}")))
}

/// 从 JSON 事件（sidecar 协议）转成 [`StreamEvent`]。
///
/// 支持 `delta` / `audio` / `done` / `error` 四类事件。
pub fn sidecar_event(value: &Value) -> Option<StreamEvent> {
    match value.get("type").and_then(Value::as_str)? {
        "delta" => Some(StreamEvent::Delta(
            value.get("text").and_then(Value::as_str)?.to_string(),
        )),
        "audio" => {
            use base64_decode as b64;
            let encoded = value.get("pcm_b64").and_then(Value::as_str)?;
            let pcm = b64::decode(encoded).ok()?;
            let sample_rate = value
                .get("sample_rate")
                .and_then(Value::as_u64)
                .unwrap_or(24000) as u32;
            Some(StreamEvent::Audio(studio_core::AudioChunk::new(
                pcm,
                sample_rate,
            )))
        }
        "done" => Some(StreamEvent::Done(Usage {
            prompt_tokens: value
                .get("prompt_tokens")
                .and_then(Value::as_u64)
                .unwrap_or(0),
            completion_tokens: value
                .get("completion_tokens")
                .and_then(Value::as_u64)
                .unwrap_or(0),
        })),
        "error" => Some(StreamEvent::Error(
            value
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("unknown sidecar error")
                .to_string(),
        )),
        _ => None,
    }
}

/// 极简 base64 解码（避免为少量调用引入额外依赖链）。
mod base64_decode {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    pub fn decode(input: &str) -> Result<Vec<u8>, ()> {
        let mut out = Vec::with_capacity(input.len() / 4 * 3);
        let mut buffer: u32 = 0;
        let mut bits = 0u32;

        for byte in input.bytes() {
            if byte == b'=' || byte == b'\n' || byte == b'\r' {
                continue;
            }
            let value = ALPHABET
                .iter()
                .position(|candidate| *candidate == byte)
                .ok_or(())? as u32;
            buffer = (buffer << 6) | value;
            bits += 6;
            if bits >= 8 {
                bits -= 8;
                out.push((buffer >> bits) as u8);
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_chat_body_with_stream_usage() {
        let req = InferenceRequest::new(
            studio_core::ModelId::new("m"),
            studio_core::TaskKind::Chat,
            "hi",
        )
        .with_system("be nice")
        .with_stream(true)
        .with_sampling(studio_core::request::Sampling {
            temperature: Some(0.7),
            top_p: None,
            max_tokens: Some(64),
            emotion: None,
        });

        let body = chat_body(&req);
        assert_eq!(body["model"], "m");
        assert_eq!(body["messages"][0]["role"], "system");
        assert_eq!(body["messages"][1]["content"], "hi");
        assert_eq!(body["stream"], true);
        assert!((body["temperature"].as_f64().unwrap() - 0.7).abs() < 1e-6);
        assert_eq!(body["max_tokens"], 64);
        assert_eq!(body["stream_options"]["include_usage"], true);
    }

    #[test]
    fn extracts_delta_and_usage() {
        let delta = json!({"choices":[{"delta":{"content":"你"}}]});
        assert_eq!(extract_delta(&delta).as_deref(), Some("你"));

        let usage = json!({"usage":{"prompt_tokens":3,"completion_tokens":5}});
        assert_eq!(
            extract_usage(&usage),
            Some(Usage {
                prompt_tokens: 3,
                completion_tokens: 5
            })
        );
    }

    #[test]
    fn decodes_sidecar_audio_event() {
        // "hello" -> aGVsbG8=
        let value = json!({"type":"audio","pcm_b64":"aGVsbG8=","sample_rate":16000});
        match sidecar_event(&value) {
            Some(StreamEvent::Audio(chunk)) => {
                assert_eq!(chunk.pcm, b"hello");
                assert_eq!(chunk.sample_rate, 16000);
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[test]
    fn base64_roundtrip_known_vector() {
        let decoded = base64_decode::decode("aGVsbG8=").expect("decode");
        assert_eq!(decoded, b"hello");
    }
}
