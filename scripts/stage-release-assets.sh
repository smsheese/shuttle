#!/usr/bin/env bash
# Prepare bundled runtime, signal-cli, and license files before a release build.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"

echo "==> Bundled Python runtime"
"$ROOT/scripts/fetch-python-runtime.sh"

echo "==> signal-cli (GPL-3.0, required for Signal connector)"
"$ROOT/connectors/signal/fetch.sh"

echo "==> Optional native helpers"
"$ROOT/connectors/gowa/fetch.sh" || echo "warn: GOWA fetch skipped"
"$ROOT/connectors/tdlib/fetch.sh" || echo "warn: TDLib fetch skipped"

if [[ ! -f "$ROOT/connectors/signal/signal-cli" && ! -f "$ROOT/connectors/signal/signal-cli.exe" && ! -f "$ROOT/connectors/signal/signal-cli.bat" ]]; then
  echo "error: signal-cli missing after fetch" >&2
  exit 1
fi

if [[ ! -f "$ROOT/third-party/licenses/signal-cli-GPL-3.0.txt" ]]; then
  echo "error: signal-cli GPL license text missing" >&2
  exit 1
fi

if [[ ! -f "$ROOT/third-party/licenses/AGPL-3.0.txt" ]]; then
  cp -f "$ROOT/LICENSE" "$ROOT/third-party/licenses/AGPL-3.0.txt"
fi

# Tauri resource map includes runtime/; native builds do not ship a JVM tree.
if [[ ! -d "$ROOT/connectors/signal/runtime" ]]; then
  mkdir -p "$ROOT/connectors/signal/runtime"
  echo "Native signal-cli build; no JVM runtime bundled." > "$ROOT/connectors/signal/runtime/README"
fi

echo "==> Release assets staged"
