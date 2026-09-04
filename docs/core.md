# Shuttle Core (Rust)

The Rust core lives in `shuttle-app/src-tauri/src/`. It owns local persistence (SQLite + `config.json`), spawns connector sidecars, applies notification and receipt policy, and bridges the Svelte UI via Tauri commands and events.

Product context: [overview.md](overview.md). On-disk layout: [storage.md](storage.md).

## Layout

| Module | Role |
|--------|------|
| `models.rs` | Serde types shared with the frontend |
| `db/` | SQLite schema, migrations, queries (`schema.rs` + `mod.rs`) |
| `config.rs` | `config.json` — appearance, notification prefs, channel colours |
| `env.rs` | `.env` + compile-time telemetry/build-channel config |
| `connectors/` | Sidecar process manager + JSON-line protocol |
| `commands.rs` | Tauri `invoke` handlers, reminder loop, backup, app state |
| `notifications.rs` | Native desktop notify (`notify-rust`) + mute / quiet-hours / receipt policy |
| `secrets.rs` | OS keyring + 0600 file fallback |
| `telemetry/` | Opt-in Sentry / PostHog, sanitizer, performance sampler |

## Data model

All timestamps are RFC 3339 strings in SQLite and `DateTime<Utc>` in Rust.

Database layout: **`app.sqlite`** (accounts, workspaces, organization, `app_meta` / `app_settings`) plus `accounts/<id>/inbox.sqlite` per account (see [storage.md](storage.md)). Older `catalog.sqlite` files are renamed to `app.sqlite` on startup; leftover `database.sqlite` files are split once into catalog + per-account inboxes.

### Accounts (`app.sqlite`)

An account binds a user-facing label to a connector implementation.

| Field | Type | Notes |
|-------|------|-------|
| `id` | UUID string | Primary key |
| `connector_id` | string | `whatsapp`, `telegram`, `signal`, `messenger`, `instagram`, `email`, `matrix` |
| `name` | string | Display name chosen by the user |
| `identity` | string? | Phone, @handle, etc. after auth |
| `status` | enum | `disconnected`, `connecting`, `connected`, `error`, `awaiting_auth`, `sleeping` |
| `metadata` | JSON object | Connector-specific state |
| `created_at`, `updated_at` | datetime | |
| `disabled` | bool | Stop sidecar, keep history, skip reconnect |
| `sleep_enabled` | bool? | `NULL` inherits app hibernation default; `0` always on; `1` hibernate |
| `sleep_after_minutes` | u32? | Idle minutes before hibernate; `NULL` inherits default (5) |
| `sleep_check_minutes` | u32? | Periodic wake while asleep; `0` = only on user action; `NULL` inherits (15) |
| `muted` | bool | Suppress notifications; still sync |
| `workspace_id` | string? | Default workspace for chats without an override |
| `notify_enabled` | bool? | `NULL` inherits app-wide; `0`/`1` force off/on |
| `send_receipts` | bool | Default **false** — do not send remote read receipts until opted in |

Disabled accounts cannot be started (`connect_account` / `start_connector` return an error). Enabling an account from the UI patches `disabled=false` and respawns the sidecar.

Hibernating is distinct from disable: the account stays enabled, local history remains, and Shuttle may wake it on chat open, send, or a periodic check. Status is `sleeping`.

`config.json` / app settings `sleep`: `enabled` (default true), `after_minutes` (5), `check_minutes` (15).

### Conversations (`accounts/<id>/inbox.sqlite`)

| Field | Type | Notes |
|-------|------|-------|
| `id` | UUID | Local primary key |
| `account_id` | UUID | Owning account |
| `remote_id` | string | ID on the remote network |
| `contact_id` | UUID? | Optional linked contact |
| `title` | string | Chat title |
| `conversation_type` | enum | `direct`, `group`, `channel` |
| `unread_count` | i64 | Badge count |
| `last_message_at` | datetime? | Sort key (message timestamp) |
| `last_message_preview` | string? | Inbox snippet |
| `pinned`, `archived`, `muted` | bool | UI flags; mute always wins over notify prefs |
| `workspace_id` | string? | Chat override; else account workspace; else `default` |
| `priority_group` | string? | e.g. `urgent`, `waiting`, `later`, or user-defined |
| `notes` | string | Local-only freeform notes |
| `notify_enabled` | bool? | Chat override of account / app notify |
| `send_receipts` | bool? | Chat override of account `send_receipts` |
| `metadata` | JSON | |

