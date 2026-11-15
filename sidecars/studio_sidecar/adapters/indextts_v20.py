"""IndexTTS 2.0 适配器。"""

from __future__ import annotations

from typing import List

from ..protocol import Event, InferRequest
from ._indextts_common import handle_with

API_VERSION = "v20"


def handle(request: InferRequest) -> List[Event]:
    return handle_with(request, API_VERSION)
