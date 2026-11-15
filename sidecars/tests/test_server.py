"""端到端测试：启动真实 HTTP 服务，用 urllib 调用。"""

import json
import sys
import threading
import unittest
import urllib.request
from http.server import ThreadingHTTPServer
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from studio_sidecar import adapters, server  # noqa: E402


class ServerTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        handler = server.make_handler("transformers", adapters.resolve)
        cls.httpd = ThreadingHTTPServer(("127.0.0.1", 0), handler)
        cls.port = cls.httpd.server_address[1]
        cls.thread = threading.Thread(target=cls.httpd.serve_forever, daemon=True)
        cls.thread.start()

    @classmethod
    def tearDownClass(cls):
        cls.httpd.shutdown()
        cls.httpd.server_close()

    def _post(self, path, payload):
        data = json.dumps(payload).encode("utf-8")
        request = urllib.request.Request(
            f"http://127.0.0.1:{self.port}{path}",
            data=data,
            headers={"content-type": "application/json"},
            method="POST",
        )
        with urllib.request.urlopen(request, timeout=5) as response:
            return response.status, response.read().decode("utf-8")

    def test_health(self):
        with urllib.request.urlopen(f"http://127.0.0.1:{self.port}/health", timeout=5) as response:
            body = json.loads(response.read().decode("utf-8"))
        self.assertEqual(body["status"], "ok")

    def test_infer_non_stream(self):
        status, body = self._post(
            "/infer",
            {"task": "text", "adapter": "echo", "prompt": "hi", "stream": False},
        )
        self.assertEqual(status, 200)
        payload = json.loads(body)
        self.assertEqual(payload["type"], "done")
        self.assertTrue(payload["completion_tokens"] > 0)

    def test_infer_stream_is_ndjson(self):
        status, body = self._post(
            "/infer/stream",
            {"task": "text", "adapter": "echo", "prompt": "hello world", "stream": True},
        )
        self.assertEqual(status, 200)
        lines = [line for line in body.split("\n") if line.strip()]
        types = [json.loads(line)["type"] for line in lines]
        self.assertIn("delta", types)
        self.assertEqual(types[-1], "done")

    def test_tts_stream_emits_audio(self):
        status, body = self._post(
            "/infer/stream",
            {"task": "tts", "adapter": "echo", "prompt": "hi", "stream": True},
        )
        self.assertEqual(status, 200)
        events = [json.loads(line) for line in body.split("\n") if line.strip()]
        audio = [e for e in events if e["type"] == "audio"]
        self.assertTrue(audio, "expected at least one audio event")
        self.assertEqual(audio[0]["sample_rate"], 22050)

    def test_unknown_path_returns_404(self):
        with self.assertRaises(urllib.error.HTTPError) as ctx:
            urllib.request.urlopen(f"http://127.0.0.1:{self.port}/nope", timeout=5)
        self.assertEqual(ctx.exception.code, 404)


if __name__ == "__main__":
    unittest.main()