"""决策模型 Laya 的 sidecar 封装。

决策模型只做结构化推理，没有官方服务端；这里把 `state` JSON 转成提示词，
再把输出包装成 `delta` 事件。真正的权重加载由 `transformers_llm` 承担。
"""

from __future__ import annotations

import json
from typing import List

from ..protocol import Event, InferRequest
from . import transformers_llm


def handle(request: InferRequest) -> List[Event]:
    if request.task not in ("decision", "text"):
        return [Event.error(f"dm_laya supports decision/text, got {request.task}")]

    state = request.prompt
    prompt = (
        "You are a decision model. Given the state JSON, output a single JSON object "
        "with fields action, x, y.\n"
        f"state: {state}\n"
    )
    patched = InferRequest.from_dict(
        {
            "task": "text",
            "model": request.model,
            "model_path": request.model_path,
            "prompt": prompt,
            "stream": request.stream,
            "sampling": {
                "temperature": request.sampling.temperature,
                "max_tokens": request.sampling.max_tokens or 64,
            },
        }
    )

    events = transformers_llm.handle(patched)
    if events and events[-1].type == "done":
        done = events.pop()
        events.append(Event.done(done.prompt_tokens, done.completion_tokens))
    return events


def _unused() -> str:
    return json.dumps({})
