#!/usr/bin/env bash
# Download a prebuilt TDLib tdjson shared library.
set -euo pipefail
DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$DIR"

os="$(uname -s)"
arch="$(uname -m)"
case "$os-$arch" in
  Linux-x86_64|Linux-amd64) needle="linux.*x86_64|linux.*amd64|manylinux.*x86_64" ;;
  Linux-aarch64|Linux-arm64) needle="linux.*aarch64|linux.*arm64" ;;
  Darwin-arm64) needle="darwin.*arm64|macos.*arm64" ;;
  Darwin-x86_64) needle="darwin.*x86_64|macos.*x86_64" ;;
  MINGW*-x86_64|MSYS*-x86_64|CYGWIN*-x86_64) needle="windows.*x86_64|windows.*amd64|win.*amd64" ;;
  MINGW*-aarch64|MSYS*-aarch64|CYGWIN*-aarch64|MINGW*-arm64|MSYS*-arm64|CYGWIN*-arm64) needle="windows.*arm64|windows.*aarch64|win.*arm64" ;;
  *)
    echo "Unsupported platform: $os $arch" >&2
    echo "Build TDLib from https://github.com/tdlib/td and place libtdjson in $DIR" >&2
    exit 1
    ;;
esac

export NEEDLE="$needle"
url="$(python3 - <<'PY'
import json, os, re, urllib.request
needle = re.compile(os.environ["NEEDLE"], re.I)
req = urllib.request.Request(
    "https://api.github.com/repos/pylakey/aiotdlib/releases/latest",
    headers={"User-Agent": "shuttle-tdlib-fetch"},
)
try:
    data = json.loads(urllib.request.urlopen(req, timeout=30).read())
except Exception as e:
    raise SystemExit(f"could not list TDLib releases: {e}")
for a in data.get("assets", []):
    name = a.get("name", "")
    if needle.search(name) and any(name.endswith(ext) for ext in (".so", ".dylib", ".dll", ".whl", ".zip", ".tar.gz")):
        print(a["browser_download_url"])
        raise SystemExit(0)
raise SystemExit("no matching tdjson asset; build TDLib from https://github.com/tdlib/td")
PY
)"

echo "Downloading $url"
archive="$(basename "$url")"
curl -fsSL -o "$archive" "$url"

if [[ "$archive" == *.whl ]]; then
  python3 - "$archive" "$DIR" <<'PY'
import sys, zipfile
from pathlib import Path
zf = zipfile.ZipFile(sys.argv[1])
dest = Path(sys.argv[2])
for name in zf.namelist():
    if "tdjson" in name and name.endswith((".so", ".dylib", ".dll")):
        target = dest / Path(name).name
        target.write_bytes(zf.read(name))
        print("extracted", target)
        break
else:
    raise SystemExit("no tdjson library inside wheel")
PY
elif [[ "$archive" == *.zip ]]; then
  python3 - "$archive" "$DIR" <<'PY'
import sys, zipfile
from pathlib import Path
zf = zipfile.ZipFile(sys.argv[1])
dest = Path(sys.argv[2])
for name in zf.namelist():
    if "tdjson" in Path(name).name:
        (dest / Path(name).name).write_bytes(zf.read(name))
        print("extracted", Path(name).name)
        break
PY
else
  chmod +x "$archive" 2>/dev/null || true
  if [[ "$archive" == *tdjson* ]]; then
    mv -f "$archive" "$DIR/libtdjson.so"
  fi
fi
rm -f "$archive"
ls -l "$DIR"/libtdjson* "$DIR"/tdjson* 2>/dev/null || true
echo "TDLib installed under $DIR (set SHUTTLE_TDLIB if the filename differs)"
