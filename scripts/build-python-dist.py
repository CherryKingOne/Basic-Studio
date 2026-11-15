#!/usr/bin/env python3
"""不依赖 pip/setuptools 构建 Python 产物（wheel + sdist）。

用法：
    python3 scripts/build-python-dist.py [输出目录]

产物：
    basic_studio-<ver>-py3-none-any.whl
    basic_studio-<ver>.tar.gz
"""

from __future__ import annotations

import base64
import hashlib
import io
import os
import sys
import tarfile
import time
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
PKG_DIR = ROOT / "sdk" / "python"
NAME = "basic-studio"
MODULE = "basic_studio"
VERSION = "0.0.1"
SUMMARY = "Official Python SDK for Basic-Studio inference control plane"
REQUIRES_PYTHON = ">=3.9"

METADATA = f"""Metadata-Version: 2.1
Name: {NAME}
Version: {VERSION}
Summary: {SUMMARY}
Requires-Python: {REQUIRES_PYTHON}
License: MIT
Project-URL: Homepage, https://github.com/CherryKingOne/Basic-Studio
Description-Content-Type: text/markdown

{SUMMARY}

See https://github.com/CherryKingOne/Basic-Studio/tree/main/sdk/python
"""

WHEEL = """Wheel-Version: 1.0
Generator: build-python-dist.py
Root-Is-Purelib: true
Tag: py3-none-any
"""


def collect_sources() -> list[Path]:
    files = sorted((PKG_DIR / MODULE).rglob("*.py"))
    if not files:
        raise SystemExit("no python sources found")
    return files


def record_line(name: str, data: bytes) -> str:
    digest = base64.urlsafe_b64encode(hashlib.sha256(data).digest()).decode().rstrip("=")
    return f"{name},sha256={digest},{len(data)}"


def build_wheel(destination: Path) -> Path:
    files = collect_sources()
    dist_info = f"{MODULE.replace('-', '_')}-{VERSION}.dist-info"
    wheel_name = f"{MODULE.replace('-', '_')}-{VERSION}-py3-none-any.whl"
    wheel_path = destination / wheel_name

    records: list[str] = []
    with zipfile.ZipFile(wheel_path, "w", zipfile.ZIP_DEFLATED) as archive:
        for source in files:
            arcname = str(Path(MODULE, source.relative_to(PKG_DIR / MODULE)).as_posix())
            data = source.read_bytes()
            archive.writestr(arcname, data)
            records.append(record_line(arcname, data))

        metadata = METADATA.encode()
        wheel_meta = WHEEL.encode()
        archive.writestr(f"{dist_info}/METADATA", metadata)
        records.append(record_line(f"{dist_info}/METADATA", metadata))
        archive.writestr(f"{dist_info}/WHEEL", wheel_meta)
        records.append(record_line(f"{dist_info}/WHEEL", wheel_meta))
        archive.writestr(f"{dist_info}/top_level.txt", MODULE.encode())

        record_name = f"{dist_info}/RECORD"
        archive.writestr(record_name, "\n".join(records) + f"\n{record_name},,\n")

    return wheel_path


def build_sdist(destination: Path) -> Path:
    sdist_name = f"{NAME.replace('-', '_')}-{VERSION}"
    sdist_path = destination / f"{sdist_name}.tar.gz"

    payload: list[tuple[str, bytes]] = [
        (f"{sdist_name}/pyproject.toml", (PKG_DIR / "pyproject.toml").read_bytes()),
        (f"{sdist_name}/README.md", (PKG_DIR / "README.md").read_bytes()),
        (f"{sdist_name}/PKG-INFO", METADATA.encode()),
    ]
    for source in collect_sources():
        relative = source.relative_to(PKG_DIR).as_posix()
        payload.append((f"{sdist_name}/{relative}", source.read_bytes()))

    with tarfile.open(sdist_path, "w:gz") as archive:
        for name, data in payload:
            info = tarfile.TarInfo(name)
            info.size = len(data)
            info.mtime = int(time.time())
            archive.addfile(info, io.BytesIO(data))

    return sdist_path


def main() -> None:
    destination = Path(sys.argv[1]) if len(sys.argv) > 1 else ROOT / "dist" / "python"
    destination.mkdir(parents=True, exist_ok=True)

    wheel = build_wheel(destination)
    sdist = build_sdist(destination)

    print(f"wheel: {wheel}")
    print(f"sdist: {sdist}")
    assert os.path.getsize(wheel) > 0


if __name__ == "__main__":
    main()