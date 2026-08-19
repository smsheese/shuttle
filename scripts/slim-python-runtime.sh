#!/usr/bin/env bash
# Remove non-essential files from a standalone Python runtime tree.
set -euo pipefail

ROOT="${1:-}"
if [[ -z "$ROOT" || ! -d "$ROOT" ]]; then
  echo "usage: $0 <python-runtime-root>" >&2
  exit 1
fi

PY_LIB="$ROOT/python/lib"
if [[ ! -d "$PY_LIB" ]]; then
  PY_LIB="$ROOT/lib"
fi

strip_dir() {
  local target="$1"
  [[ -d "$target" ]] && rm -rf "$target"
}

strip_dir "$ROOT/include"
strip_dir "$ROOT/share"
strip_dir "$PY_LIB/python3.14/idlelib"
strip_dir "$PY_LIB/python3.14/tkinter"
strip_dir "$PY_LIB/python3.14/turtledemo"
strip_dir "$PY_LIB/python3.14/test"
strip_dir "$PY_LIB/python3.14/unittest"
strip_dir "$PY_LIB/python3.14/ensurepip"
strip_dir "$PY_LIB/python3.14/site-packages/pip"
strip_dir "$PY_LIB/python3.14/site-packages/setuptools"
strip_dir "$PY_LIB/python3.14/site-packages/wheel"
find "$PY_LIB" -type d -name '__pycache__' -prune -exec rm -rf {} + 2>/dev/null || true
find "$ROOT" -name '*.pyc' -delete 2>/dev/null || true

echo "Slimmed Python runtime at $ROOT"
