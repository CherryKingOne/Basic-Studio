# Basic-Studio 架构设计

> 模型加载与推理统一控制面。Rust 编写，统一纳管 vLLM / SGLang / Transformers / 原生 Rust 等推理引擎，
> 统一暴露 OpenAI 兼容接口，支持 TTS 流式推理与决策模型（DM）负载。

## 1. 设计结论（先看这里）

| 决策 | 结论 | 理由 |
| --- | --- | --- |
| 主体语言 | Rust（tokio + axum） | 内存/并发可控、单二进制部署、跨平台 |
| 引擎集成方式 | **进程编排（Supervised Subprocess）+ 协议客户端** | vLLM/SGLang/TensorRT-LLM 原生就是 Python 服务，且各有独立 CUDA/PyTorch 依赖，内嵌不可行 |
| 少数无服务端能力 | **Python sidecar**（仓库自带、版本锁定、HTTP/WS 暴露） | Transformers 及其上的模型（IndexTTS 等）只能走 Python；sidecar 是二等公民但协议统一 |
| 是否内嵌 CPython (pyo3) | **否** | CUDA 上下文、multiprocessing、信号处理与 Rust 运行时冲突，故障域不清 |
| 引擎数量 | **仅 3 个：vLLM、SGLang、Transformers** | 不做纯 Rust 推理栈（candle/ort），不引入 llama.cpp；避免多套栈重复建设 |
| 新增引擎 | 新增 1 个 crate + 1 个 `engine-<name>.toml` 清单，不改动其他层 | 开闭原则，为后续引擎预留 |
| 控制面/数据面 | 分离：控制面管模型与进程，数据面只转发请求 | 引擎故障不影响注册表与调度决策 |

一句话：**Rust 做"操作系统 + 智能路由"，Python 引擎做"算力"，中间只有一套协议。**

## 2. 分层视图

```
┌──────────────────────────────────────────────────────────────┐
│ L6 接口层   REST(OpenAI 兼容) │ WS(SSE 流式/音频) │ gRPC(集群) │ CLI │
├──────────────────────────────────────────────────────────────┤
│ L5 网关层   鉴权/租户配额 → 协议转换 → 限流 → 流式编排 → 用量统计    │
├──────────────────────────────────────────────────────────────┤
│ L4 路由层   (model, task) → 引擎候选 → 打分选引擎 → 重试/故障转移   │
├──────────────────────────────────────────────────────────────┤
│ L3 调度层   准入控制 · 优先级队列 · 并发上限 · 取消传播 · 背压      │
├──────────────────────────────────────────────────────────────┤
│ L2 引擎层   Engine trait ← vLLM │ SGLang │ Transformers(slc) │
│              ↑ 进程监督(supervisor)：启动/就绪/健康/重启/日志/优雅退出  │
├──────────────────────────────────────────────────────────────┤
│ L1 资源层   设备发现(NVML) · 显存预算 · 端口/环境分配 · 亲和性       │
├──────────────────────────────────────────────────────────────┤
│ L0 基础层   配置 · 模型注册表 · 存储/下载 · 可观测性 · 错误模型      │
└──────────────────────────────────────────────────────────────┘
```

## 3. 领域模型（core types）

```rust
ModelId      // 稳定 ID，如 "tts/index-tts2.0"、"dm/laya-7b"
TaskKind     // Chat | Completion | Embed | Rerank | Tts | Transcribe | Decision
EngineKind   // Vllm | Sglang | Transformers
EngineCaps   // { tasks: set<TaskKind>, streaming, max_ctx, supports_tp, ttft_hint }
ModelCard    // 来自 models/<path>/model.toml：参数量、dtype、显存需求、引擎候选、sidecar 入口
EngineRef    // 引擎实例句柄：base_url / socket、pid、gpu_ids、状态机
EngineState  // Starting | Ready | Unready(reason) | Draining | Dead | Restarting
```

核心 trait（`studio-engine`）：

```rust
#[async_trait]
pub trait Engine: Send + Sync + 'static {
    fn kind(&self) -> EngineKind;
    fn caps(&self) -> &EngineCaps;
    fn state(&self) -> EngineState;
    async fn ready(&self, timeout: Duration) -> Result<()>;
    async fn health(&self) -> Health;
    async fn infer(&self, req: InferenceRequest, sink: StreamSink) -> Result<()>;
    async fn cancel(&self, req_id: ReqId) -> Result<()>;
    async fn shutdown(&self, grace: Duration) -> Result<()>;
}
```

- `InferenceRequest` 统一抽象：`messages` / `prompt` / `audio_prompt` / `sampling` / `stream` / `priority`。
- 每个 `EngineKind` 一个 adapter crate，只负责"协议翻译 + 生命周期"，不含业务逻辑。
- 新增引擎 = 新增 1 个 crate + 1 个 `engine-<name>.toml` 清单，不改动其他层（开闭原则）。

## 4. 引擎能力矩阵

| 引擎 | 形态 | 协议 | LLM | TTS | Embed | 说明 |
| --- | --- | --- | --- | --- | --- | --- |
| vLLM | 子进程 | HTTP（OpenAI 全兼容） | ✅ | ⚠️ `/v1/audio/speech`（部分模型） | ✅ | 主力 LLM 引擎，支持 TP/PP |
| SGLang | 子进程 | HTTP（OpenAI + `/generate`） | ✅ | ⚠️ 需 sidecar | ✅ | RadixAttention 前缀缓存强 |
| Transformers | sidecar | HTTP/WS | ✅ | ✅ | ✅ | 无原生服务端，sidecar 托管；IndexTTS 走这条通道 |

