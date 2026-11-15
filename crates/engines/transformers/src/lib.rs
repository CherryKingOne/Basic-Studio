//! Transformers 适配器（sidecar 通道）。
//!
//! Transformers 没有官方 HTTP 服务端，由 `sidecars/` 下的 Python sidecar 提供统一接口：
//! - `POST /infer`：文本 / 向量 / 决策，支持 NDJSON 流式（每行一个事件）
//! - `POST /infer/stream`：音频流，同样返回事件
//!
//! IndexTTS 等模型也走这条通道，具体版本适配器由 `model.toml` 的 `adapter` 决定。
//! 注意：IndexTTS 是**模型**，不是引擎。

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use async_trait::async_trait;
use futures_util::StreamExt;
use studio_config::EngineConfig;
use studio_core::{
    EngineCaps, EngineKind, Error, InferenceOutcome, InferenceRequest, ModelCard, Result,
    StreamEvent, TaskKind,
};
use studio_engine::http;
use studio_engine::{Engine, EngineState, Health};
use studio_procsup::{open_log, probe_http, SupervisedChild};
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::Mutex as AsyncMutex;

pub struct TransformersEngine {
    cfg: EngineConfig,
    label: String,
    host: String,
    log_dir: Option<PathBuf>,
    card: Option<ModelCard>,
    state: Mutex<EngineState>,
    child: AsyncMutex<Option<SupervisedChild>>,
    caps: EngineCaps,
    health_url: String,
    client: reqwest::Client,
}

impl TransformersEngine {
    pub fn new(
        cfg: EngineConfig,
        card: Option<ModelCard>,
        host: impl Into<String>,
        log_dir: Option<PathBuf>,
    ) -> Self {
        let host = host.into();
        let label = card
            .as_ref()
            .map(|c| format!("transformers/{}", c.id))
            .unwrap_or_else(|| "transformers/unknown".to_string());
        let base = cfg.base_url(&host);
        let health_url = format!("{}/{}", base, cfg.health_path.trim_start_matches('/'));

        Self {
            cfg,
            label,
            host,
            log_dir,
            card,
            state: Mutex::new(EngineState::Unready),
            child: AsyncMutex::new(None),
            caps: EngineCaps {
                streaming: true,
                max_ctx: None,
                tensor_parallel: false,
            },
            health_url,
            client: reqwest::Client::new(),
        }
    }

    fn vars(&self) -> Vec<(&'static str, String)> {
        vec![
            (
                "model_path",
                self.card
                    .as_ref()
                    .and_then(|c| c.path.as_ref())
                    .map(|p| p.display().to_string())
                    .unwrap_or_default(),
            ),
            ("port", self.cfg.port.to_string()),
            ("host", self.host.clone()),
            (
                "venv",
                self.cfg
                    .venv
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default(),
            ),
            (
                "adapter",
                self.card
                    .as_ref()
                    .and_then(|c| c.adapter.clone())
                    .unwrap_or_default(),
            ),
        ]
    }

    fn endpoint(&self, path: &str) -> String {
        format!(
            "{}/{}",
            self.cfg.base_url(&self.host),
            path.trim_start_matches('/')
        )
    }

    async fn ensure_ready(&self) -> Result<()> {
        if self.state().is_ready() {
            return Ok(());
        }
        self.start().await
    }

    /// 组装 sidecar 请求体。
    fn body(&self, req: &InferenceRequest, task: &str) -> serde_json::Value {
        let card = self.card.as_ref();
        serde_json::json!({
            "task": task,
            "model": req.model.as_str(),
            "adapter": card.and_then(|c| c.adapter.clone()),
            "api_version": card.and_then(|c| c.api_version.clone()),
            "model_path": card.and_then(|c| c.path.as_ref()).map(|p| p.display().to_string()),
            "prompt": req.prompt,
            "stream": req.stream,
            "voice": req.voice,
            "sampling": {
                "temperature": req.sampling.temperature,
                "top_p": req.sampling.top_p,
                "max_tokens": req.sampling.max_tokens,
                "emotion": req.sampling.emotion,
            },
        })
    }

