//! Basic-Studio 领域类型。
//!
//! 这里只放纯数据与纯函数，不依赖任何 I/O、运行时或第三方框架，
//! 因此可以被所有 crate 安全依赖。

pub mod engine;
pub mod error;
pub mod events;
pub mod model;
pub mod request;
pub mod task;

pub use engine::{EngineCaps, EngineKind};
pub use error::{Error, Result};
pub use events::{AudioChunk, InferenceOutcome, StreamEvent, Usage};
pub use model::{ModelCard, ModelId, ModelLimits};
pub use request::{InferenceRequest, Sampling};
pub use task::TaskKind;
