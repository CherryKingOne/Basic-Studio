# Basic-Studio

Rust 编写的模型加载与推理统一控制面：用一套 Rust 进程纳管 vLLM / SGLang / Transformers（默认）/ llama.cpp 四种推理引擎，
统一暴露 OpenAI 兼容接口，支持 LLM、TTS（IndexTTS 系列）流式推理与决策模型（DM：Laya、Kev）推理。

**M0~M3 已完成**：Rust workspace 可编译，`cargo test --workspace` 57 项通过、sidecar `python -m unittest` 11 项通过，`cargo clippy` 无告警。

已实现：

- `studio-core`：领域类型（模型/引擎/任务/流式事件/错误）
- `studio-config`：分层配置（`config/default.toml` 基线 + 深合并覆盖），未知引擎名直接报错
- `studio-registry`：扫描 `models/`、解析 `model.toml`、重复 id 检测
- `studio-procsup`：子进程监督（spawn / HTTP 与 TCP 就绪探测 / SIGTERM→SIGKILL / 日志落盘 / 端口池）
- `studio-engine`：`Engine` trait（生命周期 + `infer` + `tts`）、实例注册表、OpenAI 协议工具（SSE 解析、用量提取）
- `crates/engines/{vllm,sglang,transformers,llamacpp}`：
  - vLLM：`/v1/chat/completions`、`/v1/completions`、`/v1/embeddings`、`/v1/audio/speech`
  - SGLang：OpenAI 优先，404 时回退原生 `/generate`
  - Transformers：sidecar 通道（`/infer`、`/infer/stream`，NDJSON 事件）
  - 三者都支持“外部托管”模式（配置 `base_url` 时不拉进程，只做健康检查）
- `studio-gateway`：OpenAI 兼容 HTTP 网关，SSE 流式、TTS 音频流、`/v1/decide`、`/metrics`、`/healthz`、`/readyz`
- `studio-scheduler`：按模型并发上限 + 队列深度 + 快速失败
- `studio-auth`：可选 Bearer API Key
- `studio-obs`：Prometheus 文本指标（请求数、token、TTFT、音频字节、引擎实例）
- `studio-router`：`(模型, 任务)` → 引擎候选排序，支持按模型 id 精确匹配
- `sidecars/`：Python sidecar（echo / transformers / IndexTTS 1.0~2.5 / DM Laya 适配器）
- `studio-device`：CPU/NVIDIA/AMD/NPU/TPU 五类加速器探测与多卡分配
- `studio-cli`：`studio serve|models ls|models inspect|models route|engines ls|devices|doctor`
- 根目录 Docker：`Dockerfile`、`Dockerfile.sidecar`、`docker-compose.yml`、`docker-compose.gpu.yml`
- SDK：Python / Rust / Node.js / Java / Go（`sdk/`，见 [sdk/README.md](sdk/README.md)）

服务默认端口 **18596**（网关、sidecar、各引擎容器统一使用该端口；同机多实例时自动递增）。
网关基于 **Axum** 框架（见 `.claude/skills/axum/SKILL.md`）。

尚未开始：NVML 显存预算、权重下载器、Prometheus 之外的指标后端、gRPC 多节点集群、SDK（M4~M6 的剩余项）。

## 构建与运行

```bash
cargo build -p studio-cli

./target/debug/studio doctor                  # 环境自检
./target/debug/studio models ls               # 模型清单
./target/debug/studio models route tts        # 路由预览
./target/debug/studio serve                   # 只打印启动计划
./target/debug/studio serve --start           # 拉起引擎 + 监听 0.0.0.0:18596
```

端到端联调（无需 GPU，用内置 echo 适配器）：

```bash
# 1) 启动 sidecar（内部端口统一 18596）
cd sidecars && python3 -m studio_sidecar serve transformers --port 18596

# 2) 启动控制面（另一终端）
./target/debug/studio serve --start --config config/default.toml

# 3) 调用
curl -s localhost:18596/v1/chat/completions -H 'content-type: application/json' \
  -d '{"model":"<your-model-id>","messages":[{"role":"user","content":"hi"}]}'
```

Docker：

```bash
docker compose up -d studio              # 仅控制面
docker compose --profile vllm up -d      # 额外拉起 vLLM
docker compose -f docker-compose.yml -f docker-compose.gpu.yml --profile gpu up -d
```

## SDK

| 语言 | 目录 | 测试命令 |
| --- | --- | --- |
| Python | `sdk/python` | `python3 -m unittest discover -s sdk/python/tests` |
| Rust | `sdk/rust` | `cargo test -p studio-sdk` |
| Node.js | `sdk/nodejs` | `cd sdk/nodejs && npm test` |
| Java | `sdk/java` | `cd sdk/java && mvn test` |
| Go | `sdk/go` | `cd sdk/go && go test ./...` |

各语言 SDK 的完整用法见 [sdk/README.md](sdk/README.md)；发布流程见 [docs/PUBLISHING.md](docs/PUBLISHING.md)。

## 文档

- [docs/README.md](docs/README.md) — 文档索引
- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) — 总体架构与设计结论
- [docs/DIRECTORY_STRUCTURE.md](docs/DIRECTORY_STRUCTURE.md) — 目录结构树
- [docs/ROADMAP.md](docs/ROADMAP.md) — 里程碑与实施计划
- [docs/API.md](docs/API.md) — 接口清单
- [docs/ADR/0001-engine-integration.md](docs/ADR/0001-engine-integration.md) — 引擎集成策略决策

## 核心思路

Rust 负责注册表、调度、资源准入、路由、进程生命周期与对外 API；
推理引擎共 4 个——vLLM、SGLang、llama.cpp（均为子进程，走原生 HTTP）、Transformers（默认，无服务端，由仓库自带、
版本锁定的 Python sidecar 补上统一协议）。未在清单里声明引擎的模型默认走 Transformers。

IndexTTS（1.0/1.5/2.0/2.5）、Laya、Kev 都是**模型**，不是引擎——它们由上述 4 个引擎加载执行。

## 快速开始

```bash
./scripts/setup-envs.sh      # 创建各引擎 venv（TODO）
./scripts/fetch-models.sh    # 按 models/registry.toml 拉取权重（TODO）
./target/debug/studio doctor
./target/debug/studio serve --start
```