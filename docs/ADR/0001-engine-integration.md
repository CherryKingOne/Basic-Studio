# ADR 0001：引擎集成策略

- 状态：已接受
- 日期：2026-11-15
- 相关：`docs/ARCHITECTURE.md` §1、§4

## 背景

需要在一个 Rust 项目里统一加载并调度 vLLM、SGLang、Transformers 以及后续更多加速引擎，
同时承载 TTS（IndexTTS 系列模型）与决策模型（DM：Laya、Kev 等）。

约束：
- 这些引擎绝大多数是 Python 实现，各自绑定特定 CUDA / PyTorch / 驱动版本。
- TTS（IndexTTS）没有官方 HTTP 服务端，只有 Python 推理脚本。
- 需要多引擎共存于同一台机器，且互不影响。

## 候选方案

### A. 内嵌 CPython（pyo3 / pyo3-asyncio）

- 优点：无进程间通信，调用链短，可共享内存。
- 缺点：
  - CUDA 上下文与 Rust 线程/tokio 运行时交互复杂，易死锁。
  - 各引擎的 torch 版本冲突无法在单解释器内解决。
  - 子进程崩溃会拖垮主进程。
  - 异步与 GIL 的映射需要大量胶水代码。

### B. 纯 Rust 重写推理栈（candle / ort）

- 优点：单二进制、无 Python、资源占用低、启动快。
- 缺点：
  - vLLM 的 PagedAttention、SGLang 的 RadixAttention、IndexTTS 的完整采样链无法短期复刻。
  - 模型覆盖度不足，长尾模型无法支持。
  - 与"支持更多加速引擎"的目标冲突（只支持自己写的那一种）。

### C. 进程编排 + 协议客户端（选定）

- Rust 负责：注册表、调度、资源准入、路由、生命周期管理、可观测性、对外 API。
- 每个引擎作为独立子进程/服务存在，Rust 通过其原生 HTTP 接口通信。
- 无服务端的引擎（Transformers）由**仓库自带、版本锁定**的 Python sidecar 补上统一 HTTP/WS 接口。
  IndexTTS 等模型通过该 sidecar 加载，由 `model.toml` 的 `adapter` 选择版本适配器。
- **不做**纯 Rust 推理栈：不引入 candle / ort，不新增 `engine-native`。引擎为 4 个（含 llama.cpp）。

## 决策

采用方案 C，**引擎集合为 vLLM、SGLang、Transformers（默认）、llama.cpp**。理由：

1. 不重写轮子，第一时间拿到 vLLM/SGLang 的全部优化。
2. 故障域隔离：单个引擎崩溃不影响控制面与其他引擎。
3. 依赖隔离：每引擎独立 venv，torch 版本互不干扰。
4. 扩展性：新增引擎只需实现 `Engine` trait + 一个清单文件（但当前不扩展）。
5. 代价（进程间序列化开销、内存翻倍）通过"按需拉起 + 显存准入 + 单模型独占模式"缓解。
6. 自研推理栈（方案 B）维护成本极高、覆盖度不足，明确排除。

## 后果

- 必须实现健壮的 `studio-procsup`（进程监督）：启动、就绪探测、健康检查、重启、优雅退出、日志捕获。
- 需要 venv 管理约定：`envs/<engine>`，由 `xtask` 或 `scripts/setup-envs.sh` 创建。
- sidecar 是维护成本点：必须写契约测试（protocol contract tests）防止协议漂移。
- 内嵌 CPython（方案 A）保留为未来"极低延迟 DM 推理"的备选，不在当前范围。

## 后续演进边界

- 引擎集合为 4 个。若未来确需新引擎，仍按"1 个 adapter crate + 1 个 `[engines.<name>]` 配置段"接入，
  但不引入进程内推理栈（方案 B 已被否决）。
- DM（Laya / Kev）等小模型直接跑在 vLLM 或 transformers 引擎上，不为它们单开引擎。