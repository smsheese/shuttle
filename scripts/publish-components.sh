#!/usr/bin/env bash
# Build connector component archives and manifest.json for S3 publishing.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${SHUTTLE_COMPONENTS_OUT:-$ROOT/dist/components}"
VERSION="${SHUTTLE_COMPONENTS_VERSION:-$(node -p "require('$ROOT/shuttle-app/package.json').version")}"
BASE_URL="${SHUTTLE_COMPONENTS_BASE_URL:-https://components.example.com/shuttle/components}"
PLATFORM="${SHUTTLE_COMPONENTS_PLATFORM:-}"

platform_key() {
  if [[ -n "$PLATFORM" ]]; then
    echo "$PLATFORM"
    return
  fi
  local os arch
  os="$(uname -s)"
  arch="$(uname -m)"
  case "$os-$arch" in
    Linux-x86_64|Linux-amd64) echo "linux-x86_64" ;;
    Linux-aarch64|Linux-arm64) echo "linux-arm64" ;;
    Darwin-x86_64) echo "macos-x86_64" ;;
    Darwin-arm64) echo "macos-arm64" ;;
    MINGW*-x86_64|MSYS*-x86_64|CYGWIN*-x86_64) echo "windows-x86_64" ;;
    MINGW*-aarch64|MSYS*-aarch64|CYGWIN*-aarch64|MINGW*-arm64|MSYS*-arm64|CYGWIN*-arm64) echo "windows-arm64" ;;
    *) echo "unsupported-$os-$arch" >&2; exit 1 ;;
  esac
}

pack_dir() {
  tar -czf "$2" -C "$(dirname "$1")" "$(basename "$1")"
}

pack_file() {
  tar -czf "$2" -C "$(dirname "$1")" "$(basename "$1")"
}

PLATFORM_KEY="$(platform_key)"
STAGE="$OUT/staging/$VERSION/$PLATFORM_KEY"
PUBLISH="$OUT/publish/v$VERSION"
PLATFORM_DIR="$PUBLISH/$PLATFORM_KEY"
SCRIPTS_DIR="$PUBLISH/scripts"
MANIFEST="$PUBLISH/manifest.json"
TMP="$STAGE/tmp"

