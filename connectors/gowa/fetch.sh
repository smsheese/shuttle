#!/usr/bin/env bash
# Download the GOWA (go-whatsapp-web-multidevice) binary for this OS/arch.
set -euo pipefail
DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$DIR"

os="$(uname -s)"
arch="$(uname -m)"
case "$os-$arch" in
  Linux-x86_64|Linux-amd64) needle="linux_amd64" ;;
  Linux-aarch64|Linux-arm64) needle="linux_arm64" ;;
  Linux-armv7l) needle="linux_armv7" ;;
  Darwin-x86_64) needle="darwin_amd64" ;;
  Darwin-arm64) needle="darwin_arm64" ;;
  MINGW*-x86_64|MSYS*-x86_64|CYGWIN*-x86_64) needle="windows_amd64" ;;
  MINGW*-aarch64|MSYS*-aarch64|CYGWIN*-aarch64) needle="windows_arm64" ;;
  MINGW*-arm64|MSYS*-arm64|CYGWIN*-arm64) needle="windows_arm64" ;;
  *)
    echo "Unsupported platform: $os $arch" >&2
    exit 1
    ;;
esac

export NEEDLE="$needle"
url="$(python3 - <<'PY'
import json, os, urllib.request
needle = os.environ["NEEDLE"]
req = urllib.request.Request(
    "https://api.github.com/repos/aldinokemal/go-whatsapp-web-multidevice/releases/latest",
    headers={"User-Agent": "shuttle-gowa-fetch"},
)
data = json.loads(urllib.request.urlopen(req).read())
for a in data.get("assets", []):
    name = a.get("name", "")
    if needle in name and (name.endswith(".zip") or name.endswith(".tar.gz")):
        print(a["browser_download_url"])
        raise SystemExit(0)
raise SystemExit("no matching GOWA asset for " + needle)
PY
)"

echo "Downloading $url"
archive="$(basename "$url")"
curl -fsSL -o "$archive" "$url"

tmpdir="$(mktemp -d)"
if [[ "$archive" == *.zip ]]; then
  python3 - "$archive" "$tmpdir" <<'PY'
import sys, zipfile
zipfile.ZipFile(sys.argv[1]).extractall(sys.argv[2])
PY
else
  tar -xzf "$archive" -C "$tmpdir"
fi
rm -f "$archive"

bin="$(find "$tmpdir" -type f \( -name whatsapp -o -name whatsapp.exe \) | head -n 1 || true)"
if [[ -z "$bin" ]]; then
  bin="$(find "$tmpdir" -type f -perm -u+x ! -name '*.md' ! -name '*.txt' | head -n 1 || true)"
fi
if [[ -z "$bin" ]]; then
  bin="$(find "$tmpdir" -type f ! -name '*.md' ! -name '*.txt' ! -name '*.json' | head -n 1 || true)"
fi
if [[ -z "$bin" ]]; then
  echo "Could not find 'whatsapp' binary in the archive." >&2
  find "$tmpdir" -maxdepth 4 -print
  exit 1
fi
mv -f "$bin" "$DIR/whatsapp"
chmod +x "$DIR/whatsapp"
if [[ "$needle" == windows_* ]]; then
  mv -f "$DIR/whatsapp" "$DIR/whatsapp.exe" 2>/dev/null || true
fi
rm -rf "$tmpdir"
echo "Installed GOWA to $DIR/whatsapp"
"$DIR/whatsapp" --help >/dev/null 2>&1 || true
