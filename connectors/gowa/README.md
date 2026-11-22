# GOWA sidecar

Shuttle talks to WhatsApp through [GOWA](https://github.com/aldinokemal/go-whatsapp-web-multidevice) (`go-whatsapp-web-multidevice`). The WhatsApp connector process starts GOWA bound to `127.0.0.1`, authenticates with HTTP basic auth, and never exposes it on the LAN.

## Install the binary

```bash
./connectors/gowa/fetch.sh
```

That downloads the pinned GOWA release into `connectors/gowa/whatsapp`. Override with `SHUTTLE_GOWA_BIN` if you already have a GOWA binary.

Point Shuttle at an already-running instance with `SHUTTLE_GOWA_URL` (still must be loopback).

### Pinned version

`fetch.sh` does not follow GitHub's "latest" release. It fetches the exact tag in `GOWA_PINNED_VERSION` (currently `v9.9.0`), so every build gets the same GOWA. Use `./connectors/gowa/fetch.sh --print-version` to see the tag it will fetch.

The release assets are the archives for `linux_amd64`, `linux_arm64`, `linux_armv7`, `darwin_amd64`, `darwin_arm64`, `windows_amd64` and `windows_arm64`. `v9.9.0` has no `windows_arm64` asset, so `fetch.sh` fails on Windows ARM and `scripts/publish-components.sh` skips GOWA there with a warning.

### Checksum verification

Each pinned archive's SHA-256 is listed in [`checksums.txt`](checksums.txt) (`sha256sum` format). After download, `fetch.sh` hashes the archive and compares it with the entry for that archive's file name. Outcomes:

- **Match:** the archive is extracted and installed.
- **Mismatch:** the archive is deleted, nothing is installed, and the script exits with code `3`. `scripts/publish-components.sh` stops on exit code `3`, so a bad binary is never packaged. Any other failure (unsupported platform, no network) is skipped with a warning, as before.
- **No entry:** the download is installed but reported as `UNVERIFIED`. This happens for any tag other than the pinned one, because `checksums.txt` lists only the pinned release.

You can check the file yourself after downloading the release archives: `sha256sum -c connectors/gowa/checksums.txt`.

### Override the version

```bash
SHUTTLE_GOWA_VERSION=v9.10.0 ./connectors/gowa/fetch.sh
```

An empty `SHUTTLE_GOWA_VERSION` means the pinned version. Any other version is installed unverified (with a warning). Use this to test a newer GOWA before moving the pin. Do not ship it without moving the pin.

`scripts/publish-components.sh` honours the same variable and records the GOWA tag it packaged as `version` on the `native:gowa` entry in `manifest.json`.

### Move the pin

1. Pick the new release tag and check its changelog for behaviour changes that could affect the connector.
2. Regenerate `checksums.txt` from that release's assets. Each entry is the asset's SHA-256, which GitHub's API reports in its `digest` field (if an asset has no `digest`, download it and run `sha256sum` instead):

   ```bash
   TAG=v9.10.0
   curl -fsSL -H "User-Agent: shuttle-gowa-fetch" \
     "https://api.github.com/repos/aldinokemal/go-whatsapp-web-multidevice/releases/tags/$TAG" \
     | python3 -c '
   import json, sys
   rel = json.load(sys.stdin)
   keep = ("linux_amd64", "linux_arm64", "linux_armv7", "darwin_amd64", "darwin_arm64", "windows_amd64", "windows_arm64")
   print("# GOWA pinned tag: " + rel["tag_name"] + " (aldinokemal/go-whatsapp-web-multidevice). sha256sum format; checked by fetch.sh.")
   for a in sorted(rel["assets"], key=lambda a: a["name"]):
       n = a["name"]
       if n.endswith((".zip", ".tar.gz")) and any(k in n for k in keep):
           print(a["digest"].removeprefix("sha256:") + "  " + n)
   ' > connectors/gowa/checksums.txt
   ```

   Then check the result against the upstream `checksums-*.txt` assets or by downloading the archives and running `sha256sum -c connectors/gowa/checksums.txt`. Remove any platform line that has no asset in that release.
3. Set `GOWA_PINNED_VERSION` in `fetch.sh` to the same tag.
4. Run `./connectors/gowa/fetch.sh` with no override. It should print `Verified SHA-256`. Then run the WhatsApp connector against a test account before shipping.
5. Commit `fetch.sh` and `checksums.txt` together, and note the GOWA bump in the changelog.

## What the connector does

1. Starts or reuses a local GOWA `rest` server (`--host=127.0.0.1`).
2. Registers a device per Shuttle account (`POST /devices`).
3. Fetches a QR image (`GET /devices/{id}/login`) and sends it over the connector protocol.
4. Listens on `/ws?device_id=…` for login and inbound messages.
5. Syncs the chat list (`GET /chats`) and message bodies already in GOWA sqlite (`GET /chat/{jid}/messages`), plus WhatsApp `history-*.json` dumps. Those REST message queries do not download on-demand history from the phone.
6. Sends text (`POST /send/message`, with `reply_message_id` for a reply) and marks read (`POST /message/{id}/read`).
