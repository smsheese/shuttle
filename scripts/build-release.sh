#!/usr/bin/env bash
# Production bundle for the current host OS/arch.
# Example: ./scripts/build-release.sh -b deb
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"

# Drop a leading "--" so `./script -- -b deb` and `./script -b deb` both work.
args=("$@")
if [[ ${#args[@]} -gt 0 && ${args[0]} == "--" ]]; then
  args=("${args[@]:1}")
fi

echo "==> Installing frontend dependencies"
cd "$ROOT/shuttle-app"
npm ci

echo "==> Building Shuttle"
npx tauri build "${args[@]}"

echo "==> Done. Bundles:"
find "$ROOT/shuttle-app/src-tauri/target" -maxdepth 4 -type f \( \
  -name '*.deb' -o -name '*.AppImage' -o -name '*.dmg' -o -name '*.msi' -o -name '*.exe' \
\) 2>/dev/null | sort || true
