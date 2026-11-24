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
│   ├── whatsapp/<account>/  # sync-state.json: backfilled chats, per-chat cursors, imported history dumps
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
| Account list + org (workspaces, priority groups, chat todos, chat notes, forwarding rules, scheduled messages) plus `app_meta` / `app_settings` | SQLite (WAL) | `app.sqlite` (renamed from `catalog.sqlite` on first launch) |
| Inbox, chats, message text, per-chat flags (mute, workspace override) | SQLite (WAL, foreign keys), **one file per account** | `accounts/<account-id>/inbox.sqlite` |
| Passwords, tokens, IMAP secrets, Telegram API hash | JSON | **OS keyring** first; `secrets/<id>.json` (mode `0600`) only as fallback |
| One-time SMS/2FA codes | not stored | sent to the sidecar over stdin, then discarded |
| WhatsApp session | GOWA sqlite | `gowa/storages/` (one GOWA store shared by all WhatsApp accounts) |
| WhatsApp sync state (backfilled chats, per-chat cursors, imported history dump names, dumps re-read for reactions and for reply links, chats checked for reply links) | JSON | `connectors/whatsapp/<account-id>/sync-state.json` |
| Telegram session | TDLib own DB | `connectors/telegram/<account>/` |
| Matrix session | none yet beyond keyring token | access token in keyring; room state re-syncs from homeserver |
| Signal identity keys | signal-cli files | `connectors/signal/<account>/config/` |
| Messenger / Instagram sessions | pickle / JSON | `connectors/<network>/<account>/` mode `0600` |
| Email | none on disk besides keyring | IMAP/SMTP over TLS |
| Downloaded media, avatars and message previews | files on disk (app-private cache) | **`<data dir>/media_cache/<account id>/{media,avatars,thumbs}`**. Message rows store the path (`media_path`, `thumbnail_path`), size and MIME in `metadata`, not blobs. See [Media cache and saved media](#media-cache-and-saved-media). |
| Saved media (only what you explicitly save) | files on disk | The **save folder** (Settings → Media & storage), default `<Documents>/shuttle`. |

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
  },
  "performance": {
    "low_memory_webview": true,
    "unload_hidden_window": true,
    "unload_hidden_after_secs": 120
  }
}
```

- `color_scheme`: `system`, `light`, or `dark`. `system` follows `prefers-color-scheme`.
- `theme_id`: bundled preset (`shuttle`, `zinc`, `ocean`, `twilight`) or `custom` when pasted tweakcn CSS is applied.
- `tweakcn_css`: cached CSS fetched from a tweakcn theme URL or ID and mapped onto Shuttle tokens in the WebView.
- `channel_styles`: per-connector tag/background/font on top of the theme (conversation list row, etc.).
- `performance.low_memory_webview`: default `true`. Linux only: WebKit runs with the `DocumentViewer` cache model, no page cache and no WebGL (nothing in the UI uses it). Set it to `false` and restart if something needs WebGL. Ignored on Windows and macOS, and it never changes the hardware-acceleration policy. See [platforms.md](platforms.md#startup-and-idle-cost).
- `performance.unload_hidden_window`: default `true`. While the window sits hidden in the tray, its WebView is destroyed and its renderer process exits, which is most of Shuttle's idle memory. Showing the window again (tray icon, second launch, Dock icon) rebuilds it in about a second with the same chat, filters and unsent drafts. Notifications, sync, reminders, scheduled sends and the tray unread count run in Rust and are untouched. Set it to `false` to keep the hidden window loaded, for example while profiling.
- `performance.unload_hidden_after_secs`: default `120`, minimum 5. How long the window stays hidden before the unload. Shorter means lower idle memory and a slightly slower first show. A window that is ringing or in a call is never unloaded.

This file is **not encrypted**. It must not hold API keys.

### Catalog vs inbox

**Catalog** (`app.sqlite`)

- `connectors`, `accounts` (including `disabled`, `muted`, `workspace_id`, `notify_enabled`, `send_receipts`, `sleep_enabled`, `sleep_after_minutes`, `sleep_check_minutes`)
- `workspaces` — seeded `personal` / `work` / `others`
- `priority_groups` — seeded `urgent` / `waiting` / `later`
- `chat_todos`, `chat_notes` (and the legacy `reminders`, kept as a backup after its rows moved to `chat_notes`) — local organization; keyed by `conversation_id` + `account_id` but stored here so they are not duplicated per inbox connection
- `forwarding_rules`, `scheduled_messages` — text routing and delayed send queue
  - `scheduled_messages.forward` (0/1) marks a scheduled manual forward. The row sends its source message when due; `body` is only a preview.
- `app_meta` — anonymous `installation_id` (telemetry)
- `app_settings` — persisted app config JSON

On startup, if `catalog.sqlite` exists and `app.sqlite` does not, Shuttle renames the catalog (including WAL/SHM).

**Inbox** (`accounts/<id>/inbox.sqlite`)

- `contacts`, `conversations`, `messages`, `message_reactions`, `deleted_messages`, `inbox_meta`
- `message_reactions` holds one row per (message, reactor), keyed by the target's remote message id so a reaction can arrive before its message. See [core.md](core.md#message-reactions-message_reactions-accountsidinboxsqlite).
- `deleted_messages` lists `(conversation_id, remote_id)` of messages deleted for this account only, so a later sync does not bring them back. See [core.md](core.md#messages).
- Conversation extras: `workspace_id`, `priority_group`, `notes`, `notify_enabled`, `send_receipts`, `read_through`, `auto_download` (JSON per-chat overrides; added with `ALTER TABLE` on existing files)
- `inbox_meta` is a key/value table. It holds `unread_baseline` (the read-marker floor) and `read_marker_v1` (the one-time upgrade marker). See [core.md](core.md#inbox-metadata-inbox_meta-accountsidinboxsqlite).

### Media cache and saved media

Downloaded is not saved. Every download (automatic or on demand: images, videos, documents, voice notes, stickers, status media) and every avatar goes into an **app-private cache** next to the app data, like Telegram's `tdata/temp_data`:

```
<data dir>/media_cache/<account id>/
  media/    downloaded message and status media (what retention cleans up)
  avatars/  profile photos
  thumbs/   WhatsApp's own tiny previews, written from history dumps (kept; never cleaned by retention)
