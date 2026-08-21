# Where Shuttle stores data, and how it is protected

Shuttle keeps **all user data on this device**. Nothing is uploaded to a Shuttle cloud. Each OS uses the standard per-user application data directory from `dirs::data_dir()`, then a `shuttle` folder.

## Locations

| OS | Data directory |
| --- | --- |
| **Linux (Debian / Mint / most desktops)** | `~/.local/share/shuttle` (`$XDG_DATA_HOME/shuttle` if that is set) |
| **macOS** | `~/Library/Application Support/shuttle` |
| **Windows** | `%APPDATA%\shuttle` (usually `C:\Users\<you>\AppData\Roaming\shuttle`) |

Override the directory with `SHUTTLE_DATA_DIR` (used by sidecars and useful for tests). Product context: [overview.md](overview.md).

## What is saved, and how

```
shuttle/
├── config.json              # appearance, notification prefs, channel colours (not secrets)
├── app.sqlite               # connectors, accounts, org tables, app settings, installation id
├── app.sqlite-wal
├── app.sqlite-shm
├── accounts/
│   └── <account-id>/
│       ├── inbox.sqlite     # that account's conversations, messages, contacts
│       ├── inbox.sqlite-wal
│       └── inbox.sqlite-shm
├── database.sqlite.legacy   # leftover from pre-split installs (safe to delete)
├── secrets/                 # only if the OS keyring is unavailable (mode 0600)
│   └── <account-id>.json
├── connectors/
│   ├── matrix/<account>/    # reserved for Matrix session state if persisted later
│   ├── telegram/<account>/  # TDLib database + files
│   ├── signal/<account>/    # signal-cli config
│   ├── messenger/<account>/ # fbchat session cookies (0600)
│   ├── instagram/<account>/ # instagrapi session (0600)
│   └── email/               # no extra files; password is in the keyring
├── gowa/                    # WhatsApp GOWA process state + WhatsApp session DB
├── cache/
└── logs/
```

| Kind of data | Format | Where |
| --- | --- | --- |
| Theme, quiet hours, channel tag colours | JSON | `config.json` |
| Account list + org (workspaces, priority groups, chat todos, reminders, forwarding rules, scheduled messages) plus `app_meta` / `app_settings` | SQLite (WAL) | `app.sqlite` (renamed from `catalog.sqlite` on first launch) |
| Inbox, chats, message text, per-chat flags (mute, notes, workspace override) | SQLite (WAL, foreign keys), **one file per account** | `accounts/<account-id>/inbox.sqlite` |
| Passwords, tokens, IMAP secrets, Telegram API hash | JSON | **OS keyring** first; `secrets/<id>.json` (mode `0600`) only as fallback |
| One-time SMS/2FA codes | not stored | sent to the sidecar over stdin, then discarded |
| WhatsApp session | GOWA sqlite | `gowa/storages/` |
| Telegram session | TDLib own DB | `connectors/telegram/<account>/` |
| Matrix session | none yet beyond keyring token | access token in keyring; room state re-syncs from homeserver |
| Signal identity keys | signal-cli files | `connectors/signal/<account>/config/` |
| Messenger / Instagram sessions | pickle / JSON | `connectors/<network>/<account>/` mode `0600` |
| Email | none on disk besides keyring | IMAP/SMTP over TLS |
| Downloaded media and avatars | files on disk | **`~/Documents/shuttle/<account-id>/media` and `avatars`** (override with `SHUTTLE_FILES_DIR`). Message rows store path + MIME in `metadata`, not blobs. |

Credentials are **never** written to SQLite, **never** written to `config.json`, and **never** sent to the Svelte UI. The frontend only submits them through Tauri `invoke`; the Rust core puts persistable fields in the keyring and passes the blob to the sidecar on stdin.

After login, each sidecar pulls existing chats and messages from the network (paginated) into that account’s `inbox.sqlite`. New live events keep the same file up to date. Duplicates are ignored via a unique `(conversation_id, remote_id)` index. A one-time split copies a leftover `database.sqlite` into the catalog plus per-account inboxes, then renames it to `database.sqlite.legacy`.

### `config.json`

Written by `ConfigStore` (`config.rs`). Defaults are created on first launch. Shape:

