mod commands;
mod output;

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "studio", version, about = "Basic-Studio 控制面")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// 加载配置与模型，启动引擎实例（不传 --start 时只打印启动计划）
    Serve(commands::serve::ServeArgs),
    /// 模型清单
    Models {
        #[command(subcommand)]
        command: commands::models::Command,
    },
    /// 引擎
    Engines {
        #[command(subcommand)]
        command: commands::engines::Command,
    },
    /// 环境自检
    Doctor(commands::doctor::DoctorArgs),
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .compact()
        .init();

    let cli = Cli::parse();

    match cli.command {
        Command::Serve(args) => commands::serve::run(args).await,
        Command::Models { command } => commands::models::run(command),
        Command::Engines { command } => commands::engines::run(command),
        Command::Doctor(args) => commands::doctor::run(args),
    }
}