    /// 调用 sidecar 并转换事件流。
    async fn call(
        &self,
        path: &str,
        body: serde_json::Value,
        tx: &UnboundedSender<StreamEvent>,
    ) -> Result<InferenceOutcome> {
        let url = self.endpoint(path);
        let resp = self
            .client
            .post(&url)
            .json(&body)
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

        let emit = |event: StreamEvent, outcome: &mut InferenceOutcome| {
            outcome.record(&event);
            let _ = tx.send(event);
        };

        // 非流式：整个响应是单个 JSON 事件。
        if !content_type.contains("ndjson") && !content_type.contains("event-stream") {
            let value: serde_json::Value = resp
                .json()
                .await
                .map_err(|e| Error::Process(format!("invalid json from {url}: {e}")))?;
            // 非流式：done 事件里可能带上完整文本，先补一个 delta 再发 done。
            if let Some(text) = value.get("text").and_then(|v| v.as_str()) {
                if !text.is_empty() {
                    emit(StreamEvent::Delta(text.to_string()), &mut outcome);
                }
            }
            if let Some(event) = http::sidecar_event(&value) {
                emit(event, &mut outcome);
            }
            if outcome.events == 0 || outcome.usage.is_none() {
                emit(
                    StreamEvent::Done(outcome.usage.unwrap_or_default()),
                    &mut outcome,
                );
            }
            return Ok(outcome);
        }

        // 流式：NDJSON，每行一个事件。
        let mut stream = resp.bytes_stream();
        let mut buffer = String::new();
        let mut saw_done = false;

        while let Some(chunk) = stream.next().await {
            let chunk =
                chunk.map_err(|e| Error::Process(format!("stream from {url} failed: {e}")))?;
            buffer.push_str(&String::from_utf8_lossy(&chunk));

            while let Some(pos) = buffer.find('\n') {
                let line = buffer[..pos].trim().to_string();
                buffer.drain(..=pos);
                if line.is_empty() {
                    continue;
                }

                let value: serde_json::Value = match serde_json::from_str(&line) {
                    Ok(value) => value,
                    Err(err) => {
                        tracing::warn!(line = %line, error = %err, "bad sidecar event");
                        continue;
                    }
                };

                match http::sidecar_event(&value) {
                    Some(StreamEvent::Done(usage)) => {
                        saw_done = true;
                        emit(StreamEvent::Done(usage), &mut outcome);
                    }
                    Some(StreamEvent::Error(message)) => {
                        return Err(Error::Process(format!("sidecar error: {message}")));
                    }
                    Some(event) => emit(event, &mut outcome),
                    None => {}
                }
            }
        }

        if !saw_done {
            emit(
                StreamEvent::Done(outcome.usage.unwrap_or_default()),
                &mut outcome,
            );
        }

        Ok(outcome)
    }
}

#[async_trait]
impl Engine for TransformersEngine {
    fn kind(&self) -> EngineKind {
        EngineKind::Transformers
    }

    fn label(&self) -> &str {
        &self.label
    }

    fn state(&self) -> EngineState {
        *self.state.lock().expect("state lock poisoned")
    }

    fn caps(&self) -> &EngineCaps {
        &self.caps
    }

    fn model_path(&self) -> Option<&Path> {
        self.card.as_ref().and_then(|c| c.path.as_deref())
    }

    fn card(&self) -> Option<&ModelCard> {
        self.card.as_ref()
    }

