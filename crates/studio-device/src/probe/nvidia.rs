//! NVIDIA 探测：`nvidia-smi --query-gpu`。

use studio_core::{AcceleratorKind, Device};

use super::{has_command, run_capture};

const QUERY: &str = "index,name,memory.total,memory.free,driver_version";

pub(crate) async fn probe() -> Option<Vec<Device>> {
    if !has_command("nvidia-smi") {
        return None;
    }

    let args = vec![
        format!("--query-gpu={QUERY}"),
        "--format=csv,noheader,nounits".to_string(),
    ];
    let output = run_capture("nvidia-smi", &args).await?;

    let devices = parse_nvidia_smi(&output);
    (!devices.is_empty()).then_some(devices)
}

/// 解析 `nvidia-smi --format=csv,noheader,nounits` 输出。
///
/// 每行形如：`0, NVIDIA A100-SXM4-80GB, 81920, 80000, 535.104.05`
pub fn parse_nvidia_smi(output: &str) -> Vec<Device> {
    let mut devices = Vec::new();

    for line in super::lines(output) {
        let columns: Vec<&str> = line.split(',').map(str::trim).collect();
        if columns.len() < 3 {
            continue;
        }

        let index = columns[0].parse::<u32>().unwrap_or(devices.len() as u32);
        let total = columns[2].parse::<u64>().ok();
        let free = columns.get(3).and_then(|v| v.parse::<u64>().ok());

        devices.push(Device {
            index,
            kind: AcceleratorKind::Nvidia,
            name: columns[1].to_string(),
            memory_total_mb: total,
            memory_free_mb: free,
            driver_version: columns.get(4).map(|v| v.to_string()),
        });
    }

    devices
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = "0, NVIDIA A100-SXM4-80GB, 81920, 80000, 535.104.05\n\
                           1, NVIDIA A100-SXM4-80GB, 81920, 79500, 535.104.05";

    #[test]
    fn parses_two_gpus() {
        let devices = parse_nvidia_smi(FIXTURE);
        assert_eq!(devices.len(), 2);
        assert_eq!(devices[0].index, 0);
        assert_eq!(devices[0].name, "NVIDIA A100-SXM4-80GB");
        assert_eq!(devices[0].memory_total_mb, Some(81920));
        assert_eq!(devices[1].memory_free_mb, Some(79500));
    }

    #[test]
    fn skips_malformed_lines() {
        assert!(parse_nvidia_smi("garbage").is_empty());
        assert!(parse_nvidia_smi("").is_empty());
    }
}
