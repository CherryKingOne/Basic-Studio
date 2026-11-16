# 目录结构规划

> 目标：Rust workspace + Python sidecar + 模型仓分离。
> 约定：`models/` 与 `envs/` **不进 Git**（体积大），只提交清单与配置。

## 1. 顶层

```
app/                                  # 仓库根 = Basic-Studio
├── Cargo.toml                        # workspace 定义 + 统一依赖版本（workspace.dependencies）
├── Cargo.lock                        # 提交，保证可复现构建
├── rust-toolchain.toml               # 固定 toolchain（stable + rustfmt/clippy 组件）
├── deny.toml                         # cargo-deny：license/duplicate/advisory
├── rustfmt.toml / clippy.toml
├── justfile                          # 常用命令入口（just dev / just check / just test）
├── README.md
├── AGENTS.md
├── .gitignore
├── .dockerignore
├── Dockerfile                          # ★ 主镜像：Rust 构建阶段 + 运行时（含 sidecar）
├── Dockerfile.sidecar                  # ★ sidecar 独立镜像（Python 推理引擎）
├── docker-compose.yml                  # ★ 单机编排：studio + vllm + sglang + sidecar
├── docker-compose.gpu.yml              # GPU 叠加层（nvidia runtime + device 映射）
├── .github/workflows/                  # ci.yml(fmt clippy test) release.yml(image/binary)
│
├── crates/                             # ── Rust 主体（workspace members；引擎在 crates/engines/）
├── protos/                             # gRPC 定义（集群内部通信）
├── sidecars/                           # ── Python sidecar（有服务端能力缺口的引擎）────
├── sdk/                                # ★ 官方 SDK：python / rust / nodejs / java / go（见 sdk/README.md）
├── models/                             # ── 模型仓（gitignore，见 §4）─────────────────
├── envs/                               # ── 引擎运行时环境（venv，gitignore，见 §5）────
├── config/                             # 配置基线 default.toml（见 §6）
├── deploy/k8s/                         # Kubernetes（Helm chart / manifests）
├── scripts/                            # 运维与开发脚本
├── docs/                               # 设计文档（本目录）
├── tests/                              # 集成/端到端测试与 fixtures
├── benches/                            # criterion 基准（热路径）
└── examples/                           # 可运行的示例请求
```

> Docker 相关文件一律放**仓库根目录**：`Dockerfile`、`Dockerfile.sidecar`、`docker-compose.yml`、
> `docker-compose.gpu.yml`、`.dockerignore`。不放 `deploy/docker/`。

## 2. `crates/`（Rust workspace，按层分组）

```
crates/
├── studio-core/            # 领域类型：ModelId/TaskKind/EngineKind/EngineCaps/ModelCard/Error
│   └── src/{lib.rs, model.rs, task.rs, engine.rs, request.rs, error.rs}
│
├── studio-config/          # figment 分层配置：默认 → 文件 → 环境变量 → CLI；notify 热重载
│   └── src/{lib.rs, loader.rs, schema.rs, watch.rs}
│
├── studio-registry/        # 模型注册表：扫描 models/、解析 model.toml、校验、models.lock
│   └── src/{lib.rs, scan.rs, manifest.rs, integrity.rs, lock.rs, index.rs}
│
├── studio-procsup/         # 进程监督：spawn/就绪探测/健康/重启/优雅退出/日志捕获
│   └── src/{lib.rs, child.rs, health.rs, restart.rs, log.rs, port.rs}
│
├── studio-engine/          # Engine trait + 引擎注册表 + 通用 trait 实现
│   └── src/{lib.rs, traits.rs, registry.rs, sink.rs}
│
├── engines/                # ── 推理引擎适配器（每个引擎一个 crate）──────────────
│   ├── vllm/               # 包名 engine-vllm：OpenAI HTTP 透传 + 启动参数拼装
│   ├── sglang/             # 包名 engine-sglang：OpenAI + /generate
│   ├── transformers/       # 包名 engine-transformers：sidecar（HTTP/WS），默认引擎，IndexTTS 走此通道
│   └── llamacpp/           # 包名 engine-llamacpp：llama-server（GGUF，CPU 友好）
│                           # 目录名与包名不同，Cargo.toml 中显式声明 [package] name
│
├── studio-router/          # (model, task) → 引擎候选 → 打分 → 重试/故障转移
├── studio-scheduler/       # 准入控制、优先级队列、并发上限、取消传播、背压
├── studio-device/          # ★ 已实现：CPU/NVIDIA/AMD/NPU/TPU 探测与多卡分配
├── studio-gpu/             # 规划：NVML 显存预算与设备钉扎（现由 studio-device 覆盖发现部分）
├── studio-storage/         # 权重下载（HF/ModelScope）、分片校验、blob 缓存、清理策略
├── studio-media/           # 音频解码/编码/重采样、PCM 分块、ffmpeg 桥接（TTS 专用）
├── studio-gateway/         # axum 路由：OpenAI 兼容 + 原生 REST + SSE/WS 流式
├── studio-grpc/            # tonic：节点注册、心跳、跨节点转发
├── studio-auth/            # API Key/JWT、租户、配额
├── studio-obs/             # tracing + metrics + 日志滚动
├── studio-plugin/          # 引擎插件契约（子命令式插件；预留 wasmtime）
├── studio-testkit/         # 测试工具：FakeEngine、模型 fixtures、端口分配
│
├── studio-cli/             # bin: studio   （serve/models/pull/bench/doctor/...）
└── studio-server/          # bin: studio-server（无 CLI 的服务端入口，容器用）
```

