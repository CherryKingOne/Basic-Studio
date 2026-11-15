//! 华为昇腾 NPU 探测：`npu-smi info`，失败时回退到 `/dev/davinci*` 设备节点。

use studio_core::{AcceleratorKind, Device};

use super::{has_command, has_device_node, run_capture};

pub(crate) async fn probe() -> Option<Vec<Device>> {
    if has_command("npu-smi") {
        let args = vec!["info".to_string()];
        if let Some(output) = run_capture("npu-smi", &args).await {
            let devices = parse_npu_smi_list(&output);
            if !devices.is_empty() {
                return Some(devices);
            }
        }
    }

    // 兜底：没有 npu-smi 但有 davinci 设备节点时，按节点数量上报
    let mut index = 0u32;
    while has_device_node(&format!("davinci{index}")) || (index == 0 && has_device_node("davinci"))
    {
        index += 1;
        if index > 64 {
            break;
        }
    }

    (index > 0).then(|| {
        (0..index)
            .map(|i| Device {
                index: i,
                kind: AcceleratorKind::Npu,
                name: "Ascend NPU".to_string(),
                memory_total_mb: None,
                memory_free_mb: None,
                driver_version: None,
            })
            .collect()
    })
}

/// 解析 `npu-smi info` 的表格输出。
///
/// 只关心以 `|` 开头且首列形如 `<id> <chip>` 的数据行：
/// ```text
/// | 0     910B4     | OK            | 95.5        42                0    / 0             |
/// ```
/// 各版本列含义不同，这里只可靠提取"编号 + 芯片型号"，内存等信息留待后续按版本适配。
pub fn parse_npu_smi_list(output: &str) -> Vec<Device> {
    let mut devices = Vec::new();

    for line in super::lines(output) {
        if !line.starts_with('|') {
            continue;
        }

        let cells: Vec<&str> = line.split('|').map(str::trim).collect();
        if cells.len() < 3 {
            continue;
        }

        let mut parts = cells[1].split_whitespace();
        let (Some(index), Some(chip)) = (parts.next(), parts.next()) else {
            continue;
        };
        let Ok(index) = index.parse::<u32>() else {
            continue;
        };
        // 过滤表头与"芯片行"（芯片行第二列是纯数字）
        if chip.eq_ignore_ascii_case("name") || chip.parse::<u32>().is_ok() {
            continue;
        }

        devices.push(Device {
            index,
            kind: AcceleratorKind::Npu,
            name: chip.to_string(),
            memory_total_mb: None,
            memory_free_mb: None,
            driver_version: None,
        });
    }

    devices
}
#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = "
+------------------------------------------------------------------------------------------------+
| npu-smi 23.0.3                   Version: 23.0.3                                               |
+-------------------------------+-----------------+--------------------------------------------+
| NPU   Name      | Health        | Power(W)    Temp(C)           Hugepages-Usage(page) |
| Chip  Device    | Bus-Id        | AICore(%)   Memory-Usage(MB)  HBM-Usage(MB)         |
+===============================+=================+============================================+
| 0     910B4     | OK            | 95.5        42                0    / 0             |
| 0      0        | 0000:C1:00.0  | 0           0    / 0          2048 / 65536         |
+===============================+=================+============================================+
| 1     910B4     | OK            | 94.9        41                0    / 0             |
| 1      0        | 0000:C2:00.0  | 0           0    / 0          2048 / 65536         |
+-------------------------------+-----------------+--------------------------------------------+
";

    #[test]
    fn parses_two_npus() {
        let devices = parse_npu_smi_list(FIXTURE);
        assert_eq!(devices.len(), 2);
        assert_eq!(devices[0].index, 0);
        assert_eq!(devices[0].name, "910B4");
    }

    #[test]
    fn skips_header_rows() {
        assert!(parse_npu_smi_list("| NPU   Name      | Health        |").is_empty());
    }
}
