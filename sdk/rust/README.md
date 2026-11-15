# Basic-Studio Rust SDK

异步客户端，基于 `reqwest` + `tokio`。

## 使用

```toml
[dependencies]
studio-sdk = { path = "sdk/rust" }
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

```rust
use futures_util::StreamExt;
use studio_sdk::{ChatRequest, Client};

#[tokio::main]
async fn main() -> studio_sdk::Result<()> {
    let client = Client::new("http://127.0.0.1:18596").with_api_key("sk-...");

    let reply = client.chat(ChatRequest::new("chat/mock", "你好")).await?;
    println!("{}", reply.content());

    let mut stream = client
        .chat_stream(ChatRequest::new("chat/mock", "讲个故事"))
        .await?;
    while let Some(delta) = stream.next().await {
        print!("{}", delta?);
    }

    let pcm = client
        .tts(studio_sdk::SpeechRequest::new("tts/echomock", "你好"))
        .await?;
    std::fs::write("out.pcm", pcm)?;

    Ok(())
}
```

## 测试

```bash
cargo test -p studio-sdk
```