依赖方向（严格单向，禁止反向）：

```
studio-cli ─┐
studio-server ─┴─▶ studio-gateway ─▶ studio-scheduler ─▶ studio-router ─▶ studio-engine
                                                                     │
                                          ┌──────────────────────┤
                              engines/vllm  engines/sglang  engines/transformers
                                                                     │
                                          studio-core ◀── 所有 crate（只被依赖）
```

> 已实现：`studio-core`、`studio-config`、`studio-registry`、`studio-procsup`、`studio-engine`、`studio-device`、
> 以及 `sdk/rust`（workspace 成员）与 `sdk/{python,nodejs,java,go}`、
> `engines/{vllm,sglang,transformers}`、`studio-router`、`studio-scheduler`、`studio-auth`、
> `studio-obs`、`studio-media`、`studio-gateway`、`studio-cli`，以及 `sidecars/`。
> 仍为规划：`studio-gpu`（NVML 显存预算）、`studio-storage`（权重下载）、`studio-grpc`（多节点）、
> `studio-plugin`（插件）、`studio-testkit`（已合并进各 crate 的 dev-deps 与 `tests/`）。

## 2.1 推理引擎到底放在哪（重点）

一个引擎由**三部分**组成，缺一不可：

| # | 位置 | 内容 |
| --- | --- | --- |
| ① 适配器 | `crates/engines/<name>/` | Rust 代码：启动参数、健康检查、协议翻译、流式转发 |
| ② 运行时 | `envs/<name>/`（venv）或 `sidecars/`（Python 包） | 引擎真正的代码与依赖（vLLM / SGLang / transformers） |
| ③ 清单 | `config/default.toml` 的 `[engines.<name>]` 段 | 启动命令模板、超时、并发、健康检查路径 |

具体对应：

| 引擎 | 适配器 crate | 运行时位置 | 清单文件 |
| --- | --- | --- | --- |
| vLLM | `crates/engines/vllm`（包 `engine-vllm`） | `envs/vllm`（pip 安装的 vllm 命令） | `[engines.vllm]` |
| SGLang | `crates/engines/sglang`（包 `engine-sglang`） | `envs/sglang`（`python -m sglang.launch_server`） | `[engines.sglang]` |
| Transformers | `crates/engines/transformers`（包 `engine-transformers`） | `sidecars/studio_sidecar/`，跑在 `envs/transformers` | `[engines.transformers]`（默认） |
| llama.cpp | `crates/engines/llamacpp`（包 `engine-llamacpp`） | `llama-server`（GGUF 权重） | `[engines.llamacpp]` |

> **引擎共 4 个（默认 Transformers）。** IndexTTS / Laya / Kev 是**模型**，不是引擎：
> 它们由这些引擎加载执行。IndexTTS 属于 TTS 模型族，通过 `engine-transformers` 的 sidecar 加载，
> 具体用哪个版本适配器由 `models/tts/<model>/model.toml` 的 `adapter` 字段决定
> （如 `adapter = "indextts_v25"` → `sidecars/studio_sidecar/adapters/indextts_v25.py`）。

一句话：**Rust 里是"适配器"，`envs/` 与 `sidecars/`（以及 `llama-server`）里是"引擎本体"，`config/default.toml` 的 `[engines.*]` 段是"怎么启动"，`models/` 里是"跑什么模型"。**

## 3. `sidecars/`（Python 侧）

