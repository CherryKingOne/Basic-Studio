use std::path::PathBuf;

use clap::{Args, Subcommand};
use studio_core::EngineKind;

use crate::output::{self, table};

#[derive(Debug, Subcommand)]
pub enum Command {
    /// 列出已配置的引擎
    Ls(LsArgs),
}

#[derive(Debug, Args)]
pub struct LsArgs {
    #[arg(short, long, value_name = "PATH", env = "STUDIO_CONFIG")]
    pub config: Option<PathBuf>,
}

pub fn run(command: Command) -> anyhow::Result<()> {
    match command {
        Command::Ls(args) => ls(args),
    }
}

fn ls(args: LsArgs) -> anyhow::Result<()> {
    let cfg = studio_config::load(args.config.as_deref())?;

    output::title("引擎");
    output::kv("enabled", cfg.enabled_engines().len());

    let rows: Vec<Vec<String>> = EngineKind::ALL
        .into_iter()
        .map(|kind| {
            let engine = cfg.engine(kind);
            match engine {
                Some(cfg) if cfg.enabled => vec![
                    kind.to_string(),
                    "enabled".to_string(),
                    cfg.command.join(" "),
                    cfg.port.to_string(),
                    cfg.health_path.clone(),
                    cfg.base_url.clone().unwrap_or_else(|| "-".to_string()),
                    cfg.startup_timeout_s.to_string(),
                ],
                Some(_) => vec![
                    kind.to_string(),
                    "disabled".to_string(),
                    "-".to_string(),
                    "-".to_string(),
                    "-".to_string(),
                    "-".to_string(),
                    "-".to_string(),
                ],
                None => vec![
                    kind.to_string(),
                    "missing".to_string(),
                    "-".to_string(),
                    "-".to_string(),
                    "-".to_string(),
                    "-".to_string(),
                    "-".to_string(),
                ],
            }
        })
        .collect();

    table(
        &[
            "engine",
            "state",
            "command",
            "port",
            "health",
            "base_url",
            "timeout_s",
        ],
        &rows,
    );
    Ok(())
}
