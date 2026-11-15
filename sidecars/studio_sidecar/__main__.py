"""命令行入口：`python -m studio_sidecar serve <engine>`。"""

from __future__ import annotations

import argparse
import sys

from . import adapters, server


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(prog="studio_sidecar", description="Basic-Studio Python sidecar")
    sub = parser.add_subparsers(dest="command", required=True)

    serve = sub.add_parser("serve", help="start the HTTP sidecar")
    serve.add_argument("engine", nargs="?", default="transformers", help="engine name (informational)")
    serve.add_argument("--host", default="127.0.0.1")
    serve.add_argument("--port", type=int, default=9000)
    serve.add_argument("--model", default=None, help="model directory (model.toml's parent)")
    serve.add_argument("--adapter", default=None, help="force an adapter (echo/transformers_llm/indextts_vXX)")

    return parser


def main(argv=None) -> int:
    args = build_parser().parse_args(argv)

    if args.command == "serve":
        forced = args.adapter
        model_path = args.model

        def resolve(request):
            if forced:
                module = adapters.load(forced)
            elif request.adapter:
                module = adapters.load(request.adapter)
            elif model_path and request.task == "tts":
                module = adapters.load(_pick_tts_adapter(request))
            else:
                module = adapters.load("echo" if not model_path else "transformers_llm")
            return module.handle

        server.serve(args.host, args.port, args.engine, resolve)
        return 0

    return 2


def _pick_tts_adapter(request) -> str:
    version = (request.api_version or "").lower().replace("v", "")
    mapping = {"10": "indextts_v1", "15": "indextts_v15", "20": "indextts_v20", "25": "indextts_v25"}
    return mapping.get(version, "indextts_v25")


if __name__ == "__main__":
    sys.exit(main())