"""IndexTTS 各版本的共用实现。

IndexTTS 各版本（1.0 / 1.5 / 2.0 / 2.5）的 Python API 差异较大，
这里用一个兼容性包装收敛差异，具体版本差异写在各 `indextts_vXX.py` 里。
"""

from __future__ import annotations

import importlib
from typing import Any, Iterable, List, Optional, Tuple

from ..protocol import Event, InferRequest

CHUNK_SAMPLES = 2048


class TtsUnavailable(RuntimeError):
    pass


def _load_engine(model_path: Optional[str], api_version: str):
    """尝试导入 indextts 并构造推理引擎。"""
    if not model_path:
        raise TtsUnavailable("indextts adapter requires model_path")

    try:
        module = importlib.import_module("indextts")
    except ImportError as exc:  # pragma: no cover
        raise TtsUnavailable(f"indextts is not installed: {exc}") from exc

    factory = None
    for name in ("IndexTTS", "infer", "IndexTTS2"):
        candidate = getattr(module, name, None)
        if candidate is not None:
            factory = candidate
            break

    if factory is None:
        raise TtsUnavailable("indextts exposes no known entry point")

    try:
        engine = factory(model_path, cfg_path=None)
    except TypeError:
        try:
            engine = factory(model_path)
        except TypeError:
            engine = factory()

    setattr(engine, "_studio_api_version", api_version)
    return engine


def _synthesize(engine: Any, request: InferRequest) -> Tuple[bytes, int]:
    """调用引擎合成语音，返回 (pcm16le, sample_rate)。"""
    import numpy as np  # type: ignore

    speaker = request.voice
    emotion = request.sampling.emotion

    audio = None
    sample_rate = 0

    for attempt in (
        lambda: engine.infer(request.prompt, speaker, emotion),
        lambda: engine.infer(request.prompt, speaker),
        lambda: engine.infer(request.prompt),
        lambda: engine.infer(request.prompt, voice=speaker),
    ):
        try:
            result = attempt()
        except TypeError:
            continue
        except Exception:
            continue
        audio, sample_rate = _unpack(result)
        break

    if audio is None:
        raise TtsUnavailable("no compatible infer() signature found")

    if audio.dtype != np.int16:
        audio = np.clip(audio, -1.0, 1.0)
        audio = (audio * 32767).astype(np.int16)

    return audio.tobytes(), sample_rate or 22050


def _unpack(result: Any) -> Tuple[Any, int]:
    """兼容 (audio, sr) / {'audio':..., 'sample_rate':...} / 裸数组。"""
    sample_rate = 0
    audio = result

    if isinstance(result, tuple) and len(result) == 2:
        audio, sample_rate = result
    elif isinstance(result, dict):
        audio = result.get("audio") or result.get("wav")
        sample_rate = int(result.get("sample_rate", 0) or 0)

    return audio, sample_rate


def handle_with(request: InferRequest, api_version: str) -> List[Event]:
    if request.task != "tts":
        return [Event.error(f"adapter supports only tts, got {request.task}")]

    engine = _load_engine(request.model_path, api_version)
    pcm, sample_rate = _synthesize(engine, request)

    events: List[Event] = []
    for start in range(0, len(pcm), CHUNK_SAMPLES * 2):
        events.append(Event.audio(pcm[start : start + CHUNK_SAMPLES * 2], sample_rate))
    events.append(Event.done(prompt_tokens=len(request.prompt), completion_tokens=0))
    return events