```
sidecars/
├── pyproject.toml              # uv 管理；依赖用 optional-dependencies 分引擎组
├── uv.lock
├── README.md
├── studio_sidecar/             # 统一包名
│   ├── __init__.py
│   ├── __main__.py             # python -m studio_sidecar serve <engine>
│   ├── protocol.py             # 与 Rust 共享的请求/响应 schema（pydantic）
│   ├── server.py               # FastAPI + uvicorn 统一入口，路由 /health /infer /stream
│   ├── adapters/
│   │   ├── transformers_llm.py # Transformers：text-generation / embeddings
│   │   ├── indextts_v1.py      # ┐
│   │   ├── indextts_v15.py     # │ IndexTTS 模型族的版本适配器（不是引擎，
│   │   ├── indextts_v20.py     # │ 由 model.toml 的 adapter 字段选择）
│   │   ├── indextts_v25.py     # ┘
│   │   └── dm_laya.py          # 决策模型 Laya 的 sidecar 封装（可选）
│   └── audio/
│       ├── pcm.py              # 分块 PCM / 重采样 / 削波保护
│       └── ffmpeg.py           # 需要 mp3/opus 时的转码
└── tests/
    ├── test_protocol_contract.py   # 与 Rust 端共享 fixtures 的契约测试
    └── test_indextts_stream.py
```

约定：
- sidecar 只暴露 **一个** 统一协议（`/health`、`/infer`、`/infer/stream`），引擎差异在 `adapters/` 内消化。
- sidecar 不做鉴权、不做调度，鉴权与调度在 Rust 侧（避免双重实现）。
- sidecar 进程由 Rust supervisor 启动，stdout/stderr 由 Rust 捕获。

## 4. `models/`（模型仓）

```
models/
├── README.md                 # 目录约定说明（提交）
├── registry.toml             # 全局清单：模型→路径→默认引擎（提交，可人工编辑）
├── models.lock               # 解析后的锁定文件：路径+sha256+大小（提交）
│
├── tts/
│   ├── index-tts/            # 1.0
│   ├── index-tts1.5/
│   ├── index-tts2.0/
│   └── index-tts2.5/
│
├── llm/                      # Large Language Models
│   ├── gemma/
│   │   └── gemma-4-12B/
│   └── qwen/
│       └── Qwen/
│           └── Qwen3.8-27B/
│
└── DM/                       # Decision Models
    ├── laya/
    └── kev/

└── .cache/                   # 运行期产物：编译缓存、临时权重（gitignore）
```

**每个模型目录自包含**：权重、tokenizer、vocoder、音色等全部放在该模型自己的目录内，
不存在跨模型的公共目录。好处是版本与目录一一对应，升级/回滚只需替换单个目录，模型可整体迁移或删除。

单个模型目录（以 `models/tts/index-tts2.0/` 为例，所有依赖资源都在本目录内）：

```
models/tts/index-tts2.0/
├── model.toml                # ★ studio 清单：引擎/资源/能力/入口（提交）
├── config.json               # 上游模型自带配置（提交）
├── *.safetensors             # 权重（gitignore）
├── tokenizer/                # 分词器 / 文本正则化资源
├── vocoder/                  # 声码器权重
├── speakers/                 # 内置音色 embedding
├── voices/                   # 用户音色参考（gitignore）
└── CHECKSUMS.sha256          # 完整性校验（提交）
```

`model.toml` 示例：

```toml
id = "tts/index-tts2.0"
family = "indextts"                      # 模型族（不是引擎）
task = "tts"
params_b = 0.5
dtype = "bfloat16"
min_vram_gb = 6
engine = "transformers"                  # ★ 由哪个引擎加载
adapter = "indextts_v20"                 # ★ sidecar 内用哪个版本适配器
api_version = "v20"                      # sidecar 协议版本
engines = ["transformers"]               # 可降级顺序
streaming = true
[limits]
max_text_chars = 5000
sample_rate = 22050
[checksum]
enabled = true
policy = "warn"                        # warn | enforce
```

LLM 模型（`models/llm/gemma/gemma-4-12B/model.toml`）示例：

```toml
id = "llm/gemma/gemma-4-12B"
family = "gemma"
task = "chat"
params_b = 12
dtype = "bfloat16"
min_vram_gb = 24
engine = "vllm"
engines = ["vllm", "transformers"]       # 可降级顺序
streaming = true
[limits]
max_concurrency = 4
```

`models/registry.toml` 示例：

```toml
[[models]]
id = "tts/index-tts1.5"
path = "tts/index-tts1.5"
enabled = true
engine = "transformers"
adapter = "indextts_v15"
engines = ["transformers"]
default_engine = "transformers"

[[models]]
id = "dm/laya"
path = "DM/laya"
enabled = false                        # 按需启用
engines = ["vllm"]                     # 可降级顺序；DM 模型按体积选择 vLLM 或 transformers
```

## 5. `envs/`（每引擎独立 Python 环境）

```
envs/
├── vllm/          # python3.11 + vllm（CUDA 12.x）
├── sglang/        # python3.11 + sglang
└── transformers/  # python3.11 + transformers + torch（IndexTTS 等模型也在此环境）
```