mkdir -p "$STAGE" "$PLATFORM_DIR" "$SCRIPTS_DIR" "$TMP"
rm -rf "$TMP"/*
MANIFEST_ENTRIES="$STAGE/entries.jsonl"
: > "$MANIFEST_ENTRIES"

add_entry() {
  local key="$1"
  local file="$2"
  local url_path="$3"
  node - "$MANIFEST_ENTRIES" "$key" "$file" "$BASE_URL" "$VERSION" "$url_path" <<'NODE'
const fs = require('fs');
const crypto = require('crypto');
const [,, out, key, file, baseUrl, version, urlPath] = process.argv;
const buf = fs.readFileSync(file);
const sha256 = crypto.createHash('sha256').update(buf).digest('hex');
const url = `${baseUrl.replace(/\/$/, '')}/v${version}/${urlPath}`;
fs.appendFileSync(out, JSON.stringify({ key, url, sha256, size: buf.length }) + '\n');
NODE
}

echo "==> Staging connector components for $PLATFORM_KEY (v$VERSION)"

echo "==> Native helpers"
"$ROOT/connectors/gowa/fetch.sh" || echo "warn: GOWA fetch skipped"
"$ROOT/connectors/tdlib/fetch.sh" || echo "warn: TDLib fetch skipped"
"$ROOT/connectors/signal/fetch.sh"

echo "==> Slim Python runtime (no connector deps)"
SHUTTLE_PYTHON_SKIP_DEPS=1 "$ROOT/scripts/fetch-python-runtime.sh"
"$ROOT/scripts/slim-python-runtime.sh" "$ROOT/connectors/python-runtime/current"

if [[ -d "$ROOT/connectors/python-runtime/current/python" ]]; then
  rm -rf "$TMP/python"
  cp -a "$ROOT/connectors/python-runtime/current/python" "$TMP/python"
  OUT_FILE="$PLATFORM_DIR/python-runtime.tar.gz"
  pack_dir "$TMP/python" "$OUT_FILE"
  add_entry "python-runtime" "$OUT_FILE" "$PLATFORM_KEY/python-runtime.tar.gz"
fi

if [[ -f "$ROOT/connectors/gowa/whatsapp" || -f "$ROOT/connectors/gowa/whatsapp.exe" ]]; then
  rm -rf "$TMP/gowa"
  mkdir -p "$TMP/gowa"
  cp -a "$ROOT/connectors/gowa/whatsapp"* "$TMP/gowa/" 2>/dev/null || true
  OUT_FILE="$PLATFORM_DIR/native-gowa.tar.gz"
  pack_dir "$TMP/gowa" "$OUT_FILE"
  add_entry "native:gowa" "$OUT_FILE" "$PLATFORM_KEY/native-gowa.tar.gz"
fi

if compgen -G "$ROOT/connectors/tdlib/"'tdjson*' >/dev/null || compgen -G "$ROOT/connectors/tdlib/libtdjson*' >/dev/null; then
  rm -rf "$TMP/tdlib"
  mkdir -p "$TMP/tdlib"
  cp -a "$ROOT/connectors/tdlib/"* "$TMP/tdlib/" 2>/dev/null || true
  OUT_FILE="$PLATFORM_DIR/native-tdlib.tar.gz"
  pack_dir "$TMP/tdlib" "$OUT_FILE"
  add_entry "native:tdlib" "$OUT_FILE" "$PLATFORM_KEY/native-tdlib.tar.gz"
fi

if [[ -d "$ROOT/connectors/signal" ]]; then
  rm -rf "$TMP/signal"
  mkdir -p "$TMP/signal"
  cp -a "$ROOT/connectors/signal/signal-cli"* "$TMP/signal/" 2>/dev/null || true
  [[ -d "$ROOT/connectors/signal/runtime" ]] && cp -a "$ROOT/connectors/signal/runtime" "$TMP/signal/"
  OUT_FILE="$PLATFORM_DIR/native-signal-cli.tar.gz"
  pack_dir "$TMP/signal" "$OUT_FILE"
  add_entry "native:signal-cli" "$OUT_FILE" "$PLATFORM_KEY/native-signal-cli.tar.gz"
fi

pack_python_deps() {
  local name="$1"
  local requirement="$2"
  local dest="$TMP/$name"
  rm -rf "$dest"
  mkdir -p "$dest"
  python3 -m pip install --disable-pip-version-check --upgrade pip wheel >/dev/null
  python3 -m pip install --disable-pip-version-check "$requirement" -t "$dest" >/dev/null
  local out="$PLATFORM_DIR/python-deps-$name.tar.gz"
  pack_dir "$dest" "$out"
  add_entry "python:deps:$name" "$out" "$PLATFORM_KEY/python-deps-$name.tar.gz"
}

if command -v python3 >/dev/null 2>&1; then
  echo "==> Python dependency packs"
  pack_python_deps messenger "fbchat>=2.0.0a5,<2.1"
  pack_python_deps instagram "instagrapi>=2.1.0"
fi

echo "==> Connector scripts"
for script in shuttle_ipc.py whatsapp-connector.py telegram-connector.py signal-connector.py messenger-connector.py instagram-connector.py email-connector.py matrix-connector.py; do
  src="$ROOT/connectors/$script"
  [[ -f "$src" ]] || continue
  cp -f "$src" "$SCRIPTS_DIR/$script"
  if [[ "$script" == "shuttle_ipc.py" ]]; then
    key="script:shuttle_ipc"
  else
    key="script:${script%-connector.py}"
  fi
  OUT_FILE="$PLATFORM_DIR/${key//:/-}.tar.gz"
  pack_file "$src" "$OUT_FILE"
  add_entry "$key" "$OUT_FILE" "$PLATFORM_KEY/${key//:/-}.tar.gz"
done

node - "$MANIFEST" "$MANIFEST_ENTRIES" "$VERSION" "$PLATFORM_KEY" <<'NODE'
const fs = require('fs');
const [,, manifestPath, entriesPath, version, platform] = process.argv;
let manifest = { schema: 1, shuttle_version: version, platforms: {} };
if (fs.existsSync(manifestPath)) {
  try { manifest = JSON.parse(fs.readFileSync(manifestPath, 'utf8')); } catch {}
}
manifest.schema = 1;
manifest.shuttle_version = version;
manifest.platforms[platform] = manifest.platforms[platform] || {};
const lines = fs.readFileSync(entriesPath, 'utf8').trim().split('\n').filter(Boolean);
for (const line of lines) {
  const entry = JSON.parse(line);
  manifest.platforms[platform][entry.key] = {
    url: entry.url,
    sha256: entry.sha256,
    size: entry.size,
  };
}
fs.mkdirSync(require('path').dirname(manifestPath), { recursive: true });
fs.writeFileSync(manifestPath, JSON.stringify(manifest, null, 2));
console.log('Wrote', manifestPath);
NODE

echo "==> Component staging complete"
echo "Publish root: $PUBLISH"
echo "Platform artifacts: $PLATFORM_DIR"
echo "Manifest: $MANIFEST"
