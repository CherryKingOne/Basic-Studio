# Basic-Studio

Rust 编写的模型加载与推理统一控制面：用一套 Rust 进程纳管 vLLM / SGLang / Transformers / 原生 Rust 等多种加速引擎，
统一暴露 OpenAI 兼容接口，支持 LLM、TTS（IndexTTS 系列）流式推理与决策模型（DM：Laya、Kev）推理。

## 当前状态

设计阶段，尚未开始编码。目录结构以 `docs/DIRECTORY_STRUCTURE.md` 为准。

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