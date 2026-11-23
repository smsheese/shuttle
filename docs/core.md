# Shuttle Core (Rust)

The Rust core lives in `shuttle-app/src-tauri/src/`. It owns local persistence (SQLite + `config.json`), spawns connector sidecars, applies the chat-state, notification and receipt rules, and bridges the Svelte UI via Tauri commands and events.

Product context: [overview.md](overview.md). On-disk layout: [storage.md](storage.md).

## Layout

| Module | Role |
|--------|------|
| `models.rs` | Serde types shared with the frontend |
| `db/` | SQLite schema, migrations, queries (`schema.rs` + `mod.rs`). `chat.rs` holds the chat-state rules |
| `config.rs` | `config.json` — appearance, notification prefs, channel colours |
| `env.rs` | `.env` + compile-time telemetry/build-channel config |
| `connectors/` | Sidecar process manager + JSON-line protocol |
| `commands.rs` | Tauri `invoke` handlers, reminder loop, backup, app state |
| `notifications.rs` | Native desktop notify (`notify-rust`) + mute / quiet-hours / receipt policy |
| `secrets.rs` | OS keyring + 0600 file fallback |
| `telemetry/` | Opt-in PostHog, sanitizer, performance sampler |

## Data model

All timestamps are RFC 3339 strings in SQLite and `DateTime<Utc>` in Rust. Times are compared as parsed values (or with `unixepoch()` in SQL), never as stored text, because older rows use mixed formats.

Database layout: **`app.sqlite`** (accounts, workspaces, organization, `app_meta` / `app_settings`) plus `accounts/<id>/inbox.sqlite` per account (see [storage.md](storage.md)). Older `catalog.sqlite` files are renamed to `app.sqlite` on startup; leftover `database.sqlite` files are split once into catalog + per-account inboxes.

### Accounts (`app.sqlite`)

