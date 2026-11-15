//! Transformers 适配器（sidecar 通道）。
//!
//! Transformers 没有官方 HTTP 服务端，因此由 `sidecars/` 下的 Python sidecar 提供统一接口
//! （`/health`、`/infer`、`/infer/stream`）。IndexTTS 等模型也走这条通道，
//! 具体适配器由 `model.toml` 的 `adapter` 字段决定。
//!
//! 注意：IndexTTS 是**模型**，不是引擎。

use std::path::{Path, PathBuf};
use std::time::Duration;

use async_trait::async_trait;
use studio_config::EngineConfig;
use studio_core::{EngineCaps, EngineKind, ModelCard, Result};
use studio_engine::{Engine, EngineState, Health};
use studio_procsup::{open_log, probe_http, SupervisedChild};

pub struct TransformersEngine {
    cfg: EngineConfig,
    label: String,
    host: String,
    log_dir: Option<PathBuf>,
    card: Option<ModelCard>,
    child: Option<SupervisedChild>,
    state: EngineState,
    caps: EngineCaps,
    health_url: String,
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
            child: None,
            state: EngineState::Unready,
            caps: EngineCaps {
                streaming: true,
                max_ctx: None,
                tensor_parallel: false,
            },
            health_url,
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
        self.state
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

    async fn start(&mut self) -> Result<()> {
        if self.state.is_ready() {
            return Ok(());
        }
        self.state = EngineState::Starting;

        let timeout = Duration::from_secs(self.cfg.startup_timeout_s.max(1));
        self.cfg.validate(EngineKind::Transformers)?;

        if self.cfg.base_url.is_some() {
            probe_http(&self.health_url, timeout).await?;
            self.state = EngineState::Ready;
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
        let mut child = SupervisedChild::spawn(cmd, self.label.clone(), log_path)?;

        child.wait_ready_http(&self.health_url, timeout).await?;

        self.child = Some(child);
        self.state = EngineState::Ready;
        tracing::info!(label = %self.label, url = %self.health_url, "sidecar ready");
        Ok(())
    }

    async fn health(&self) -> Health {
        if self.state.is_ready() {
            match probe_http(&self.health_url, Duration::from_secs(3)).await {
                Ok(()) => Health::ready(),
                Err(err) => Health::unready(err.to_string()),
            }
        } else {
            Health::unready(format!("engine state is {}", self.state))
        }
    }

    async fn shutdown(&mut self, grace: Duration) -> Result<()> {
        if let Some(mut child) = self.child.take() {
            child.shutdown(grace).await?;
        }
        self.state = EngineState::Dead;
        Ok(())
    }
}
