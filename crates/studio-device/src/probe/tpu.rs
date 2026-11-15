//! TPU 探测：`/dev/accel*` 设备节点 + `tpu-info`（如可用）。

use studio_core::{AcceleratorKind, Device};

use super::{has_command, has_device_node, run_capture};

pub(crate) async fn probe() -> Option<Vec<Device>> {
    let count = if has_command("tpu-info") {
        match run_capture("tpu-info", &Vec::new()).await {
            Some(output) => count_accelerators(&output).max(1),
            None => count_nodes(),
        }
    } else {
        count_nodes()
    };

    (count > 0).then(|| {
        (0..count)
            .map(|i| Device {
                index: i as u32,
                kind: AcceleratorKind::Tpu,
                name: "Google TPU".to_string(),
                memory_total_mb: None,
                memory_free_mb: None,
                driver_version: None,
            })
            .collect()
    })
}

fn count_nodes() -> usize {
    let mut index = 0usize;
    while has_device_node(&format!("accel{index}")) {
        index += 1;
        if index > 64 {
            break;
        }
    }
    index
}

/// 统计 `tpu-info` 输出里的芯片数量（宽松匹配 `accel` / `chip` 行）。
pub fn count_accelerators(output: &str) -> usize {
    output
        .lines()
        .filter(|line| {
            let lower = line.to_ascii_lowercase();
            lower.contains("accel") || lower.contains("chip")
        })
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_chip_lines() {
        let output = "accel0\ntype: TPU v5e\naccel1\ntype: TPU v5e\n";
        assert_eq!(count_accelerators(output), 2);
    }

    #[test]
    fn no_chips_when_output_empty() {
        assert_eq!(count_accelerators(""), 0);
    }
}
