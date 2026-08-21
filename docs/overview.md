# Shuttle overview

Shuttle is a **lightweight, local-first desktop messaging app**: one inbox for several networks, with data and sessions on this machine. There is no Shuttle cloud and no Shuttle account.

The product aim is simple: **low resource use and snappy UI should be the obvious answer**, with real quality-of-life and the same binary story on Windows, Linux, and macOS — while staying pleasant to extend (Python sidecars + a stable JSON protocol, not a Chromium recipe zoo). It replaces a pile of browser tabs and Electron clients without becoming a hosted inbox service.

This document is the current architecture. For the public pitch and integrator table see the [root README](../README.md). For remaining work see [roadmap.md](roadmap.md).

## Principles

- **Lightweight by default.** Idle RSS and wake latency are product requirements. Prefer one OS WebView, sleep idle connectors, and never pay a Chromium-per-account tax. When someone asks “what’s light?”, Shuttle should be the no-thought answer — see the Ferdium resource bar in [roadmap.md](roadmap.md#13--resource-budget-vs-ferdium).
- **Responsive and QoL-first.** The UI should feel native: fast list/thread switching, tray, shortcuts, mute/sleep, routing, and organization that reduce mental load — not just “all the web apps in one window.”
- **Cross-platform, one codebase.** Windows, Linux, and macOS × amd64 and arm64 from the same Tauri + Rust + Svelte stack; platform gaps stay documented, not forked products.
- **Easy to extend.** A new network is a new sidecar that speaks the shared newline-delimited JSON protocol. Contributors should not need to touch the inbox UI or SQLite schema for basic text sync/send.
- **Local-first.** Messages, sessions, and organization stay under the OS application data directory. Optional AI (planned) and opt-in crash/usage telemetry are the only things that may leave the device.
- **Channel-agnostic core.** The UI and SQLite schema do not know WhatsApp vs Telegram. A new network is a new sidecar, not a fork of the inbox.
- **Connectors are processes.** Sidecars speak newline-delimited JSON on stdin/stdout. The WebView never talks to networks or the filesystem.
- **Credentials stay out of SQLite and the UI.** OS keyring first; mode `0600` file fallback. Backup/restore must keep that split.
- **Unofficial APIs stay marked.** Messenger (`fbchat`) and Instagram (`instagrapi`) are optional and easy to disable. They may conflict with those networks’ terms of service.
- **Copyleft on Shuttle, not on every helper.** The app is AGPL-3.0. GOWA, TDLib, signal-cli, and Python libraries keep their own licenses ([licensing.md](licensing.md)).

## Stack

| Layer | Choice | Why |
| --- | --- | --- |
| Desktop shell | Tauri 2 | OS WebView instead of bundled Chromium — light + cross-platform |
| UI | Svelte 5 + TypeScript + Vite | Small reactive frontend; fast to iterate |
| Core | Rust | Process manager, SQLite, keyring, notifications, routing |
| Store | SQLite (WAL, foreign keys) | `app.sqlite` catalog + per-account `inbox.sqlite` |
| Secrets | OS keyring (`keyring` crate) | Windows Credential Manager, macOS Keychain, Linux Secret Service |
| Connectors | Python sidecars + native helpers | Isolated; easy to write; release builds ship a managed CPython runtime |

## Architecture

```
Svelte UI  ←→  Tauri IPC  ←→  Rust core (SQLite, events, keyring, telemetry)
                                    ↕ JSON-lines on stdin/stdout
         WhatsApp · Telegram · Signal · Messenger · Instagram · Matrix · Email
```

1. The UI invokes Tauri commands (`list_conversations`, `send_message`, …) and listens on `shuttle-event`.
2. The Rust core persists normalized accounts, chats, and messages; applies mute / receipt / notify policy; and spawns sidecars.
3. Each sidecar translates one network into the shared protocol (`handshake`, `authenticate`, `sync_history`, `send_message`, events).
4. Native helpers (GOWA, TDLib `tdjson`, signal-cli) stay in those sidecars. Replacing a helper should not require a UI rewrite.

Protocol, commands, and schema: [core.md](core.md). Files on disk: [storage.md](storage.md).

## What 0.1.0 includes

- Unified inbox, multi-account setup (QR / phone / password / IMAP), search, unread badges
- Seven connectors: WhatsApp (GOWA), Telegram (TDLib), Signal (signal-cli), Messenger, Instagram, Matrix, email
- History sync into per-account SQLite; native notifications; read receipts off by default
- Workspaces, priority groups, notes, todos, reminders
- Text forwarding, forwarding rules, scheduled send
- Channel-aware composer, password-protected backup, themes
- amd64 + arm64 packaging on Windows, Linux, and macOS; embedded Python; AGPL + attributions
- Opt-in Sentry / PostHog; local `.env` vs GitHub `production` / `testing` environments

Version history: [CHANGELOG.md](../CHANGELOG.md).

## Still planned

Account sleep and a Ferdium-beating RSS budget, tray / quick-switch chrome, media viewers and a basic image editor, richer backup restore, AI replies (opt-in, sanitized), more networks (X, iMessage, SMS, Google Chat, LinkedIn, …), calls where a backend actually supports them, Android, and encrypting the message database at rest. Details: [roadmap.md](roadmap.md).

## Platforms

64-bit desktop only: **Windows, Linux, macOS** × **amd64 and arm64**. Android is documented, not implemented. See [platforms.md](platforms.md).
