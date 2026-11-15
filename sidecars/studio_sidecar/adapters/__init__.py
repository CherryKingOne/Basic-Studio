"""适配器注册表。

每个适配器实现 `handle(request) -> list[Event]`。
默认在没有指定模型时使用 `echo`，便于在没有 GPU / 没有权重时联调。
"""

from __future__ import annotations

from importlib import import_module
from typing import Dict

from ..protocol import InferRequest

_ADAPTERS: Dict[str, str] = {
    "echo": "studio_sidecar.adapters.echo",
    "transformers_llm": "studio_sidecar.adapters.transformers_llm",
    "indextts_v1": "studio_sidecar.adapters.indextts_v1",
    "indextts_v15": "studio_sidecar.adapters.indextts_v15",
    "indextts_v20": "studio_sidecar.adapters.indextts_v20",
    "indextts_v25": "studio_sidecar.adapters.indextts_v25",
    "dm_laya": "studio_sidecar.adapters.dm_laya",
}


def load(name: str):
    """按名字加载适配器模块；未知名字回落到 echo。"""
    module_path = _ADAPTERS.get(name)
    if module_path is None:
        return import_module("studio_sidecar.adapters.echo")
    return import_module(module_path)


def resolve(request: InferRequest):
    """根据请求里的 adapter 选择处理器，异常时回落到 echo 并给出提示。"""
    name = request.adapter or ("echo" if not request.model_path else "transformers_llm")
    try:
        module = load(name)
        return module.handle
    except Exception as exc:  # noqa: BLE001
        print(f"[studio_sidecar] adapter {name!r} unavailable ({exc}); falling back to echo", flush=True)
        return load("echo").handle