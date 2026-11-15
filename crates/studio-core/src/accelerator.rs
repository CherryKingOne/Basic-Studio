use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::Error;

/// 支持的加速器类型。
///
/// 顺序即默认优先级：GPU 类优先于 CPU。实际启用哪些由配置决定。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AcceleratorKind {
    Cpu,
    Nvidia,
    Amd,
    Npu,
    Tpu,
}

impl AcceleratorKind {
    pub const ALL: [AcceleratorKind; 5] = [
        AcceleratorKind::Cpu,
        AcceleratorKind::Nvidia,
        AcceleratorKind::Amd,
        AcceleratorKind::Npu,
        AcceleratorKind::Tpu,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            AcceleratorKind::Cpu => "cpu",
            AcceleratorKind::Nvidia => "nvidia",
            AcceleratorKind::Amd => "amd",
            AcceleratorKind::Npu => "npu",
            AcceleratorKind::Tpu => "tpu",
        }
    }

    /// 是否为 GPU 类（具备显存、可做张量并行）。
    pub fn is_accelerator(self) -> bool {
        !matches!(self, AcceleratorKind::Cpu)
    }

    /// 用于 `CUDA_VISIBLE_DEVICES` 一类环境变量的前缀名。
    pub fn visible_env(self) -> Option<&'static str> {
        match self {
            AcceleratorKind::Cpu => None,
            AcceleratorKind::Nvidia => Some("CUDA_VISIBLE_DEVICES"),
            AcceleratorKind::Amd => Some("ROCR_VISIBLE_DEVICES"),
            AcceleratorKind::Npu => Some("ASCEND_RT_VISIBLE_DEVICES"),
            AcceleratorKind::Tpu => Some("TPU_VISIBLE_CHIPS"),
        }
    }
}

impl fmt::Display for AcceleratorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for AcceleratorKind {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "cpu" => Ok(AcceleratorKind::Cpu),
            "nvidia" | "cuda" | "gpu" => Ok(AcceleratorKind::Nvidia),
            "amd" | "rocm" | "hip" => Ok(AcceleratorKind::Amd),
            "npu" | "ascend" => Ok(AcceleratorKind::Npu),
            "tpu" => Ok(AcceleratorKind::Tpu),
            other => Err(Error::Accelerator(format!("unknown accelerator: {other}"))),
        }
    }
}

/// 单个设备。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Device {
    /// 设备在同类中的序号，从 0 开始。
    pub index: u32,
    pub kind: AcceleratorKind,
    pub name: String,
    /// 总内存（MB），CPU 用 `None` 表示不限制。
    pub memory_total_mb: Option<u64>,
    /// 可用内存（MB）。
    pub memory_free_mb: Option<u64>,
    /// 驱动/固件版本（如有）。
    pub driver_version: Option<String>,
}

impl Device {
    pub fn is_cpu(&self) -> bool {
        self.kind == AcceleratorKind::Cpu
    }

    pub fn label(&self) -> String {
        match self.memory_total_mb {
            Some(total) => format!("{}/{}/{}MB", self.kind, self.index, total),
            None => format!("{}/{}", self.kind, self.index),
        }
    }
}

/// 多卡编排策略。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PlacementStrategy {
    /// 每个引擎实例独占一张卡（数据并行）。
    #[default]
    OnePerDevice,
    /// 多个实例共享同一组卡（模型并行）。
    SharedGroup,
}

/// 引擎实例的设备分配。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Placement {
    pub devices: Vec<u32>,
    pub tensor_parallel: usize,
    pub data_parallel: usize,
    pub pipeline_parallel: usize,
    /// 该实例的 `visible` 环境变量内容（逗号分隔的设备号）。
    pub visible: String,
}

impl Placement {
    pub fn cpu_only() -> Self {
        Self {
            devices: Vec::new(),
            tensor_parallel: 1,
            data_parallel: 1,
            pipeline_parallel: 1,
            visible: String::new(),
        }
    }

    pub fn describe(&self) -> String {
        format!(
            "devices=[{}] tp={} dp={} pp={}",
            self.visible, self.tensor_parallel, self.data_parallel, self.pipeline_parallel
        )
    }
}
