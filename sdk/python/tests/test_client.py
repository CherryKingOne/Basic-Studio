"""SDK 测试：启动一个本地桩服务，覆盖主要接口。"""

import json
import sys
import threading
import unittest
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from basic_studio import ChatMessage, Client, ClientError  # noqa: E402


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *_args):  # noqa: A003
        return

    def _send_json(self, status, payload):
        body = json.dumps(payload).encode()
        self.send_response(status)
        self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def _read_json(self):
        length = int(self.headers.get("content-length", "0") or 0)
        if not length:
            return {}
        return json.loads(self.rfile.read(length))

    def do_GET(self):  # noqa: N802
        if self.path == "/healthz":
            self._send_json(200, {"status": "ok"})
        elif self.path == "/readyz":
            self._send_json(200, {"ready": True, "ready_engines": 1, "total_engines": 1})
        elif self.path == "/v1/models":
            self._send_json(
                200,
                {
                    "object": "list",
                    "data": [
                        {
                            "id": "chat/mock",
                            "task": "chat",
                            "engines": ["vllm"],
                            "streaming": True,
                        }
                    ],
                },
            )
        else:
            self._send_json(404, {"error": {"type": "not_found", "message": "nope"}})

    def do_POST(self):  # noqa: N802
        payload = self._read_json()

        if self.path == "/v1/chat/completions":
            if payload.get("stream"):
                self._send_stream(
                    [
                        ("", {"choices": [{"delta": {"content": "he"}}]}),
                        ("", {"choices": [{"delta": {"content": "llo"}}]}),
                        ("", {"choices": [], "usage": {"prompt_tokens": 2, "completion_tokens": 2}}),
                    ]
                )
                return
            self._send_json(
                200,
                {
                    "id": "chatcmpl-1",
                    "model": payload.get("model", ""),
                    "choices": [{"message": {"role": "assistant", "content": "hello"}}],
                    "engine": "vllm/mock",
                    "usage": {"prompt_tokens": 2, "completion_tokens": 1},
                },
            )
            return

        if self.path == "/v1/responses":
            if payload.get("stream"):
                self._send_stream(
                    [
                        ("response.created", {"type": "response.created", "response": {}}),
                        (
                            "response.output_text.delta",
                            {"type": "response.output_text.delta", "delta": "hi"},
                        ),
                        ("response.completed", {"type": "response.completed"}),
                    ],
                    done_marker=False,
                )
                return
            self._send_json(
                200,
                {
                    "id": "resp_1",
                    "object": "response",
                    "model": payload.get("model", ""),
                    "status": "completed",
                    "output_text": "hello",
                },
            )
            return

        if self.path == "/v1/embeddings":
            self._send_json(200, {"data": [{"embedding": [0.1, 0.2]}]})
            return

        if self.path == "/v1/audio/speech":
            self._send_bytes(200, b"\x01\x02" * 64)
            return

        if self.path == "/v1/decide":
            self._send_json(
                200,
                {"model": payload.get("model", ""), "output": '{"action":"move"}', "engine": "vllm/dm"},
            )
            return

        self._send_json(404, {"error": {"type": "not_found", "message": "unknown path"}})

    def _send_bytes(self, status, body):
        self.send_response(status)
        self.send_header("content-type", "audio/pcm")
        self.send_header("content-length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def _send_stream(self, events, done_marker=True):
        self.send_response(200)
        self.send_header("content-type", "text/event-stream")
        self.send_header("transfer-encoding", "chunked")
        self.end_headers()

        for name, data in events:
            lines = []
            if name:
                lines.append(f"event: {name}")
            lines.append(f"data: {json.dumps(data)}")
            chunk = ("\n".join(lines) + "\n\n").encode()
            self.wfile.write(f"{len(chunk):X}\r\n".encode())
            self.wfile.write(chunk)
            self.wfile.write(b"\r\n")
            self.wfile.flush()

        self.wfile.write(b"0\r\n\r\n")


class ClientTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.httpd = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        cls.port = cls.httpd.server_address[1]
        cls.thread = threading.Thread(target=cls.httpd.serve_forever, daemon=True)
        cls.thread.start()

    @classmethod
    def tearDownClass(cls):
        cls.httpd.shutdown()
        cls.httpd.server_close()

    def client(self, **kwargs):
        return Client(base_url=f"http://127.0.0.1:{self.port}", **kwargs)

    def test_health_and_models(self):
        client = self.client()
        self.assertEqual(client.health()["status"], "ok")
        self.assertTrue(client.ready()["ready"])
        models = client.models()
        self.assertEqual(models[0].id, "chat/mock")

    def test_chat(self):
        response = self.client().chat("chat/mock", "hi", system="be brief")
        self.assertEqual(response.content, "hello")
        self.assertEqual(response.total_tokens, 3)

    def test_chat_with_chat_message_objects(self):
        response = self.client().chat(
            "chat/mock", [ChatMessage.user("hi"), ChatMessage.assistant("prev")]
        )
        self.assertEqual(response.content, "hello")

    def test_chat_stream(self):
        chunks = list(self.client().chat_stream("chat/mock", "hi"))
        self.assertEqual("".join(chunks), "hello")

    def test_responses(self):
        response = self.client().responses("chat/mock", "hi", instructions="be brief")
        self.assertEqual(response.text, "hello")
        self.assertEqual(response.id, "resp_1")

    def test_responses_stream(self):
        events = list(self.client().responses_stream("chat/mock", "hi"))
        self.assertEqual(events[0].name, "response.created")
        self.assertEqual(events[1].delta(), "hi")
        self.assertEqual(events[-1].type, "response.completed")

    def test_embeddings_and_tts_and_decide(self):
        client = self.client()
        self.assertIn("data", client.embeddings("chat/mock", "hi"))
        self.assertEqual(len(client.tts("tts/mock", "hi")), 128)
        decision = client.decide("dm/laya", {"hp": 10})
        self.assertIn("action", decision.output)

    def test_error_carries_status_and_payload(self):
        with self.assertRaises(ClientError) as ctx:
            self.client().models.__self__._json("GET", "/v1/nope")
        self.assertEqual(ctx.exception.status, 404)


if __name__ == "__main__":
    unittest.main()