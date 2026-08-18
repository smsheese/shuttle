# Shuttle roadmap

What is still open after **0.1.0**. The unified inbox, Rust core, per-account SQLite, keyring, seven connectors, routing, backup commands, telemetry, and desktop packaging already exist. This document is the remaining product surface.

Items are grouped by dependency, not by the order they were suggested. Overlapping requests (chat reminders vs reminders; notifications vs per-chat mute) are merged into one track.

**Principles that do not change**

- Local-first. No Shuttle cloud. Optional AI APIs are the only thing that may leave the device, and only for chats the user opts in.
- Connectors stay isolated sidecars. New networks are new processes, not UI special cases.
- Credentials never go in SQLite or the WebView. Backup/restore must keep that split.
- Unofficial APIs (Messenger, Instagram, X, LinkedIn, …) stay clearly marked and easy to disable.

Status: **done** · **partial** (schema or a first cut exists) · **planned**.

Architecture: [overview.md](overview.md). History: [CHANGELOG.md](../CHANGELOG.md).

---

## Snapshot

| Track | Highlights | Status |
| --- | --- | --- |
| 1. Desktop shell | Kill Inspect/Reload, real context menus, account disable/mute/remove | done |
| 2. Notifications | Native notifications, per-account and per-chat, optional no read receipts | done (`notify-rust`; receipts off by default) |
| 3. Organization | Workspaces, priority groups, notes/todos, chat reminders | done (no recurring / “if unreplied” yet) |
| 4. Appearance | Light/dark, tweakcn paste, bundled presets, per-channel colours | done |
| 5. Routing | Cross-channel forward, forwarding rules, scheduled send | done for text routing and scheduling |
| 6. Media & composer | Players, PDF, basic image editor, rich text per channel | partial (channel-aware rich-text composer landed) |
| 7. Backup | Encrypted export of config + secrets + session pointers | partial (password-protected export/restore commands and settings UI) |
| 8. AI replies | OpenAI / Claude / OpenRouter, personality, delay, sanitization | planned |
| 9. More networks | X, iMessage, SMS, Google Chat, LinkedIn, then others | partial (Matrix added; rest planned) |
| 10. Calls | Audio first, then video — network-dependent | planned, hardest |
| 11. Platforms | amd64 + arm64 desktop on Windows, Linux, macOS; Android later | partial (desktop CI + docs; Android future) |
| 12. Telemetry | Opt-in Sentry / PostHog, `.env` vs GitHub Environments | done (0.0.9–0.1.0) |

---

## Already shipped (do not rebuild)

Use these instead of inventing parallel systems:

- `delete_account` / `update_account` (disable, mute, workspace, receipts) plus UI account menus and settings.
- Conversation flags: `pinned`, `archived`, `muted`, workspace override, priority, notes, per-chat notify/receipts.
- `mark_read` always clears local unread; sidecar `mark_read` only when receipt policy allows (default off).
- Native notifications via `notify-rust` + `config.json` quiet hours; muted always wins.
- `config.json` for appearance (system/light/dark, theme id, tweakcn CSS) and channel styles — not SQLite.
- Catalog tables in `app.sqlite`: `workspaces`, `priority_groups`, `chat_todos`, `reminders`, `forwarding_rules`, `scheduled_messages`, `app_meta`, `app_settings`.
- In-app context menus; default browser Inspect/Reload menu blocked; DevTools debug-only.
- `attachments/` directory reserved; message `metadata` JSON for media later.
- Connector catalog is a list in `ConnectorManager::list_connectors()` plus SQLite seed rows (including Matrix).
- Telemetry commands and Privacy toggles; see [telemetry-events.md](telemetry-events.md).

---

## 1 — Desktop shell and account hygiene

**Landed.** In-app context menus, account disable/mute/remove, chat flags, and debug-only DevTools. Remaining from the original list: schedule follow-up, save media, and richer forward (track 5).

The WebView still behaved like a browser; this track made it feel like a messaging client first. Later features hang off these menus.

### WebView chrome

