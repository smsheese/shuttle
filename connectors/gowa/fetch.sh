#!/usr/bin/env bash
# Download the pinned GOWA (go-whatsapp-web-multidevice) release for this OS/arch
# and verify its SHA-256 against connectors/gowa/checksums.txt before installing.
#
# Environment:
#   SHUTTLE_GOWA_VERSION  release tag to fetch instead of the pinned one (empty = pinned)
#
# Flags:
#   --print-version       print the release tag this script would fetch, then exit
#
# Exit codes:
#   0  installed (verified, or unverified with a warning)
#   1  unsupported platform, no matching asset, or network/extract failure
#   3  SHA-256 mismatch against checksums.txt (archive deleted; do not package it)
set -euo pipefail
DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$DIR"

# Pinned GOWA release. To move the pin, follow "Moving the pin" in README.md:
# update this tag and regenerate checksums.txt in the same change.
GOWA_PINNED_VERSION="v9.9.0"
GOWA_VERSION="${SHUTTLE_GOWA_VERSION:-$GOWA_PINNED_VERSION}"
GOWA_REPO="aldinokemal/go-whatsapp-web-multidevice"
CHECKSUMS="$DIR/checksums.txt"

if [[ "${1:-}" == "--print-version" ]]; then
  echo "$GOWA_VERSION"
  exit 0
fi

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

# SHA-256 of a file, computed with python3's hashlib so it works on Linux,
# macOS and Windows Git Bash alike (no sha256sum/shasum dependency).
sha256_file() {
  python3 -I -c '
import hashlib, sys
h = hashlib.sha256()
with open(sys.argv[1], "rb") as f:
    for chunk in iter(lambda: f.read(1 << 20), b""):
        h.update(chunk)
print(h.hexdigest())
' "$1"
}

# Expected SHA-256 for an archive file name, read from checksums.txt in
# sha256sum format ("<hex>  <name>"). Prints nothing if there is no entry.
expected_sha256() {
  local want="$1" hash name
  [[ -f "$CHECKSUMS" ]] || return 0
  while read -r hash name; do
    name="${name%$'\r'}"
    name="${name#\*}"
    [[ -n "$hash" && "$hash" != \#* && "$name" == "$want" ]] || continue
    printf '%s\n' "$hash" | tr 'A-F' 'a-f'
    return 0
  done < "$CHECKSUMS"
}

asset_url="$(python3 -I - "$GOWA_REPO" "$GOWA_VERSION" "$needle" <<'PY'
import json, sys, urllib.error, urllib.request
repo, tag, needle = sys.argv[1], sys.argv[2], sys.argv[3]
req = urllib.request.Request(
    f"https://api.github.com/repos/{repo}/releases/tags/{tag}",
    headers={"User-Agent": "shuttle-gowa-fetch", "Accept": "application/vnd.github+json"},
)
try:
    data = json.loads(urllib.request.urlopen(req).read())
except urllib.error.HTTPError as e:
    raise SystemExit(f"could not read GOWA release {tag}: HTTP {e.code}")
except OSError as e:
    raise SystemExit(f"could not reach GitHub for GOWA release {tag}: {e}")
for a in data.get("assets", []):
    name = a.get("name", "")
    if needle in name and (name.endswith(".zip") or name.endswith(".tar.gz")):
        print(a["browser_download_url"])
        raise SystemExit(0)
raise SystemExit(f"no GOWA asset for {needle} in release {tag}")
PY
)"

echo "Downloading GOWA $GOWA_VERSION: $asset_url"
archive="$(basename "$asset_url")"
curl -fsSL -o "$archive" "$asset_url"

expected="$(expected_sha256 "$archive")"
if [[ -z "$expected" ]]; then
  echo "warning: no SHA-256 entry for $archive (GOWA $GOWA_VERSION) in checksums.txt." >&2
  echo "warning: the download is UNVERIFIED. Pinned version is $GOWA_PINNED_VERSION." >&2
else
  actual="$(sha256_file "$archive")"
  if [[ "$actual" != "$expected" ]]; then
    rm -f "$archive"
    echo "error: SHA-256 mismatch for $archive (GOWA $GOWA_VERSION)." >&2
    echo "  expected: $expected" >&2
    echo "  actual:   $actual" >&2
    echo "The archive was deleted. No binary was installed from it; do not package GOWA from this run." >&2
    exit 3
  fi
  echo "Verified SHA-256 for $archive"
fi

tmpdir="$(mktemp -d)"
if [[ "$archive" == *.zip ]]; then
  python3 -I - "$archive" "$tmpdir" <<'PY'
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
echo "Installed GOWA $GOWA_VERSION to $DIR/whatsapp"
"$DIR/whatsapp" --help >/dev/null 2>&1 || true
