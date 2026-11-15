# SDK

官方客户端，覆盖 **Python / Rust / Node.js / Java / Go** 五种语言。
所有 SDK 面向同一套网关接口，默认地址 `http://127.0.0.1:18596`。

当前版本：**0.0.1**（与 git tag `0.0.1` 对应）。

## 安装方式（发布产物）

| 语言 | 产物 | 安装命令 |
| --- | --- | --- |
| Python | wheel / sdist（`*.whl`、`*.tar.gz`） | `pip install basic-studio==0.0.1`，或 `pip install ./dist/python/basic_studio-0.0.1-py3-none-any.whl` |
| Rust | crate（`*.crate`） | `cargo add studio-sdk@0.0.1`；或 `studio-sdk = { path = "sdk/rust" }` |
| Node.js | npm tarball（`*.tgz`） | `npm install @basic-studio/sdk@0.0.1`，或 `npm install ./dist/nodejs/basic-studio-sdk-0.0.1.tgz` |
| Java | jar | `mvn install:install-file -Dfile=dist/java/basic-studio-sdk-0.0.1.jar` |
| Go | 源码模块（tag 即版本） | `go get github.com/CherryKingOne/Basic-Studio/sdk/go@v0.0.1` |

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

## 本地构建产物

```bash
./scripts/build-sdk.sh              # 全部（缺工具链的自动跳过）
./scripts/build-sdk.sh python       # 只构建 Python
./scripts/build-sdk.sh python nodejs
```

产物落在 `dist/{python,nodejs,rust,java}/`（已 gitignore）。

各语言单独构建：

```bash
python3 -m build sdk/python --outdir dist/python
cd sdk/nodejs && npm pack --pack-destination ../../dist/nodejs
cargo package -p studio-sdk --allow-dirty --no-verify
mvn -f sdk/java/pom.xml -DskipTests package
cd sdk/go && go vet ./... && go test ./...
```

## 发布流程

```bash
# 1) 版本号对齐（Cargo.toml / pyproject.toml / package.json / pom.xml / sdk/go/version.go）
# 2) 构建 + 打 tag
./scripts/build-sdk.sh
git commit -am "release: 0.0.1"
git tag 0.0.1
git push origin main --tags
```

推送 tag 后 `.github/workflows/sdk-release.yml` 自动：
构建各语言产物 → 生成 GitHub Release（含产物附件）→ 发布 PyPI / npm / crates.io / Maven Central。
需要的 Secrets：`NPM_TOKEN`、`CARGO_REGISTRY_TOKEN`、`OSSRH_USERNAME`、`OSSRH_TOKEN`（PyPI 使用 trusted publishing）。

## 版本策略

- 主版本号 = 网关协议不兼容变更；次版本 = 新增接口；补丁 = 修 bug。
- 各 SDK 与网关同版本号发布，保持 `0.0.1` 对齐。
- Go 模块版本即 tag（`v0.0.1` 形式），`sdk/go/version.go` 里的 `Version` 常量需与 tag 同步。

## 鉴权

网关配置 `[gateway].api_keys` 后，各 SDK 传 API Key：

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
- 流式接口只产出内容增量；`responses_stream` 产出原始事件（含类型化事件名）。
- 错误统一为 `StudioError` / `StudioException` / `APIError`，含 HTTP 状态码与服务端 message。