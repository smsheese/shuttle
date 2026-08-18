# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Versions **0.0.1–0.0.9** reconstruct the development history from Cursor agent sessions and the implementations that landed. **0.1.0** is the current release (environment-variable configuration).

## [Unreleased]

## [0.1.1] - 2026-08-18

### Fixed

- Startup crash on launch: telemetry background tasks (PostHog batching and performance sampling) now use Tauri's async runtime instead of bare `tokio::spawn`, which panicked with "there is no reactor running".

## [0.1.0] - 2026-08-18

Testing vs production telemetry configuration via environment variables. Local `.env` for development; GitHub Environments bake secrets into CI binaries. Sentry `environment` and PostHog `$environment` are `testing` or `production`.

### Added

- `.env.example` with `SENTRY_DSN`, `POSTHOG_API_KEY`, `POSTHOG_HOST`, `SHUTTLE_BUILD_CHANNEL`, and `SHUTTLE_GIT_COMMIT`.
- Runtime `.env` loader (`env.rs`) that searches the repo root, `shuttle-app/`, and the current directory without overwriting existing process env.
- Compile-time embedding of the same keys through `build.rs` (`cargo:rustc-env`) so installed apps do not need a `.env` file.
- GitHub Environments **`production`** and **`testing`**: pushes/tags on `main` use production; manual `workflow_dispatch` can choose either (default testing).
- Resolution order documented in [docs/telemetry-events.md](docs/telemetry-events.md): process env → `.env` → baked-in values.

### Changed

- Release workflow passes telemetry secrets and `SHUTTLE_BUILD_CHANNEL` / `SHUTTLE_GIT_COMMIT` into `tauri build` per GitHub Environment.

## [0.0.9] - 2026-08-18

Privacy-first telemetry. Crash reports and usage diagnostics stay off until the user opts in.

### Added

- Sentry crash reports and PostHog usage diagnostics with Settings → Privacy toggles, a sanitizer, and typed events ([docs/telemetry-events.md](docs/telemetry-events.md)).
- Performance snapshots: 60s sample / 15 min send in the foreground; 180s sample / 30 min send in the background.
- `app_meta` / `app_settings` in `app.sqlite` for installation ID and persisted app config.
- Implementation plan at [docs/telemetry-implementation-plan.md](docs/telemetry-implementation-plan.md).

### Changed

- `catalog.sqlite` renamed to **`app.sqlite`** on startup (WAL/SHM included).

## [0.0.8] - 2026-08-18

Open-source licensing and third-party credit so Shuttle can stay community-owned without a hidden SaaS wrapper.

### Added

- Settings → **Attributions** listing open-source components and licenses.
- Release builds bundle **signal-cli** (GPL-3.0) with license texts and source metadata.
- [docs/licensing.md](docs/licensing.md) explaining AGPL intent and third-party obligations.

### Changed

- Shuttle relicensed from MIT to **AGPL-3.0**; `LICENSE`, `Cargo.toml`, and package metadata updated.

## [0.0.7] - 2026-08-18

Managed Python so connectors are not tied to whatever CPython the user has installed.

### Added

- Release bundles prepare and ship a standalone CPython runtime (`./scripts/fetch-python-runtime.sh`) for Linux, Windows, and macOS.

### Changed

- Connector sidecars spawn the bundled Python in production (no bash-only launchers); `connectors/bin/` remains for local development.

## [0.0.6] - 2026-08-18

Cross-platform packaging and Matrix, using free/open backends that do not require a paid developer membership to use messaging.

### Added

- Matrix connector with homeserver login over the standard Matrix client-server API.
- amd64 and arm64 targets for Windows, Linux, and macOS; Android documented as future-only ([docs/platforms.md](docs/platforms.md)).
- Release CI matrix (Linux amd64/arm64, macOS universal, Windows amd64/arm64) plus `.deb`, AppImage, and Windows installer production builds.
- `./scripts/build-release.sh` / `stage-release-assets.sh` for local packaging.

## [0.0.5] - 2026-08-18

Remaining product tracks from the roadmap except new networks: routing, composer, and backup.

