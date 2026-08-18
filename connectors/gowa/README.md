# GOWA sidecar

Shuttle talks to WhatsApp through [GOWA](https://github.com/aldinokemal/go-whatsapp-web-multidevice) (`go-whatsapp-web-multidevice`). The WhatsApp connector process starts GOWA bound to `127.0.0.1`, authenticates with HTTP basic auth, and never exposes it on the LAN.

## Install the binary

```bash
./connectors/gowa/fetch.sh
```

That downloads the latest release into `connectors/gowa/whatsapp`. Override with `SHUTTLE_GOWA_BIN` if you already have a GOWA binary.

Point Shuttle at an already-running instance with `SHUTTLE_GOWA_URL` (still must be loopback).

## What the connector does

1. Starts or reuses a local GOWA `rest` server (`--host=127.0.0.1`).
2. Registers a device per Shuttle account (`POST /devices`).
3. Fetches a QR image (`GET /devices/{id}/login`) and sends it over the connector protocol.
4. Listens on `/ws?device_id=…` for login and inbound messages.
5. Syncs chats (`GET /chats`) and recent messages (`GET /chat/{jid}/messages`).
6. Sends text (`POST /send/message`) and marks read (`POST /message/{id}/read`).
