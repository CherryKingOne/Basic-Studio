# studio_sidecar

Transformers 引擎的 Python 侧车：把 Transformers / IndexTTS 的推理封装成统一 HTTP 协议，
供 Rust 侧的 `engine-transformers` 调用。

## 协议

| 方法 | 路径 | 说明 |
| --- | --- | --- |
| GET | `/health` | 就绪探测，返回 `{"status":"ok"}` |
| POST | `/infer` | 文本 / 向量 / 决策，返回单个 JSON 事件 |
| POST | `/infer/stream` | 同上，但返回 NDJSON（每行一个事件），TTS 亦走这里 |

事件格式（与 Rust 侧 `studio-engine::http::sidecar_event` 对应）：

```json
{"type":"delta","text":"你"}
{"type":"audio","pcm_b64":"...","sample_rate":22050}
{"type":"done","prompt_tokens":12,"completion_tokens":34}
{"type":"error","message":"..."}
```

## 运行

```bash
python -m studio_sidecar serve transformers --port 9000 --model models/tts/index-tts2.5
```

不带模型时用内置 echo 适配器，便于联调：

```bash
python -m studio_sidecar serve transformers --port 9000
curl -s localhost:9000/health
curl -s localhost:9000/infer -H 'content-type: application/json' \
  -d '{"task":"text","prompt":"hi","stream":false}'
```

## 适配器

- `adapters/transformers_llm.py`：文本生成、embedding（需要 transformers）
- `adapters/indextts_v1/v15/v20/v25.py`：IndexTTS 各版本（需要 indextts 依赖）
- `adapters/echo.py`：无依赖的假实现，用于测试与本地联调

## 测试

```bash
python -m unittest discover -s tests
```