An account binds a user-facing label to a connector implementation. Its optional `metadata.accent_color` is a user-selected hex colour used for the sidebar and combined-list account markers. Sidebar unread badges sum unarchived conversations for that account, including muted chats.

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
| `title` | string | Chat title (see [Chat state rules](#chat-state-rules)) |
| `conversation_type` | enum | `direct`, `group`, `channel` |
| `unread_count` | i64 | Badge count. A cache of the read marker, not a counter (see [Chat state rules](#chat-state-rules)) |
| `last_message_at` | datetime? | Chat time: the newest message time seen. Only moves forward; sort key |
| `last_message_preview` | string? | Inbox snippet; follows `last_message_at` |
| `read_through` | datetime? | Read marker. Inbound messages newer than it are unread |
| `pinned`, `archived`, `muted` | bool | UI flags; mute always wins over notify prefs |
| `workspace_id` | string? | Chat override; else account workspace; else `default` |
| `priority_group` | string? | e.g. `urgent`, `waiting`, `later`, or user-defined |
| `notes` | string | Local-only freeform notes |
| `notify_enabled` | bool? | Chat override of account / app notify |
| `send_receipts` | bool? | Chat override of account `send_receipts` |
| `metadata` | JSON | Keys listed below |

Unique constraint: `(account_id, remote_id)`. A chat that the network reports under several IDs is still one row. See [Chat state rules](#chat-state-rules).

`metadata` keys used by the chat rules:

| Key | Type | Meaning |
|-----|------|---------|
| `names` | object | Real names reported for this chat: `contact`, `subject`, `verified`, `push`. Each key is merged on its own, so a missing key never erases one |
| `alt_remote_ids` | string[] | Other IDs of the same chat, such as the `@lid` form |
| `is_self` | bool | The chat is the account's own number |
| `marked_unread` | bool | Set by "mark as unread". Keeps at least one unread until the chat is read again |
| `unread_keep` | bool | Set by the upgrade from the old counter. The chat keeps its own read marker until it is read |

`list_conversations` filters by optional `account_id`, `workspace_id` (effective workspace, chat override wins), and `priority_group`. It returns one page (`offset`, `limit`). Archived chats are omitted unless requested. Order: pinned first, then `last_message_at` newest first (chats without a time last), then title case-insensitively, then `id`. The UI uses the same order in `shuttle-app/src/lib/conversationOrder.ts`.

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
| `metadata` | JSON | Attachments and other connector fields. Reactions live in their own table (below) |

Edits and deletions are facts too (`message.changed`, see [Events](#events-connectorevent)), applied only by `db/chat.rs`. An edit replaces `body` and sets `metadata.edited_at`; an older or repeated edit is ignored. "Deleted for everyone" turns the row into a tombstone: `body` becomes empty, `metadata` becomes `{revoked_at}`, its reactions are deleted, and a later report of the same message never refills it. "Deleted for me" deletes the row and records `(conversation_id, remote_id)` in `deleted_messages`, so a later sync of that message is ignored. When the changed message is the one the chat preview comes from, the preview is re-derived from the newest stored message (a tombstone previews as "This message was deleted"); chat time never moves.

Unique index: `(conversation_id, remote_id)` where `remote_id IS NOT NULL`. History sync and live events share this; duplicates are ignored. `list_messages` takes the latest *n* rows then reverses so the UI gets chronological order.

### Replies (`metadata.reply_to`)

A reply is a fact on the message: `metadata.reply_to = { remote_id, sender?, snippet?, media_type?, from_me? }`, where `remote_id` is the message it answers. Connectors report it for live events, REST rows and history dumps; the app stores the same shape (plus `sender_name`) on a reply sent from Shuttle. It never touches a chat row. When a message is listed or emitted, `db/replies.rs` resolves the link into `Message.reply` (`ReplyPreview`: `message_id` of the stored original, `sender_name` (`You` for the account's own), `snippet`, `media_type`, `filename`, `thumbnail_path`, `available`). The quote is built from the stored original when there is one, so an edited or newer text wins; otherwise from the connector's `snippet` and `available` is `false` (the UI shows the quote but does not link it). `reply` is never stored. A later report of the same message merges `reply_to` field by field into the stored one, so a history pass can add a link to a message stored before replies were reported and a thinner echo never erases a fuller link.

A duplicate report can still fix one thing: a group message stored with the group's own JID (or no person) as `sender_id` by an older build gets the real author when a later report names one. A real author is never overwritten.

### Message reactions (`message_reactions`, `accounts/<id>/inbox.sqlite`)

| Field | Type | Notes |
|-------|------|-------|
| `conversation_id` | UUID | Parent conversation (the inbox file fixes the account). Deleted with the chat; moved to the survivor when chats merge |
| `target_remote_id` | string | **Remote** id of the message reacted to, not `messages.id`. A reaction that arrives before its message attaches when the message is stored |
| `reactor_id` | string | `me` for this account, otherwise the reactor's JID (phone JID when known) |
| `reactor_name` | string? | Name shown in the tooltip |
| `emoji` | string | Never empty in storage |
| `from_me` | bool | The account itself reacted |
| `reacted_at` | datetime | When the reaction was made |

Primary key `(conversation_id, target_remote_id, reactor_id)`: one reactor has one reaction per message. A newer fact replaces the stored one, an empty emoji deletes it, and a fact older than the stored one is ignored (history replayed after a live event cannot undo it). A reaction is not a message: it never changes the chat's time, preview or unread count, never creates a chat, and a reaction for a chat that is not stored yet is dropped. `list_messages` returns each message with `reactions: [{emoji, count, names, mine}]` (most used emoji first, names oldest first, `You` for the account).

Sidecar events carry `payload.history`. It is `true` for bulk sync (connect-time sync, the one-time backfill, history-dump import, opening a chat) and `false` for live delivery and the periodic catch-up. It decides only whether a message notifies, and whether an old message is announced to the UI (`message.received` is not sent for history older than the inbox baseline). For WhatsApp it does not change unread, titles or chat order. Other connectors keep their previous title rules. `chat.synced` tells the UI to reload the open thread after `sync_chat`.

Outbound status moves `pending` → `sent` (the network's message ID is stored in `remote_id`) or `failed`. See [Chat state rules](#chat-state-rules).

### Inbox metadata (`inbox_meta`, `accounts/<id>/inbox.sqlite`)

| Key | Value | Meaning |
|-----|-------|---------|
| `unread_baseline` | RFC 3339 time | Set once, when the inbox is first migrated. The effective read marker is never earlier than this, so old history is not unread (except for chats flagged `unread_keep`) |
| `read_marker_v1` | timestamp | Set once the upgrade from the unread counter has run. The upgrade does not run again |

### Contacts

Defined in `models.rs` and schema; populated by connectors during sync (not yet wired in stubs).

### Workspaces (`app.sqlite`)

Named buckets. Seeded: `default` (`builtin = 1`). Users can add more. Builtin rows cannot be deleted. Deleting a custom workspace nulls `accounts.workspace_id` and `conversations.workspace_id` that pointed at it.

A chat's **effective workspace** is `conversation.workspace_id` → else `account.workspace_id` → else `default`.

### Priority groups (`app.sqlite`)

Independent of workspace. Seeded: `urgent`, `waiting`, `later`. Builtin rows cannot be deleted. Stored on the conversation as `priority_group` (id string).

### App meta and settings (`app.sqlite`)

| Table | Purpose |
|-------|---------|
| `app_meta` | Installation ID and created-at (anonymous telemetry identity) |
| `app_settings` | Persisted app config JSON (imported from `config.json` when missing) |

### Chat todos and notes (`app.sqlite`)

Local only — never sent to a sidecar.

| Table | Purpose |
|-------|---------|
| `chat_todos` | Checklist items (`body`, optional `due_at`, `done`, optional `message_id` + `message_snippet` quoting the message it came from) keyed by conversation + account |
| `chat_notes` | Per-chat note items: `body`, optional `message_id` + `message_snippet`, optional schedule (`kind` `timer` or `alarm`, `fire_at` as UTC RFC 3339 with `Z`, `fired`). A timer is a duration resolved to `fire_at = now + duration` when created; an alarm is an absolute date-time (past times are refused, with 90 s slack) |
| `reminders` | Legacy, no longer written. Unfired rows were copied into `chat_notes` as alarms (migration `chat_notes_from_reminders_v1` in `app_meta`); the table is kept as a backup |

Notes replace the old single `conversations.notes` text: on first start after the upgrade each non-empty text becomes one note item (`legacy-notes-<conversation id>`, guarded per inbox by `notes_items_v1` in `inbox_meta`). The column itself is left untouched as a backup and is no longer shown. Code: `shuttle-app/src-tauri/src/db/notes.rs`.

A tokio interval (20s) loads due notes (`db.due_notes()`), shows a native notification (the note text, or its quoted snippet), marks them fired, and emits `reminder.fired`. This loop is the only place timers and alarms fire. Deleting an account also cleans up its local todos, notes, forwarding rules, and scheduled messages from the catalog; merging chats repoints todos and notes.

### Connectors (registry table)

Static rows seeded at migration: `whatsapp`, `telegram`, `signal`, `messenger`, `instagram`, `email`, `matrix`. Runtime metadata comes from `ConnectorManager::list_connectors()`.

Each `ConnectorInfo` carries `actions` (`MessageActions`), from the one table in `connectors/message_ops.rs`:

| Field | WhatsApp | Others |
|-------|----------|--------|
| `react` | `true` | `false` |
| `edit` | `true` (own messages, text or a media caption) | `false` |
| `delete` (delete for me) | `true` | `false` |
| `delete_everyone` | `true` (own messages) | `false` |
| `edit_window_secs` | `900` | `null` |
| `delete_everyone_window_secs` | `172800` | `null` |

`null` means no time limit. The UI decides what to offer from this table only. `apply_message_op` checks the same rules (`check_op`, which takes the current time as a parameter) before anything reaches a connector.

### App config (not SQLite)

`config.json` in the data dir holds appearance, app-wide notification prefs, per-connector channel styles, `translate` (`endpoint`, `api_key`, both empty by default, which keeps translation off), and `performance` (`low_memory_webview`, Linux WebKit tuning, default on). See [storage.md](storage.md) and `config.rs`. Invalid / missing files fall back to defaults (`color_scheme: system`, `theme_id: shuttle`).

## Chat state rules

Connectors report facts. The Rust core decides what is stored, and all of those decisions live in one module, `shuttle-app/src-tauri/src/db/chat.rs`. The same facts in any order give the same stored state. Connectors must not add per-path flags to steer this. The old `history`, `force_recency`, `replace_title` and `list_rank` hints are no longer used for WhatsApp.

- **Chat time and preview.** `last_message_at` is the later of the stored and incoming times, so it never moves back. The preview is replaced only by something at least as new as the stored chat time.
- **Titles (WhatsApp).** One function computes the title. Direct chats: saved contact, then business verified name, push name, the stored title if it is not a placeholder, then `+<digits>`. Groups and channels: subject, the stored title, then `Group`. The account's own chat is `<name> (You)` when a saved name exists, else `Message yourself`. Message events never rename a chat, and a chat created by a message is never titled with the sender's name. Events without `names` (other connectors) keep their earlier title rules.
- **Unread.** The badge is the number of inbound messages newer than the chat's effective read marker, the later of `read_through` and `unread_baseline`. It is at least 1 while `marked_unread` is set. The marker moves forward when the chat is opened (`mark_read`), when the account sends a message from any device, and on `chat.read`. Which sync path delivered a message does not matter.
- **One chat, several IDs.** An event that names a chat by `remote_id` and `alt_remote_ids` either renames the stored row to the canonical ID, or merges the rows that exist under several IDs. A merge moves messages (skipping duplicates), keeps the later read marker, fills settings gaps from the other row, and repoints todos, notes, forwarding rules and scheduled messages.
- **Sends.** The local row's ID goes out as `request_id`. A reply with a network message ID sets that ID and `sent`. An error with the `request_id` marks only that row `failed`. When the echo of a message the account sent arrives and no row has its ID, it is matched to a local outbound row with no ID (same body, within 180 seconds) instead of adding a second row.
- **Upgrade.** Existing chats keep their badge. Each chat's marker is set so that exactly its current number of inbound messages is newer than it.

Tests for these rules are in `shuttle-app/src-tauri/src/db/tests.rs` and run with `cargo test` in `shuttle-app/src-tauri`.

## Connector protocol

Sidecars are Python processes. Local dev may use wrappers under `connectors/bin/`; release builds spawn the bundled CPython against `*-connector.py`. They communicate with the core over **newline-delimited JSON** on stdin (requests) and stdout (responses/events).

Protocol version: **1** (`PROTOCOL_VERSION` in `shuttle-app/src-tauri/src/connectors/protocol.rs`).

### Lifecycle

1. Core spawns **one sidecar process per connector type** (`python` + `*-connector.py`). Further accounts of the same network attach to that process with `authenticate` / `account_id`.
2. Core sends `handshake` → sidecar replies `handshake_ok`.
3. Core sends `authenticate` with `account_id` and stored `credentials` (from the keyring).
4. Sidecar may reply `auth_required` (QR, phone, etc.) then `status` updates. Extra codes go through `submit_auth`.
5. On `account.connected`, core sends `sync_history`. Sidecars paginate existing chats/messages.
6. Sidecar emits `event` lines for async activity (messages, connection, etc.).

Because one process serves several accounts, the WhatsApp connector puts `account_id` on every response and event, and each account has its own worker thread inside the process.

### Requests (`ConnectorRequest`)

| `type` | Fields | Purpose |
|--------|--------|---------|
| `handshake` | `protocol_version` | Version negotiation |
| `authenticate` | `account_id`, `credentials` | Start auth flow with stored secrets |
| `submit_auth` | `account_id`, `credentials` | QR follow-up, SMS/2FA code, etc. |
| `connect` | `account_id`, `credentials` | Connect with stored creds |
| `disconnect` | `account_id` | Tear down session |
| `send_message` | `account_id`, `conversation_id`, `text`, `reply_to_id?`, `reply_to_sender?`, `request_id?` | Outbound message. `request_id` is the local message row ID, echoed in the reply. `reply_to_id` is the remote ID of the message being answered (a real reply, not quoted text); `reply_to_sender` is its author JID when it is not this account |
| `message_op` | `account_id`, `conversation_id`, `message_id`, `op`, `request_id` | Act on a message already on the network. `conversation_id` and `message_id` are remote IDs. `op` is one of `{"kind":"react","emoji":"👍"}` (`emoji: null` removes this account's reaction), `{"kind":"edit","text":"..."}`, `{"kind":"delete_for_everyone"}`, `{"kind":"delete_for_me"}`. The connector answers `ok` or `error` with the same `request_id` and, on success, reports the result as a `reaction` or `message.changed` fact |
| `mark_read` | `account_id`, `conversation_id`, `message_id?` | Remote read receipt for `message_id` (the newest inbound message), only if policy allows. Without `message_id` the WhatsApp connector does nothing |
| `sync_history` | `account_id` | Paginate existing chats/messages after connect |
| `get_status` | `account_id` | Poll status |
| `shutdown` | — | Exit sidecar |

`send_message` and `send_attachment` both take `request_id`, `reply_to_id`, `reply_to_sender` and `forwarded`. Optional fields are left out of the JSON when unset (`forwarded` when false), so older connectors keep working. `forwarded: true` marks the message as forwarded on networks that show it; the WhatsApp connector passes GOWA v9.9.0's `is_forwarded`. A forwarded attachment names its cached file in `path` instead of sending `data_base64`.

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
| `ok` | `request_id?`, `message_id?`, `conversation_id?`, `account_id?`. A send reply carries the WhatsApp message ID in `message_id` |
| `error` | `message`, `account_id?`, `request_id?`. `request_id` is set when the error answers one send |

A reply whose `request_id` belongs to a waiting `message_op` goes to that op only (the command returns its outcome) and never touches a message row or the account status. A send reply with `request_id` and `message_id` sets that message's remote ID and status `sent`. A reply with `request_id` and `conversation_id` but no `message_id` marks the row `sent` without an ID; its ID arrives with the echo. An error with `request_id` fails that row only and leaves the account status alone. An error with `account_id` and no `request_id` is an account error.

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

WhatsApp `conversation.updated` payloads carry `remote_id` (the phone JID whenever the connector knows it, otherwise the `@lid` JID), `alt_remote_ids`, `names`, `is_self`, `title`, `conversation_type`, `last_message_at`, `preview`, `archived`, `pinned` and `history`. `names` is always present and may be `{}`. Its keys are `contact`, `subject`, `verified` and `push`. A payload without `names` is a legacy event from another connector and is handled by the earlier rules. WhatsApp message events carry the same `alt_remote_ids` and a validated `message.timestamp`. A message without a valid timestamp or ID is dropped, never stamped with the current time. The core ignores `force_recency`, `replace_title`, `list_rank` and `unread_count` if they are sent.

`chat.read` tells the core that the account read a chat on another device, up to `read_through`:

```json
{
  "type": "event",
  "event": "chat.read",
  "account_id": "550e8400-e29b-41d4-a716-446655440000",
  "payload": {
    "remote_id": "remote-chat-id",
    "alt_remote_ids": ["remote-lid-id@lid"],
    "read_through": "2026-11-10T09:30:00Z"
  }
}
```

`reaction` reports reactions to messages of one chat. It carries `remote_id`, `alt_remote_ids`, `history` and a `reactions` list. WhatsApp sends one entry live (GOWA `message.reaction`) and batches of up to 200 from history dumps:

```json
{
  "type": "event",
  "event": "reaction",
  "account_id": "550e8400-e29b-41d4-a716-446655440000",
  "payload": {
    "remote_id": "120363000000@g.us",
    "history": false,
    "reactions": [
      {
        "target_message_id": "3EB0C767D0A1",
        "reactor_id": "919686361793@s.whatsapp.net",
        "reactor_name": "Dad",
        "emoji": "❤️",
        "timestamp": "2026-11-11T18:56:50Z",
        "from_me": false
      }
    ]
  }
}
```

An empty `emoji` means the reaction was removed. Connectors report the facts only; `db/reactions.rs` stores them and the core sends the UI a `message.reactions` event for the messages that changed. A reaction sent from Shuttle comes back as the same fact with `reactor_id: "me"` and `from_me: true`.

`message.changed` reports that a stored message was edited or deleted, whether on the phone, by the other person, or from Shuttle:

```json
{
  "type": "event",
  "event": "message.changed",
  "account_id": "550e8400-e29b-41d4-a716-446655440000",
  "payload": {
    "remote_id": "15551234567@s.whatsapp.net",
    "alt_remote_ids": ["99887766@lid"],
    "message_id": "3EB0C767D26A1B0A",
    "change": {"kind": "edited", "text": "fixed typo", "at": "2026-11-16T10:00:00+00:00"}
  }
}
```

`change` is `{"kind":"edited","text","at"}`, `{"kind":"revoked","at"}` (deleted for everyone) or `{"kind":"deleted_for_me"}`. `db/chat.rs` applies it (see [Messages](#messages)); the UI gets `message.updated` or `message.removed`, plus `conversation.updated` when the preview changed.

`poll.changed` reports something that happened to an existing poll: a vote, an edited question, or an added option. A poll's creation is a normal message event whose `message` carries `media_type: "poll"`, the question as its text, and `poll` (`question`, `multiple`, `options` of `{id, text}`). A vote is never a message of its own, and the chat's time, preview and unread count never move because of one. GOWA reports the `poll` object beside the message; `db/chat.rs` folds it into `metadata.poll` of the poll message, keyed by voter, so a replayed vote or a change of mind converges on the same counts:

```json
{
  "type": "event",
  "event": "poll.changed",
  "account_id": "550e8400-e29b-41d4-a716-446655440000",
  "payload": {
    "remote_id": "120363000000@g.us",
    "poll_id": "3EB0C767D0A1",
    "kind": "vote",
    "poll": {
      "question": "Where shall we eat?",
      "multiple": false,
      "options": [{"id": "4f2a...", "text": "Pizza"}]
    },
    "vote": {
      "voter_id": "me",
      "voter_name": "You",
      "from_me": true,
      "selected": ["4f2a..."],
      "timestamp": "2026-11-17T09:05:00Z"
    }
  }
}
```

`kind` is `vote`, `edit` or `add_option`; `vote` is present only for a vote. `vote.voter_id` is `me` for this account, else the voter's JID, as with reactions. An empty `selected` clears that voter's vote. The option `id` is the network's option hash, which is also what `selected` names, so a vote still folds after a question is edited. `poll` may be `{}` when GOWA could not resolve the poll's own definition. A vote for a poll message the core has not stored yet is dropped, never invented as a message.

The core parses stdout lines as either `ConnectorResponse` or `ConnectorEvent`, applies the [chat state rules](#chat-state-rules), updates SQLite, rebroadcasts to the UI, and may show a native notification (see [Notifications and read receipts](#notifications-and-read-receipts)).

## Event types (UI / `shuttle-event`)

The core forwards internal events to the frontend via Tauri `emit("shuttle-event", …)`.

| `kind` | When | Payload highlights |
|--------|------|-------------------|
| `auth.required` | Sidecar needs QR/phone auth | `account_id`, `method`, `qr_data`, `url`, `message` |
| `account.status` | Status line from sidecar | `account_id`, `status`, `identity` |
| `account.connected` | Session ready | `account_id` |
| `history.sync.started` | Core requested history, or sidecar began paging | `account_id` |
| `history.sync.completed` | Sidecar finished paging | `account_id` (passthrough) |
| `status.feed` | WhatsApp status/stories snapshot or one new status | `account_id`, `items[]` (each with `posts[]`) and/or `upsert`. A post carries `thumbnail_path` when its media has a preview, and `link_preview` (`url`, `title`, `thumbnail_path` or `null`) when the status is a link |
| `status.media` | Downloaded status media for the story viewer | `account_id`, `message_id`, `media_path` / `error` |
| `account.avatar` | Connected account profile photo for the rail | `account_id`, `avatar_data` (data URL) |
| `media.thumbnails` | Connector extracted previews for messages it already reported | `items[]` of `conversation_id`, `message_id`, `thumbnail_path` (the core merges them into message metadata; the UI event carries only `account_id`) |
| `media.purged` | Retention deleted cached media and reset those messages' `media_path` | `accounts[]` (UI event only) |
| `conversation.updated` | Chat upserted from sidecar, or changed by a merge or `chat.read` | `account_id`, `conversation` (`unread_total` on read events) |
| `conversation.removed` | A chat was merged into another | `account_id`, `conversation_id`, `into_conversation_id`. A `conversation.updated` for the surviving chat follows |
| `chat.synced` | Open-chat history pull finished | `account_id`, `remote_id` |
| `message.received` | New inbound message persisted, and not old history | `account_id`, `conversation_id`, `conversation`, `message`, `unread_total` |
| `message.sent` | New outbound message persisted, and not old history | `account_id`, `conversation_id`, `conversation`, `message`, `unread_total` |
| `message.updated` | Existing message row changed: sent status or network ID after a send, an echo matched a local row, or the message was edited or deleted for everyone | `account_id`, `conversation_id`, `message` |
| `message.removed` | A send duplicated a message already stored so its local row was deleted, or the message was deleted for this account | `account_id`, `conversation_id`, `message_id` |
| `message.reactions` | Reactions on stored messages changed (live reaction, removal or replacement). Not sent for a large history import; the next message-list load shows those | `account_id`, `conversation_id`, `updates[]` of `{remote_id, reactions[]}` (the full new groups for that message) |
| `reminder.fired` | Due note timer/alarm delivered | `note_id`, `conversation_id`, `account_id` |
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
| `list_conversations` | `account_id?`, `workspace_id?`, `priority_group?`, `archived_only?`, `offset?`, `limit?` | `Conversation[]` | Sorted and paged in SQL. Omits archived unless `archived_only` |
| `count_conversations` | same filters as `list_conversations` | `number` | Total for the paged list |
| `update_conversation` | `conversation_id`, `patch` | `Conversation` | Pin, archive, mute, workspace, priority, notes, notify, receipts |
| `get_messages` | `conversation_id`, `limit?` | `Message[]` | Default limit 100 |
| `send_message` | `account_id`, `conversation_id`, `text`, `reply_to_message_id?` | `Message` | Stores the row as `pending`, then sets its network ID and `sent`, or `failed`, from the connector's reply. `reply_to_message_id` is the local ID of a message in the same chat; the row stores `metadata.reply_to` and the connector sends a real reply (an error if that message has no network ID yet). `send_attachment` takes the same field. Forwards and scheduled messages are never replies |
| `apply_message_op` | `conversation_id`, `message_ids[]`, `op` (`MessageOp`, same shape as the `message_op` request) | `MessageOpResult[]` (`{message_id, error?}`) | Checks each message against the network's `actions` (ownership, text or caption, time window, sent, not deleted), sends one `message_op` per message and waits up to 30 s for each answer. The stored result arrives as events. Takes a list so several messages can be deleted at once |
| `translate_text` | `conversation_id`, `message_ids[]`, `target` (language code, the UI sends the app locale) | `Translation[]` (`{message_id, text?, error?}`) | Sends each message body to the LibreTranslate-compatible server in `config.translate` (`POST <endpoint>/translate`, `source: auto`, `api_key` only when set). Errors with `{kind:"not_configured"}` when no endpoint is set. Results are not stored |
| `forward_messages` | `source_conversation_id`, `message_ids[]`, `targets[]` (`{account_id, conversation_id}`, any account), `send_at?` | `ForwardResult[]` (`{account_id, conversation_id, message_id, status}`) | Forwards stored messages, never text. Each source resolves in `connectors/forward.rs`: text goes out through `send_message`; image, video, audio, voice note, sticker and document go through `send_attachment` with the cached file and the original caption, downloading it first through the normal media download when the file is missing; locations, polls, contacts and deleted messages are `not_forwardable` (with `reason`). One result per (target, source), targets in order, sources in chat order. `status` is `sent`, `scheduled`, `not_forwardable` or `failed` (with `error`); one failure never stops the other pairs. With `send_at` each forwardable pair becomes a scheduled message with `forward = 1` that stores the source message, not its content |
| `sender_contact` | `account_id`, `sender_id` | `SenderContact` (`{conversation?, contact?}`) | A group member's direct chat on the same account (by JID or a recorded alias) and their contact row. Reads only, never creates a chat |
| `list_forward_rules` / `create_forward_rule` / `update_forward_rule` / `delete_forward_rule` | | | Persistent inbound text routing rules |
| `list_scheduled_messages` / `schedule_message` / `delete_scheduled_message` | `include_sent?` on list | | Queue for delayed sends and delayed forwarding. A row with `forward` set sends its source message (`source_conversation_id`, `source_message_id`) through the same path as `forward_messages` when due; its `body` is only a preview |
| `export_backup` | `path`, `password`, `include_messages?` | `BackupManifest` | Password-protected `age` bundle |
| `restore_backup` | `path`, `password` | `()` | Restores files into the data dir; restart afterward |
| `mark_read` | `conversation_id`, `send_remote?` | `()` | Moves the read marker to now (or the newest message time, if later), which clears the badge. Remote receipt only if `send_remote` or policy (`should_send_receipt`), for the newest inbound message |
| `mark_unread` | `conversation_id` | `()` | Keeps at least one unread until the chat is read again |
| `search_conversations` | `query` | `Conversation[]` | Title + body search |
| `total_unread` | — | `number` | Sum of non-archived, non-muted unreads (muted accounts excluded) |
| `get_app_config` | — | `AppConfig` | From `config.json` |
| `save_app_config` | `config` | `AppConfig` | Writes `config.json` |
| `list_workspaces` / `create_workspace` / `rename_workspace` / `delete_workspace` | | | Builtin workspaces cannot be deleted |
| `list_priority_groups` / `create_priority_group` / `rename_priority_group` / `delete_priority_group` | | | Builtin groups cannot be deleted |
| `list_todos` / `add_todo` / `set_todo_done` / `delete_todo` | `add_todo` takes optional `message_id`, `message_snippet` | | Per conversation |
| `list_notes` / `add_note` / `update_note` / `delete_note` | `add_note`: `body`, optional `message_id`, `message_snippet`, `schedule` (`{timer_secs}` or `{alarm_at}`); `update_note`: optional `body`, `schedule`, `clear_schedule` | `ChatNote` | Per conversation. Setting a schedule re-arms a fired note; `clear_schedule` cancels it |
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

Media downloads report completion through `media.downloaded`; the download command only acknowledges queuing (`download_message_media(account_id, conversation_id, message_id, auto)` returns whether a download was requested; `auto = true` is skipped unless the effective auto-download setting allows the kind). Saving or explicitly retrying media waits for that event, filtered by account, conversation and local message ID, then `save_media_file(source_path, suggested_name)` copies the cached file to the save folder. Success clears `media_error`. Downloads land in the app-private media cache, not Documents (see [storage.md](storage.md#media-cache-and-saved-media)). Media facts the connector may attach to a message: `file_length`, `mime`, `width`, `height`, `duration` and `thumbnail_path`. GOWA can wrap WhatsApp CDN 403/404/410 responses in HTTP 500; these produce a safe retry/resend message rather than exposing raw upstream errors. The pinned GOWA download endpoint does not request a media re-upload from the phone.

1. Download the GOWA binary: `./connectors/gowa/fetch.sh`. The release is pinned (`GOWA_PINNED_VERSION` in `connectors/gowa/fetch.sh`), and each archive is checked against `connectors/gowa/checksums.txt`. See [connectors/gowa/README.md](../connectors/gowa/README.md).
2. One GOWA process serves every WhatsApp account. Shuttle starts `whatsapp rest --host=127.0.0.1` with basic auth. It is not bound to the LAN.
3. Each Shuttle account is a GOWA device (`POST /devices`), and its device ID is the account ID. GOWA webhooks name the device in `session_id`, and the connector routes each event to that account.
4. QR login uses `GET /devices/{id}/login`. The PNG is sent to the UI as a data URI.
5. Live events come from a loopback webhook (`PATCH /devices/{id}/webhook`), because GOWA's `/ws` often only carries login codes. Receipts (`message.ack`) arrive the same way. A read receipt from this account's own number becomes `chat.read`. `message.reaction` becomes a `reaction` event: the payload's `reaction` (empty = removed), `reacted_message_id`, `chat_id`, `from` / `from_lid` and `timestamp`, or `is_from_me` for reactions made on another device of this account. `message.edited` (`original_message_id`, `body`), `message.revoked` (`revoked_message_id`, `chat_id` or `revoked_chat`) and `message.deleted` (`deleted_message_id`, a delete-for-me from another device) become `message.changed` events. Message ops use GOWA v9.9.0's `POST /message/:message_id/reaction` (`{phone, emoji}`, empty emoji removes), `/update` (`{phone, message}`), `/revoke` and `/delete` (`{phone}`). A message payload that carries GOWA's `poll` object (`type`, `poll_id`, `question`, `options` of `{name, hash}`, `selectable_options_count`, `selected_options`, `selected_option_hashes`) becomes a poll: `type: "creation"` is the poll message itself, with `metadata.poll`; `vote`, `edit` and `add_option` become `poll.changed` facts about the poll message named by `poll_id`, so they never appear in a thread as their own message. GOWA v9.9.0 has no route that casts a vote, so the poll card is display only.
6. Catch-up is the fallback for missed webhooks. While connected, about every 8 seconds, it reads one `GET /chats` page and fetches `GET /chat/{jid}/messages` only for chats whose last message time moved, or that are new. It emits nothing when nothing changed. Every 38th cycle it also re-checks the listed chats, so a message that shares its second with the last one seen is still recovered. A failed fetch leaves that chat's cursor where it was.
7. Connect-time sync lists all chats and emits their rows, then fetches messages newer than each chat's cursor. Chats with no cursor yet are backfilled once, in the background (up to 500 messages each). Backfilled chats, per-chat cursors and imported dump names are kept in `connectors/whatsapp/<account id>/sync-state.json` under the data directory.
8. History dumps (`history-*.json` in `gowa/storages`) are imported once each. Only dumps whose file name carries this account's own number are used. The periodic metadata refresh also checks for late-arriving dumps after connect-time sync finishes. A group message's author is the message-level `participant` of the dump row (a LID, mapped to the phone JID when known), never the group JID. Dump rows also carry `reactions`; they are reported as `reaction` events. Dumps imported by an older build get one extra pass (tracked as `reaction_files` in `sync-state.json`) that reports their reactions and re-reports group messages so the core can repair the stored author. A dump message that answers another carries `contextInfo.stanzaID` (the replied-to ID), `participant` and `quotedMessage`; they become `reply_to`. Dumps imported before replies were reported get one more pass (`reply_files`) that re-reports only the replying messages, and chats backfilled before that get one REST pass (`reply_chats`); both send messages only, never the chat row. Live webhook payloads name a reply with `replied_to_id` and `quoted_body`; a REST row holds `{"replied_to_id": ...}` as the JSON string `context_metadata`.
9. Opening a chat reads that chat's most recent messages (up to 80) from GOWA's store and emits `chat.synced`. It does not parse history dumps, and it does not request older history from the phone the way WhatsApp Web does.
10. Chat identity: the phone JID is the chat ID whenever WhatsApp's LID-to-phone map (whatsmeow's `whatsmeow_lid_map`) knows the mapping. The `@lid` form is sent as `alt_remote_ids`. The map is loaded at connect and re-read, at most every 30 seconds, when an unknown LID appears.
11. Names: each chat's `names` come from the address book, the group subject, the business verified name and the push name. The account's own name is allowed only for the self chat; phone numbers and JIDs are never sent as names. Contacts and account-scoped pinned/archived settings refresh about every minute, including cached chats outside the catch-up page. Group subjects come from `GET /user/my/groups`, refreshed every five minutes after the first non-empty response (empty/failed initial results retry each minute). Failed or transiently empty contacts/group responses preserve existing names. Settings from whatsmeow take precedence over stale flags on GOWA chat rows.
12. Sending uses `POST /send/message`; a reply adds `reply_message_id` (GOWA looks the quoted message and its author up in its own store, so the request carries no participant; a group reply works the same way). GOWA v9.9.0 documents `reply_message_id` for text only: for images, video, audio, stickers and files the connector still sends the field, but GOWA may ignore it, so a media reply can arrive unlinked. The reply carries the WhatsApp message ID, which goes back to the core with the request ID. `mark_read` calls `POST /message/{id}/read` for the newest inbound message, if the core sent one.
13. Per-account isolation: pinned and archived flags are read from WhatsApp's chat settings only for this account's own JID, since the table is shared by every account in GOWA's database. Media, avatars and previews go to the account's own cache folder (see [storage.md](storage.md)).

If GOWA is already running locally, set `SHUTTLE_GOWA_URL` (loopback only) and optional `SHUTTLE_GOWA_USER` / `SHUTTLE_GOWA_PASSWORD`.

Rebuild sidecars after editing Python:

```bash
./connectors/build.sh
```

The WhatsApp connector's unit tests use only the standard library: `python3 -m unittest discover -s connectors/tests`.

## Notifications and read receipts

Native notifications use **`notify-rust`** (Linux: Desktop Notifications over D-Bus; macOS / Windows: the crate’s native backends). Failures are logged at debug under `shuttle::notification`; the INFO log line is still written either way.

Linux needs a notification daemon (Cinnamon, GNOME Shell, `dunst`, etc.). Without one, `notify-rust` fails quietly.

### When a notification is shown

A message can notify only if all of these hold (see `IngestOutcome::notify_eligible` in `db/chat.rs`):

- It is a newly stored inbound message.
- It arrived live or through catch-up (`history` is false). History never notifies.
- Its time is after the chat's read marker.
- It is at most 10 minutes old.

Then `notifications::should_notify` applies the rules below (most specific wins):

1. **Muted always blocks** — conversation `muted` or account `muted`.
2. Chat `notify_enabled` if set (`false` blocks; `true` still has to pass app-wide).
3. Else account `notify_enabled` if set.
4. App-wide `config.notifications.enabled` and quiet hours (`quiet_hours_enabled`, `HH:MM` start/end; overnight ranges such as `22:00`–`08:00` are supported).

Note timers and alarms use the same `notify()` helper and ignore mute/quiet-hours (they are explicit user-scheduled).

### Read receipts

`mark_read` **always** moves the chat's read marker, which clears the local unread badge. The sidecar `mark_read` request is sent only when:

- the command’s `send_remote` argument is `true`, or
- `send_remote` is omitted and `should_send_receipt` is true: conversation `send_receipts` if set, otherwise account `send_receipts` (default **false**).

The request names the newest inbound message (`message_id`). A read on another device arrives as `chat.read`. It moves the marker here and sends no receipt.

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
