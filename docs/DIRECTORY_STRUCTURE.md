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
├── .github/workflows/                # ci.yml(fmt clippy test) release.yml(docker/binary)
│
├── crates/                           # ── Rust 主体（workspace members）──────────────
├── bins/                             # 可执行入口（薄壳，只做参数装配）
├── protos/                           # gRPC 定义（集群内部通信）
├── sidecars/                         # ── Python sidecar（有服务端能力缺口的引擎）────
├── sdk/                              # 外部客户端 SDK（薄封装）
├── models/                           # ── 模型仓（gitignore，见 §4）─────────────────
├── envs/                             # ── 每引擎独立 venv（gitignore）───────────────
├── config/                           # 配置模板与引擎清单
├── deploy/                           # 容器 / 编排
├── scripts/                          # 运维与开发脚本
├── docs/                             # 设计文档（本目录）
├── tests/                            # 集成/端到端测试与 fixtures
├── benches/                          # criterion 基准（热路径）
└── examples/                         # 可运行的示例请求
```

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
├── engine-vllm/            # vLLM 适配器（OpenAI HTTP 透传 + 启动参数拼装）
├── engine-sglang/          # SGLang 适配器（OpenAI + /generate）
├── engine-transformers/    # Transformers sidecar 适配器（HTTP/WS）
├── engine-indextts/        # IndexTTS sidecar 适配器（PCM 流式）
├── engine-native/          # candle / ort / llama.cpp 进程内后端
│
├── studio-router/          # (model, task) → 引擎候选 → 打分 → 重试/故障转移
├── studio-scheduler/       # 准入控制、优先级队列、并发上限、取消传播、背压
├── studio-gpu/             # NVML 设备发现、显存预算、设备钉扎、nvidia-smi 兜底
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
                                          ┌──────────────────────────┤
                                    engine-vllm  engine-sglang  engine-transformers
                                    engine-indextts  engine-native
                                                                     │
                                          studio-core ◀── 所有 crate（只被依赖）
```

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
│   │   ├── transformers_llm.py # text-generation / embeddings
│   │   ├── indextts_v1.py      # index-tts 1.0
│   │   ├── indextts_v15.py     # index-tts 1.5
│   │   ├── indextts_v20.py     # index-tts 2.0（情感控制参数）
│   │   ├── indextts_v25.py     # index-tts 2.5
│   │   └── dm_laya.py          # 决策模型 Laya 推理封装
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
family = "indextts"
task = "tts"
params_b = 0.5
dtype = "bfloat16"
min_vram_gb = 6
recommended = { engine = "indextts", api_version = "v20" }
engines = ["indextts"]                 # 可降级顺序
streaming = true
[limits]
max_text_chars = 5000
sample_rate = 22050
[checksum]
enabled = true
policy = "warn"                        # warn | enforce
```

`models/registry.toml` 示例：

```toml
[[models]]
id = "tts/index-tts1.5"
path = "tts/index-tts1.5"
enabled = true
engines = ["indextts"]
default_engine = "indextts"

[[models]]
id = "dm/laya"
path = "DM/laya"
enabled = false                        # 按需启用
engines = ["native", "vllm"]           # 优先进程内，其次 vLLM
```

## 5. `envs/`（每引擎独立 Python 环境）

```
envs/
├── vllm/          # python3.11 + vllm（CUDA 12.x）
├── sglang/        # python3.11 + sglang
├── transformers/  # python3.11 + transformers + torch
└── indextts/      # python3.11 + indextts 依赖（版本按模型区分）
```

由 `scripts/setup-envs.sh`（或 `xtask envs`）创建，`.gitignore` 排除。
Rust 侧通过 `config` 中 `venv = "envs/vllm"` 解析解释器路径，**不假设 PATH 上有 vllm**。

## 6. `config/`

```
config/
├── default.toml            # 基线配置（提交）
├── engines/
│   ├── vllm.toml           # 启动命令模板、健康检查、超时、并发
│   ├── sglang.toml
│   ├── transformers.toml
│   ├── indextts.toml       # 各 TTS 版本分别一份
│   └── native.toml
├── models/                 # 可选：按模型的覆盖配置
│   └── tts-index-tts2.5.toml
└── cluster.toml            # 集群拓扑：控制节点/工作节点
```

启动参数支持占位符：`{model_path}` `{port}` `{gpu_ids}` `{tensor_parallel}` `{venv}`。

## 7. `deploy/`、`scripts/`、`tests/`

```
deploy/
├── docker/
│   ├── Dockerfile           # Rust 构建阶段 + 运行时阶段（含 sidecar 依赖）
│   ├── Dockerfile.sidecar   # 单独的 Python sidecar 镜像
│   └── entrypoint.sh
├── compose/
│   └── docker-compose.yml   # 单机：studio + vllm + sidecar
└── k8s/
    ├── helm/                # Chart：Deployment/Service/HPA/ConfigMap
    └── manifests/

scripts/
├── setup-envs.sh            # 创建各引擎 venv
├── fetch-models.sh          # 按 registry.toml 拉取权重（HF/ModelScope）
├── dev-up.sh                # 本地起 studio + 一个假引擎
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