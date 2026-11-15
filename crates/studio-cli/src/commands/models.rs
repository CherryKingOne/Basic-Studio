use std::path::{Path, PathBuf};

use clap::{Args, Subcommand};
use studio_core::TaskKind;
use studio_registry::ModelIndex;

use crate::output::{self, engine_list, table};

#[derive(Debug, Subcommand)]
pub enum Command {
    /// 列出全部模型
    Ls(LsArgs),
    /// 查看单个模型详情
    Inspect(InspectArgs),
    /// 路由预览：某任务会被路由到哪些引擎
    Route(RouteArgs),
}

#[derive(Debug, Args)]
pub struct LsArgs {
    #[arg(short, long, value_name = "PATH", env = "STUDIO_CONFIG")]
    pub config: Option<PathBuf>,
    #[arg(long, value_name = "TASK")]
    pub task: Option<String>,
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct InspectArgs {
    #[arg(value_name = "ID")]
    pub id: String,
    #[arg(short, long, value_name = "PATH", env = "STUDIO_CONFIG")]
    pub config: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub struct RouteArgs {
    #[arg(value_name = "TASK", default_value = "tts")]
    pub task: String,
    #[arg(short, long, value_name = "PATH", env = "STUDIO_CONFIG")]
    pub config: Option<PathBuf>,
}

pub fn run(command: Command) -> anyhow::Result<()> {
    match command {
        Command::Ls(args) => ls(args),
        Command::Inspect(args) => inspect(args),
        Command::Route(args) => route(args),
    }
}

fn load(config: Option<&Path>) -> anyhow::Result<(studio_config::Config, ModelIndex)> {
    let cfg = studio_config::load(config)?;
    let (index, _) = studio_registry::discover(&cfg.models.root)?;
    Ok((cfg, index))
}

fn ls(args: LsArgs) -> anyhow::Result<()> {
    let (cfg, index) = load(args.config.as_deref())?;
    let task = match &args.task {
        Some(raw) => Some(
            raw.parse::<TaskKind>()
                .map_err(|e| anyhow::anyhow!("{e}"))?,
        ),
        None => None,
    };

    let cards: Vec<&studio_core::ModelCard> = match task {
        Some(task) => index.by_task(task),
        None => index.iter().collect(),
    };

    if args.json {
        let json: Vec<serde_json::Value> = cards
            .iter()
            .map(|card| {
                serde_json::json!({
                    "id": card.id.to_string(),
                    "task": card.task.to_string(),
                    "engines": card.engines.iter().map(|e| e.to_string()).collect::<Vec<_>>(),
                    "adapter": card.adapter,
                    "api_version": card.api_version,
                    "streaming": card.streaming,
                    "path": card.path.as_ref().map(|p| p.display().to_string()),
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&json)?);
        return Ok(());
    }

    output::title("模型");
    output::kv("root", cfg.models.root.display());
    output::kv("total", index.len());

    let rows: Vec<Vec<String>> = cards
        .iter()
        .map(|card| {
            vec![
                card.id.to_string(),
                card.task.to_string(),
                engine_list(&card.engines),
                card.adapter.clone().unwrap_or_else(|| "-".to_string()),
                card.streaming.to_string(),
                card.path
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "-".to_string()),
            ]
        })
        .collect();

    table(
        &["id", "task", "engines", "adapter", "stream", "path"],
        &rows,
    );
    Ok(())
}

fn inspect(args: InspectArgs) -> anyhow::Result<()> {
    let (_cfg, index) = load(args.config.as_deref())?;
    let card = index
        .get(&args.id)
        .ok_or_else(|| anyhow::anyhow!("model not found: {}", args.id))?;

    output::title(&format!("model {}", card.id));
    output::kv("family", &card.family);
    output::kv("task", card.task);
    output::kv(
        "preferred engine",
        card.preferred_engine()
            .map(|e| e.to_string())
            .unwrap_or_else(|| "-".into()),
    );
    output::kv("engines", engine_list(&card.engines));
    output::kv("adapter", card.adapter.as_deref().unwrap_or("-"));
    output::kv("api_version", card.api_version.as_deref().unwrap_or("-"));
    output::kv("streaming", card.streaming);
    output::kv(
        "min_vram_gb",
        card.min_vram_gb
            .map(|v| v.to_string())
            .unwrap_or_else(|| "-".into()),
    );
    output::kv(
        "params_b",
        card.params_b
            .map(|v| v.to_string())
            .unwrap_or_else(|| "-".into()),
    );
    output::kv("dtype", card.dtype.as_deref().unwrap_or("-"));
    output::kv(
        "max_text_chars",
        card.limits
            .max_text_chars
            .map(|v| v.to_string())
            .unwrap_or_else(|| "-".into()),
    );
    output::kv(
        "sample_rate",
        card.limits
            .sample_rate
            .map(|v| v.to_string())
            .unwrap_or_else(|| "-".into()),
    );
    output::kv(
        "path",
        card.path
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "-".into()),
    );
    Ok(())
}

fn route(args: RouteArgs) -> anyhow::Result<()> {
    let (cfg, index) = load(args.config.as_deref())?;
    let task = args
        .task
        .parse::<TaskKind>()
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let available = cfg.enabled_engines();

    let selections = studio_router::best(&index, task, &available);

    output::title(&format!("路由预览: {task}"));
    output::kv("available engines", engine_list(&available));

    let rows: Vec<Vec<String>> = selections
        .iter()
        .map(|sel| {
            vec![
                sel.model_id.clone(),
                sel.engine.to_string(),
                sel.preferred.to_string(),
                sel.reason.clone(),
            ]
        })
        .collect();

    if rows.is_empty() {
        println!("  （没有匹配的模型）");
    } else {
        table(&["model", "engine", "preferred", "reason"], &rows);
    }
    Ok(())
}