Unique constraint: `(account_id, remote_id)`.

`list_conversations` filters by optional `account_id`, `workspace_id` (effective workspace, chat override wins), and `priority_group`. Archived chats are omitted unless requested.

### Messages

| Field | Type | Notes |
|-------|------|-------|
| `id` | UUID | Local primary key |
| `conversation_id` | UUID | Parent conversation |
| `remote_id` | string? | Remote message id |
| `sender_id`, `sender_name` | string? | Sender info |
| `direction` | enum | `inbound` or `outbound` |
| `body` | string | Plain text (media later) |
| `timestamp` | datetime | |
| `status` | enum | `pending`, `sent`, `delivered`, `read`, `failed` |
| `metadata` | JSON | Attachments, reactions, etc. |

Unique index: `(conversation_id, remote_id)` where `remote_id IS NOT NULL`. History sync and live events share this; duplicates are ignored. `list_messages` takes the latest *n* rows then reverses so the UI gets chronological order.

History-tagged sidecar events (`payload.history = true`) are persisted without incrementing unread or emitting `message.received` to the UI. Live inbound WhatsApp traffic is delivered via a loopback GOWA webhook (plus a short REST catch-up); `chat.synced` tells the UI to reload the open thread after `sync_chat`.

### Contacts

Defined in `models.rs` and schema; populated by connectors during sync (not yet wired in stubs).

### Workspaces (`app.sqlite`)

Named buckets. Seeded: `default` (`builtin = 1`). Users can add more. Builtin rows cannot be deleted. Deleting a custom workspace nulls `accounts.workspace_id` and `conversations.workspace_id` that pointed at it.

A chat’s **effective workspace** is `conversation.workspace_id` → else `account.workspace_id` → else `default`.

### Priority groups (`app.sqlite`)

Independent of workspace. Seeded: `urgent`, `waiting`, `later`. Builtin rows cannot be deleted. Stored on the conversation as `priority_group` (id string).

### App meta and settings (`app.sqlite`)

| Table | Purpose |
|-------|---------|
| `app_meta` | Installation ID and created-at (anonymous telemetry identity) |
| `app_settings` | Persisted app config JSON (imported from `config.json` when missing) |

### Chat todos and reminders (`app.sqlite`)

Local only — never sent to a sidecar.

| Table | Purpose |
|-------|---------|
| `chat_todos` | Checklist items (`body`, optional `due_at`, `done`) keyed by conversation + account |
| `reminders` | Single fire time (`fire_at` RFC 3339). `kind` defaults to `nudge`. `fired` is set after delivery |

A tokio interval (20s) loads due reminders, shows a native notification, marks them fired, and emits `reminder.fired`. Deleting an account also cleans up its local todos, reminders, forwarding rules, and scheduled messages from the catalog.

### Connectors (registry table)

Static rows seeded at migration: `whatsapp`, `telegram`, `signal`, `messenger`, `instagram`, `email`, `matrix`. Runtime metadata comes from `ConnectorManager::list_connectors()`.

### App config (not SQLite)

`config.json` in the data dir holds appearance, app-wide notification prefs, and per-connector channel styles. See [storage.md](storage.md) and `config.rs`. Invalid / missing files fall back to defaults (`color_scheme: system`, `theme_id: shuttle`).

## Connector protocol

Sidecars are Python processes. Local dev may use wrappers under `connectors/bin/`; release builds spawn the bundled CPython against `*-connector.py`. They communicate with the core over **newline-delimited JSON** on stdin (requests) and stdout (responses/events).

Protocol version: **1** (`PROTOCOL_VERSION` in `connectors/protocol.rs`).

### Lifecycle

1. Core spawns **one sidecar process per connector type** (`python` + `*-connector.py`). Further accounts of the same network attach to that process with `authenticate` / `account_id`.
2. Core sends `handshake` → sidecar replies `handshake_ok`.
3. Core sends `authenticate` with `account_id` and stored `credentials` (from the keyring).
4. Sidecar may reply `auth_required` (QR, phone, etc.) then `status` updates. Extra codes go through `submit_auth`.
5. On `account.connected`, core sends `sync_history`. Sidecars paginate existing chats/messages.
6. Sidecar emits `event` lines for async activity (messages, connection, etc.).

