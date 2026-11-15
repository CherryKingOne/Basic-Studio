# Basic-Studio

Rust 编写的模型加载与推理统一控制面：用一套 Rust 进程纳管 vLLM / SGLang / Transformers 三种推理引擎，
统一暴露 OpenAI 兼容接口，支持 LLM、TTS（IndexTTS 系列）流式推理与决策模型（DM：Laya、Kev）推理。

## 当前状态

**M0 骨架已完成**：Rust workspace 可编译，`cargo test --workspace` 24 个测试通过，`cargo clippy` 无告警。

已实现：

- `studio-core`：模型/引擎/任务领域类型与错误模型
- `studio-config`：分层配置（`config/default.toml` 基线 + 深合并覆盖），未知引擎名直接报错
- `studio-registry`：扫描 `models/`，解析 `model.toml`，重复 id 检测
- `studio-procsup`：子进程监督（spawn / HTTP 与 TCP 就绪探测 / SIGTERM→SIGKILL / 日志落盘 / 端口池）
- `studio-engine`：`Engine` trait 与实例注册表
- `crates/engines/{vllm,sglang,transformers}`：三个引擎适配器（含外部托管模式，直接连 `base_url`）
- `studio-router`：`(模型, 任务)` → 引擎候选排序
- `studio-cli`：`studio serve|models ls|models inspect|models route|engines ls|doctor`

尚未开始：HTTP 网关与流式转发、调度与鉴权、Python sidecar、Docker 镜像（M1~M3）。

## 构建与运行

```bash
cargo build -p studio-cli
./target/debug/studio doctor
./target/debug/studio models ls
./target/debug/studio models route tts
./target/debug/studio serve            # 只打印启动计划
./target/debug/studio serve --start    # 真正拉起引擎（需要 GPU 与权重）
```

## 文档

- [docs/README.md](docs/README.md) — 文档索引
- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) — 总体架构与设计结论
- [docs/DIRECTORY_STRUCTURE.md](docs/DIRECTORY_STRUCTURE.md) — 目录结构树
- [docs/ROADMAP.md](docs/ROADMAP.md) — 里程碑与实施计划
- [docs/API.md](docs/API.md) — 接口清单
- [docs/ADR/0001-engine-integration.md](docs/ADR/0001-engine-integration.md) — 引擎集成策略决策

## 核心思路

Rust 负责注册表、调度、资源准入、路由、进程生命周期与对外 API；
推理引擎共 3 个——vLLM、SGLang（均为子进程，走原生 HTTP）、Transformers（无服务端，由仓库自带、
版本锁定的 Python sidecar 补上统一协议）。

IndexTTS（1.0/1.5/2.0/2.5）、Laya、Kev 都是**模型**，不是引擎——它们由上述 3 个引擎加载执行。

## 快速开始（规划）

```bash
./scripts/setup-envs.sh      # 创建各引擎 venv
./scripts/fetch-models.sh    # 按 models/registry.toml 拉取权重
studio doctor                # 环境自检
studio serve --config config/default.toml
```