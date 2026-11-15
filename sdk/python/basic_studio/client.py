"""HTTP 客户端：标准库实现，支持普通 JSON 与 SSE 流式。"""

from __future__ import annotations

import json
import socket
import urllib.error
import urllib.request
from typing import Any, Dict, Iterable, Iterator, List, Optional, Sequence, Tuple, Union

from .types import ChatMessage, ChatResponse, DecideResponse, ModelInfo, ResponsesResponse

DEFAULT_BASE_URL = "http://127.0.0.1:18596"
DEFAULT_TIMEOUT = 60.0


class ClientError(RuntimeError):
    """服务端返回错误或网络异常。"""

    def __init__(self, message: str, status: Optional[int] = None, payload: Any = None) -> None:
        super().__init__(message)
        self.status = status
        self.payload = payload


class StreamEvent:
    """Responses API 的流式事件。"""

    def __init__(self, name: str, data: Dict[str, Any]) -> None:
        self.name = name
        self.data = data

    @property
    def type(self) -> str:
        return str(self.data.get("type", self.name))

    def delta(self) -> str:
        return str(self.data.get("delta", ""))

    def __repr__(self) -> str:  # pragma: no cover - 调试用
        return f"StreamEvent(name={self.name!r}, type={self.type!r})"


class Client:
    """Basic-Studio 网关客户端。

    :param base_url: 网关地址，默认 ``http://127.0.0.1:18596``
    :param api_key: 可选 Bearer Token；网关配置了 ``[gateway].api_keys`` 时必填
    :param timeout: 单次请求超时（秒）
    """

    def __init__(
        self,
        base_url: str = DEFAULT_BASE_URL,
        api_key: Optional[str] = None,
        timeout: float = DEFAULT_TIMEOUT,
    ) -> None:
        self.base_url = base_url.rstrip("/")
        self.api_key = api_key
        self.timeout = timeout

    # ------------------------------------------------------------------ 基础设施

    def _headers(self, extra: Optional[Dict[str, str]] = None) -> Dict[str, str]:
        headers = {"content-type": "application/json"}
        if self.api_key:
            headers["authorization"] = f"Bearer {self.api_key}"
        if extra:
            headers.update(extra)
        return headers

    def _request(
        self,
        method: str,
        path: str,
        body: Optional[Any] = None,
        accept: str = "application/json",
    ) -> Tuple[int, bytes]:
        data = None
        if body is not None:
            data = json.dumps(body).encode("utf-8")

        request = urllib.request.Request(
            f"{self.base_url}{path}",
            data=data,
            headers=self._headers({"accept": accept}),
            method=method,
        )

        try:
            with urllib.request.urlopen(request, timeout=self.timeout) as response:
                return response.status, response.read()
        except urllib.error.HTTPError as exc:  # 4xx / 5xx 也要把服务端 JSON 带出来
            raw = exc.read()
            raise ClientError(_error_message(raw, exc.reason), exc.code, _safe_json(raw)) from exc
        except (urllib.error.URLError, socket.timeout) as exc:
            raise ClientError(f"cannot reach {self.base_url}: {exc}") from exc

    def _json(
        self, method: str, path: str, body: Optional[Any] = None
    ) -> Dict[str, Any]:
        _, raw = self._request(method, path, body)
        return json.loads(raw or b"{}")

    # ------------------------------------------------------------------ 基础接口

    def health(self) -> Dict[str, Any]:
        return self._json("GET", "/healthz")

    def ready(self) -> Dict[str, Any]:
        return self._json("GET", "/readyz")

    def models(self) -> List[ModelInfo]:
        payload = self._json("GET", "/v1/models")
        return [ModelInfo.from_dict(item) for item in payload.get("data", [])]

    # ------------------------------------------------------------------ 对话

    def chat(
        self,
        model: str,
        messages: Union[str, Sequence[Union[str, ChatMessage, Dict[str, Any]]]],
        system: Optional[str] = None,
        temperature: Optional[float] = None,
        top_p: Optional[float] = None,
        max_tokens: Optional[int] = None,
    ) -> ChatResponse:
        payload = self.chat_raw(
            model,
            messages,
            system=system,
            temperature=temperature,
            top_p=top_p,
            max_tokens=max_tokens,
        )
        return ChatResponse.from_dict(payload)

    def chat_raw(
        self,
        model: str,
        messages: Union[str, Sequence[Union[str, ChatMessage, Dict[str, Any]]]],
        system: Optional[str] = None,
        temperature: Optional[float] = None,
        top_p: Optional[float] = None,
        max_tokens: Optional[int] = None,
    ) -> Dict[str, Any]:
        body: Dict[str, Any] = {
            "model": model,
            "messages": _normalize_messages(messages, system),
            "stream": False,
        }
        _apply_sampling(body, temperature, top_p, max_tokens)
        return self._json("POST", "/v1/chat/completions", body)

    def chat_stream(
        self,
        model: str,
        messages: Union[str, Sequence[Union[str, ChatMessage, Dict[str, Any]]]],
        system: Optional[str] = None,
        temperature: Optional[float] = None,
        top_p: Optional[float] = None,
        max_tokens: Optional[int] = None,
    ) -> Iterator[str]:
        """流式对话，逐块产出文本增量。"""
        body: Dict[str, Any] = {
            "model": model,
            "messages": _normalize_messages(messages, system),
            "stream": True,
        }
        _apply_sampling(body, temperature, top_p, max_tokens)

        for name, data in self._sse("POST", "/v1/chat/completions", body):
            if name == "error" or data.get("error"):
                raise ClientError(str(data.get("error") or data.get("message") or "stream error"))
            choices = data.get("choices") or []
            if not choices:
                continue
            delta = (choices[0].get("delta") or {}).get("content")
            if delta:
                yield str(delta)

    # ------------------------------------------------------------------ Responses

    def responses(
        self,
        model: str,
        input: Union[str, Sequence[Dict[str, Any]]],
        instructions: Optional[str] = None,
        temperature: Optional[float] = None,
        top_p: Optional[float] = None,
        max_output_tokens: Optional[int] = None,
    ) -> ResponsesResponse:
        body: Dict[str, Any] = {
            "model": model,
            "input": input,
            "stream": False,
        }
        if instructions is not None:
            body["instructions"] = instructions
        if temperature is not None:
            body["temperature"] = temperature
        if top_p is not None:
            body["top_p"] = top_p
        if max_output_tokens is not None:
            body["max_output_tokens"] = max_output_tokens

        return ResponsesResponse.from_dict(self._json("POST", "/v1/responses", body))

    def responses_stream(
        self,
        model: str,
        input: Union[str, Sequence[Dict[str, Any]]],
        instructions: Optional[str] = None,
    ) -> Iterator[StreamEvent]:
        """Responses 流式接口，产出 :class:`StreamEvent`。"""
        body: Dict[str, Any] = {"model": model, "input": input, "stream": True}
        if instructions is not None:
            body["instructions"] = instructions

        for name, data in self._sse("POST", "/v1/responses", body):
            if name == "response.failed":
                raise ClientError(str(data.get("message") or "response failed"))
            yield StreamEvent(name, data)

    # ------------------------------------------------------------------ 其它任务

    def embeddings(self, model: str, input: str) -> Dict[str, Any]:
        return self._json(
            "POST", "/v1/embeddings", {"model": model, "input": input}
        )

    def tts(
        self,
        model: str,
        text: str,
        voice: Optional[str] = None,
        stream: bool = False,
        emotion: Optional[str] = None,
    ) -> bytes:
        """语音合成，返回 PCM（16bit little-endian）字节。

        :param stream: 为 ``True`` 时仍返回完整音频（SDK 内部拼接），
                       需要边收边播请直接用 ``tts_stream``
        """
        return b"".join(self.tts_stream(model, text, voice=voice, emotion=emotion))

    def tts_stream(
        self,
        model: str,
        text: str,
        voice: Optional[str] = None,
        emotion: Optional[str] = None,
    ) -> Iterator[bytes]:
        body: Dict[str, Any] = {
            "model": model,
            "input": text,
            "stream": True,
        }
        if voice is not None:
            body["voice"] = voice
        if emotion is not None:
            body["emotion"] = emotion

        request = urllib.request.Request(
            f"{self.base_url}/v1/audio/speech",
            data=json.dumps(body).encode("utf-8"),
            headers=self._headers(),
            method="POST",
        )
        try:
            with urllib.request.urlopen(request, timeout=self.timeout) as response:
                while True:
                    chunk = response.read(4096)
                    if not chunk:
                        break
                    yield chunk
        except urllib.error.HTTPError as exc:
            raw = exc.read()
            raise ClientError(_error_message(raw, exc.reason), exc.code, _safe_json(raw)) from exc
        except (urllib.error.URLError, socket.timeout) as exc:
            raise ClientError(f"cannot reach {self.base_url}: {exc}") from exc

    def decide(
        self,
        model: str,
        state: Any,
        context: Optional[str] = None,
    ) -> DecideResponse:
        body: Dict[str, Any] = {"model": model, "state": state}
        if context is not None:
            body["context"] = context
        return DecideResponse.from_dict(self._json("POST", "/v1/decide", body))

    # ------------------------------------------------------------------ 内部

    def _sse(
        self, method: str, path: str, body: Dict[str, Any]
    ) -> Iterator[Tuple[str, Dict[str, Any]]]:
        """逐行解析 SSE，产出 (event, data)。"""
        request = urllib.request.Request(
            f"{self.base_url}{path}",
            data=json.dumps(body).encode("utf-8"),
            headers=self._headers({"accept": "text/event-stream"}),
            method=method,
        )

        try:
            response = urllib.request.urlopen(request, timeout=self.timeout)
        except urllib.error.HTTPError as exc:
            raw = exc.read()
            raise ClientError(_error_message(raw, exc.reason), exc.code, _safe_json(raw)) from exc
        except (urllib.error.URLError, socket.timeout) as exc:
            raise ClientError(f"cannot reach {self.base_url}: {exc}") from exc

        name = ""
        with response:
            for raw_line in response:
                line = raw_line.decode("utf-8").rstrip("\r\n")
                if line.startswith("event:"):
                    name = line[len("event:") :].strip()
                elif line.startswith("data:"):
                    payload = line[len("data:") :].strip()
                    if payload == "[DONE]":
                        break
                    try:
                        data = json.loads(payload)
                    except json.JSONDecodeError:
                        continue
                    yield name, data
                    if data.get("type") == "response.completed":
                        break

    def close(self) -> None:  # pragma: no cover - 无连接池需要释放
        return None

    def __enter__(self) -> "Client":
        return self

    def __exit__(self, *_exc: Any) -> None:
        self.close()


