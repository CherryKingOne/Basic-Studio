"""SDK 数据结构。

这里只放轻量的 dataclass，字段与网关的 JSON 一一对应，
未知字段会被忽略（`from_dict` 只取关心的键），保证服务端新增字段时 SDK 仍然可用。
"""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any, Dict, List, Optional


@dataclass
class ChatMessage:
    role: str
    content: Any

    def to_dict(self) -> Dict[str, Any]:
        return {"role": self.role, "content": self.content}

    @classmethod
    def of(cls, role: str, content: Any) -> "ChatMessage":
        return cls(role=role, content=content)

    @classmethod
    def system(cls, content: Any) -> "ChatMessage":
        return cls(role="system", content=content)

    @classmethod
    def user(cls, content: Any) -> "ChatMessage":
        return cls(role="user", content=content)

    @classmethod
    def assistant(cls, content: Any) -> "ChatMessage":
        return cls(role="assistant", content=content)


@dataclass
class ModelInfo:
    id: str
    task: str = ""
    engines: List[str] = field(default_factory=list)
    adapter: Optional[str] = None
    api_version: Optional[str] = None
    streaming: bool = False

    @classmethod
    def from_dict(cls, data: Dict[str, Any]) -> "ModelInfo":
        return cls(
            id=data.get("id", ""),
            task=data.get("task", ""),
            engines=list(data.get("engines") or []),
            adapter=data.get("adapter"),
            api_version=data.get("api_version"),
            streaming=bool(data.get("streaming", False)),
        )


@dataclass
class ChatResponse:
    id: str
    model: str
    content: str
    engine: Optional[str] = None
    prompt_tokens: int = 0
    completion_tokens: int = 0

    @property
    def total_tokens(self) -> int:
        return self.prompt_tokens + self.completion_tokens

    @classmethod
    def from_dict(cls, data: Dict[str, Any]) -> "ChatResponse":
        choices = data.get("choices") or []
        content = ""
        if choices:
            message = choices[0].get("message") or {}
            content = message.get("content") or ""

        usage = data.get("usage") or {}
        return cls(
            id=data.get("id", ""),
            model=data.get("model", ""),
            content=content,
            engine=data.get("engine"),
            prompt_tokens=int(usage.get("prompt_tokens", 0) or 0),
            completion_tokens=int(usage.get("completion_tokens", 0) or 0),
        )


@dataclass
class ResponsesResponse:
    id: str
    model: str
    text: str
    status: str = "completed"

    @classmethod
    def from_dict(cls, data: Dict[str, Any]) -> "ResponsesResponse":
        return cls(
            id=data.get("id", ""),
            model=data.get("model", ""),
            text=data.get("output_text") or "",
            status=data.get("status", "completed"),
        )


@dataclass
class DecideResponse:
    model: str
    output: str
    engine: Optional[str] = None

    @classmethod
    def from_dict(cls, data: Dict[str, Any]) -> "DecideResponse":
        return cls(
            model=data.get("model", ""),
            output=data.get("output", ""),
            engine=data.get("engine"),
        )