# Attribution

Shuttle’s own source (the Tauri/Svelte app, Rust core, and Python connector wrappers) is licensed under the [GNU AGPL-3.0-or-later](LICENSE). A [commercial license](COMMERCIAL-LICENSE.md) is available.

This file credits the third-party software Shuttle talks to or optionally downloads. Those components keep their own licenses. Shuttle does **not** relicense them.

The installer ships only Shuttle itself plus license texts. Connector helpers (**signal-cli** (GPL-3.0), GOWA, TDLib `tdjson` and an embedded CPython runtime) are published as separate components on Shuttle's GitHub release and downloaded on demand when you add an account. Whoever hosts or redistributes those component files must satisfy their licenses; license texts ship in the app bundle under `licenses/` and in Settings → Attributions.

The Shuttle name and logo (`shuttle-logov1.1.*`, `shuttle-app/static/shuttle-logo.*`) are not licensed under the AGPL for use as a brand; see [docs/licensing.md](docs/licensing.md#name-and-logo).

Network names and logos (WhatsApp, Telegram, Signal, Messenger, Instagram, Facebook, Matrix, and others) are trademarks of their respective owners. Shuttle is an independent project and is not affiliated with, endorsed by, or sponsored by those companies.

## Connector backends

These are the libraries and binaries the sidecars use. Shuttle’s wrappers live in `connectors/` and speak a private stdin/stdout JSON protocol; they do not embed these projects’ source.

| Shuttle connector | Third-party software | Role | License | Source |
| --- | --- | --- | --- | --- |
| WhatsApp | **GOWA** (`go-whatsapp-web-multidevice`) | Local REST + WebSocket gateway, bound to `127.0.0.1` | MIT | [aldinokemal/go-whatsapp-web-multidevice](https://github.com/aldinokemal/go-whatsapp-web-multidevice) |
| WhatsApp | **whatsmeow** | WhatsApp Web multi-device protocol library used by GOWA | MPL-2.0 | [tulir/whatsmeow](https://github.com/tulir/whatsmeow) |
| Telegram | **TDLib** (`tdjson`) | Official Telegram client library, loaded locally | Boost Software License 1.0 | [tdlib/td](https://github.com/tdlib/td) |
| Telegram | **aiotdlib** release assets | Optional source of prebuilt `tdjson` shared libraries (`./connectors/tdlib/fetch.sh`) | MIT | [pylakey/aiotdlib](https://github.com/pylakey/aiotdlib) |
| Signal | **signal-cli** | Local JSON-RPC client for Signal (separate sidecar, downloaded on demand) | GPL-3.0 | [AsamK/signal-cli](https://github.com/AsamK/signal-cli) |
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

Fetched by `./connectors/signal/fetch.sh`, published as a component on Shuttle's GitHub release, and downloaded when you add a Signal account. **signal-cli is GPL-3.0.** Shuttle’s wrappers talk to it as a separate process over JSON-RPC.

Whoever hosts or redistributes the signal-cli component (including the Shuttle project's own releases) must:

1. Include the GPL-3.0 license text (`licenses/signal-cli-GPL-3.0.txt` in the app bundle).
2. Preserve copyright notices and `SOURCE.json` metadata under `connectors/signal/`.
3. Offer corresponding source for signal-cli (upstream: [AsamK/signal-cli](https://github.com/AsamK/signal-cli)).

Shuttle is AGPL-3.0-or-later; signal-cli runs as a separate GPL-3.0 process that Shuttle talks to over JSON-RPC and is not linked into the app. Each license applies to its own program.

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
| emoji-picker-element | Emoji picker | Apache-2.0 | [nolanlawson/emoji-picker-element](https://github.com/nolanlawson/emoji-picker-element) |
| Giphy JS SDK (`@giphy/js-fetch-api`) | GIF and sticker search | MIT | [Giphy/giphy-js](https://github.com/Giphy/giphy-js) |
| SunCalc | Sunrise/sunset times for the automatic theme | BSD-2-Clause | [mourner/suncalc](https://github.com/mourner/suncalc) |
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

1. Include this file and [LICENSE](LICENSE) with source and binary distributions of Shuttle itself.
2. If you ship GOWA, tdjson, signal-cli, fbchat, or instagrapi, include their license texts as well.
3. **Shuttle (AGPL-3.0-or-later):** keep the copyright and license notice and offer the corresponding source. See [docs/licensing.md](docs/licensing.md).
4. **signal-cli (GPL-3.0):** published as a downloadable component; include GPL text and source availability as above.
5. **whatsmeow (MPL-2.0):** if you modify whatsmeow (including via a patched GOWA), publish those file-level changes under MPL-2.0.
6. Messenger and Instagram connectors use unofficial APIs; redistribution does not grant any right to those services.

See also Settings → **Attributions** in the app.
