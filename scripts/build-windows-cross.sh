#!/usr/bin/env bash
# Cross-compile Windows amd64 bundle from Linux using cargo-xwin (MSVC target).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"

if ! command -v cargo-xwin >/dev/null; then
  echo "Install cargo-xwin: cargo install cargo-xwin --locked" >&2
  exit 1
fi

rustup target add x86_64-pc-windows-msvc >/dev/null 2>&1 || true

cd "$ROOT/shuttle-app"
npm ci

"$ROOT/scripts/stage-windows-assets.sh"

echo "==> Building Windows bundle (x86_64-pc-windows-msvc via cargo-xwin)"
npm run tauri build -- --runner cargo-xwin --target x86_64-pc-windows-msvc -b nsis "$@"

echo "==> Windows artifacts:"
find "$ROOT/shuttle-app/src-tauri/target" -path '*x86_64-pc-windows-msvc*' -type f \( \
  -name '*.exe' -o -name '*.msi' \) 2>/dev/null | sort || true
