use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use clap::Args;
use studio_auth::ApiKeys;
use studio_config::{Config, EngineConfig};
use studio_core::{EngineKind, ModelCard};
use studio_engine::{Engine, EngineRegistry};
use studio_registry::ModelIndex;
use studio_scheduler::Scheduler;

use crate::output::{self, table};

#[derive(Debug, Args)]
pub struct ServeArgs {
    #[arg(short, long, value_name = "PATH", env = "STUDIO_CONFIG")]
    pub config: Option<PathBuf>,

    /// 不传时只打印启动计划，不拉起进程也不监听端口
    #[arg(long)]
    pub start: bool,

    #[arg(long, default_value = "127.0.0.1", env = "STUDIO_HOST")]
    pub host: String,

    #[arg(long, value_name = "DIR")]
    pub log_dir: Option<PathBuf>,
}

pub async fn run(args: ServeArgs) -> anyhow::Result<()> {
    let cfg = studio_config::load(args.config.as_deref())?;
    let root = cfg.models.root.clone();
    let (index, discovery) = studio_registry::discover(&root)?;

    output::title("Basic-Studio");
    output::kv("models root", root.display());
    output::kv("models found", index.len());
    output::kv("enabled engines", cfg.enabled_engines().len());
    output::kv("gateway", &cfg.gateway.listen);

    for err in &discovery.errors {
        tracing::warn!(path = %err.path.display(), reason = %err.reason, "bad model manifest");
    }
    let duplicates = index.duplicate_ids();
    if !duplicates.is_empty() {
        tracing::warn!(ids = ?duplicates, "duplicate model ids");
    }

    let engines = build_engines(&cfg, &index, &args.host, args.log_dir.clone())?;

    output::title("启动计划");
    let rows: Vec<Vec<String>> = engines
        .iter()
        .map(|engine| {
            vec![
                engine.label().to_string(),
                engine.state().to_string(),
                engine
                    .model_path()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "<none>".to_string()),
            ]
        })
        .collect();
    if rows.is_empty() {
        println!("  （没有需要启动的引擎实例）");
    } else {
        table(&["engine", "state", "model"], &rows);
    }

    if !args.start {
        println!("\n  未启动（加 --start 才会拉起引擎并监听端口）");
        return Ok(());
    }

    let mut registry = EngineRegistry::new();
    for engine in engines {
        registry.register(engine);
    }

    if let Err(err) = registry.start_all().await {
        tracing::error!(error = %err, "failed to start engines");
        registry.shutdown_all(Duration::from_secs(10)).await;
        return Err(err.into());
    }
    println!("\n  引擎已就绪，网关监听 {}", cfg.gateway.listen);

    let scheduler = Scheduler::new(&cfg.scheduler);
    let auth = ApiKeys::new(cfg.gateway.api_keys.clone());
    if auth.is_open() {
        tracing::warn!("no api keys configured: 鉴权已关闭（仅建议本地开发）");
    }

    let state =
        studio_gateway::AppState::new(cfg.clone(), index, registry.clone(), scheduler, auth);

    let shutdown = async {
        let _ = tokio::signal::ctrl_c().await;
        println!("  收到停止信号，正在关闭...");
    };

    studio_gateway::serve(state, &cfg.gateway.listen, shutdown).await?;
    registry.shutdown_all(Duration::from_secs(15)).await;
    println!("  已停止");
    Ok(())
}

/// 每个 (模型, 引擎) 组合构造一个实例，避免多个模型共用一个引擎导致请求串台。
///
/// 选择规则：遍历所有模型，凡声明了该引擎的都建实例；本地拉起时自动错开端口。
fn build_engines(
    cfg: &Config,
    index: &ModelIndex,
    host: &str,
    log_dir: Option<PathBuf>,
) -> anyhow::Result<Vec<Arc<dyn Engine>>> {
    let mut engines: Vec<Arc<dyn Engine>> = Vec::new();

    for kind in cfg.enabled_engines() {
        let mut spawned: usize = 0;

        for card in cards_for(index, kind) {
            let mut engine_cfg: EngineConfig = cfg
                .engine(kind)
                .cloned()
                .unwrap_or_else(|| EngineConfig::default_for(kind));
            engine_cfg.validate(kind)?;

            // 本地拉起时同一引擎的多个实例不能共用端口
            if engine_cfg.base_url.is_none() && spawned > 0 {
                engine_cfg.port = engine_cfg.port.saturating_add(spawned as u16);
            }
            spawned += 1;

            let engine: Arc<dyn Engine> = match kind {
                EngineKind::Vllm => Arc::new(engine_vllm::VllmEngine::new(
                    engine_cfg,
                    Some(card),
                    host,
                    log_dir.clone(),
                )),
                EngineKind::Sglang => Arc::new(engine_sglang::SglangEngine::new(
                    engine_cfg,
                    Some(card),
                    host,
                    log_dir.clone(),
                )),
                EngineKind::Transformers => Arc::new(engine_transformers::TransformersEngine::new(
                    engine_cfg,
                    Some(card),
                    host,
                    log_dir.clone(),
                )),
            };

            engines.push(engine);
        }

        if spawned == 0 {
            tracing::warn!(engine = %kind, "no model declares this engine, no instance created");
        }
    }

    Ok(engines)
}

/// 声明了指定引擎的模型列表。
fn cards_for(index: &ModelIndex, kind: EngineKind) -> Vec<ModelCard> {
    index
        .iter()
        .filter(|card| card.engines.contains(&kind))
        .cloned()
        .collect()
}
