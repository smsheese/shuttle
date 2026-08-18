#!/usr/bin/env bash
# Download signal-cli (prefers GraalVM native image, falls back to JVM tarball).
# Bundled in Shuttle release builds under GPL-3.0; see third-party/licenses/signal-cli-GPL-3.0.txt.
set -euo pipefail
DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$DIR/../.." && pwd)"
cd "$DIR"

os="${SHUTTLE_SIGNAL_OS:-$(uname -s)}"
arch="${SHUTTLE_SIGNAL_ARCH:-$(uname -m)}"
case "$os-$arch" in
  Linux-x86_64|Linux-amd64) native="Linux-native" ; jvm="Linux" ;;
  Linux-aarch64|Linux-arm64) native="Linux-arm64-native|Linux-aarch64-native" ; jvm="Linux" ;;
  Darwin-arm64) native="Darwin-native|macOS-arm64" ; jvm="Darwin|macOS" ;;
  Darwin-x86_64) native="Darwin-native|macOS-x64" ; jvm="Darwin|macOS" ;;
  Windows-x86_64|Windows-amd64) native="Windows-native|win.*x64" ; jvm="Windows" ;;
  MINGW*-x86_64|MSYS*-x86_64|CYGWIN*-x86_64) native="Windows-native|win.*x64" ; jvm="Windows" ;;
  Windows-aarch64|Windows-arm64) native="Windows-arm64-native|win.*arm64" ; jvm="Windows" ;;
  MINGW*-aarch64|MSYS*-aarch64|CYGWIN*-aarch64|MINGW*-arm64|MSYS*-arm64|CYGWIN*-arm64) native="Windows-arm64-native|win.*arm64" ; jvm="Windows" ;;
  *)
    echo "Unsupported platform: $os $arch" >&2
    exit 1
    ;;
esac

export NATIVE="$native" JVM="$jvm"
meta="$(python3 - <<'PY'
import json, os, re, urllib.request
native = re.compile(os.environ["NATIVE"], re.I)
jvm = re.compile(os.environ["JVM"], re.I)
req = urllib.request.Request(
    "https://api.github.com/repos/AsamK/signal-cli/releases/latest",
    headers={"User-Agent": "shuttle-signal-fetch"},
)
data = json.loads(urllib.request.urlopen(req, timeout=30).read())
native_url = jvm_url = None
for a in data.get("assets", []):
    name = a.get("name", "")
    url = a["browser_download_url"]
    if "native" in name.lower() and native.search(name) and (name.endswith(".tar.gz") or name.endswith(".zip")):
        native_url = url
    elif jvm.search(name) and name.endswith(".tar.gz") and "native" not in name.lower():
        jvm_url = url
print(json.dumps({
    "tag": data.get("tag_name", ""),
    "url": native_url or jvm_url or "",
    "kind": "native" if native_url else ("jvm" if jvm_url else ""),
}))
PY
)"

url="$(python3 -c 'import json,sys; print(json.loads(sys.argv[1])["url"])' "$meta")"
if [[ -z "$url" ]]; then
  echo "No signal-cli asset found for this platform." >&2
  exit 1
fi

tag="$(python3 -c 'import json,sys; print(json.loads(sys.argv[1])["tag"])' "$meta")"
kind="$(python3 -c 'import json,sys; print(json.loads(sys.argv[1])["kind"])' "$meta")"

echo "Downloading signal-cli $tag ($kind) from $url"
archive="$(basename "$url")"
curl -fsSL -o "$archive" "$url"
tmpdir="$(mktemp -d)"
if [[ "$archive" == *.zip ]]; then
  unzip -q "$archive" -d "$tmpdir"
else
  tar -xzf "$archive" -C "$tmpdir"
fi
rm -f "$archive"

bin="$(find "$tmpdir" -type f \( -name signal-cli -o -name signal-cli.exe \) | head -n 1 || true)"
if [[ -z "$bin" ]]; then
  echo "signal-cli binary missing from archive" >&2
  find "$tmpdir" -maxdepth 4 -print
  exit 1
fi

rm -rf "$DIR/runtime" "$DIR/signal-cli" "$DIR/signal-cli.exe"
if [[ -d "$(dirname "$bin")/../lib" ]]; then
  mv "$(dirname "$bin")/.." "$DIR/runtime"
  if [[ "$os" == MINGW* || "$os" == MSYS* || "$os" == CYGWIN* ]]; then
    cat > "$DIR/signal-cli.bat" <<'WRAP'
@echo off
set "DIR=%~dp0"
"%DIR%runtime\bin\signal-cli.bat" %*
WRAP
    cp -f "$DIR/signal-cli.bat" "$DIR/signal-cli.exe.bat" 2>/dev/null || true
    target="$DIR/signal-cli.bat"
  else
    cat > "$DIR/signal-cli" <<'WRAP'
#!/usr/bin/env sh
DIR="$(cd "$(dirname "$0")" && pwd)"
exec "$DIR/runtime/bin/signal-cli" "$@"
WRAP
    chmod +x "$DIR/signal-cli"
    target="$DIR/signal-cli"
  fi
else
  cp -f "$bin" "$DIR/signal-cli"
  chmod +x "$DIR/signal-cli"
  target="$DIR/signal-cli"
  if [[ "$bin" == *.exe ]]; then
    cp -f "$bin" "$DIR/signal-cli.exe"
  fi
fi
rm -rf "$tmpdir"

python3 - <<PY
import json
from pathlib import Path
meta = {
    "component": "signal-cli",
    "version": "$tag",
    "download_url": "$url",
    "kind": "$kind",
    "license": "GPL-3.0",
    "source": "https://github.com/AsamK/signal-cli",
}
Path("$DIR/SOURCE.json").write_text(json.dumps(meta, indent=2) + "\n", encoding="utf-8")
PY

license_src="$ROOT/third-party/licenses/signal-cli-GPL-3.0.txt"
if [[ -f "$license_src" ]]; then
  cp -f "$license_src" "$DIR/THIRD-PARTY-LICENSE.txt"
fi

echo "Installed signal-cli to $target"
echo "signal-cli is GPL-3.0. Source: https://github.com/AsamK/signal-cli"