- Disable the default right-click menu (Inspect, Reload, Back, View Source) in production builds.
- Keep a hidden/dev-only way to open DevTools (Tauri `devtools` feature or a secret gesture), never in release AppImages.
- Production CSP already limits script; pair that with `contextmenu` prevention on `document` and Tauri window config.

### Context menus

Native-feeling menus (Tauri menu or an in-app overlay), not the browser menu.

| Target | Actions (initial set) |
| --- | --- |
| Conversation | Open, pin, mute, archive, priority, workspace, notes, remind, notification prefs, mark unread |
| Message | Copy, reply, forward (same chat / other chat / other account), schedule follow-up, select text, save media |
| Selected / structured text | Copy; open URL; copy code fence; copy inside `` ` ``, `"…"`, `'…'`, `()`, `[]`, `{}` |

Structured-text selection: on right-click, detect the innermost quoted/bracketed/fenced span under the cursor and offer “Copy inner text” vs “Copy whole message”.

### Accounts

- **Remove** — wire `delete_account` in the UI (confirm, then wipe sidecar + SQLite + keyring + session dir).
- **Disable** — stop the sidecar, keep local history, skip reconnect until enabled.
- **Mute** — account-level: no notifications, still sync. Chat-level mute already has a DB column; expose it.

---

## 2 — Notifications and read receipts

**Landed** with `notify-rust` (not `tauri-plugin-notification`). Linux needs a notification daemon; that is noted in the README. Workspace/priority *default* notify prefs are not implemented — only app / account / chat plus mute.

**Rules (most specific wins)**

1. App-wide on/off and quiet hours.
2. Per-account.
3. Per-chat (and priority group / workspace defaults).
4. Muted always wins.

**Read receipts**

- Per-account and per-chat: open a thread **without** sending `mark_read` to the sidecar.
- Local unread badges still clear if the user wants “mark read locally only”.
- Default: do not send receipts until the user opts in per network (WhatsApp/Telegram/Signal all treat this differently).

Reminders (track 3) fire through this same notification backend.

---

## 3 — Organization

**Landed** for workspaces, priority groups, notes/todos, and one-shot reminders. Not yet: recurring reminders, “if they have not replied by …”, per-account channel-style overrides, unread totals scoped to the workspace filter.

### Workspaces

Named buckets: Personal, Work, Others, plus user-defined. An account or a single chat can belong to one workspace (chat override wins). The sidebar filters by workspace; search and unread totals respect the filter.

### Priority groups

User-defined groups (e.g. Urgent, Waiting, Later) independent of workspace. Inbox sections or a filter chip. Not the same as pin — pin is a shortcut; priority is a queue.

### Channel styling

Per connector (and optional per-account override): background, tag/badge colour, font. Applied to the conversation list row, thread header, and outbound bubble accent. Does not restyle the whole app (that is tweakcn).

### Notes and todos per chat

Local only — never synced to the network.

- Freeform notes on a conversation.
- Checklist todos with due dates.
- Shown in a thread side panel or overflow, not inline as fake messages.

### Chat reminders

One system: “nudge me about this chat at …”. Optional: “if they have not replied by …”. Delivered via the notification track. Recurring later if needed; first version is a single datetime.

---

## 4 — Appearance

**Landed.** Light/dark (OS default + override), bundled presets (`shuttle`, `zinc`, `ocean`, `twilight`), pasted tweakcn CSS, and per-connector tag colours in `config.json`. Per-account style overrides and chat wallpapers are still open.

Tokens live in `app.css` (`--bg-main`, `--accent`, …) with `:root[data-theme=light]` and preset data attributes.

- **Light and dark** as first-class `color-scheme`s, following the OS by default, with a user override.
- **tweakcn**: map Shuttle tokens onto the shadcn/tweakcn CSS variable set (background, foreground, primary, muted, radius, fonts). User can:
  - paste a tweakcn theme (CSS or id/slug from [tweakcn.com](https://tweakcn.com)),
  - pick a bundled preset,
  - keep light and dark variants of the same theme.
- Theme id is stored in local config, not SQLite messages. Invalid ids fall back to the default Shuttle theme.

Channel styling (track 3) sits *on top* of the active theme (tag colour, chat wallpaper), so a tweakcn swap does not wipe per-network accents.

---

## 5 — Cross-network routing

This is the product differentiator. Implementation lives in the Rust core, not in individual sidecars.

### Manual forward

1. Same account, another chat (WhatsApp → WhatsApp).
2. Same person, another of *their* accounts if we can match contacts later.
3. Any chat on any connected account (WhatsApp → Telegram, Telegram → Signal, …).

Payload: text first; then images/files when attachments exist. If the destination cannot take the original media type, fall back to a labelled text quote plus a local file path / “open in Shuttle”.

Each outbound copy is a normal `send_message` on the destination sidecar. Shuttle now has:

- manual text forward to any loaded conversation,
- forwarding rules stored in SQLite and evaluated on inbound live messages,
- optional rule delay via the scheduled-message queue,
- a composer “Later” flow and a scheduled follow-up context-menu path.

### Forwarding rules

Persistent rules, evaluated in the core on `message.received`:

- Source: account, chat, or workspace.
- Destination: one or more chats (possibly other networks).
- Filters: inbound only, from me / from them, keyword, media vs text.
- Options: delay, prefix/suffix, strip sender name, skip if already forwarded (loop guard).

Rules must be easy to disable. Never forward from a chat that is in an AI auto-reply loop without an explicit allow.

### Scheduled messages

Queue in SQLite: destination account + chat + body + `send_at`. A tokio scheduler task now sends when due while the app is running and marks rows as sent after a successful dispatch.

Composer: “Send later” on the send button and via message context menu.

---

## 6 — Media and composer

### Viewers / players

In-thread, then a lightbox:

- Images (png/jpeg/webp/gif).
- Video (system codecs; don’t bundle a full ffmpeg if the OS player suffices).
- Audio (voice notes and files).
- PDF (embedded viewer; print/save).

Store files under `attachments/` as already reserved; SQLite keeps paths and MIME, not blobs.

### Basic image editor

Crop, rotate, annotate (pen, arrow, blur), then send. Used from the composer attach flow and from “edit before forward”.

### Rich text editor, channel-aware

The composer should not pretend every network is Slack.

| Network | What we actually send |
| --- | --- |
| Telegram / Signal | Markdown-ish / their native entities |
| WhatsApp | Bold/italic/strike/mono as WhatsApp understands them |
| Email | Multipart plain + simple HTML |
| Messenger / Instagram | Plain text unless the backend supports more |

UI: a small formatting bar. Switching destination account restyles the preview and strips unsupported marks on send rather than failing.

---

## 7 — Backup and restore

Goal: after a crash or a new machine, restore *accounts and login material*, not necessarily every historical message (those can re-sync where the network allows).

Current state: Shuttle can export a password-protected backup bundle from Rust and restore it back into the data dir. The current restore path is intentionally conservative: it copies files into place and expects an app restart to reload open SQLite handles safely.

**Export bundle** (user-chosen path, encrypted):

- App config (theme id, workspaces, rules, notification prefs, AI settings without API keys duplicated in plaintext).
- Account catalog (connector id, name, identity).
- Keyring items and connector session dirs (GOWA store, TDLib db, signal-cli config, 0600 session files).
- Optional: SQLite inbox databases.

**Restore** writes into a fresh `SHUTTLE_DATA_DIR`, re-seals secrets into the OS keyring, then reconnects sidecars.

Must never dump secrets into the Svelte UI. Export/import are Tauri commands that stream files from Rust.

---

## 8 — AI chat replies

Opt-in per chat / per channel. Default off. API keys live in the keyring, same as IMAP passwords.

**Providers:** OpenAI, Anthropic (Claude), OpenRouter (and thus other models behind one key). User picks provider + model per profile.

**Personality:** named styles (concise, formal, “sounds like me” with a short writing sample). Samples stay local.

**Auto-reply**

- Which accounts/chats.
- Delay (e.g. 30s–15m) so the user can cancel.
- Quiet hours, skip groups unless mentioned, skip if the user is already typing.

**Sanitization (required, not optional)**

Before any request leaves the machine:

- Strip keyring-shaped values, tokens, cookies, `api_hash`, session JSON.
- Redact phone numbers, emails, and auth codes unless the user enables “include contact details” for that chat.
- Never attach GOWA/TDLib/signal-cli files.
- Cap history window; do not upload whole inboxes.
- Log only that a call happened (provider, model, chat id, token counts) — not prompt text — unless the user enables a local debug file.

Failed sanitization = do not send. Show a local preview of the outbound prompt on demand.

---

## 9 — More networks

Same sidecar protocol. Each network is a fetch script + Python/Rust wrapper + catalog row.

| Network | Notes |
| --- | --- |
| **X / Twitter** | DMs; unofficial or official API depending on what remains usable. Mark ToS risk. |
| **iMessage** | macOS only (Messages / private APIs / AppleScript). No Linux equivalent. |
| **SMS** | Platform-specific (Android gateway, modem, or macOS). Linux desktop has no universal SMS. |
| **Google Chat** | Prefer official Workspace APIs with OAuth over scraping. |
| **LinkedIn** | Messaging is hostile to unofficial clients; expect breakage; keep optional. |
| **Later** | Slack, Discord, RCS, others as demand and APIs allow. UI already has unused Slack/Discord colour stubs. |

Ship one network at a time with a capability matrix (text, media, receipts, groups, calls). Do not block the rest of the roadmap on the hardest ones (iMessage/SMS).

---

## 10 — Calls (audio, then video)

Highest uncertainty. Do not fake a unified WebRTC layer that none of the backends share.

- **Telegram:** TDLib has call APIs — first candidate for 1:1 audio.
- **Signal:** depends on signal-cli (often messaging-only); may never match the official app.
- **WhatsApp:** GOWA is a Web multi-device gateway; calls may be impossible or poor.
- **Others:** treat as unsupported until proven.

Product shape when a backend allows it: in-thread call button, native audio device access via Tauri, optional video. Cross-network calls (WhatsApp user ↔ Telegram user) are **out of scope** — Shuttle is not a SIP bridge.

---

## Suggested build order

```
1 Shell + menus + account controls
        ↓
