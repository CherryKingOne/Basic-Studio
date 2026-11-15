//! CPU 探测：核数 + 型号 + 物理内存。
//!
//! 只上报一个逻辑设备（index 0）；多路 CPU 的 NUMA 拓扑后续可扩展。

use std::fs;

use studio_core::{AcceleratorKind, Device};

pub(crate) async fn probe() -> Option<Vec<Device>> {
    let cores = std::thread::available_parallelism()
        .map(|n| n.get() as u64)
        .unwrap_or(1);

    let model = fs::read_to_string("/proc/cpuinfo")
        .ok()
        .and_then(|content| {
            content.lines().find_map(|line| {
                line.strip_prefix("model name")
                    .and_then(|rest| rest.split(':').nth(1))
                    .map(str::trim)
                    .filter(|name| !name.is_empty())
                    .map(str::to_string)
            })
        })
        .unwrap_or_else(|| "cpu".to_string());

    let total_mb = fs::read_to_string("/proc/meminfo")
        .ok()
        .and_then(|content| {
            content.lines().find_map(|line| {
                line.strip_prefix("MemTotal:")
                    .and_then(|rest| rest.split_whitespace().next())
                    .and_then(|kb| kb.parse::<u64>().ok())
                    .map(|kb| kb / 1024)
            })
        });

    Some(vec![Device {
        index: 0,
        kind: AcceleratorKind::Cpu,
        name: format!("{model} ({cores} cores)"),
        memory_total_mb: total_mb,
        memory_free_mb: None,
        driver_version: None,
    }])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn always_reports_cpu() {
        let devices = probe().await.expect("cpu always available");
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].kind, AcceleratorKind::Cpu);
        assert!(devices[0].memory_total_mb.unwrap_or(0) > 0);
    }
}
