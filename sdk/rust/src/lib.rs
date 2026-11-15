//! Basic-Studio Rust SDK（异步）。
//!
//! ```no_run
//! use studio_sdk::{ChatRequest, Client};
//!
//! #[tokio::main]
//! async fn main() -> studio_sdk::Result<()> {
//!     let client = Client::new("http://127.0.0.1:18596");
//!     let reply = client
//!         .chat(ChatRequest::new("chat/mock", "你好"))
//!         .await?;
//!     println!("{}", reply.content());
//!     Ok(())
//! }
//! ```

mod client;
mod error;
mod types;

pub use client::Client;
pub use error::{Error, Result};
pub use types::{
    ChatMessage, ChatRequest, ChatResponse, DecideRequest, DecideResponse, EmbeddingRequest,
    ModelInfo, ResponsesRequest, ResponsesResponse, SpeechRequest,
};

pub const DEFAULT_BASE_URL: &str = "http://127.0.0.1:18596";
