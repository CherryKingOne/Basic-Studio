# 实施路线图

> 原则：**先把"能跑通一个引擎"做扎实，再横向复制引擎**。避免一开始铺开 6 个引擎导致都不可用。

## 里程碑总览

| 阶段 | 目标 | 关键交付 | 验收标准 |
| --- | --- | --- | --- |
| M0 | 骨架可编译 | workspace、core/config/registry/cli、FakeEngine | `cargo check` 通过；`studio doctor` 输出环境报告 |
| M1 | vLLM 打通 | procsup + engine-vllm + 透传 API | `studio serve` 起 vLLM，`/v1/chat/completions` 流式返回 |
| M2 | SGLang + 路由 | engine-sglang + router + failover | 同模型双引擎可切换；引擎崩溃自动转移 |
| M3 | TTS 主线（核心） | sidecar 框架 + IndexTTS 1.5/2.0/2.5 适配 + 流式音频 | `/v1/audio/speech` 返回 PCM 流，RTF < 1 |
| M4 | 生产化 | 调度/配额/鉴权/指标/日志 | 压测下无泄漏、无僵尸进程；指标齐全 |
| M5 | DM 与原生后端 | engine-native(candle/ort) + DM 模型接入 | Laya/Kev 进程内推理，无 Python 依赖 |
| M6 | 集群与生态 | gRPC 多节点、插件、Python/TS SDK、Helm | 两节点转发正常；SDK 可用 |

## M0 骨架（约 1~2 周）

- `Cargo.toml` workspace，`rust-toolchain.toml`，`justfile`，CI（fmt/clippy/test）。
- `studio-core`：所有领域类型先定稿（**接口先行**，后续引擎都依赖它）。
- `studio-config`：配置加载与校验（缺字段直接报错，不静默取默认）。
- `studio-registry`：扫描 `models/`，解析 `model.toml`，生成索引。
- `studio-cli`：`studio doctor` / `studio models ls` / `studio models inspect <id>`。
- `studio-testkit::FakeEngine`：让上层在没有 GPU 时也能开发和测试。

> 里程碑产物：`models/` 目录 + `model.toml` 清单规范定稿（这份规范后面不好改，值得多花时间）。

## M1 vLLM（约 2 周）

- `studio-procsup`：spawn（venv 解释器）→ 端口池 → 轮询 `/health` → 记录日志。
- `engine-vllm`：参数模板渲染（TP/PP/显存利用率/max-model-len）、能力探测。
- `studio-gateway`：`/v1/models`、`/v1/chat/completions`（含 SSE 逐块转发）、`/v1/embeddings`。
- 超时与取消：客户端断开 → `DELETE` 或引擎 abort。
- 验收：单卡 7B 模型，首包 < 2s，流式正常，Ctrl-C 后无残留进程。

## M2 SGLang + 路由（约 2 周）

- `engine-sglang`：`/generate` 与 OpenAI 两条路径，前缀缓存命中率上报。
- `studio-router`：候选打分（就绪度 × 显存余量 × 队列深度 × 亲和标签）。
- 重试策略：仅对幂等请求重试；流式已开始不重试。
- 验收：杀掉引擎进程，请求在 5s 内被转移到另一副本或返回明确 503。

## M3 TTS 主线（约 3 周，优先级最高）

- sidecar 统一协议定稿：`/health`、`/infer`、`/infer/stream`（chunked PCM）。
- `studio-media`：PCM 分块（20~40ms）、重采样、削波保护、可选 mp3/opus 编码。
- `engine-indextts`：按 `api_version` 分派 1.0 / 1.5 / 2.0 / 2.5 adapter。
- 流式链路：文本分句 → 增量合成 → 边合成边下发；句间不留长静音。
- 验收：2.5 版本 100 字文本 RTF < 1；2.0 情感控制参数生效；客户端首块 < 800ms。

## M4 生产化（约 2~3 周）

- `studio-scheduler`：优先级队列（interactive/batch）、并发上限、租户配额。
- `studio-auth`：API Key + 租户 + 日 token 限额。
- `studio-obs`：Prometheus 指标、OTLP trace、日志滚动与脱敏。
- `studio-gpu`：显存预算、启动前准入检查、设备钉扎。
- 验收：并发 50 路 TTS 无 OOM；指标可观测；重启引擎后指标归零且恢复。

## M5 DM 与原生后端（约 3 周）

- `engine-native`：`ort` / `candle` 加载 DM 模型（Laya / Kev），进程内推理。
- 决策模型专用 API：`/v1/decide`（返回结构化动作），与聊天接口分离。
- 验收：无 Python 环境可运行 DM 推理；P99 延迟显著低于 sidecar 路径。

## M6 集群与生态（约 3~4 周）

- `studio-grpc`：节点注册、心跳、跨节点转发、模型放置。
- `studio-plugin`：子命令式引擎插件（第三方可外挂）。
- SDK：`sdk/python`、`sdk/typescript`（薄封装，OpenAI 兼容）。
- `deploy/helm`：HPA 按队列深度扩容。
- 验收：两节点下请求正确转发；插件可被动态注册。

## 横切任务（贯穿全程）

- 每个引擎一个 `contract test`，防止 sidecar 与 Rust 协议漂移。
- 每个引擎一个 `bench`：TTFT / TPOT / RTF 基线，提交到 `benches/` 报告。
- 文档同步：`docs/ARCHITECTURE.md` 变更需同步更新。

## 主要风险与对策

| 风险 | 影响 | 对策 |
| --- | --- | --- |
| 各引擎 torch/CUDA 版本冲突 | 多引擎无法共存 | 每引擎独立 venv（`envs/<engine>`） |
| IndexTTS 各版本 API 不兼容 | TTS 适配反复重写 | adapter 按 `api_version` 隔离 + 契约测试 |
| 显存超卖导致 OOM | 服务不可用 | 启动前预算检查 + 单模型独占开关 |
| 模型体积过大 | 无法进 Git / 部署慢 | `models/` 全 ignore，只提交清单；按需下载 |
| 进程泄漏 | 长时间运行后资源耗尽 | supervisor 统一回收 + 定期巡检 |
| 过早横向铺开引擎 | 全部半成品 | 严格按 M0→M1→M2→M3 串行推进 |