### Requests (`ConnectorRequest`)

| `type` | Fields | Purpose |
|--------|--------|---------|
| `handshake` | `protocol_version` | Version negotiation |
| `authenticate` | `account_id`, `credentials` | Start auth flow with stored secrets |
| `submit_auth` | `account_id`, `credentials` | QR follow-up, SMS/2FA code, etc. |
| `connect` | `account_id`, `credentials` | Connect with stored creds |
| `disconnect` | `account_id` | Tear down session |
| `send_message` | `account_id`, `conversation_id`, `text` | Outbound message |
| `mark_read` | `account_id`, `conversation_id` | Remote read receipt (only if policy allows) |
| `sync_history` | `account_id` | Paginate existing chats/messages after connect |
| `get_status` | `account_id` | Poll status |
| `shutdown` | — | Exit sidecar |

Example:

```json
{"type":"handshake","protocol_version":1}
{"type":"authenticate","account_id":"550e8400-e29b-41d4-a716-446655440000","credentials":{}}
{"type":"sync_history","account_id":"550e8400-e29b-41d4-a716-446655440000"}
```

### Responses (`ConnectorResponse`)

| `type` | Fields |
|--------|--------|
| `handshake_ok` | `connector_id`, `version`, `capabilities` |
| `auth_required` | `method`, `qr_data?`, `url?`, `message?` |
| `status` | `account_id`, `status`, `identity?` |
| `ok` | `request_id?` |
| `error` | `message` |

### Events (`ConnectorEvent`)

| `type` | Fields |
|--------|--------|
| `event` | `event`, `account_id`, `payload` |

Example inbound message event:

```json
{
  "type": "event",
  "event": "message.received",
  "account_id": "550e8400-e29b-41d4-a716-446655440000",
  "payload": {
    "conversation_id": "remote-chat-id",
    "history": false,
    "message": {
      "id": "remote-msg-id",
      "text": "Hello",
      "sender_id": "123",
      "sender_name": "Alice"
    }
  }
}
```

The core parses stdout lines as either `ConnectorResponse` or `ConnectorEvent`, updates SQLite, rebroadcasts to the UI, and may show a native notification on live `message.received`.

## Event types (UI / `shuttle-event`)

The core forwards internal events to the frontend via Tauri `emit("shuttle-event", …)`.

| `kind` | When | Payload highlights |
|--------|------|-------------------|
| `auth.required` | Sidecar needs QR/phone auth | `account_id`, `method`, `qr_data`, `url`, `message` |
| `account.status` | Status line from sidecar | `account_id`, `status`, `identity` |
| `account.connected` | Session ready | `account_id` |
| `history.sync.started` | Core requested history, or sidecar began paging | `account_id` |
| `history.sync.completed` | Sidecar finished paging | `account_id` (passthrough) |
| `status.feed` | WhatsApp status/stories snapshot or one new status | `account_id`, `items[]` (each with `posts[]`) and/or `upsert` |
| `status.media` | Downloaded status media for the story viewer | `account_id`, `message_id`, `media_path` / `error` |
| `account.avatar` | Connected account profile photo for the rail | `account_id`, `avatar_data` (data URL) |
| `conversation.updated` | Chat upserted from sidecar | `account_id`, `conversation` |
| `chat.synced` | Open-chat history pull finished | `account_id`, `remote_id` |
| `message.received` | Live inbound message persisted | `account_id`, `conversation_id`, `message` |
| `message.sent` | Live outbound message persisted | `account_id`, `conversation_id`, `message` |
| `reminder.fired` | Due reminder delivered | `reminder_id`, `conversation_id`, `account_id` |
| `scheduled_message.sent` | Queued outbound message dispatched | `scheduled_message_id`, `conversation_id`, `account_id` |
| *(passthrough)* | Other sidecar events | `account_id`, `data` |

Subscribe in the frontend:

```typescript
import { listen } from '@tauri-apps/api/event';

listen<{ kind: string; payload: unknown }>('shuttle-event', (e) => {
  console.log(e.payload.kind, e.payload.payload);
});
```

## Tauri commands

All commands are registered in `lib.rs` and implemented in `commands.rs`.

