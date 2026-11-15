use std::net::TcpListener;
use std::path::{Path, PathBuf};

use clap::Args;
use studio_config::Config;
use studio_core::EngineKind;

use crate::output::{self, Status};

#[derive(Debug, Args)]
pub struct DoctorArgs {
    #[arg(short, long, value_name = "PATH", env = "STUDIO_CONFIG")]
    pub config: Option<PathBuf>,
}

pub fn run(args: DoctorArgs) -> anyhow::Result<()> {
    let mut failures = 0usize;
    let mut warnings = 0usize;

    output::title("配置");
    let cfg = match studio_config::load(args.config.as_deref()) {
        Ok(cfg) => {
            output::status(
                Status::Ok,
                format!("loaded, {} engines configured", cfg.engines.len()),
            );
            cfg
        }
        Err(err) => {
            output::status(Status::Fail, err);
            fail_exit(1);
        }
    };

    output::title("模型仓");
    failures += check_models(&cfg, &mut warnings);

    output::title("引擎环境");
    for kind in cfg.enabled_engines() {
        failures += check_engine(&cfg, kind, &mut warnings);
    }

    output::title("端口");
    warnings += check_port(&cfg.gateway.listen);

    output::title("结果");
    output::status(Status::Ok, format!("failures: {failures}"));
    output::status(Status::Ok, format!("warnings: {warnings}"));

    fail_exit(if failures > 0 { 1 } else { 0 })
}

fn fail_exit(code: i32) -> ! {
    std::process::exit(code)
}

fn check_models(cfg: &Config, warnings: &mut usize) -> usize {
    let root = cfg.models.root.clone();
    if !root.is_dir() {
        output::status(
            Status::Fail,
            format!("models root missing: {}", root.display()),
        );
        return 1;
    }

    match studio_registry::discover(&root) {
        Ok((index, discovery)) => {
            output::status(Status::Ok, format!("{} models", index.len()));

            for err in &discovery.errors {
                *warnings += 1;
                output::status(
                    Status::Warn,
                    format!("bad manifest {}: {}", err.path.display(), err.reason),
                );
            }

            let duplicates = index.duplicate_ids();
            if !duplicates.is_empty() {
                *warnings += 1;
                output::status(Status::Warn, format!("duplicate model ids: {duplicates:?}"));
            }

            if index.is_empty() {
                output::status(Status::Warn, "no models registered yet");
            }
            0
        }
        Err(err) => {
            output::status(Status::Fail, format!("scan failed: {err}"));
            1
        }
    }
}

fn check_engine(cfg: &Config, kind: EngineKind, warnings: &mut usize) -> usize {
    let engine = match cfg.engine(kind) {
        Some(engine) => engine,
        None => {
            *warnings += 1;
            output::status(Status::Warn, format!("{kind}: no config section"));
            return 0;
        }
    };

    if engine.base_url.is_some() {
        output::status(
            Status::Ok,
            format!("{kind}: externally managed (skip local checks)"),
        );
        return 0;
    }

    let vars = vec![
        ("model_path", "<model>".to_string()),
        ("port", engine.port.to_string()),
        ("host", "127.0.0.1".to_string()),
        ("venv", String::new()),
    ];

    match engine.command_line(&vars) {
        Ok((program, _args)) => match which(&program) {
            Some(_) => output::status(Status::Ok, format!("{kind}: `{program}` found")),
            None => {
                *warnings += 1;
                output::status(
                    Status::Warn,
                    format!(
                        "{kind}: `{program}` not found in PATH (engines may run in containers)"
                    ),
                );
            }
        },
        Err(err) => {
            *warnings += 1;
            output::status(Status::Warn, format!("{kind}: {err}"));
        }
    }

    match &engine.venv {
        Some(path) if path.is_dir() => {
            output::status(Status::Ok, format!("{kind}: venv {}", path.display()))
        }
        Some(path) => {
            *warnings += 1;
            output::status(
                Status::Warn,
                format!("{kind}: venv missing {}", path.display()),
            );
        }
        None => {}
    }

    0
}

fn check_port(listen: &str) -> usize {
    match TcpListener::bind(listen) {
        Ok(listener) => {
            drop(listener);
            output::status(Status::Ok, format!("{listen} available"));
            0
        }
        Err(err) => {
            output::status(Status::Warn, format!("{listen} not bindable: {err}"));
            1
        }
    }
}

/// 在 PATH 中查找可执行文件。
fn which(program: &str) -> Option<PathBuf> {
    let path = Path::new(program);
    if path.is_absolute() || program.contains('/') {
        return path.is_file().then(|| path.to_path_buf());
    }

    let paths = std::env::var_os("PATH")?;
    std::env::split_paths(&paths)
        .map(|dir| dir.join(program))
        .find(|candidate| candidate.is_file())
}
