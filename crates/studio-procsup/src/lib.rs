//! 子进程监督：启动、就绪探测、优雅退出、日志捕获、端口分配。
//!
//! 推理引擎（vLLM / SGLang / sidecar）都是外部进程，本模块是它们与 Rust 之间唯一的边界。

mod child;
mod log;
mod port;
mod signal;

pub use child::{probe_http, SupervisedChild};
pub use log::open_log;
pub use port::PortPool;
pub mod placement;
pub use placement::apply as apply_placement;