2 Notifications + read-receipt policy
        ↓
3 Workspaces / priority / notes / reminders
        ↓
4 Light/dark + tweakcn
        ↓
5 Forward + rules + schedule          6 Media viewers (can overlap)
        ↓
7 Backup/restore
        ↓
8 AI (after backup + sanitization design)
        ↓
9 Extra connectors (ongoing, parallel after 1)
        ↓
10 Calls (only where a sidecar actually can)
```

Tracks 6 and 9 can proceed in parallel with 4–8 once attachments and the connector protocol exist. Track 10 waits on real backend support.

---

## Protocol / schema additions

Most of this already landed. Remaining when media and AI ship:

- Connector capability flags: `calls`, `rich_text`, `read_receipts_optional`, `media`.
- Attachment metadata on messages (paths under `attachments/`, MIME) — not blobs in SQLite.

Already in `app.sqlite` / the protocol: `forward_message`, `schedule_message`, conversation `workspace_id` / `priority_group` / notes, tables `workspaces`, `forwarding_rules`, `scheduled_messages`, `reminders`, `chat_todos`, `app_meta`.

---

## Explicitly later or no

- Shuttle-hosted accounts or a Shuttle relay for calls/AI.
- Encrypting the message DB at rest (still desirable; not in this feature list).
- Cross-network audio bridges.
- 32-bit desktop builds.

signal-cli **is** bundled in release builds (GPL-3.0 sidecar, not linked into Rust). See [ATTRIBUTION.md](../ATTRIBUTION.md) and [licensing.md](licensing.md).

When a track ships, move it to [CHANGELOG.md](../CHANGELOG.md) and tick the snapshot table above.
