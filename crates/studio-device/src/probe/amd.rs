//! AMD ROCm 探测：`rocm-smi --csv`。

use studio_core::{AcceleratorKind, Device};

use super::{has_command, run_capture};

pub(crate) async fn probe() -> Option<Vec<Device>> {
    if !has_command("rocm-smi") {
        return None;
    }

    let args = vec![
        "--showproductname".to_string(),
        "--showmeminfo".to_string(),
        "vram".to_string(),
        "--csv".to_string(),
    ];
    let output = run_capture("rocm-smi", &args).await?;
    let devices = parse_rocm_smi(&output);
    (!devices.is_empty()).then_some(devices)
}

/// 解析 `rocm-smi --csv` 输出。
///
/// 形如：
/// ```text
/// device,VRAM Total Memory (B),VRAM Total Used Memory (B),Card series
/// card0,34359738368,123456789,Radeon Instinct MI210
/// ```
pub fn parse_rocm_smi(output: &str) -> Vec<Device> {
    let mut devices = Vec::new();

    for line in super::lines(output) {
        if !line.to_ascii_lowercase().starts_with("card") {
            continue;
        }
        let columns: Vec<&str> = line.split(',').map(str::trim).collect();
        if columns.len() < 2 {
            continue;
        }

        let index = columns[0]
            .trim_start_matches("card")
            .parse::<u32>()
            .unwrap_or(devices.len() as u32);

        let total_mb = columns
            .get(1)
            .and_then(|v| v.parse::<u64>().ok())
            .map(|bytes| bytes / 1024 / 1024);
        let name = columns
            .get(3)
            .filter(|v| !v.is_empty())
            .map(|v| v.to_string())
            .unwrap_or_else(|| "AMD GPU".to_string());

        devices.push(Device {
            index,
            kind: AcceleratorKind::Amd,
            name,
            memory_total_mb: total_mb,
            memory_free_mb: None,
            driver_version: None,
        });
    }

    devices
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = "device,VRAM Total Memory (B),VRAM Total Used Memory (B),Card series\n\
                           card0,34359738368,123456789,Radeon Instinct MI210\n\
                           card1,34359738368,223456789,Radeon Instinct MI210";

    #[test]
    fn parses_rocm_cards() {
        let devices = parse_rocm_smi(FIXTURE);
        assert_eq!(devices.len(), 2);
        assert_eq!(devices[1].index, 1);
        assert_eq!(devices[0].memory_total_mb, Some(32768));
        assert_eq!(devices[0].name, "Radeon Instinct MI210");
    }

    #[test]
    fn ignores_header_and_blank() {
        assert!(parse_rocm_smi("device,VRAM Total Memory (B)\n").is_empty());
    }
}
