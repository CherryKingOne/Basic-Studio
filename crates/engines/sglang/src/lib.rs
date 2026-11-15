//! SGLang 适配器。
//!
//! 优先走 OpenAI 兼容接口（`/v1/chat/completions`）；当模型未实现该路由（404）时，
//! 回退到 SGLang 原生的 `/generate`。两条路径对外表现一致。

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use async_trait::async_trait;
use studio_config::EngineConfig;
use studio_core::{
    EngineCaps, EngineKind, Error, InferenceOutcome, InferenceRequest, ModelCard, Result,
    StreamEvent, TaskKind, Usage,
};
use studio_engine::http;
use studio_engine::{Engine, EngineState, Health};
use studio_procsup::{open_log, probe_http, SupervisedChild};
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::Mutex as AsyncMutex;

pub struct SglangEngine {
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

impl SglangEngine {
    pub fn new(
        cfg: EngineConfig,
        card: Option<ModelCard>,
        host: impl Into<String>,
        log_dir: Option<PathBuf>,
    ) -> Self {
        let host = host.into();
        let label = card
            .as_ref()
            .map(|c| format!("sglang/{}", c.id))
            .unwrap_or_else(|| "sglang/unknown".to_string());
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
                tensor_parallel: true,
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

    /// 原生 `/generate`：非流式，返回 `{ "text": "...", "meta_info": {...} }`。
    async fn generate(
        &self,
        req: &InferenceRequest,
        tx: &UnboundedSender<StreamEvent>,
    ) -> Result<InferenceOutcome> {
        let url = self.endpoint("/generate");
        let body = serde_json::json!({
            "text": req.prompt,
            "sampling_params": {
                "temperature": req.sampling.temperature,
                "top_p": req.sampling.top_p,
                "max_new_tokens": req.sampling.max_tokens,
            },
            "stream": false,
        });

        let value = http::post_json(&self.client, &url, &body).await?;
        let text = value
            .get("text")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();

        let mut outcome = InferenceOutcome::default();
        if !text.is_empty() {
            let event = StreamEvent::Delta(text);
            outcome.record(&event);
            if tx.send(event).is_err() {
                return Ok(outcome);
            }
        }

        let completion_tokens = value
            .pointer("/meta_info/completion_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let done = StreamEvent::Done(Usage {
            prompt_tokens: 0,
            completion_tokens,
        });
        outcome.record(&done);
        let _ = tx.send(done);
        Ok(outcome)
    }
}

#[async_trait]
impl Engine for SglangEngine {
    fn kind(&self) -> EngineKind {
        EngineKind::Sglang
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
        self.cfg.validate(EngineKind::Sglang)?;

        if self.cfg.base_url.is_some() {
            probe_http(&self.health_url, timeout).await?;
            *self.state.lock().expect("state lock poisoned") = EngineState::Ready;
            tracing::info!(label = %self.label, url = %self.health_url, "sglang is externally managed and ready");
            return Ok(());
        }

        let (program, args) = self.cfg.command_line(&self.vars())?;
        let log_path = match &self.log_dir {
            Some(dir) => Some(open_log(dir, &self.label)?.1),
            None => None,
        };

        let mut cmd = tokio::process::Command::new(&program);
        cmd.args(&args).kill_on_drop(true);
        let mut child = SupervisedChild::spawn(cmd, self.label.clone(), log_path)?;

        match child.wait_ready_http(&self.health_url, timeout).await {
            Ok(()) => {
                *self.child.lock().await = Some(child);
                *self.state.lock().expect("state lock poisoned") = EngineState::Ready;
                tracing::info!(label = %self.label, url = %self.health_url, "sglang ready");
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

    async fn infer(
        &self,
        req: &InferenceRequest,
        tx: UnboundedSender<StreamEvent>,
    ) -> Result<InferenceOutcome> {
        self.ensure_ready().await?;

        match req.task {
            TaskKind::Chat | TaskKind::Decision | TaskKind::Completion => {
                let url = self.endpoint("/v1/chat/completions");
                let body = http::chat_body(req);
                match http::post_chat(&self.client, &url, &body, &tx).await {
                    Ok(outcome) => Ok(outcome),
                    Err(err) if err.to_string().contains("404") => {
                        tracing::debug!(label = %self.label, "OpenAI route unavailable, falling back to /generate");
                        self.generate(req, &tx).await
                    }
                    Err(err) => Err(err),
                }
            }
            TaskKind::Embed | TaskKind::Rerank => {
                let url = self.endpoint("/v1/embeddings");
                let value =
                    http::post_json(&self.client, &url, &http::embeddings_body(req)).await?;
                let event = StreamEvent::Delta(value.to_string());
                let mut outcome = InferenceOutcome::default();
                outcome.record(&event);
                let _ = tx.send(event);
                let done = StreamEvent::Done(Usage::default());
                outcome.record(&done);
                let _ = tx.send(done);
                Ok(outcome)
            }
            TaskKind::Tts | TaskKind::Transcribe => Err(Error::Process(format!(
                "sglang instance `{}` cannot serve task {}",
                self.label, req.task
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_endpoints() {
        let mut cfg = EngineConfig::default_for(EngineKind::Sglang);
        cfg.base_url = Some("http://sglang:30000".to_string());
        let engine = SglangEngine::new(cfg, None, "127.0.0.1", None);

        assert_eq!(
            engine.endpoint("/v1/chat/completions"),
            "http://sglang:30000/v1/chat/completions"
        );
        assert_eq!(engine.endpoint("/generate"), "http://sglang:30000/generate");
        assert_eq!(engine.kind(), EngineKind::Sglang);
    }
}
