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
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DIST="$ROOT/dist"
TARGETS="${*:-all}"

mkdir -p "$DIST"/python "$DIST"/nodejs "$DIST"/rust "$DIST"/java

build_python() {
  command -v python3 >/dev/null || { echo "skip python: no python3"; return; }
  python3 -c "import build" 2>/dev/null || python3 -m pip install --quiet build
  python3 -m build "$ROOT/sdk/python" --outdir "$DIST/python"
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
  command -v mvn >/dev/null || { echo "skip java: no mvn"; return; }
  mvn -q -f "$ROOT/sdk/java/pom.xml" -DskipTests package
  cp "$ROOT"/sdk/java/target/*.jar "$DIST/java/"
}

build_go() {
  command -v go >/dev/null || { echo "skip go: no go toolchain"; return; }
  # Go 模块通过 git tag 发布，这里只做校验
  ( cd "$ROOT/sdk/go" && go vet ./... && go test ./... )
  echo "go: 由 git tag 发布（tag 即版本），无二进制产物"
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