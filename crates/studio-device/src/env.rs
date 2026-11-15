//! 环境变量与并行参数：把 [`Placement`] 翻译成引擎能理解的参数。

use studio_core::{AcceleratorKind, Placement};

/// 生成该实例的环境变量（用于子进程）。
pub fn env_vars(kind: AcceleratorKind, placement: &Placement) -> Vec<(String, String)> {
    let mut out = Vec::new();

    if let Some(name) = kind.visible_env() {
        if !placement.visible.is_empty() {
            out.push((name.to_string(), placement.visible.clone()));
        }
    }

    match kind {
        // 昇腾需要额外声明 device logic id
        AcceleratorKind::Npu => {
            out.push((
                "ASCEND_DEVICE_ID".to_string(),
                first_or_zero(&placement.visible),
            ));
        }
        AcceleratorKind::Tpu => {
            out.push(("TPU_CHIPS_PER_HOST_BOUNDS".to_string(), "1,1,1".to_string()));
        }
        _ => {}
    }

    out
}

fn first_or_zero(visible: &str) -> String {
    visible.split(',').next().unwrap_or("0").trim().to_string()
}

/// 引擎通用的并行度参数（vLLM / SGLang 均识别）。
pub fn parallel_args(placement: &Placement) -> Vec<String> {
    let mut args = Vec::new();

    if placement.tensor_parallel > 1 {
        args.push("--tensor-parallel-size".to_string());
        args.push(placement.tensor_parallel.to_string());
    }
    if placement.pipeline_parallel > 1 {
        args.push("--pipeline-parallel-size".to_string());
        args.push(placement.pipeline_parallel.to_string());
    }
    if placement.data_parallel > 1 {
        args.push("--data-parallel-size".to_string());
        args.push(placement.data_parallel.to_string());
    }

    args
}

/// 引擎的 `--device` 取值（CPU 推理时需要显式指定）。
pub fn device_flag(kind: AcceleratorKind) -> Option<&'static str> {
    match kind {
        AcceleratorKind::Cpu => Some("cpu"),
        AcceleratorKind::Nvidia => Some("cuda"),
        AcceleratorKind::Amd => Some("cuda"), // ROCm 兼容 CUDA 接口
        AcceleratorKind::Npu => Some("npu"),
        AcceleratorKind::Tpu => Some("tpu"),
    }
}

/// 引擎的 `--device` 参数（仅在非 CUDA 后端需要显式声明）。
pub fn device_args(kind: AcceleratorKind) -> Vec<String> {
    match kind {
        AcceleratorKind::Cpu => vec!["--device".to_string(), "cpu".to_string()],
        AcceleratorKind::Npu => vec!["--device".to_string(), "npu".to_string()],
        AcceleratorKind::Tpu => vec!["--device".to_string(), "tpu".to_string()],
        AcceleratorKind::Nvidia | AcceleratorKind::Amd => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn placement(visible: &str, tp: usize) -> Placement {
        Placement {
            devices: vec![],
            tensor_parallel: tp,
            data_parallel: 1,
            pipeline_parallel: 1,
            visible: visible.to_string(),
        }
    }

    #[test]
    fn sets_visible_devices() {
        let vars = env_vars(AcceleratorKind::Nvidia, &placement("0,1", 2));
        assert!(vars.contains(&("CUDA_VISIBLE_DEVICES".to_string(), "0,1".to_string())));
    }

    #[test]
    fn cpu_has_no_visible_env() {
        assert!(env_vars(AcceleratorKind::Cpu, &placement("", 1)).is_empty());
    }

    #[test]
    fn npu_gets_device_id() {
        let vars = env_vars(AcceleratorKind::Npu, &placement("2,3", 1));
        assert!(vars.contains(&("ASCEND_RT_VISIBLE_DEVICES".to_string(), "2,3".to_string())));
        assert!(vars.contains(&("ASCEND_DEVICE_ID".to_string(), "2".to_string())));
    }

    #[test]
    fn parallel_args_only_when_needed() {
        assert!(parallel_args(&placement("0", 1)).is_empty());
        let args = parallel_args(&placement("0,1", 2));
        assert_eq!(args, vec!["--tensor-parallel-size", "2"]);
    }

    #[test]
    fn device_flags_cover_all_backends() {
        assert_eq!(device_flag(AcceleratorKind::Cpu), Some("cpu"));
        assert_eq!(device_flag(AcceleratorKind::Npu), Some("npu"));
        assert_eq!(device_flag(AcceleratorKind::Amd), Some("cuda"));
    }

    #[test]
    fn device_args_only_for_non_cuda() {
        assert!(device_args(AcceleratorKind::Nvidia).is_empty());
        assert_eq!(device_args(AcceleratorKind::Cpu).len(), 2);
    }
}

/// 单个引擎实例的完整启动计划：环境变量 + 参数。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandPlan {
    pub env: Vec<(String, String)>,
    pub args: Vec<String>,
}

impl CommandPlan {
    pub fn is_empty(&self) -> bool {
        self.env.is_empty() && self.args.is_empty()
    }
}

/// 由后端与设备分配生成启动计划。
pub fn plan(kind: AcceleratorKind, placement: &Placement) -> CommandPlan {
    CommandPlan {
        env: env_vars(kind, placement),
        args: parallel_args(placement)
            .into_iter()
            .chain(device_args(kind))
            .collect(),
    }
}

#[cfg(test)]
mod plan_tests {
    use super::*;

    fn placement(visible: &str, tp: usize) -> Placement {
        Placement {
            devices: vec![],
            tensor_parallel: tp,
            data_parallel: 1,
            pipeline_parallel: 1,
            visible: visible.to_string(),
        }
    }

    #[test]
    fn cuda_plan_has_env_only() {
        let plan = plan(AcceleratorKind::Nvidia, &placement("0,1", 2));
        assert_eq!(plan.env.len(), 1);
        assert_eq!(plan.args, vec!["--tensor-parallel-size", "2"]);
    }

    #[test]
    fn cpu_plan_has_device_flag() {
        let plan = plan(AcceleratorKind::Cpu, &placement("", 1));
        assert!(plan.env.is_empty());
        assert_eq!(plan.args, vec!["--device", "cpu"]);
    }

    #[test]
    fn empty_plan_when_nothing_needed() {
        let plan = plan(AcceleratorKind::Nvidia, &placement("", 1));
        assert!(plan.is_empty());
    }
}
