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
  if [[ -d "$target" ]]; then rm -rf "$target"; fi
}

strip_dir "$ROOT/include"
strip_dir "$ROOT/share"
for lib in "$PY_LIB"/python3.*; do
  [[ -d "$lib" ]] || continue
  for sub in idlelib tkinter turtledemo test unittest ensurepip \
    site-packages/pip site-packages/setuptools site-packages/wheel; do
    strip_dir "$lib/$sub"
  done
done
find "$PY_LIB" -type d -name '__pycache__' -prune -exec rm -rf {} + 2>/dev/null || true
find "$ROOT" -name '*.pyc' -delete 2>/dev/null || true

echo "Slimmed Python runtime at $ROOT"