### Added

- Routing: forwarding rules, delayed sends, and a scheduled-message queue in SQLite.
- Channel-aware composer formatting with plain-text fallback on networks that do not support markup.
- Password-protected backup export/restore commands and settings UI.

## [0.0.4] - 2026-08-17

Roadmap tracks 1–4: a desktop messaging shell instead of a browser WebView, plus organization and appearance.

### Added

- In-app context menus; account disable/mute/remove; chat mute/pin/archive/unread; hidden debug DevTools (release builds omit Inspect).
- Native desktop notifications (`notify-rust`) with app / account / chat prefs, quiet hours, and mute always winning.
- Workspaces (Personal / Work / Others + user-defined), priority groups, per-chat notes and todos, one-shot chat reminders.
- Appearance: OS light/dark with override, bundled presets, pasted tweakcn CSS, per-channel tag colours in `config.json`.
- Project docs: README, attribution notes, [docs/roadmap.md](docs/roadmap.md); `docs/core.md` and `docs/storage.md` aligned with the command/protocol surface.

### Changed

- Read receipts off by default; opening a thread still clears the local unread badge.

## [0.0.3] - 2026-08-17

History import so a newly linked account is not an empty inbox.

### Added

- History sync after login (`sync_history`) into a per-account `inbox.sqlite` rather than a single shared message store.

## [0.0.2] - 2026-08-17

Remaining channel sidecars, real WhatsApp via GOWA, and documented local storage.

### Added

- WhatsApp connector via local GOWA (QR login, chat sync, send, read receipts, WebSocket events).
- Telegram connector via local TDLib `tdjson` (phone + code, optional 2FA, own `api_id` / `api_hash`).
- Signal connector via local signal-cli JSON-RPC (phone registration and messaging).
- Messenger connector via `fbchat` (email/password, session pickle, groups).
- Instagram connector via `instagrapi` (username/password, 2FA/challenge codes, DMs).
- Email connector via IMAP IDLE/poll and SMTP STARTTLS, with common-host presets.
- OS keyring for credentials (Windows Credential Manager, macOS Keychain, Linux Secret Service) with a `0600` file fallback.
- Account wipe that stops the sidecar, drops SQLite rows, removes connector session dirs, and deletes the keyring item.
- Local-only data layout under the OS application data directory (`docs/storage.md`).
- `./connectors/build.sh` plus fetch scripts for GOWA, TDLib, and signal-cli.

### Changed

- Linux bundle target set to AppImage for this stage.

## [0.0.1] - 2026-08-17

First Shuttle: a local-first unified inbox with a Tauri 2 desktop shell, a Rust core, and channel-agnostic connector sidecars. Built against a Beeper Desktop quality bar (desktop and mobile viewports).

### Added

- Desktop app (Tauri 2 + Svelte 5) with a three-pane inbox: account sidebar, conversation list, and thread view.
- Account setup flows (QR, phone, password) with Beeper-style pairing UI.
- Unified conversations, message send/receive, mark-as-read, search, unread badges, and account filtering.
- Responsive layout with a mobile inbox/thread split and a `?setup=1` standalone setup route.
- Mock API so `npm run dev` works without Tauri system libraries.
- Rust core with SQLite persistence (accounts, contacts, conversations, messages), WAL, and foreign keys.
- Newline-delimited JSON connector protocol (handshake, auth, connect, send, mark-read, status, events).
- Sidecar process manager: spawn `{connector_id}-connector`, persist inbound/outbound messages, and rebroadcast `shuttle-event` to the UI.
- WhatsApp and Telegram sidecar scaffolding (GOWA / TDLib) as isolated processes.
- Screenshot helper (`scripts/screenshot.mjs`) and gauntlet progress page (`progress.html`).

### Known limitations

- The SQLite message database is not encrypted at rest.
- Native desktop notifications need a running OS notification daemon on Linux.
- Messenger and Instagram use unofficial private APIs.
- signal-cli is GPL-3.0 and is bundled in release builds (see [ATTRIBUTION.md](ATTRIBUTION.md)).
- Android is documented only; not implemented.