# ---------------------------------------------------------------------- 辅助函数


def _normalize_messages(
    messages: Union[str, Sequence[Union[str, ChatMessage, Dict[str, Any]]]],
    system: Optional[str],
) -> List[Dict[str, Any]]:
    normalized: List[Dict[str, Any]] = []

    if system:
        normalized.append({"role": "system", "content": system})

    if isinstance(messages, str):
        normalized.append({"role": "user", "content": messages})
        return normalized

    for item in messages:
        if isinstance(item, ChatMessage):
            normalized.append(item.to_dict())
        elif isinstance(item, dict):
            normalized.append(item)
        else:
            normalized.append({"role": "user", "content": str(item)})

    return normalized


def _apply_sampling(
    body: Dict[str, Any],
    temperature: Optional[float],
    top_p: Optional[float],
    max_tokens: Optional[int],
) -> None:
    if temperature is not None:
        body["temperature"] = temperature
    if top_p is not None:
        body["top_p"] = top_p
    if max_tokens is not None:
        body["max_tokens"] = max_tokens


def _safe_json(raw: bytes) -> Any:
    try:
        return json.loads(raw or b"null")
    except json.JSONDecodeError:
        return None


def _error_message(raw: bytes, reason: Optional[str]) -> str:
    payload = _safe_json(raw)
    if isinstance(payload, dict):
        error = payload.get("error")
        if isinstance(error, dict):
            return str(error.get("message") or reason or "request failed")
        if error:
            return str(error)
    if raw:
        return raw.decode("utf-8", "replace")
    return reason or "request failed"