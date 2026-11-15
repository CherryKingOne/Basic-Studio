"""Transformers 文本生成 / embedding 适配器。

依赖 `transformers` + `torch`；缺失时抛出明确错误，由注册表回落到 echo。
"""

from __future__ import annotations

from typing import Any, Dict, List, Optional, Tuple

from ..protocol import Event, InferRequest

_CACHE: Dict[str, Tuple[Any, Any]] = {}


def _load(model_path: Optional[str], task: str):
    if not model_path:
        raise RuntimeError("transformers_llm requires model_path")

    key = f"{model_path}:{task}"
    if key in _CACHE:
        return _CACHE[key]

    try:
        from transformers import AutoModelForCausalLM, AutoTokenizer  # type: ignore
    except ImportError as exc:  # pragma: no cover - 依赖缺失时的分支
        raise RuntimeError(f"transformers is not installed: {exc}") from exc

    tokenizer = AutoTokenizer.from_pretrained(model_path, trust_remote_code=False)
    model = AutoModelForCausalLM.from_pretrained(model_path, trust_remote_code=False)
    model.eval()
    _CACHE[key] = (tokenizer, model)
    return _CACHE[key]


def handle(request: InferRequest) -> List[Event]:
    if request.task == "embed":
        return _embed(request)

    tokenizer, model = _load(request.model_path, "generate")

    prompt = request.prompt
    if request.sampling.temperature is not None:
        prompt = prompt  # 采样参数在实现时透传给 generate

    inputs = tokenizer(prompt, return_tensors="pt")
    max_new_tokens = request.sampling.max_tokens or 256
    with __import__("contextlib").nullcontext():
        output_ids = model.generate(**inputs, max_new_tokens=max_new_tokens)
    text = tokenizer.decode(output_ids[0], skip_special_tokens=True)

    events = [Event.delta(text)]
    events.append(
        Event.done(
            prompt_tokens=int(inputs["input_ids"].shape[-1]),
            completion_tokens=max(1, int(output_ids.shape[-1]) - int(inputs["input_ids"].shape[-1])),
        )
    )
    return events


def _embed(request: InferRequest) -> List[Event]:
    tokenizer, model = _load(request.model_path, "embed")
    inputs = tokenizer(request.prompt, return_tensors="pt")
    with __import__("contextlib").nullcontext():
        hidden = model(**inputs).last_hidden_state
    vector = hidden.mean(dim=1)[0].tolist()
    import json

    return [Event.delta(json.dumps(vector)), Event.done(prompt_tokens=int(inputs["input_ids"].shape[-1]))]