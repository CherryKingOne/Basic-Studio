//! vLLM 适配器。
//!
//! 以子进程方式启动 `vllm serve <model>`，通过 OpenAI 兼容 HTTP 接口通信。
//! M0 只实现生命周期；M1 增加请求透传与流式转发。

use std::path::{Path, PathBuf};
use std::time::Duration;

use async_trait::async_trait;
use studio_config::EngineConfig;
use studio_core::{EngineCaps, EngineKind, ModelCard, Result};
use studio_engine::{Engine, EngineState, Health};
use studio_procsup::{open_log, probe_http, SupervisedChild};

pub struct VllmEngine {
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

impl VllmEngine {
    pub fn new(
        cfg: EngineConfig,
        card: Option<ModelCard>,
        host: impl Into<String>,
        log_dir: Option<PathBuf>,
    ) -> Self {
        let host = host.into();
        let label = card
            .as_ref()
            .map(|c| format!("vllm/{}", c.id))
            .unwrap_or_else(|| "vllm/unknown".to_string());
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
                tensor_parallel: true,
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
            ("tensor_parallel", "1".to_string()),
        ]
    }
}

#[async_trait]
impl Engine for VllmEngine {
    fn kind(&self) -> EngineKind {
        EngineKind::Vllm
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
        self.cfg.validate(EngineKind::Vllm)?;

        // 外部托管（docker compose / 已有服务）：不拉起进程，只等端口就绪。
        if self.cfg.base_url.is_some() {
            probe_http(&self.health_url, timeout).await?;
            self.state = EngineState::Ready;
            tracing::info!(label = %self.label, url = %self.health_url, "vllm is externally managed and ready");
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
        tracing::info!(label = %self.label, url = %self.health_url, "vllm ready");
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
