# 对外接口清单

## 1. OpenAI 兼容（默认前缀 `/v1`）

| 方法 | 路径 | 说明 |
| --- | --- | --- |
| GET | `/v1/models` | 已注册模型列表 |
| GET | `/v1/models/{id}` | 模型详情（含引擎、能力、状态） |
| POST | `/v1/chat/completions` | 对话补全，`stream: true` 走 SSE |
| POST | `/v1/completions` | 文本补全 |
| POST | `/v1/embeddings` | 向量化 |
| POST | `/v1/audio/speech` | TTS，`stream: true` 返回 chunked PCM |
| POST | `/v1/audio/transcriptions` | ASR（预留） |

## 2. Studio 原生接口

| 方法 | 路径 | 说明 |
| --- | --- | --- |
| POST | `/v1/decide` | 决策模型（DM）结构化推理，返回 JSON 动作 |
| GET | `/v1/engines` | 引擎实例列表与状态机 |
| POST | `/v1/engines/{kind}/up` | 手动拉起引擎 |
| POST | `/v1/engines/{kind}/down` | 优雅下线（drain） |
| POST | `/v1/jobs` | 提交异步任务（批量 TTS / 批量决策） |
| GET | `/v1/jobs/{id}` | 任务状态与进度 |
| GET | `/v1/jobs/{id}/events` | 任务事件流（SSE） |
| DELETE | `/v1/jobs/{id}` | 取消任务 |
| GET | `/v1/cluster/nodes` | 集群节点列表 |
| GET | `/v1/healthz` `/readyz` | 健康与就绪探针 |

## 3. WebSocket

| 路径 | 说明 |
| --- | --- |
| `/ws/tts` | 双向音频会话：文本分句下推，PCM 分块回推，支持中途取消 |
| `/ws/jobs/{id}` | 任务进度订阅 |

## 4. gRPC（集群内部，仅节点间）

| RPC | 说明 |
| --- | --- |
| `Node.Heartbeat` | 节点上报 GPU/显存/在跑模型 |
| `Node.Register` | 工作节点注册到控制节点 |
| `Router.Forward` | 跨节点转发请求（流式逐块回传） |
| `Router.Cancel` | 跨节点取消 |

## 5. CLI（`studio`）

```
studio serve [--config config/default.toml] [--gpus 0,1]
studio models ls [--task tts] [--json]
studio models inspect <id>
studio models verify [--all]
studio models pull <id>
studio engines ls | up <kind> | down <kind>
studio bench tts --model <id> --text-file <f> --runs 20
studio doctor [--fix]
studio cluster join <control-addr>
```

## 6. 错误模型

统一 JSON：

```json
{ "error": { "type": "engine_unavailable", "code": "E_ENGINE_DOWN",
             "message": "vllm/tts-index-tts2.0 exited during startup (code 137)",
             "retryable": true, "retry_after_s": 5, "request_id": "req_..." } }
```

| code | 含义 |
| --- | --- |
| `E_MODEL_NOT_FOUND` | 模型未注册或未启用 |
| `E_NO_CAPABLE_ENGINE` | 无引擎支持该 task |
| `E_ENGINE_DOWN` | 引擎未就绪/已崩溃 |
| `E_VRAM_INSUFFICIENT` | 显存预算不足（启动前拒绝） |
| `E_RATE_LIMITED` | 租户配额或并发超限 |
| `E_CANCELED` | 客户端取消 |
| `E_UPSTREAM_TIMEOUT` | 引擎首包/空闲超时 |