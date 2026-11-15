# SDK

官方客户端，覆盖 **Python / Rust / Node.js / Java / Go** 五种语言。
所有 SDK 面向同一套网关接口，默认地址 `http://127.0.0.1:18596`。

| 语言 | 目录 | 安装 | 测试 |
| --- | --- | --- | --- |
| Python | `sdk/python` | `pip install ./sdk/python` | `python3 -m unittest discover -s sdk/python/tests` |
| Rust | `sdk/rust` | `cargo add studio-sdk --path sdk/rust` | `cargo test -p studio-sdk` |
| Node.js | `sdk/nodejs` | `npm install @basic-studio/sdk`（本地 `npm install ./sdk/nodejs`） | `cd sdk/nodejs && npm test` |
| Java | `sdk/java` | `mvn install`（Java 17+） | `mvn test` |
| Go | `sdk/go` | `go get github.com/CherryKingOne/Basic-Studio/sdk/go` | `cd sdk/go && go test ./...` |

## 统一 API 对照

| 能力 | Python | Rust | Node.js | Java | Go |
| --- | --- | --- | --- | --- | --- |
| 健康检查 | `client.health()` | `client.health()` | `client.health()` | `client.health()` | `client.Health(ctx)` |
| 就绪 | `client.ready()` | `client.ready()` | `client.ready()` | `client.ready()` | `client.Ready(ctx)` |
| 模型列表 | `client.models()` | `client.models()` | `client.models()` | `client.models()` | `client.Models(ctx)` |
| 对话 | `client.chat(...)` | `client.chat(req)` | `client.chat({...})` | `client.chat(req)` | `client.Chat(ctx, req)` |
| 流式对话 | `client.chat_stream(...)` | `client.chat_stream(req)` | `client.chatStream({...})` | `client.chatStream(req, cb)` | `client.ChatStream(ctx, req)` |
| Responses | `client.responses(...)` | `client.responses(req)` | `client.responses({...})` | `client.responses(req)` | `client.Responses(ctx, req)` |
| 向量化 | `client.embeddings(...)` | `client.embeddings(req)` | `client.embeddings({...})` | `client.embeddings(model, input)` | `client.Embeddings(ctx, model, input)` |
| 语音合成 | `client.tts(...)` | `client.tts(req)` | `client.tts({...})` | `client.tts(req)` | `client.TTS(ctx, req)` |
| 决策模型 | `client.decide(...)` | `client.decide(req)` | `client.decide({...})` | `client.decide(req)` | `client.Decide(ctx, req)` |

## 鉴权

网关配置了 `[gateway].api_keys` 后，各 SDK 传 API Key 即可：

```python
Client(api_key="sk-...")
```
```rust
Client::new(url).with_api_key("sk-...")
```
```js
new Client({ apiKey: "sk-..." })
```
```java
BasicStudioClient.builder().apiKey("sk-...").build()
```
```go
basicstudio.New(url).WithAPIKey("sk-...")
```

## 一致性约定

- `chat` 的 `messages` 支持字符串或消息对象；`system` 作为独立参数。
- 流式接口只在 **内容增量** 上产出，usage 等非文本帧不透传（Responses 流式除外，它产出原始事件）。
- 错误统一抛 `StudioError` / `StudioException` / `APIError`，含 HTTP 状态码与服务端 message。
- 所有方法均可通过 context（Go）、取消令牌或关闭客户端释放连接。