```json
{
  "appearance": {
    "color_scheme": "system",
    "theme_id": "shuttle",
    "tweakcn_css": null
  },
  "notifications": {
    "enabled": true,
    "quiet_hours_enabled": false,
    "quiet_hours_start": "22:00",
    "quiet_hours_end": "08:00"
  },
  "channel_styles": {
    "whatsapp": { "tag": "#25D366", "background": null, "font": null }
  }
}
```

- `color_scheme`: `system`, `light`, or `dark`. `system` follows `prefers-color-scheme`.
- `theme_id`: bundled preset (`shuttle`, `zinc`, `ocean`, `twilight`) or `custom` when pasted tweakcn CSS is applied.
- `tweakcn_css`: optional pasted CSS; mapped onto Shuttle tokens in the WebView. Not validated as a remote theme fetch — paste only.
- `channel_styles`: per-connector tag/background/font on top of the theme (conversation list row, etc.).

This file is **not encrypted**. It must not hold API keys.

### Catalog vs inbox

**Catalog** (`app.sqlite`)

- `connectors`, `accounts` (including `disabled`, `muted`, `workspace_id`, `notify_enabled`, `send_receipts`, `sleep_enabled`, `sleep_after_minutes`, `sleep_check_minutes`)
- `workspaces` — seeded `personal` / `work` / `others`
- `priority_groups` — seeded `urgent` / `waiting` / `later`
- `chat_todos`, `reminders` — local organization; keyed by `conversation_id` + `account_id` but stored here so they are not duplicated per inbox connection
- `forwarding_rules`, `scheduled_messages` — text routing and delayed send queue
- `app_meta` — anonymous `installation_id` (telemetry)
- `app_settings` — persisted app config JSON

On startup, if `catalog.sqlite` exists and `app.sqlite` does not, Shuttle renames the catalog (including WAL/SHM).

**Inbox** (`accounts/<id>/inbox.sqlite`)

- `contacts`, `conversations`, `messages`
- Conversation extras: `workspace_id`, `priority_group`, `notes`, `notify_enabled`, `send_receipts` (added with `ALTER TABLE` on existing files)

## Security guarantees (and gaps)

**In place**

- Data directory and connector session dirs are created with mode **0700** on Unix.
- Secret files (keyring fallback, Instagram/Messenger sessions) use mode **0600**.
- OS secret stores: **Windows Credential Manager**, **macOS Keychain**, **Linux Secret Service** (`libsecret` / GNOME Keyring / KWallet).
- Sidecars are separate processes. The WebView cannot spawn them or read the filesystem.
- Connector protocol is stdin/stdout JSON, not a LAN HTTP API (GOWA is bound to `127.0.0.1` with basic auth).
- Email uses TLS (`IMAP4_SSL`, SMTP `STARTTLS`).
- SMS/2FA codes are not persisted in the keyring.
- Deleting an account stops the sidecar, drops that account’s inbox database (`accounts/<id>/`), removes `connectors/<id>/<account>/`, and deletes the keyring item.
- Native notifications go through the OS notification service (`notify-rust`); message bodies in those toasts are a short preview (first 180 characters).
- Backups are encrypted with a user passphrase via `age`; export and restore run entirely in Rust, not in the WebView.

**Not yet**

- The **message databases and `config.json` are not encrypted at rest**. Anyone with OS access to your user account can read `app.sqlite`, `accounts/*/inbox.sqlite`, and theme/notification prefs. Full-disk encryption (BitLocker, FileVault, LUKS) is the practical protection.
- Linux without a running Secret Service falls back to a **0600 JSON file**, which is permission-protected, not encrypted.
- `fbchat` and `instagrapi` talk to **unofficial private APIs**. Session cookies are as sensitive as a password; treat those accounts as higher risk than TDLib / IMAP.
- `signal-cli` is **GPL-3.0** and unofficial. Release builds bundle it as a sidecar; see [licensing.md](licensing.md) and [ATTRIBUTION.md](../ATTRIBUTION.md).
- Telegram needs **your own** `api_id` / `api_hash` from [my.telegram.org](https://my.telegram.org).
- Restore copies backup files into the live data dir and expects a restart before open SQLite handles fully reflect the restored state.

## Linux Secret Service

On Debian/Mint, install a keyring daemon if passwords should stay out of `secrets/`:

```bash
sudo apt install gnome-keyring libsecret-1-0
```

Desktop notifications need a running notification daemon (usually already present on Cinnamon / GNOME / KDE). Headless or minimal sessions may need something like `dunst`.