> `envs/` 是**引擎运行环境**，只有上面三个（每个引擎一个，避免 torch 版本冲突）。
> 模型不需要自己的环境；IndexTTS 各版本共用 `envs/transformers`，版本差异由 `adapters/indextts_v*.py` 处理。
> 若某模型依赖与其他模型冲突，再单独加 `envs/<model>`，但这属于例外，不是常态。

由 `scripts/setup-envs.sh`（或 `xtask envs`）创建，`.gitignore` 排除。
Rust 侧通过 `config` 中 `venv = "envs/vllm"` 解析解释器路径，**不假设 PATH 上有 vllm**。

## 6. `config/`

```
config/
└── default.toml            # ★ 唯一默认来源：模型仓、[engines.*]、sidecar、gateway、scheduler
```

- 引擎启动参数写在 `[engines.<name>]` 段（`command` / `args` / `health_path` / `port` / `startup_timeout_s`）。
- 覆盖方式（优先级从低到高）：`config/default.toml` → `--config local.toml`（TOML 深合并）→ 环境变量 → CLI。
- 启动参数支持占位符：`{model_path}` `{port}` `{host}` `{venv}` `{adapter}`。

## 7. Docker 部署（根目录）、`scripts/`、`tests/`

Docker 相关文件**全部位于仓库根目录**：

```
app/
├── Dockerfile                # 主镜像：Rust 多阶段构建 → 精简运行时
├── Dockerfile.sidecar        # sidecar 镜像：Python + 各引擎依赖
├── docker-compose.yml        # 单机编排（CPU/通用）
├── docker-compose.gpu.yml    # GPU 叠加层：nvidia runtime、device/driver 挂载、shm
└── .dockerignore             # 排除 target/、models/、envs/、.git 等
```

镜像划分与构建方式：

| 文件 | 基础镜像 | 内容 |
| --- | --- | --- |
| `Dockerfile` | `rust:1.x` 构建 → `ubuntu:22.04` 或 `nvidia/cuda:12.x-runtime` | `studio` / `studio-server` 二进制 + 配置 + sidecar 代码 + ffmpeg |
| `Dockerfile.sidecar` | `nvidia/cuda:12.x-devel`（需编译）或 `python:3.11-slim` | transformers + IndexTTS 依赖（sidecar），按 build-arg 选择模型组 |
| `docker-compose.yml` | — | `studio` + `vllm` + `sglang` + `sidecar`，全部内部端口 18596，共享 `models/` 只读卷 |
| `docker-compose.gpu.yml` | — | 追加 `deploy.resources.reservations.devices`、`/dev/shm`、`ulimits` |

多引擎共存策略（关键）：

- 每个引擎一个 Compose service，独立镜像/独立 venv，**互不共享 torch 版本**。
- `studio` 通过 `[engines.*].base_url`（如 `http://vllm:18596`）连接各引擎。
- 容器内运行时统一用 `envs/` 挂载卷或镜像内 venv；`studio` 不假设引擎在同一容器。

GPU 相关注意事项：

- 需要 `nvidia-container-toolkit`，Compose 里 `runtime: nvidia` 或 `deploy.resources.reservations.devices`。
- `--ipc=host`（或 `shm_size: 16gb`）：vLLM/SGLang 的 NCCL 多进程通信需要大共享内存。
- `--ulimit memlock=-1`、`--ulimit stack=67108864`。
- 首次启动拉权重较慢：给足 `healthcheck.start_period` 与 `startup_timeout_s`。

`scripts/`、`tests/`：

```
scripts/
├── setup-envs.sh            # 创建各引擎 venv
├── fetch-models.sh          # 按 registry.toml 拉取权重（HF/ModelScope）
├── dev-up.sh                # 本地起 studio + 一个假引擎
├── docker-up.sh             # docker compose --profile gpu up -d
└── bench-tts.sh             # 批量 TTS 压测，输出 RTF

tests/
├── unit/                    # 与 crates 内 #[cfg(test)] 互补
├── integration/             # 起真实 vLLM（CI 可选，标记 ignored）
├── e2e/                     # 端到端：请求 → 音频 → 校验时长/采样率
└── fixtures/
    ├── audio/               # 短参考音频、期望输出
    └── contracts/           # Rust↔sidecar 共享 JSON 契约
```

## 8. `.gitignore` 要点

```gitignore
/target
/models/**                 # 例外：!models/**/model.toml 等清单
/envs
/sidecars/.venv
*.safetensors
*.bin
*.gguf
*.pt
*.ckpt
/models/.cache
*.wav
*.mp3
```