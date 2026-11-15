"""协议契约测试：事件 JSON 形状必须与 Rust 侧 studio-engine::http::sidecar_event 一致。"""

import json
import unittest
from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from studio_sidecar.protocol import Event, InferRequest, encode_ndjson  # noqa: E402


class ProtocolTest(unittest.TestCase):
    def test_delta_event_shape(self):
        payload = Event.delta("你").to_dict()
        self.assertEqual(payload, {"type": "delta", "text": "你"})

    def test_audio_event_shape(self):
        event = Event.audio(b"hello", 16000)
        payload = event.to_dict()
        self.assertEqual(payload["type"], "audio")
        self.assertEqual(payload["sample_rate"], 16000)
        self.assertIn("pcm_b64", payload)

    def test_done_event_shape(self):
        payload = Event.done(prompt_tokens=3, completion_tokens=5).to_dict()
        self.assertEqual(payload["type"], "done")
        self.assertEqual(payload["prompt_tokens"], 3)
        self.assertEqual(payload["completion_tokens"], 5)

    def test_error_event_shape(self):
        payload = Event.error("boom").to_dict()
        self.assertEqual(payload, {"type": "error", "message": "boom"})

    def test_ndjson_lines(self):
        raw = encode_ndjson([Event.delta("a"), Event.done()])
        lines = raw.decode("utf-8").strip().split("\n")
        self.assertEqual(len(lines), 2)
        self.assertEqual(json.loads(lines[0])["type"], "delta")

    def test_infer_request_defaults(self):
        request = InferRequest.from_dict({"task": "tts", "prompt": "hi"})
        self.assertEqual(request.task, "tts")
        self.assertFalse(request.stream)
        self.assertIsNone(request.adapter)


if __name__ == "__main__":
    unittest.main()