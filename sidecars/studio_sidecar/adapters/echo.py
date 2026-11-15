"""无依赖假适配器：用于联调、CI 与协议测试。"""

from __future__ import annotations

import math
import struct
from typing import Iterable, List

from ..protocol import Event, InferRequest

SAMPLE_RATE = 22050


def handle(request: InferRequest) -> List[Event]:
    if request.task == "tts":
        return _tts_events(request)

    if request.task in ("embed", "rerank"):
        return [
            Event.delta("[0.01, 0.02, 0.03]"),
            Event.done(prompt_tokens=1, completion_tokens=1),
        ]

    if request.task == "decision":
        return [
            Event.delta('{"action":"move","x":1,"y":2}'),
            Event.done(prompt_tokens=len(request.prompt), completion_tokens=8),
        ]

    text = f"echo:{request.prompt}"
    events = [Event.delta(text)]
    events.append(Event.done(prompt_tokens=len(request.prompt), completion_tokens=max(1, len(text) // 4)))
    return events


def _tts_events(request: InferRequest) -> List[Event]:
    duration = 0.2
    samples = int(SAMPLE_RATE * duration)
    pcm = bytearray()
    for index in range(samples):
        value = int(12000 * math.sin(2 * math.pi * 440 * index / SAMPLE_RATE))
        pcm += struct.pack("<h", value)
    return [
        Event.audio(bytes(pcm), SAMPLE_RATE),
        Event.done(prompt_tokens=len(request.prompt), completion_tokens=0),
    ]