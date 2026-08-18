#!/usr/bin/env bash
# Stage Windows amd64 connector assets for cross-compiling on Linux.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"

export SHUTTLE_PYTHON_TRIPLE=x86_64-pc-windows-msvc
export SHUTTLE_FORCE_PYTHON_FETCH=1
export SHUTTLE_SIGNAL_OS=Windows
export SHUTTLE_SIGNAL_ARCH=x86_64

echo "==> Windows Python runtime (MSVC build for Windows bundles)"
"$ROOT/scripts/fetch-python-runtime.sh"

echo "==> Windows signal-cli"
if ! SHUTTLE_SIGNAL_OS=Windows SHUTTLE_SIGNAL_ARCH=x86_64 "$ROOT/connectors/signal/fetch.sh"; then
  echo "warn: no Windows signal-cli release asset; shipping placeholder (Signal connector requires manual install on Windows)" >&2
  cat > "$ROOT/connectors/signal/signal-cli.bat" <<'WRAP'
@echo off
echo signal-cli is not bundled for this platform release. Install from https://github.com/AsamK/signal-cli or set SHUTTLE_SIGNAL_CLI. >&2
exit 1
WRAP
fi

if [[ ! -f "$ROOT/connectors/signal/signal-cli.exe" && ! -f "$ROOT/connectors/signal/signal-cli.bat" && ! -f "$ROOT/connectors/signal/signal-cli" ]]; then
  echo "error: Windows signal-cli placeholder missing" >&2
  exit 1
fi

if [[ ! -f "$ROOT/third-party/licenses/AGPL-3.0.txt" ]]; then
  cp -f "$ROOT/LICENSE" "$ROOT/third-party/licenses/AGPL-3.0.txt"
fi

if [[ ! -d "$ROOT/connectors/signal/runtime" ]]; then
  mkdir -p "$ROOT/connectors/signal/runtime"
  echo "Native signal-cli build; no JVM runtime bundled." > "$ROOT/connectors/signal/runtime/README"
fi

echo "==> Windows release assets staged"