> **引擎只有 3 个：vLLM、SGLang、Transformers。**
> 不做纯 Rust 推理栈（candle/ort），不引入 llama.cpp。
> 引擎 ≠ 模型族：IndexTTS、Laya、Kev 是**模型**，由上述 3 个引擎加载执行。
> IndexTTS 属于 TTS 模型族，通过 Transformers 引擎（sidecar）加载，
> 由 `model.toml` 的 `adapter` 字段选择对应版本的适配器（`indextts_v10/v15/v20/v25`）。

⚠️ = 视模型与版本而定，由 `model.toml` 显式声明。

## 5. 请求生命周期

```
client
  │ POST /v1/chat/completions (stream=true)
  ▼
studio-gateway ──auth/quota──▶ studio-scheduler (准入: 队列/并发/租户配额)
  │
  ▼ studio-router: 解析 ModelId → 候选引擎列表 → 打分（就绪度/显存余量/队列深度/亲和）
  │
  ▼ studio-engine::infer → Engine 实现
  │     ├─ Vllm/Sglang: HTTP 透传 + SSE 逐块转发
  │     └─ Transformers(sidecar): WS/HTTP 长连接，PCM 块按 20~40ms 推送
  ▼
流式回写（SSE / WS）→ 用量统计 → 指标上报
  │  客户端断开 ⇒ cancel 向下游传播（引擎侧 abort）
  ▼
engine 故障 ⇒ router 重试其他候选（幂等请求）或返回 503 + Retry-After
```

TTS 关键指标：`RTF`（实时率）、`first_chunk_ms`、`underrun` 次数。

## 6. 目录与部署形态

- 单节点：`studio serve`（1 个 Rust 进程 + N 个引擎子进程）。
- 单机多卡：`--gpus 0,1` 与引擎 `tensor_parallel` 对齐，`studio-gpu` 做显存预算与设备钉扎。
- 集群：控制节点跑 `studio-server`，工作节点跑 `studio-agent`，gRPC 心跳注册；路由按节点标签（GPU 型号/显存/区域）转发。
- 容器：Docker 文件位于**仓库根目录**（`Dockerfile` / `Dockerfile.sidecar` / `docker-compose.yml` / `docker-compose.gpu.yml` / `.dockerignore`）；
  `studio` 与各引擎分镜像/分 service，通过 `base_url` 连接，模型目录只读挂载；
  GPU 场景需 nvidia-container-toolkit 与 `--ipc=host`（或 `shm_size`）以支持 NCCL 共享内存。

## 7. 可靠性

| 故障 | 处理 |
| --- | --- |
| 引擎启动超时 | 分阶段超时（拉权重/编译图/加载完成），超时给出明确原因 |
| 引擎崩溃 | 指数退避重启；连续 N 次失败 → 标记 `Unready` 并停止引流 |
| OOM | 启动前显存准入检查；运行中捕获 OOM 日志关键字 → 主动隔离 |
| 端口冲突 | 端口池分配，不硬编码 |
| 请求挂死 | 双层超时（首包 + 空闲），取消传播到子进程 |
| 僵尸子进程 | supervisor 统一回收，`waitpid` + 进程组 kill |

## 8. 安全

- API Key / JWT，租户级配额（QPS、并发、日 token）。
- 子进程参数为结构化数组，**不拼 shell 字符串**；禁止 `shell=True`。
- `trust_remote_code` 默认 `false`（IndexTTS 等需显式白名单开启）。
- 模型目录只读挂载；模型文件加载后校验 sha256（可选强制）。
- 引擎日志脱敏（prompt 默认不落盘，可采样且限时）。

## 9. 可观测性

- `tracing` + OTLP；`tracing-opentelemetry` 上报 trace。
- Prometheus `/metrics`：`queue_depth`、`inflight`、`ttft_ms`、`tpot_ms`、`rtf`、`gpu_mem_used`、`engine_uptime`、`restart_total`。
- 每个引擎子进程 stdout/stderr 落盘滚动日志，并带 `[engine=vllm model=...]` 前缀。
- `studio doctor`：检查 Rust 版本、各引擎可执行文件、CUDA 驱动、模型完整性、端口可用性。

## 10. 配置示例（`config/default.toml`）

```toml
[models]
root = "models"
auto_discover = true
checksum_policy = "warn"          # warn | enforce

[engines.vllm]
enabled = true
executable = "vllm"
args = ["serve", "{model_path}", "--port", "{port}", "--gpu-memory-utilization", "0.85"]
health_path = "/health"
startup_timeout_s = 900
max_instances = 1

[engines.sglang]
enabled = true
module = "sglang.launch_server"

[sidecars.transformers]
venv = "envs/transformers"
entry = "sidecars/studio_sidecar.serve.transformers"
adapters = { indextts_v25 = "studio_sidecar.adapters.indextts_v25" }

[gateway]
listen = "0.0.0.0:17283"
default_engine_policy = "prefer_local"

[scheduler]
max_concurrency_per_model = 8
queue_max_depth = 256
```

## 11. 关键风险

1. **引擎依赖地狱**：vLLM / SGLang / Transformers 需要不同 torch 版本 → 每个引擎独立 venv（`envs/<engine>`），supervisor 用该 venv 的解释器启动。
2. **TTS 流式协议不统一**：IndexTTS 各版本 API 差异大（2.0 情感控制、2.5 版本接口变化）→ 在 transformers sidecar 内做版本适配层，`model.toml` 用 `adapter` 指定适配器，`api_version` 声明协议版本。
3. **显存超卖**：多引擎共存时 OOM → 显存预算表 + 准入拒绝（比事后崩溃好）。
4. **模型体积**：TB 级权重不进 Git，`models/` 全目录 gitignore，只提交 `model.toml` 清单。

详见 `docs/ROADMAP.md`、`docs/ADR/0001-engine-integration.md`。