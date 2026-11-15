use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use clap::Args;
use studio_auth::ApiKeys;
use studio_config::{Config, EngineConfig};
use studio_core::{AcceleratorKind, Device, EngineKind, ModelCard, Placement};
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
    let (index, discovery) =
        studio_registry::discover_with_default(&root, cfg.gateway.default_engine)?;

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

    let devices = studio_device::detect(&cfg.accelerators.kinds).await;
    report_devices(&cfg, &devices);

    let engines = build_engines(&cfg, &index, &devices, &args.host, args.log_dir.clone())?;

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
    devices: &[Device],
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

            // 选定后端并计算设备分配
            let accelerator = engine_cfg
                .accelerator
                .unwrap_or_else(|| choose_accelerator(cfg, devices));
            let placements = studio_device::plan(
                accelerator,
                devices,
                cfg.accelerators.tensor_parallel,
                cfg.accelerators.data_parallel,
                cfg.accelerators.pipeline_parallel,
                cfg.accelerators.placement,
            );
            let placement = placements
                .into_iter()
                .nth(spawned.saturating_sub(1))
                .unwrap_or_else(Placement::cpu_only);
            engine_cfg.accelerator = Some(accelerator);
            engine_cfg.placement = Some(placement);

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
                EngineKind::LlamaCpp => Arc::new(engine_llamacpp::LlamaCppEngine::new(
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

/// 选择后端：按 `[accelerators].kinds` 顺序挑第一个有设备的非 CPU 后端；否则 CPU。
fn choose_accelerator(cfg: &Config, devices: &[Device]) -> AcceleratorKind {
    if !cfg.accelerators.enabled {
        return AcceleratorKind::Cpu;
    }

    for kind in &cfg.accelerators.kinds {
        if *kind == AcceleratorKind::Cpu {
            continue;
        }
        if devices.iter().any(|device| device.kind == *kind) {
            return *kind;
        }
    }

    let _ = (cfg.accelerators.cpu_fallback, devices);
    AcceleratorKind::Cpu
}

/// 打印检测到的设备与规划结果。
fn report_devices(cfg: &Config, devices: &[Device]) {
    output::title("加速器");
    if !cfg.accelerators.enabled {
        output::kv("status", "disabled (强制 CPU)");
        return;
    }

    if devices.is_empty() {
        output::kv("status", "未检测到任何设备");
        return;
    }

    let rows: Vec<Vec<String>> = devices
        .iter()
        .map(|device| {
            vec![
                device.kind.to_string(),
                device.index.to_string(),
                device.name.clone(),
                device
                    .memory_total_mb
                    .map(|mb| format!("{mb}MB"))
                    .unwrap_or_else(|| "-".to_string()),
                device
                    .memory_free_mb
                    .map(|mb| format!("{mb}MB"))
                    .unwrap_or_else(|| "-".to_string()),
            ]
        })
        .collect();

    table(&["backend", "#", "name", "total", "free"], &rows);
    output::kv(
        "parallel",
        format!(
            "tp={} dp={} pp={} strategy={}",
            cfg.accelerators.tensor_parallel,
            cfg.accelerators.data_parallel,
            cfg.accelerators.pipeline_parallel,
            match cfg.accelerators.placement {
                studio_core::PlacementStrategy::OnePerDevice => "one_per_device",
                studio_core::PlacementStrategy::SharedGroup => "shared_group",
            }
        ),
    );
}