    async fn start(&self) -> Result<()> {
        {
            let mut state = self.state.lock().expect("state lock poisoned");
            if state.is_ready() {
                return Ok(());
            }
            *state = EngineState::Starting;
        }

        let timeout = Duration::from_secs(self.cfg.startup_timeout_s.max(1));
        self.cfg.validate(EngineKind::Transformers)?;

        if self.cfg.base_url.is_some() {
            probe_http(&self.health_url, timeout).await?;
            *self.state.lock().expect("state lock poisoned") = EngineState::Ready;
            tracing::info!(label = %self.label, url = %self.health_url, "sidecar is externally managed and ready");
            return Ok(());
        }

        let (program, args) = self.cfg.command_line(&self.vars())?;
        let log_path = match &self.log_dir {
            Some(dir) => Some(open_log(dir, &self.label)?.1),
            None => None,
        };

        let mut cmd = tokio::process::Command::new(&program);
        cmd.args(&args).kill_on_drop(true);
        studio_procsup::apply_placement(&mut cmd, &self.cfg);
        let mut child = SupervisedChild::spawn(cmd, self.label.clone(), log_path)?;

        match child.wait_ready_http(&self.health_url, timeout).await {
            Ok(()) => {
                *self.child.lock().await = Some(child);
                *self.state.lock().expect("state lock poisoned") = EngineState::Ready;
                tracing::info!(label = %self.label, url = %self.health_url, "sidecar ready");
                Ok(())
            }
            Err(err) => {
                drop(child);
                *self.state.lock().expect("state lock poisoned") = EngineState::Unready;
                Err(err)
            }
        }
    }

    async fn health(&self) -> Health {
        if self.state().is_ready() {
            match probe_http(&self.health_url, Duration::from_secs(3)).await {
                Ok(()) => Health::ready(),
                Err(err) => Health::unready(err.to_string()),
            }
        } else {
            Health::unready(format!("engine state is {}", self.state()))
        }
    }

    async fn shutdown(&self, grace: Duration) -> Result<()> {
        if let Some(mut child) = self.child.lock().await.take() {
            child.shutdown(grace).await?;
        }
        *self.state.lock().expect("state lock poisoned") = EngineState::Dead;
        Ok(())
    }

    async fn tts(
        &self,
        req: &InferenceRequest,
        tx: UnboundedSender<StreamEvent>,
    ) -> Result<InferenceOutcome> {
        // sidecar 的 /infer/stream 同时服务文本与音频，直接复用 infer 路径。
        self.infer(req, tx).await
    }

    async fn infer(
        &self,
        req: &InferenceRequest,
        tx: UnboundedSender<StreamEvent>,
    ) -> Result<InferenceOutcome> {
        self.ensure_ready().await?;

        let task = match req.task {
            TaskKind::Chat | TaskKind::Completion => "text",
            TaskKind::Embed | TaskKind::Rerank => "embed",
            TaskKind::Decision => "decision",
            TaskKind::Tts => "tts",
            TaskKind::Transcribe => "transcribe",
        };

        let path = if req.task == TaskKind::Tts {
            "/infer/stream"
        } else {
            "/infer"
        };

        self.call(path, self.body(req, task), &tx).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use studio_core::{ModelId, TaskKind};

    #[test]
    fn builds_sidecar_body() {
        let cfg = EngineConfig::default_for(EngineKind::Transformers);
        let engine = TransformersEngine::new(cfg, None, "127.0.0.1", None);
        let req = InferenceRequest::new(ModelId::new("tts/index-tts2.5"), TaskKind::Tts, "hello")
            .with_voice("female");

        let body = engine.body(&req, "tts");
        assert_eq!(body["task"], "tts");
        assert_eq!(body["model"], "tts/index-tts2.5");
        assert_eq!(body["voice"], "female");
        assert_eq!(body["stream"], false);
    }

    #[test]
    fn endpoint_uses_base_url() {
        let mut cfg = EngineConfig::default_for(EngineKind::Transformers);
        cfg.base_url = Some("http://sidecar:9000".to_string());
        let engine = TransformersEngine::new(cfg, None, "127.0.0.1", None);
        assert_eq!(engine.endpoint("/infer"), "http://sidecar:9000/infer");
        assert_eq!(engine.kind(), EngineKind::Transformers);
    }
}
