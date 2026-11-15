//! 设备发现。
//!
//! 每个后端一个探测函数：优先调用厂商 SMI 工具，失败时退回 sysfs / 环境变量。
//! 所有探测都是"尽力而为"：探测失败只记日志，不阻断启动。

use std::path::Path;

use studio_core::{AcceleratorKind, Device};

mod amd;
mod cpu;
mod npu;
mod nvidia;
mod tpu;

pub use amd::parse_rocm_smi;
pub use npu::parse_npu_smi_list;
pub use nvidia::parse_nvidia_smi;

/// 按请求的后端顺序探测，返回去重后的设备列表。
pub async fn detect(kinds: &[AcceleratorKind]) -> Vec<Device> {
    let mut devices = Vec::new();

    for kind in kinds {
        let found = match kind {
            AcceleratorKind::Cpu => cpu::probe().await,
            AcceleratorKind::Nvidia => nvidia::probe().await,
            AcceleratorKind::Amd => amd::probe().await,
            AcceleratorKind::Npu => npu::probe().await,
            AcceleratorKind::Tpu => tpu::probe().await,
        };

        match found {
            Some(mut list) if !list.is_empty() => {
                tracing::info!(backend = %kind, count = list.len(), "accelerator detected");
                devices.append(&mut list);
            }
            _ => tracing::debug!(backend = %kind, "accelerator not available"),
        }
    }

    devices
}

/// 把工具输出按行切开，过滤空行（供各后端解析复用）。
pub(crate) fn lines(output: &str) -> impl Iterator<Item = &str> {
    output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
}

/// 判断某个可执行文件是否存在。
pub(crate) fn has_command(program: &str) -> bool {
    if program.contains('/') {
        return Path::new(program).is_file();
    }
    let Some(paths) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&paths)
        .map(|dir| dir.join(program))
        .any(|candidate| candidate.is_file())
}

/// 判断设备节点是否存在（用于 NPU / TPU 的兜底探测）。
pub(crate) fn has_device_node(pattern: &str) -> bool {
    let Ok(entries) = std::fs::read_dir("/dev") else {
        return false;
    };
    entries
        .filter_map(|entry| entry.ok())
        .any(|entry| entry.file_name().to_string_lossy().starts_with(pattern))
}

/// 执行外部命令并返回 stdout；失败返回 `None`。
pub(crate) async fn run_capture(program: &str, args: &[String]) -> Option<String> {
    let output = tokio::process::Command::new(program)
        .args(args)
        .output()
        .await
        .map_err(|err| {
            tracing::debug!(program, error = %err, "command not runnable");
            err
        })
        .ok()?;

    if !output.status.success() {
        tracing::debug!(program, status = %output.status, "command failed");
        return None;
    }

    Some(String::from_utf8_lossy(&output.stdout).to_string())
}
