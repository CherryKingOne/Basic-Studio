//! 加速器发现与多卡编排。
//!
//! 支持 CPU / NVIDIA / AMD / 华为 NPU / TPU 五类后端：
//! - `probe`：调用各厂商 SMI 工具或 sysfs 发现设备，缺失时优雅降级；
//! - `planner`：把设备分配给引擎实例（每张卡一个实例，或整组共享）；
//! - `env`：生成 `CUDA_VISIBLE_DEVICES` 一类环境变量与并行度参数。

pub mod env;
pub mod planner;
pub mod probe;

pub use planner::{plan, plan_all};
pub use probe::detect;