| Command | Args | Returns | Notes |
|---------|------|---------|-------|
| `list_accounts` | — | `Account[]` | |
| `list_connectors` | — | `ConnectorInfo[]` | Static catalog |
| `create_account` | `connector_id`, `name` | `Account` | Status `awaiting_auth` |
| `delete_account` | `account_id` | `()` | Stops sidecar, wipes inbox dir + session + keyring |
| `update_account` | `account_id`, `patch` | `Account` | Mute / disable / workspace / notify / receipts / hibernation. Disable stops sidecar; enable respawns it |
| `wake_account` | `account_id` | `string` | Mark account active and start its sidecar if sleeping |
| `set_active_account` | `account_id?` | `()` | Account the user is looking at; skipped by the hibernation timer |
| `connect_account` | `account_id`, `credentials?` | `string` | Merges secrets, spawns sidecar. Fails if disabled |
| `submit_auth` | `account_id`, `credentials` | `()` | SMS/2FA/etc. Persists persistable fields then forwards to sidecar |
| `list_conversations` | `account_id?`, `workspace_id?`, `priority_group?` | `Conversation[]` | Omits archived |
| `update_conversation` | `conversation_id`, `patch` | `Conversation` | Pin, archive, mute, workspace, priority, notes, notify, receipts |
| `get_messages` | `conversation_id`, `limit?` | `Message[]` | Default limit 100 |
| `send_message` | `account_id`, `conversation_id`, `text` | `Message` | Persists + emits |
| `forward_message` | `dest_account_id`, `dest_conversation_id`, `text` | `Message` | Immediate manual text forward |
| `list_forward_rules` / `create_forward_rule` / `update_forward_rule` / `delete_forward_rule` | | | Persistent inbound text routing rules |
| `list_scheduled_messages` / `schedule_message` / `delete_scheduled_message` | `include_sent?` on list | | Queue for delayed sends and delayed forwarding |
| `export_backup` | `path`, `password`, `include_messages?` | `BackupManifest` | Password-protected `age` bundle |
| `restore_backup` | `path`, `password` | `()` | Restores files into the data dir; restart afterward |
| `mark_read` | `conversation_id`, `send_remote?` | `()` | Always clears local unread. Remote receipt only if `send_remote` or policy (`should_send_receipt`) |
| `mark_unread` | `conversation_id` | `()` | Local badge only |
| `search_conversations` | `query` | `Conversation[]` | Title + body search |
| `total_unread` | — | `number` | Sum of non-archived, non-muted unreads (muted accounts excluded) |
| `get_app_config` | — | `AppConfig` | From `config.json` |
| `save_app_config` | `config` | `AppConfig` | Writes `config.json` |
| `list_workspaces` / `create_workspace` / `rename_workspace` / `delete_workspace` | | | Builtin workspaces cannot be deleted |
| `list_priority_groups` / `create_priority_group` / `rename_priority_group` / `delete_priority_group` | | | Builtin groups cannot be deleted |
| `list_todos` / `add_todo` / `set_todo_done` / `delete_todo` | | | Per conversation |
| `list_reminders` / `create_reminder` / `delete_reminder` | `conversation_id?` on list | | `fire_at` RFC 3339; `kind` defaults to `nudge` |
| `open_external` | `url` | `()` | System opener |
| `open_devtools` | — | `()` | No-op in release (`#[cfg(debug_assertions)]` only) |
| `telemetry_track` / `telemetry_error` / `telemetry_performance` / `telemetry_set_foreground` | | | Frontend → Rust telemetry (dropped unless the matching Privacy toggle is on) |

`AccountPatch` / `ConversationPatch` use optional fields plus `clear_*` flags (`clear_workspace`, `clear_notify`, `clear_priority`, `clear_receipts`) so the UI can reset nullable columns to inherit.

`ForwardRuleDraft` stores source filters (`account`, `conversation`, `workspace`) plus destination, keyword, prefix/suffix, sender stripping, loop guard, and optional delay in seconds. Matching inbound live messages are turned into `scheduled_messages`; zero delay means they send on the next scheduler tick.

## Connector sidecars

Built with `./connectors/build.sh` for local dev launchers, or run directly — the Rust core spawns `python3` / `py -3` against the bundled `*-connector.py` scripts in release builds.

