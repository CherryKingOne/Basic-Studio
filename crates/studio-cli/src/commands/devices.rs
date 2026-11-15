use std::path::PathBuf;

use clap::Args;
use studio_core::{AcceleratorKind, Device};

use crate::output::{self, table};

#[derive(Debug, Args)]
pub struct DevicesArgs {
    #[arg(short, long, value_name = "PATH", env = "STUDIO_CONFIG")]
    pub config: Option<PathBuf>,
}

pub async fn run(args: DevicesArgs) -> anyhow::Result<()> {
    let cfg = studio_config::load(args.config.as_deref())?;
    let devices = studio_device::detect(&cfg.accelerators.kinds).await;

    output::title("加速器");
    output::kv("探测顺序", kinds_text(&cfg.accelerators.kinds));
    output::kv(
        "parallel",
        format!(
            "tp={} dp={} pp={}",
            cfg.accelerators.tensor_parallel,
            cfg.accelerators.data_parallel,
            cfg.accelerators.pipeline_parallel
        ),
    );

    if devices.is_empty() {
        println!("  （未检测到任何加速器）");
        return Ok(());
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

    println!();
    for kind in AcceleratorKind::ALL {
        let count = devices.iter().filter(|d| d.kind == kind).count();
        if count > 0 {
            output::kv(kind.as_str(), format!("{count} device(s)"));
        }
    }

    Ok(())
}

pub fn kinds_text(kinds: &[AcceleratorKind]) -> String {
    kinds
        .iter()
        .map(|kind| kind.to_string())
        .collect::<Vec<_>>()
        .join(" → ")
}

/// 供 doctor 复用：把设备列表格式化成一行摘要。
pub fn summary(devices: &[Device]) -> String {
    if devices.is_empty() {
        return "no accelerator".to_string();
    }

    let mut counts: Vec<String> = Vec::new();
    for kind in AcceleratorKind::ALL {
        let count = devices.iter().filter(|d| d.kind == kind).count();
        if count > 0 {
            counts.push(format!("{kind}×{count}"));
        }
    }
    counts.join(", ")
}