```

`media/` files are named `<message id>.<ext>`. The extension is the reported file name's suffix, else the mime type, else the file's magic bytes after download, else the media type's default; a download that guessed wrong is renamed and the new `media_path` is reported. Files from older builds without an extension are adopted (and renamed) the next time they are downloaded, and are given their type from their bytes whenever they are copied or saved. `<data dir>/media_cache/clipboard/` is not an account folder: it holds the short-lived links *Copy media* stages (see [platforms.md](platforms.md#copying-media-to-the-clipboard)).

`<data dir>` is `~/.local/share/shuttle` on Linux, `%APPDATA%\shuttle` on Windows and `~/Library/Application Support/shuttle` on macOS (`dirs::data_dir()/shuttle`; the same folder as `app.sqlite`). The core passes the cache root to connectors as `SHUTTLE_FILES_ROOT` / `SHUTTLE_FILES_DIR` and sends the account's cache folder as `files_dir` on authentication and with every request, so core and connector agree. The folder is keyed by account ID (not the account label), so renaming an account never moves files.

**Save folder.** Only an explicit save copies a file out of the cache: right-click → *Save media*, the viewer's *Save*, a document's save button, and the Contact details media tabs all call the `save_media_file` command, which downloads first when needed. The destination is **Settings → Media & storage → Save folder** (default `<Documents>/shuttle`, resolved per OS with `dirs::document_dir()`), or the native save dialog when *Ask where to save each time* is on (default off). A save **never overwrites**: a name that exists becomes `name (1).ext`, `name (2).ext`, and the copy is created with an exclusive-create open so two saves cannot collide. Suggested names are `<account>-<chat>-<yymmdd>-<hhmm>.<ext>` for photos, videos, stickers and audio (the message's local time, not the save time) and sanitised (no path separators, Windows-forbidden characters or reserved names); documents keep their own name. After a save a toast shows *Saved to <path>* for 3 seconds; clicking the path reveals the file in the file manager (Linux: `org.freedesktop.FileManager1.ShowItems` over D-Bus, else `xdg-open` on the parent folder; Windows: Explorer with the file selected; macOS: Finder reveal, i.e. `open -R`). The webview never navigates to media to "download" it.

**Legacy folder.** Older builds wrote downloads to `<Documents>/shuttle/Whatsapp/<account name>/{media,avatars}` and `<Documents>/shuttle/<account id>/`. Those files stay where they are and stay readable (the media path check accepts the cache and this legacy root, including symlink resolution), but nothing new is written there and retention never touches them.

**Retention.** *Keep downloaded media for N days* (default 60, or *forever*) is applied by a cleanup that runs shortly after startup and then about every 24 hours, plus *Clear downloaded media now*. Idleness is the file's modification time; opening a cached file touches it, so media you still look at is kept (`atime` is not used: it is unreliable under `noatime`/`relatime`). The cleanup only visits regular files directly inside each account's `media/` folder and Shuttle's own GOWA download scratch folder (`<data dir>/gowa/statics/media`, where GOWA writes a duplicate of every download; the connector now deletes that copy right after transferring it). It never follows symlinks and never touches the save folder, the legacy Documents folder, avatars, previews or anything on the network account. In-flight transfers (`*.part`) are left alone. After deleting, the core clears `media_path` on the affected messages through the database layer (`clear_media_paths`) and emits `media.purged`, so the thread shows the preview and a Download button again and can re-download (subject to the auto-download setting below). *Cache size* in Settings sums `media/` and the GOWA scratch folder.

**Auto-download.** Per account (Settings → Media & storage): *Images* (default on), *Videos* (off) and *Files* (off). Voice notes and audio follow Files for automatic download, but always download when the user presses play. Stickers always load: the thread requests them as an explicit download rather than as `auto = true`, so the group setting cannot leave a sticker as a blank placeholder. Stored in `accounts.auto_download` (JSON; a missing key means the built-in default). Per chat, the chat-list context menu offers a toggle per group and *use account default*; stored in `conversations.auto_download`. The effective choice is chat override, then account default, then built-in default. The core enforces it: the UI's rendering-time request carries `auto = true` and `download_message_media` skips kinds that are not allowed, while a user-initiated download always runs. GOWA's own auto-download stays off (`WHATSAPP_AUTO_DOWNLOAD_MEDIA=false`).

**Previews (thumbnails).** WhatsApp image, video and document messages embed a small JPEG preview (`JPEGThumbnail` in the protobuf; about 2 KB each). GOWA v9.9.0 does **not** forward it over REST (`/chat/<jid>/messages` rows have no thumbnail column) or in live webhooks (`image`/`video`/`document` carry a path or URL and `media_info` with MIME type, size, width, height and duration only). The only place it appears is the history-sync dumps in `gowa/storages/history-*.json`. The connector writes those previews to `thumbs/<message id>.jpg` and reports `thumbnail_path` as a fact on the message (and on status posts) and, for messages it already reported, through the `media.thumbnails` event. Live messages that arrive after the history sync therefore have no preview, only a type icon, size and a Download button. Previews are files, not blobs in SQLite, so list queries stay small.

Media transfers use 64 KiB chunks and publish a file only after the transfer finishes; failed temporary files are removed. Message downloads allow up to 2 GB and status downloads up to 30 MB. The UI loads files through Tauri's asset protocol, scoped to the media cache and the legacy Documents folder, rather than encoding whole files as base64. Media paths are canonicalized and checked against those folders, including symlink resolution. CDN failures reported by GOWA are shown with a safe reason and an explicit retry; unavailable upstream media may need to be resent.

**Backup.** *Include downloaded media* adds `media_cache/` (and the legacy Documents folder as `user-media/`, if present). GOWA's `statics/media` scratch copies are always left out of backups. Media paths inside messages are absolute, so restore on a machine with a different user name or data directory keeps the files but the thread will offer to download them again.

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

Dark appearance settings in `config.json` include a schedule for the selected theme’s light and dark modes. Custom times use the device clock. Location searches use Open-Meteo; selected coordinates are stored locally and sunrise and sunset are calculated locally.
