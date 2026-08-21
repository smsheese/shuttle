# Shuttle

**Lightweight, local-first unified messaging for the desktop.** One inbox for WhatsApp, Telegram, Signal, Messenger, Instagram DMs, Matrix, and email — on your machine, not in a Shuttle cloud.

Shuttle aims to be the **no-thought answer** for low resource use: fast and responsive on Windows, Linux, and macOS (amd64 and arm64), with quality-of-life features that make multi-account messaging feel native — without the Chromium-per-service tax. The shell is [Tauri 2](https://tauri.app/) + [Svelte 5](https://svelte.dev/); the core is Rust and SQLite. Each network runs as an isolated sidecar so the UI never talks to providers directly. New connectors stay easy: one protocol, one process, no UI fork.

## Features

- **Unified inbox** across networks and accounts, with search, unread badges, pin / mute / archive
- **Sign-in that matches the network** — QR, phone + code, password, or IMAP/SMTP
- **Data stays local** — per-account SQLite inboxes, appearance in `config.json`, credentials in the OS keyring
- **Native notifications** with quiet hours and per-account / per-chat mute; read receipts off until you opt in
- **Organization** — workspaces, priority groups, chat notes, todos, and one-shot reminders
- **Routing** — forward between chats or accounts, delayed send, scheduled messages
- **Themes** — system light/dark, bundled presets, or pasted CSS
- **Daily-driver chrome** — system tray (close-to-tray), Ctrl/Cmd+K quick switch, keyboard shortcuts
- **Account sleep** — idle connectors hibernate after a few minutes; wake when you open that account (distinct from disable)
- **Shared sidecars** — one Python process per network, not per account
- **Media** — in-thread players, lightbox, canvas image editor on attach
- **Backup / restore** — password-protected export/import with optional media; restart after restore
- **Optional telemetry** (Sentry / PostHog) — off by default, with Settings → Privacy toggles

Product north star (lightweight · responsive · cross-platform · QoL · contributor-friendly) and remaining work: [docs/roadmap.md](docs/roadmap.md). Architecture: [docs/](docs/README.md).

## Connectors

Sidecars are Python processes over a newline-delimited JSON protocol — **one process per network**, shared across accounts of that type. **Release installers ship the core app only.** When you add an account, Shuttle downloads the connector scripts, native helpers, slim Python runtime, and any pip deps from a versioned S3 manifest (`SHUTTLE_COMPONENTS_BASE_URL`, baked at build time). System Python 3.12+ is used when compatible.

| Network | Integrator | Auth | Notes |
| --- | --- | --- | --- |
| WhatsApp | [GOWA](https://github.com/aldinokemal/go-whatsapp-web-multidevice) (local, `127.0.0.1`) | QR | MIT; uses [whatsmeow](https://github.com/tulir/whatsmeow) (MPL-2.0) |
| Telegram | [TDLib](https://github.com/tdlib/td) `tdjson` | Phone + code | Official client library (BSL-1.0). You supply your own `api_id` / `api_hash` |
| Signal | [signal-cli](https://github.com/AsamK/signal-cli) JSON-RPC | Phone | Unofficial. **GPL-3.0**; downloaded on demand (~350 MB) |
| Messenger | [fbchat](https://github.com/fbchat-dev/fbchat) | Email + password | Unofficial private API (BSD-3-Clause) |
| Instagram | [instagrapi](https://github.com/subzeroid/instagrapi) | Username + password | Unofficial private API (MIT) |
| Matrix | Matrix Client-Server API | Homeserver + password | Standard HTTPS API |
| Email | IMAP + SMTP (Python stdlib, TLS) | Address + password | Common-host presets |

Messenger and Instagram are unofficial and may conflict with those networks’ terms of service. Shuttle is not affiliated with Meta, WhatsApp, Telegram, Signal, or Matrix.

Full credits and redistribution notes: [ATTRIBUTION.md](ATTRIBUTION.md). Protocol and data model: [docs/core.md](docs/core.md).

## License

Shuttle (the desktop app, Rust core, and connector wrappers) is **[AGPL-3.0](LICENSE)**.

AGPL is a copyleft license with a **network clause**: if you modify Shuttle and run it as a service that users interact with over a network, you must offer them the corresponding source. That is intentional — Shuttle is meant to stay open, not disappear behind a proprietary SaaS wrapper.

Third-party integrators keep **their own licenses**. Shuttle does not relicense them. In particular:

- **signal-cli** is GPL-3.0 and is downloaded when you add Signal (license text ships with the app)
- Embedded CPython is MPL-2.0 / PSF
- GOWA, TDLib, fbchat, and instagrapi keep MIT / BSL / BSD as listed above

See [docs/licensing.md](docs/licensing.md) for intent and obligations, and **Settings → Attributions** in the app.

## Architecture

```
Svelte UI  ←→  Tauri IPC  ←→  Rust Core (SQLite, events, keyring)
                                    ↕ JSON-lines on stdin/stdout
         WhatsApp · Telegram · Signal · Messenger · Instagram · Matrix · Email
```

Messages and sessions live under the OS application data directory (on Linux, `~/.local/share/shuttle`). The message database is not encrypted at rest — use full-disk encryption. Details: [docs/storage.md](docs/storage.md).

## Platforms

| OS | amd64 | arm64 | Installers |
| --- | --- | --- | --- |
| Linux | yes | yes | `.deb`, AppImage |
| macOS | yes | yes | `.dmg` / universal `.app` |
| Windows | yes | yes | `.msi`, NSIS `.exe` |

Android is a possible future target and is not in the current build. See [docs/platforms.md](docs/platforms.md).

## Quick start

```bash
# Frontend preview (works without Tauri system deps)
cd shuttle-app && npm install && npm run dev

# Full desktop app (requires Tauri prerequisites)
cd shuttle-app && npm run tauri dev

# Connector sidecars (local dev — production downloads on add-account)
./connectors/build.sh
./connectors/gowa/fetch.sh         # WhatsApp / GOWA
./connectors/tdlib/fetch.sh        # Telegram / TDLib
./connectors/signal/fetch.sh       # Signal / signal-cli
./scripts/fetch-python-runtime.sh  # Embedded CPython + Messenger/Instagram deps
```

```bash
./scripts/build-release.sh
# Builds a slim core-only installer for the host OS.
# macOS Intel + Apple Silicon in one app:
./scripts/build-release.sh -- --target universal-apple-darwin

# Publish lazy-install component archives to S3 (maintainers):
SHUTTLE_COMPONENTS_BASE_URL=https://your-cdn.example/shuttle/components ./scripts/publish-components.sh
```

Set `SHUTTLE_COMPONENTS_BASE_URL` when building release installers so the app knows where to fetch `v{version}/manifest.json`.

Optional overrides: `SHUTTLE_DATA_DIR`, `SHUTTLE_PYTHON`, `SHUTTLE_GOWA_BIN`, `SHUTTLE_TDLIB`, `SHUTTLE_SIGNAL_CLI`.

For local telemetry testing, copy [`.env.example`](.env.example) to `.env`. Production CI builds bake GitHub Environment secrets into the binary. See [docs/telemetry-events.md](docs/telemetry-events.md).

### Linux prerequisites

Tauri needs the WebKit **development** headers, not only the runtime libraries:

```bash
sudo apt install \
  build-essential \
  pkg-config \
  libglib2.0-dev \
  libgtk-3-dev \
  libwebkit2gtk-4.1-dev \
  libjavascriptcoregtk-4.1-dev \
  libsoup-3.0-dev \
  librsvg2-dev \
  libdbus-1-dev \
  libssl-dev \
  libayatana-appindicator3-dev \
  libxdo-dev
```

OS keyring (instead of `~/.local/share/shuttle/secrets/`):

```bash
sudo apt install gnome-keyring libsecret-1-0
```

Desktop notifications use D-Bus (`notify-rust`). Cinnamon / GNOME / KDE already provide a daemon; a minimal session may need `dunst` or equivalent.

## Documentation

| Doc | Topic |
| --- | --- |
| [docs/overview.md](docs/overview.md) | Architecture and principles |
| [docs/core.md](docs/core.md) | Protocol, data model, Tauri commands |
| [docs/storage.md](docs/storage.md) | Where files live and how they are protected |
| [docs/platforms.md](docs/platforms.md) | OS / CPU matrix and packaging |
| [docs/licensing.md](docs/licensing.md) | AGPL-3.0 and third-party licenses |
| [docs/roadmap.md](docs/roadmap.md) | Remaining work |
| [docs/telemetry-events.md](docs/telemetry-events.md) | Opt-in telemetry |

Index: [docs/README.md](docs/README.md).

## Changelog

See [CHANGELOG.md](CHANGELOG.md).
