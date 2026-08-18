#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DEST="$ROOT/connectors/python-runtime/current"
TMPDIR="$(mktemp -d)"
cleanup() { rm -rf "$TMPDIR"; }
trap cleanup EXIT

os="$(uname -s)"
arch="$(uname -m)"
if [[ -n "${SHUTTLE_PYTHON_TRIPLE:-}" ]]; then
  triple="$SHUTTLE_PYTHON_TRIPLE"
else
case "$os-$arch" in
  Linux-x86_64|Linux-amd64) triple="x86_64-unknown-linux-gnu" ;;
  Linux-aarch64|Linux-arm64) triple="aarch64-unknown-linux-gnu" ;;
  Darwin-x86_64) triple="x86_64-apple-darwin" ;;
  Darwin-arm64) triple="aarch64-apple-darwin" ;;
  MINGW*-x86_64|MSYS*-x86_64|CYGWIN*-x86_64) triple="x86_64-pc-windows-msvc" ;;
  MINGW*-aarch64|MSYS*-aarch64|CYGWIN*-aarch64|MINGW*-arm64|MSYS*-arm64|CYGWIN*-arm64) triple="aarch64-pc-windows-msvc" ;;
  *)
    echo "Unsupported platform for bundled Python: $os $arch" >&2
    exit 1
    ;;
esac
fi

meta_file_json="$TMPDIR/release.json"
curl -fsSL https://api.github.com/repos/astral-sh/python-build-standalone/releases/latest -o "$meta_file_json"

asset_json="$(node - "$triple" "$meta_file_json" <<'NODE'
const fs = require('fs');
const triple = process.argv[2];
const release = JSON.parse(fs.readFileSync(process.argv[3], 'utf8'));
const assets = release.assets || [];
const stable = [];
for (const asset of assets) {
  const name = asset.name || '';
  const m = name.match(/^cpython-(\d+)\.(\d+)\.(\d+)\+.*-(install_only(?:_stripped)?)\.tar\.gz$/);
  if (!m) continue;
  if (!name.includes(`-${triple}-`)) continue;
  if (name.includes('-freethreaded-')) continue;
  const [, major, minor, patch, flavor] = m;
  stable.push({
    name,
    url: asset.browser_download_url,
    version: [Number(major), Number(minor), Number(patch)],
    stripped: flavor.includes('stripped'),
  });
}
stable.sort((a, b) => {
  for (let i = 0; i < 3; i++) {
    if (a.version[i] !== b.version[i]) return b.version[i] - a.version[i];
  }
  if (a.stripped !== b.stripped) return a.stripped ? -1 : 1;
  return 0;
});
if (!stable.length) {
  console.error(`No standalone Python asset found for ${triple}`);
  process.exit(1);
}
process.stdout.write(JSON.stringify(stable[0]));
NODE
)"

asset_name="$(node -e 'const a=JSON.parse(process.argv[1]); process.stdout.write(a.name)' "$asset_json")"
asset_url="$(node -e 'const a=JSON.parse(process.argv[1]); process.stdout.write(a.url)' "$asset_json")"

mkdir -p "$DEST"
meta_file="$DEST/.python-runtime-meta.json"
if [[ -f "$meta_file" ]]; then
  current_name="$(node -e 'const fs=require("fs"); const p=process.argv[1]; try { const j=JSON.parse(fs.readFileSync(p,"utf8")); process.stdout.write(j.asset_name||""); } catch { process.stdout.write(""); }' "$meta_file")"
  if [[ "$current_name" == "$asset_name" ]] && [[ "${SHUTTLE_FORCE_PYTHON_FETCH:-0}" != "1" ]]; then
    echo "Bundled Python already prepared: $asset_name"
    exit 0
  fi
fi

archive="$TMPDIR/$asset_name"
extract_dir="$TMPDIR/extracted"
mkdir -p "$extract_dir"

echo "Downloading standalone Python runtime: $asset_name"
curl -fL "$asset_url" -o "$archive"
tar -xzf "$archive" -C "$extract_dir"

runtime_root="$(node - "$extract_dir" <<'NODE'
const fs = require('fs');
const path = require('path');
const root = process.argv[2];
function exists(p) { try { fs.accessSync(p); return true; } catch { return false; } }
function search(dir, depth) {
  if (depth < 0) return null;
  const entries = fs.readdirSync(dir, { withFileTypes: true });
  for (const entry of entries) {
    const full = path.join(dir, entry.name);
    if (!entry.isDirectory()) continue;
    if (exists(path.join(full, 'bin', 'python3')) || exists(path.join(full, 'python.exe')) || exists(path.join(full, 'install', 'bin', 'python3')) || exists(path.join(full, 'install', 'python.exe'))) {
      return full;
    }
    const nested = search(full, depth - 1);
    if (nested) return nested;
  }
  return null;
}
const found = search(root, 4);
if (!found) process.exit(1);
process.stdout.write(found);
NODE
)"

rm -rf "$DEST/python"
mkdir -p "$DEST/python"
cp -a "$runtime_root"/. "$DEST/python/"

if [[ -d "$DEST/python/install" ]]; then
  tmp_move="$TMPDIR/install-normalized"
  mkdir -p "$tmp_move"
  cp -a "$DEST/python/install"/. "$tmp_move/"
  rm -rf "$DEST/python"
  mkdir -p "$DEST/python"
  cp -a "$tmp_move"/. "$DEST/python/"
fi

if [[ -x "$DEST/python/bin/python3" ]]; then
  PYTHON_BIN="$DEST/python/bin/python3"
elif [[ -x "$DEST/python/bin/python" ]]; then
  PYTHON_BIN="$DEST/python/bin/python"
elif [[ -f "$DEST/python/python.exe" ]]; then
  PYTHON_BIN="$DEST/python/python.exe"
else
  echo "Could not find Python executable in staged runtime" >&2
  exit 1
fi

echo "Bootstrapping pip into bundled runtime"
if [[ "$PYTHON_BIN" == *.exe ]] && [[ "$(uname -s)" != MINGW* && "$(uname -s)" != MSYS* && "$(uname -s)" != CYGWIN* ]]; then
  SITE="$DEST/python/Lib/site-packages"
  mkdir -p "$SITE"
  echo "Cross-host Windows runtime: installing connector deps with host pip"
  python3 -m pip install --disable-pip-version-check --upgrade pip setuptools wheel --break-system-packages >/dev/null
  python3 -m pip install --disable-pip-version-check -r "$ROOT/connectors/requirements.txt" \
    --target "$SITE" \
    --platform win_amd64 \
    --python-version 3.14 \
    --implementation cp \
    --only-binary=:all: \
    --break-system-packages 2>/dev/null || {
      echo "warn: some connector wheels may be missing for win_amd64; continuing" >&2
    }
else
  "$PYTHON_BIN" -m ensurepip --upgrade >/dev/null 2>&1 || true
  "$PYTHON_BIN" -m pip install --upgrade pip setuptools wheel >/dev/null
  "$PYTHON_BIN" -m pip install --disable-pip-version-check -r "$ROOT/connectors/requirements.txt" >/dev/null
fi

node - "$meta_file" "$asset_name" "$asset_url" <<'NODE'
const fs = require('fs');
const [file, asset_name, asset_url] = process.argv.slice(2);
fs.writeFileSync(file, JSON.stringify({
  asset_name,
  asset_url,
  prepared_at: new Date().toISOString()
}, null, 2));
NODE

echo "Bundled Python ready at $DEST/python"