| Binary | Source | Auth | Backend |
|--------|--------|------|---------|
| `whatsapp-connector` | `connectors/whatsapp-connector.py` | Live QR via GOWA | [GOWA](https://github.com/aldinokemal/go-whatsapp-web-multidevice) REST, loopback webhook, and WebSocket on `127.0.0.1` |
| `telegram-connector` | `connectors/telegram-connector.py` | Phone + code, optional 2FA | TDLib `tdjson` (own `api_id` / `api_hash`) |
| `signal-connector` | `connectors/signal-connector.py` | Phone registration | signal-cli JSON-RPC |
| `messenger-connector` | `connectors/messenger-connector.py` | Email + password | `fbchat` (unofficial) |
| `instagram-connector` | `connectors/instagram-connector.py` | Username + password, 2FA | `instagrapi` (unofficial) |
| `email-connector` | `connectors/email-connector.py` | IMAP/SMTP | stdlib TLS (`IMAP4_SSL`, SMTP `STARTTLS`) |
| `matrix-connector` | `connectors/matrix-connector.py` | Homeserver + username/password | Matrix Client-Server API over HTTPS |

### WhatsApp / GOWA

1. Download the GOWA binary: `./connectors/gowa/fetch.sh`
2. Shuttle starts `whatsapp rest --host=127.0.0.1` with basic auth. It is not bound to the LAN.
3. Each Shuttle account is a GOWA device (`POST /devices`).
4. QR login uses `GET /devices/{id}/login`. The PNG is sent to the UI as a data URI.
5. Live chat events come from a loopback webhook (`PATCH /devices/{id}/webhook`) because GOWA’s `/ws` often only carries login codes. Catch-up still polls `GET /chats` + `GET /chat/{jid}/messages`.
6. Sending uses `POST /send/message`.
7. The conversation list is WhatsApp app-state (`GET /chats`, including `last_message_time`). Message bodies are only what GOWA stored in chatstorage plus `history-*.json` dumps from companion history sync. `GET /chat/{jid}/messages` pages that local store. It does not fetch on-demand history the way WhatsApp Web does when you open a chat. Opening a thread re-reads GOWA sqlite and the history JSON for that JID, then emits `chat.synced`.

If GOWA is already running locally, set `SHUTTLE_GOWA_URL` (loopback only) and optional `SHUTTLE_GOWA_USER` / `SHUTTLE_GOWA_PASSWORD`.

Rebuild sidecars after editing Python:

```bash
./connectors/build.sh
```

## Notifications and read receipts

Native notifications use **`notify-rust`** (Linux: Desktop Notifications over D-Bus; macOS / Windows: the crate’s native backends). Failures are logged at debug under `shuttle::notification`; the INFO log line is still written either way.

Linux needs a notification daemon (Cinnamon, GNOME Shell, `dunst`, etc.). Without one, `notify-rust` fails quietly.

### When a notification is shown

Evaluated in `notifications::should_notify` (most specific wins):

1. **Muted always blocks** — conversation `muted` or account `muted`.
2. Chat `notify_enabled` if set (`false` blocks; `true` still has to pass app-wide).
3. Else account `notify_enabled` if set.
4. App-wide `config.notifications.enabled` and quiet hours (`quiet_hours_enabled`, `HH:MM` start/end; overnight ranges such as `22:00`–`08:00` are supported).

History sync messages do not notify. Reminders use the same `notify()` helper and ignore mute/quiet-hours (they are explicit user-scheduled).

### Read receipts

`mark_read` **always** clears the local unread badge. The sidecar `mark_read` request is sent only when:

- the command’s `send_remote` argument is `true`, or
- `send_remote` is omitted and `should_send_receipt` is true: conversation `send_receipts` if set, otherwise account `send_receipts` (default **false**).

## Desktop shell notes

- Default browser context menu is blocked in the WebView (`contextmenu` capture on `document`).
- In-app overlays handle conversation, message, account, and structured-text menus.
- DevTools: `open_devtools` is compiled out of release builds. Debug builds can open them via the command, Ctrl+Shift+I, or eight clicks on the sidebar brand.

## Build notes

`cargo check` / `cargo build` in `shuttle-app/src-tauri` requires Linux system libraries for Tauri’s WebView stack:

```bash
sudo apt install libdbus-1-dev pkg-config libwebkit2gtk-4.1-dev librsvg2-dev
```

On machines without these libraries, `cargo check --offline` fails at **`libdbus-sys`** (pulled in by `tao`/Tauri and `notify-rust`), not in Shuttle application code. The Rust sources above are self-contained aside from that platform layer.

Frontend-only development does not need these deps:

```bash
cd shuttle-app && npm run dev
```
