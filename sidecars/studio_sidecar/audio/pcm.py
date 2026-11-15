"""PCM 辅助函数：base64、分块、重采样占位。"""

from __future__ import annotations

import base64
import struct
from typing import Iterable, List

BYTES_PER_SAMPLE = 2


def encode(pcm: bytes) -> str:
    return base64.b64encode(pcm).decode("ascii")


def decode(payload: str) -> bytes:
    return base64.b64decode(payload.encode("ascii"))


def chunk(pcm: bytes, sample_rate: int, frame_ms: int = 20) -> List[bytes]:
    """按固定时长切分 PCM（16bit 小端），保持采样对齐。"""
    frame_bytes = max(1, int(sample_rate * frame_ms / 1000)) * BYTES_PER_SAMPLE
    return [pcm[i : i + frame_bytes] for i in range(0, len(pcm), frame_bytes)]


def duration_ms(pcm: bytes, sample_rate: int) -> float:
    if sample_rate <= 0:
        return 0.0
    samples = len(pcm) // BYTES_PER_SAMPLE
    return samples * 1000.0 / sample_rate


def to_int16_samples(pcm: bytes) -> Iterable[int]:
    return struct.unpack(f"<{len(pcm) // BYTES_PER_SAMPLE}h", pcm[: len(pcm) // BYTES_PER_SAMPLE * BYTES_PER_SAMPLE])