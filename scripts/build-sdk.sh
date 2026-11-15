#!/usr/bin/env bash
# 构建全部 SDK 发布产物到 dist/。
#
# 用法：
#   ./scripts/build-sdk.sh            # 全部（缺工具链的部分会跳过）
#   ./scripts/build-sdk.sh python     # 只构建 Python
#
# 产物：
#   dist/python/*.whl, *.tar.gz
#   dist/nodejs/*.tgz
#   dist/rust/*.crate
#   dist/java/*.jar
#   dist/go/sdk-go-<ver>-src.zip
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DIST="$ROOT/dist"
TARGETS="${*:-all}"

mkdir -p "$DIST/python" "$DIST/nodejs" "$DIST/rust" "$DIST/java" "$DIST/go"

build_python() {
  command -v python3 >/dev/null || { echo "skip python: no python3"; return; }
  if python3 -c "import build" 2>/dev/null; then
    python3 -m build "$ROOT/sdk/python" --outdir "$DIST/python"
  else
    # 没有 pip/build 时，用零依赖脚本生成 wheel + sdist
    python3 "$ROOT/scripts/build-python-dist.py" "$DIST/python"
  fi
}

build_nodejs() {
  command -v npm >/dev/null || { echo "skip nodejs: no npm"; return; }
  ( cd "$ROOT/sdk/nodejs" && npm pack --pack-destination "$DIST/nodejs" )
}

build_rust() {
  command -v cargo >/dev/null || { echo "skip rust: no cargo"; return; }
  cargo package -p studio-sdk --allow-dirty --no-verify
  cp "$ROOT"/target/package/*.crate "$DIST/rust/"
}

build_java() {
  if ! command -v javac >/dev/null; then
    echo "skip java: no javac"
    return
  fi

  local deps="$ROOT/sdk/java/deps"
  local classes="$ROOT/sdk/java/target/classes"
  mkdir -p "$deps"

  # 缺少 Jackson 时从 Maven 镜像下载（Maven Central 会限流）
  if ! ls "$deps"/jackson-databind-*.jar >/dev/null 2>&1; then
    echo "downloading jackson jars into sdk/java/deps"
    local base="${MAVEN_MIRROR:-https://maven.aliyun.com/repository/public}"
    local artifact
    for artifact in \
      "com/fasterxml/jackson/core/jackson-databind/2.17.2/jackson-databind-2.17.2.jar" \
      "com/fasterxml/jackson/core/jackson-core/2.17.2/jackson-core-2.17.2.jar" \
      "com/fasterxml/jackson/core/jackson-annotations/2.17.2/jackson-annotations-2.17.2.jar"
    do
      curl -sSL -o "$deps/$(basename "$artifact")" "$base/$artifact"
    done
  fi

  local cp=""
  local jar
  for jar in "$deps"/*.jar; do
    [ -f "$jar" ] && cp="$cp$jar:"
  done

  rm -rf "$classes"
  mkdir -p "$classes"
  javac --release 17 -nowarn -d "$classes" -cp "$cp" \
    $(find "$ROOT/sdk/java/src/main/java" -name '*.java')

  local manifest="$ROOT/sdk/java/target/manifest.txt"
  printf 'Manifest-Version: 1.0\nImplementation-Title: Basic-Studio Java SDK\nImplementation-Version: 0.0.1\n\n' > "$manifest"

  jar cfm "$DIST/java/basic-studio-sdk-0.0.1.jar" "$manifest" -C "$classes" .
  jar cf "$DIST/java/basic-studio-sdk-0.0.1-sources.jar" -C "$ROOT/sdk/java/src/main/java" .
}

build_go() {
  if command -v go >/dev/null; then
    ( cd "$ROOT/sdk/go" && go vet ./... && go test ./... )
  else
    echo "skip go vet/test: no go toolchain"
  fi

  python3 - "$ROOT/sdk/go" "$DIST/go/sdk-go-0.0.1-src.zip" <<'PY'
import pathlib
import sys
import zipfile

source = pathlib.Path(sys.argv[1])
target = pathlib.Path(sys.argv[2])
with zipfile.ZipFile(target, "w", zipfile.ZIP_DEFLATED) as archive:
    for path in sorted(source.rglob("*")):
        if path.is_file():
            archive.write(path, path.as_posix())
print("zip:", target)
PY
}

run() {
  case "$1" in
    python) build_python ;;
    nodejs) build_nodejs ;;
    rust) build_rust ;;
    java) build_java ;;
    go) build_go ;;
    *) echo "unknown target: $1" >&2; return 1 ;;
  esac
}

if [ "$TARGETS" = "all" ]; then
  for name in python nodejs rust java go; do
    echo "==> $name"
    run "$name"
  done
else
  for name in $TARGETS; do
    echo "==> $name"
    run "$name"
  done
fi

echo
echo "产物目录："
find "$DIST" -type f | sort