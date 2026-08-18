# Attribution

Shuttle’s own source (the Tauri/Svelte app, Rust core, and Python connector wrappers) is licensed under the [GNU Affero General Public License v3.0 (AGPL-3.0)](LICENSE).

This file credits the third-party software Shuttle talks to or optionally downloads. Those components keep their own licenses. Shuttle does **not** relicense them.

Release builds bundle **signal-cli** (GPL-3.0) and an embedded CPython runtime for connector sidecars. Other native helpers (GOWA, TDLib) are fetched at build time when available. If you redistribute a Shuttle build that includes those files, you must satisfy their licenses — license texts ship in the app bundle under `licenses/` and in Settings → Attributions.

Network names and logos (WhatsApp, Telegram, Signal, Messenger, Instagram, Facebook, Matrix, and others) are trademarks of their respective owners. Shuttle is an independent project and is not affiliated with, endorsed by, or sponsored by those companies.

## Connector backends

These are the libraries and binaries the sidecars use. Shuttle’s wrappers live in `connectors/` and speak a private stdin/stdout JSON protocol; they do not embed these projects’ source.

| Shuttle connector | Third-party software | Role | License | Source |
| --- | --- | --- | --- | --- |
| WhatsApp | **GOWA** (`go-whatsapp-web-multidevice`) | Local REST + WebSocket gateway, bound to `127.0.0.1` | MIT | [aldinokemal/go-whatsapp-web-multidevice](https://github.com/aldinokemal/go-whatsapp-web-multidevice) |
| WhatsApp | **whatsmeow** | WhatsApp Web multi-device protocol library used by GOWA | MPL-2.0 | [tulir/whatsmeow](https://github.com/tulir/whatsmeow) |
| Telegram | **TDLib** (`tdjson`) | Official Telegram client library, loaded locally | Boost Software License 1.0 | [tdlib/td](https://github.com/tdlib/td) |
| Telegram | **aiotdlib** release assets | Optional source of prebuilt `tdjson` shared libraries (`./connectors/tdlib/fetch.sh`) | MIT | [pylakey/aiotdlib](https://github.com/pylakey/aiotdlib) |
| Signal | **signal-cli** | Local JSON-RPC client for Signal (bundled sidecar in release builds) | GPL-3.0 | [AsamK/signal-cli](https://github.com/AsamK/signal-cli) |
| Messenger | **fbchat** | Unofficial Facebook Messenger client | BSD-3-Clause | [fbchat-dev/fbchat](https://github.com/fbchat-dev/fbchat) |
| Instagram | **instagrapi** | Unofficial Instagram private API client | MIT | [subzeroid/instagrapi](https://github.com/subzeroid/instagrapi) |
| Matrix | Matrix Client-Server API | HTTPS messaging to a homeserver | Apache-2.0 (spec) · server-dependent | [spec.matrix.org](https://spec.matrix.org) |
| Email | Python 3 stdlib (`imaplib`, `smtplib`) | IMAP over TLS and SMTP STARTTLS | PSF License | [python.org](https://docs.python.org/3/license.html) |

### WhatsApp / GOWA

Downloaded by `./connectors/gowa/fetch.sh` into `connectors/gowa/whatsapp`. Shuttle starts GOWA as `whatsapp rest --host=127.0.0.1` with HTTP basic auth and never binds it to the LAN.

GOWA is MIT-licensed. It uses **whatsmeow** (MPL-2.0). MPL-2.0 is file-level copyleft: unmodified whatsmeow may be used in a larger work, but modifications to whatsmeow itself must stay under MPL-2.0.

### Telegram / TDLib

Downloaded by `./connectors/tdlib/fetch.sh` (prebuilt `tdjson` from aiotdlib releases) or built from [tdlib/td](https://github.com/tdlib/td). Telegram accounts also need your own `api_id` and `api_hash` from [my.telegram.org](https://my.telegram.org).

### Signal / signal-cli

Downloaded by `./connectors/signal/fetch.sh` and **bundled in release builds**. **signal-cli is GPL-3.0.** Shuttle’s AGPL wrappers talk to it as a separate process over JSON-RPC.

When you redistribute Shuttle with bundled signal-cli you must:

1. Include the GPL-3.0 license text (`licenses/signal-cli-GPL-3.0.txt` in release bundles).
2. Preserve copyright notices and `SOURCE.json` metadata under `connectors/signal/`.
3. Offer corresponding source for signal-cli (upstream: [AsamK/signal-cli](https://github.com/AsamK/signal-cli)).

Shuttle itself remains under AGPL-3.0; bundling GPL signal-cli as a sidecar is a common combined-distribution pattern. AGPL and GPL-3.0 are compatible for this use.

Developers can still point Shuttle at a system-installed binary with `SHUTTLE_SIGNAL_CLI`.

### Messenger / Instagram

`fbchat` and `instagrapi` talk to unofficial private APIs. They are not affiliated with Meta. Session cookies are as sensitive as a password; see [docs/storage.md](docs/storage.md). Using them may violate the networks’ terms of service.

Install with:

```bash
pip install -r connectors/requirements.txt
```

(or use the embedded runtime from `./scripts/fetch-python-runtime.sh`).

## Application stack

Major libraries used by the desktop app (not an exhaustive dependency list). Rust crates are recorded in `shuttle-app/src-tauri/Cargo.lock`; npm packages in `shuttle-app/package-lock.json`.

| Project | Used for | License | Source |
| --- | --- | --- | --- |
| Tauri 2 | Desktop shell, IPC, bundling | MIT OR Apache-2.0 | [tauri-apps/tauri](https://github.com/tauri-apps/tauri) |
| Svelte 5 / SvelteKit | UI | MIT | [sveltejs/svelte](https://github.com/sveltejs/svelte) |
| Vite | Frontend toolchain | MIT | [vitejs/vite](https://github.com/vitejs/vite) |
| python-build-standalone | Embedded CPython for connector sidecars | MPL-2.0 (runtime) · PSF (CPython) | [astral-sh/python-build-standalone](https://github.com/astral-sh/python-build-standalone) |
| rusqlite (bundled SQLite) | Local message store | MIT (rusqlite); SQLite is public domain | [rusqlite](https://github.com/rusqlite/rusqlite) |
| tokio | Async runtime, sidecar processes | MIT | [tokio-rs/tokio](https://github.com/tokio-rs/tokio) |
| serde / serde_json | Serialization | MIT OR Apache-2.0 | [serde-rs/serde](https://github.com/serde-rs/serde) |
| keyring | OS credential store | MIT OR Apache-2.0 | [hwchen/keyring-rs](https://github.com/hwchen/keyring-rs) |
| dirs | Per-user data directory | MIT OR Apache-2.0 | [dirs-dev/dirs-rs](https://github.com/dirs-dev/dirs-rs) |
| Playwright | Optional screenshot script (`scripts/screenshot.mjs`) | Apache-2.0 | [microsoft/playwright](https://github.com/microsoft/playwright) |

To dump a full crate license report from a machine with cargo-license:

```bash
cd shuttle-app/src-tauri && cargo license
```

## Notices that apply when redistributing

1. Include this file and [LICENSE](LICENSE) (AGPL-3.0) with source and binary distributions of Shuttle itself.
2. If you ship GOWA, tdjson, signal-cli, fbchat, or instagrapi, include their license texts as well.
3. **Shuttle (AGPL-3.0):** if you run a modified version as a network service, you must offer corresponding source to users interacting with it over the network. See [docs/licensing.md](docs/licensing.md).
4. **signal-cli (GPL-3.0):** bundled in release builds; include GPL text and source availability as above.
5. **whatsmeow (MPL-2.0):** if you modify whatsmeow (including via a patched GOWA), publish those file-level changes under MPL-2.0.
6. Messenger and Instagram connectors use unofficial APIs; redistribution does not grant any right to those services.

See also Settings → **Attributions** in the app.
