"""请求/响应协议。

这里定义的类型是 Rust 侧 `studio-engine::http::sidecar_event` 的镜像，
修改时必须两边同步（见 `sidecars/tests/test_protocol_contract.py`）。
"""

from __future__ import annotations

import base64
from dataclasses import dataclass, field
from typing import Any, Dict, Literal, Optional

EventType = Literal["delta", "audio", "done", "error"]


@dataclass
class Sampling:
    temperature: Optional[float] = None
    top_p: Optional[float] = None
    max_tokens: Optional[int] = None
    emotion: Optional[str] = None

    @classmethod
    def from_dict(cls, data: Optional[Dict[str, Any]]) -> "Sampling":
        data = data or {}
        return cls(
            temperature=data.get("temperature"),
            top_p=data.get("top_p"),
            max_tokens=data.get("max_tokens"),
            emotion=data.get("emotion"),
        )


@dataclass
class InferRequest:
    task: str = "text"
    model: str = ""
    adapter: Optional[str] = None
    api_version: Optional[str] = None
    model_path: Optional[str] = None
    prompt: str = ""
    stream: bool = False
    voice: Optional[str] = None
    sampling: Sampling = field(default_factory=Sampling)

    @classmethod
    def from_dict(cls, data: Dict[str, Any]) -> "InferRequest":
        return cls(
            task=data.get("task", "text"),
            model=data.get("model", ""),
            adapter=data.get("adapter"),
            api_version=data.get("api_version"),
            model_path=data.get("model_path"),
            prompt=data.get("prompt", ""),
            stream=bool(data.get("stream", False)),
            voice=data.get("voice"),
            sampling=Sampling.from_dict(data.get("sampling")),
        )


@dataclass
class Event:
    type: EventType
    text: Optional[str] = None
    pcm_b64: Optional[str] = None
    sample_rate: Optional[int] = None
    prompt_tokens: int = 0
    completion_tokens: int = 0
    message: Optional[str] = None

    def to_dict(self) -> Dict[str, Any]:
        payload: Dict[str, Any] = {"type": self.type}
        if self.text is not None:
            payload["text"] = self.text
        if self.pcm_b64 is not None:
            payload["pcm_b64"] = self.pcm_b64
            payload["sample_rate"] = self.sample_rate
        if self.message is not None:
            payload["message"] = self.message
        if self.type == "done":
            payload["prompt_tokens"] = self.prompt_tokens
            payload["completion_tokens"] = self.completion_tokens
        return payload

    @classmethod
    def from_dict(cls, data: Dict[str, Any]) -> "Event":
        return cls(
            type=data.get("type", "error"),
            text=data.get("text"),
            pcm_b64=data.get("pcm_b64"),
            sample_rate=data.get("sample_rate"),
            prompt_tokens=int(data.get("prompt_tokens", 0) or 0),
            completion_tokens=int(data.get("completion_tokens", 0) or 0),
            message=data.get("message"),
        )

    @classmethod
    def delta(cls, text: str) -> "Event":
        return cls(type="delta", text=text)

    @classmethod
    def audio(cls, pcm: bytes, sample_rate: int) -> "Event":
        return cls(
            type="audio",
            pcm_b64=base64.b64encode(pcm).decode("ascii"),
            sample_rate=sample_rate,
        )

    @classmethod
    def done(cls, prompt_tokens: int = 0, completion_tokens: int = 0, text: Optional[str] = None) -> "Event":
        return cls(
            type="done",
            prompt_tokens=prompt_tokens,
            completion_tokens=completion_tokens,
            text=text,
        )

    @classmethod
    def error(cls, message: str) -> "Event":
        return cls(type="error", message=message)


def encode_ndjson(events) -> bytes:
    """把事件序列编码为 NDJSON 字节流。"""
    import json

    lines = []
    for event in events:
        lines.append(json.dumps(event.to_dict(), ensure_ascii=False))
    return ("\n".join(lines) + "\n").encode("utf-8")