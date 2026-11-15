//! 多卡编排：把设备分配给引擎实例。

use studio_core::{AcceleratorKind, Device, Placement, PlacementStrategy};

/// 计算某类加速器的实例分配。
///
/// 规则：
/// - 每实例需要的设备数 = `tensor_parallel * pipeline_parallel`；
/// - `OnePerDevice`：按组切分，`data_parallel` 个实例共享同一批设备时自动收敛为 `1`；
/// - `SharedGroup`：所有设备分给一个实例（模型并行），`data_parallel` 视为 1；
/// - 该类型没有设备时返回单个 CPU 占位（由调用方决定是否真正回退）。
pub fn plan(
    kind: AcceleratorKind,
    devices: &[Device],
    tensor_parallel: usize,
    data_parallel: usize,
    pipeline_parallel: usize,
    strategy: PlacementStrategy,
) -> Vec<Placement> {
    if kind == AcceleratorKind::Cpu {
        return vec![Placement::cpu_only()];
    }

    let selected: Vec<&Device> = devices.iter().filter(|d| d.kind == kind).collect();
    if selected.is_empty() {
        return vec![Placement::cpu_only()];
    }

    let tp = tensor_parallel.max(1);
    let pp = pipeline_parallel.max(1);
    let group_size = (tp * pp).max(1);
    let ids: Vec<u32> = selected.iter().map(|d| d.index).collect();

    let mut placements = Vec::new();

    match strategy {
        PlacementStrategy::SharedGroup => {
            placements.push(Placement {
                devices: ids.clone(),
                tensor_parallel: tp,
                data_parallel: 1,
                pipeline_parallel: pp,
                visible: join(&ids),
            });
        }
        PlacementStrategy::OnePerDevice => {
            let groups = (data_parallel.max(1)).max(ids.len() / group_size);
            for group in 0..groups.min(ids.len().div_ceil(group_size).max(1)) {
                let start = group * group_size;
                let end = (start + group_size).min(ids.len());
                if start >= ids.len() {
                    break;
                }
                let slice = &ids[start..end];
                placements.push(Placement {
                    devices: slice.to_vec(),
                    tensor_parallel: tp,
                    data_parallel: 1,
                    pipeline_parallel: pp,
                    visible: join(slice),
                });
            }
        }
    }

    if placements.is_empty() {
        placements.push(Placement::cpu_only());
    }

    placements
}

fn join(ids: &[u32]) -> String {
    ids.iter()
        .map(|id| id.to_string())
        .collect::<Vec<_>>()
        .join(",")
}

/// 一次性规划全部后端（按配置顺序），用于启动时打印拓扑。
pub fn plan_all(
    kinds: &[AcceleratorKind],
    devices: &[Device],
    tensor_parallel: usize,
    data_parallel: usize,
    pipeline_parallel: usize,
    strategy: PlacementStrategy,
) -> Vec<(AcceleratorKind, Vec<Placement>)> {
    kinds
        .iter()
        .map(|kind| {
            (
                *kind,
                plan(
                    *kind,
                    devices,
                    tensor_parallel,
                    data_parallel,
                    pipeline_parallel,
                    strategy,
                ),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gpu(index: u32) -> Device {
        Device {
            index,
            kind: AcceleratorKind::Nvidia,
            name: "A100".to_string(),
            memory_total_mb: Some(81920),
            memory_free_mb: Some(80000),
            driver_version: None,
        }
    }

    #[test]
    fn cpu_always_single() {
        let placements = plan(
            AcceleratorKind::Cpu,
            &[],
            4,
            4,
            1,
            PlacementStrategy::OnePerDevice,
        );
        assert_eq!(placements.len(), 1);
        assert!(placements[0].devices.is_empty());
    }

    #[test]
    fn one_instance_per_card() {
        let devices: Vec<Device> = (0..4).map(gpu).collect();
        let placements = plan(
            AcceleratorKind::Nvidia,
            &devices,
            1,
            4,
            1,
            PlacementStrategy::OnePerDevice,
        );
        assert_eq!(placements.len(), 4);
        assert_eq!(placements[0].visible, "0");
        assert_eq!(placements[3].visible, "3");
    }

    #[test]
    fn tensor_parallel_groups_cards() {
        let devices: Vec<Device> = (0..4).map(gpu).collect();
        let placements = plan(
            AcceleratorKind::Nvidia,
            &devices,
            2,
            1,
            1,
            PlacementStrategy::OnePerDevice,
        );
        assert_eq!(placements.len(), 2);
        assert_eq!(placements[0].visible, "0,1");
        assert_eq!(placements[1].visible, "2,3");
    }

    #[test]
    fn shared_group_uses_all_cards() {
        let devices: Vec<Device> = (0..2).map(gpu).collect();
        let placements = plan(
            AcceleratorKind::Nvidia,
            &devices,
            2,
            4,
            1,
            PlacementStrategy::SharedGroup,
        );
        assert_eq!(placements.len(), 1);
        assert_eq!(placements[0].visible, "0,1");
        assert_eq!(placements[0].data_parallel, 1);
    }

    #[test]
    fn missing_backend_falls_back_to_cpu() {
        let placements = plan(
            AcceleratorKind::Npu,
            &[gpu(0)],
            1,
            1,
            1,
            PlacementStrategy::OnePerDevice,
        );
        assert_eq!(placements.len(), 1);
        assert!(placements[0].devices.is_empty());
    }
}
