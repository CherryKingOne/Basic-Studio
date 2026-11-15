//! llama.cpp 适配器（`llama-server`）。
//!
//! `llama-server` 自带 OpenAI 兼容接口，因此复用 `studio-engine::http` 的协议工具：
//! - `/v1/chat/completions`、`/v1/completions`、`/v1/embeddings`
//! - 健康检查 `/health`
//!
//! 面向 GGUF 模型（CPU / 混合推理友好），不支持 TTS。

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use async_trait::async_trait;
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

pub struct LlamaCppEngine {
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

impl LlamaCppEngine {
    pub fn new(
        cfg: EngineConfig,
        card: Option<ModelCard>,
        host: impl Into<String>,
        log_dir: Option<PathBuf>,
    ) -> Self {
        let host = host.into();
        let label = card
            .as_ref()
            .map(|c| format!("llamacpp/{}", c.id))
            .unwrap_or_else(|| "llamacpp/unknown".to_string());
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
                max_ctx: Some(4096),
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
}

#[async_trait]
impl Engine for LlamaCppEngine {
    fn kind(&self) -> EngineKind {
        EngineKind::LlamaCpp
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
        self.cfg.validate(EngineKind::LlamaCpp)?;

        if self.cfg.base_url.is_some() {
            probe_http(&self.health_url, timeout).await?;
            *self.state.lock().expect("state lock poisoned") = EngineState::Ready;
            tracing::info!(label = %self.label, url = %self.health_url, "llama.cpp is externally managed and ready");
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
                tracing::info!(label = %self.label, url = %self.health_url, "llama.cpp ready");
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
            TaskKind::Chat | TaskKind::Decision => {
                let url = self.endpoint("/v1/chat/completions");
                http::post_chat(&self.client, &url, &http::chat_body(req), &tx).await
            }
            TaskKind::Completion => {
                let url = self.endpoint("/v1/completions");
                let body = serde_json::json!({
                    "model": req.model.as_str(),
                    "prompt": req.prompt,
                    "stream": req.stream,
                    "max_tokens": req.sampling.max_tokens,
                });
                http::post_chat(&self.client, &url, &body, &tx).await
            }
            TaskKind::Embed | TaskKind::Rerank => {
                let url = self.endpoint("/v1/embeddings");
                let value =
                    http::post_json(&self.client, &url, &http::embeddings_body(req)).await?;
                let event = StreamEvent::Delta(value.to_string());
                let mut outcome = InferenceOutcome::default();
                outcome.record(&event);
                let _ = tx.send(event);
                let done = StreamEvent::Done(Default::default());
                outcome.record(&done);
                let _ = tx.send(done);
                Ok(outcome)
            }
            TaskKind::Tts | TaskKind::Transcribe => Err(Error::Process(format!(
                "llama.cpp instance `{}` cannot serve task {}",
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
        let mut cfg = EngineConfig::default_for(EngineKind::LlamaCpp);
        cfg.base_url = Some("http://llama:8081".to_string());
        let engine = LlamaCppEngine::new(cfg, None, "127.0.0.1", None);

        assert_eq!(engine.kind(), EngineKind::LlamaCpp);
        assert_eq!(
            engine.endpoint("/v1/chat/completions"),
            "http://llama:8081/v1/chat/completions"
        );
        assert_eq!(engine.caps().max_ctx, Some(4096));
    }
}
