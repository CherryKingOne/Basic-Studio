"""HTTP 服务端：仅依赖标准库，避免为了一个侧车引入 web 框架。"""

from __future__ import annotations

import json
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from typing import Callable, Dict, Iterable, List, Tuple

from .protocol import Event, InferRequest

Handler = Callable[[InferRequest], Iterable[Event]]


def make_handler(engine_name: str, resolve: Callable[[InferRequest], Handler]):
    class StudioSidecarHandler(BaseHTTPRequestHandler):
        server_version = "studio-sidecar/0.1"
        # HTTP/1.1 才能使用 chunked 传输（流式响应依赖它）
        protocol_version = "HTTP/1.1"

        def log_message(self, fmt: str, *args) -> None:  # noqa: A003
            return

        def _send(self, status: int, body: bytes, content_type: str) -> None:
            self.send_response(status)
            self.send_header("content-type", content_type)
            self.send_header("content-length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def _read_json(self) -> Dict:
            length = int(self.headers.get("content-length", "0") or 0)
            if length == 0:
                return {}
            raw = self.rfile.read(length)
            return json.loads(raw.decode("utf-8"))

        def do_GET(self) -> None:  # noqa: N802
            if self.path.rstrip("/") in ("/health", ""):
                self._send(200, json.dumps({"status": "ok", "engine": engine_name}).encode(), "application/json")
                return
            self._send(404, json.dumps({"error": "not found"}).encode(), "application/json")

        def do_POST(self) -> None:  # noqa: N802
            path = self.path.rstrip("/")
            if path not in ("/infer", "/infer/stream"):
                self._send(404, json.dumps({"error": "not found"}).encode(), "application/json")
                return

            try:
                payload = self._read_json()
            except json.JSONDecodeError as exc:
                self._send(400, json.dumps({"error": f"invalid json: {exc}"}).encode(), "application/json")
                return

            request = InferRequest.from_dict(payload)
            try:
                handler = resolve(request)
                events: List[Event] = list(handler(request))
            except Exception as exc:  # noqa: BLE001
                self._send(500, json.dumps({"error": str(exc)}).encode(), "application/json")
                return

            # /infer/stream 始终是流式（音频场景即使 stream=false 也走这里）
            if request.stream or path.endswith("/stream"):
                self._stream(events)
            else:
                body = _single_event(events).to_dict()
                self._send(200, json.dumps(body, ensure_ascii=False).encode("utf-8"), "application/json")

        def _stream(self, events: List[Event]) -> None:
            self.send_response(200)
            self.send_header("content-type", "application/x-ndjson")
            self.send_header("cache-control", "no-cache")
            self.send_header("transfer-encoding", "chunked")
            self.end_headers()

            for event in events:
                line = json.dumps(event.to_dict(), ensure_ascii=False) + "\n"
                chunk = line.encode("utf-8")
                self.wfile.write(f"{len(chunk):X}\r\n".encode("ascii"))
                self.wfile.write(chunk)
                self.wfile.write(b"\r\n")
                self.wfile.flush()

            self.wfile.write(b"0\r\n\r\n")

    return StudioSidecarHandler


def _single_event(events: List[Event]) -> Event:
    """非流式响应：把 delta 合并进 done 的 text，避免文本丢失。"""
    if not events:
        return Event.error("no events produced")

    done = next((e for e in reversed(events) if e.type == "done"), None)
    text = "".join(e.text or "" for e in events if e.type == "delta")

    if done is None:
        if text:
            return Event.done(text=text)
        return events[-1]

    if text and not done.text:
        done.text = text
    return done


def serve(host: str, port: int, engine_name: str, resolve: Callable[[InferRequest], Handler]) -> None:
    handler = make_handler(engine_name, resolve)
    httpd = ThreadingHTTPServer((host, port), handler)
    print(f"studio sidecar ({engine_name}) listening on http://{host}:{port}", flush=True)
    try:
        httpd.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        httpd.server_close()


def make_text_response(text: str) -> List[Event]:
    """把整段文本切分成若干 delta，便于观察流式效果。"""
    events: List[Event] = []
    for index in range(0, len(text), 8):
        events.append(Event.delta(text[index : index + 8]))
    events.append(Event.done(completion_tokens=max(1, len(text) // 4)))
    return events