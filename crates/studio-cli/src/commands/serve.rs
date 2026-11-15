use std::path::PathBuf;
use std::time::Duration;

use clap::Args;
use studio_config::{Config, EngineConfig};
use studio_core::{EngineKind, ModelCard};
use studio_engine::{Engine, EngineRegistry};
use studio_registry::ModelIndex;

use crate::output::{self, table};

#[derive(Debug, Args)]
pub struct ServeArgs {
    #[arg(short, long, value_name = "PATH", env = "STUDIO_CONFIG")]
    pub config: Option<PathBuf>,

    /// 真正启动引擎并阻塞；默认只打印启动计划
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
        println!("\n  未启动（加 --start 才会真正拉起引擎进程）");
        return Ok(());
    }

    let mut registry = EngineRegistry::new();
    for engine in engines {
        registry.register(engine);
    }

    registry.start_all().await?;
    println!("\n  所有引擎已就绪，Ctrl-C 停止");

    if tokio::signal::ctrl_c().await.is_ok() {
        println!("  收到停止信号，正在关闭引擎...");
    }
    registry.shutdown_all(Duration::from_secs(15)).await;
    println!("  已停止");
    Ok(())
}

/// 为每个启用的引擎挑选一个模型，构造引擎实例。
///
/// 选择规则：优先选 `preferred_engine == kind` 的模型；否则退化为任何声明了该引擎的模型。
fn build_engines(
    cfg: &Config,
    index: &ModelIndex,
    host: &str,
    log_dir: Option<PathBuf>,
) -> anyhow::Result<Vec<Box<dyn Engine>>> {
    let mut engines: Vec<Box<dyn Engine>> = Vec::new();

    for kind in cfg.enabled_engines() {
        let engine_cfg: EngineConfig = match cfg.engine(kind) {
            Some(cfg) => cfg.clone(),
            None => continue,
        };
        engine_cfg.validate(kind)?;

        let card = match pick_card(index, kind) {
            Some(card) => card,
            None => {
                tracing::warn!(engine = %kind, "no model declares this engine, skip instance");
                continue;
            }
        };

        let engine: Box<dyn Engine> = match kind {
            EngineKind::Vllm => Box::new(engine_vllm::VllmEngine::new(
                engine_cfg,
                Some(card.clone()),
                host,
                log_dir.clone(),
            )),
            EngineKind::Sglang => Box::new(engine_sglang::SglangEngine::new(
                engine_cfg,
                Some(card.clone()),
                host,
                log_dir.clone(),
            )),
            EngineKind::Transformers => Box::new(engine_transformers::TransformersEngine::new(
                engine_cfg,
                Some(card.clone()),
                host,
                log_dir.clone(),
            )),
        };

        engines.push(engine);
    }

    Ok(engines)
}

fn pick_card(index: &ModelIndex, kind: EngineKind) -> Option<ModelCard> {
    index
        .iter()
        .find(|card| card.preferred_engine() == Some(kind) && card.engines.contains(&kind))
        .or_else(|| index.iter().find(|card| card.engines.contains(&kind)))
        .cloned()
}
