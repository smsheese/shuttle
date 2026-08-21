# Shuttle roadmap

What is still open after **0.3.0**. The unified inbox, Rust core, per-account SQLite, keyring, seven connectors, shared per-network sidecars, routing, backup pickers, media viewers/editor, daily-driver chrome (tray, Ctrl+K, shortcuts, account sleep), telemetry, and desktop packaging already exist. This document is the remaining product surface under Shuttle’s north star.

Items are grouped by dependency, not by the order they were suggested. Overlapping requests (chat reminders vs reminders; notifications vs per-chat mute) are merged into one track.

**North star**

Shuttle should be the **no-thought answer** for low-resource desktop messaging: lightweight and responsive, cross-platform, strong quality of life — and still easy for contributors to extend (sidecar + JSON protocol, not a webview recipe zoo).

**Principles that do not change**

- **Lightweight + responsive.** Idle RSS, wake latency, and UI snappiness are product requirements. Ferdium’s measured ~2.7 GiB multi-account idle load is the public bar to beat by 50–70% ([track 13](#13--resource-budget-vs-ferdium)); do not regress for feature parity with Electron aggregators.
- **Cross-platform.** Same app on Windows, Linux, and macOS (amd64 + arm64). Prefer shared code over per-OS forks.
- **Quality of life.** Tray, shortcuts, sleep, mute, routing, organization — reduce friction without embedding every website.
- **Developer ease.** New networks = new isolated sidecars. Keep the protocol small, documented, and Python-friendly; avoid designs that force UI or Chromium work per service.
- Local-first. No Shuttle cloud. Optional AI APIs are the only thing that may leave the device, and only for chats the user opts in.
- Credentials never go in SQLite or the WebView. Backup/restore must keep that split.
- Unofficial APIs (Messenger, Instagram, X, LinkedIn, …) stay clearly marked and easy to disable.

Status: **done** · **partial** (schema or a first cut exists) · **planned**.

Architecture: [overview.md](overview.md). History: [CHANGELOG.md](../CHANGELOG.md). Resource bar and daily-driver chrome: [track 13](#13--resource-budget-vs-ferdium) and [track 14](#14--daily-driver-chrome--qol).

---

## Snapshot

| Track | Highlights | Status |
| --- | --- | --- |
| 1. Desktop shell | Kill Inspect/Reload, real context menus, account disable/mute/remove | done |
| 2. Notifications | Native notifications, per-account and per-chat, optional no read receipts | done (`notify-rust`; receipts off by default) |
| 3. Organization | Workspaces, priority groups, notes/todos, chat reminders | done (no recurring / “if unreplied” yet) |
| 4. Appearance | Light/dark, tweakcn paste, bundled presets, per-channel colours | done |
| 5. Routing | Cross-channel forward, forwarding rules, scheduled send | done for text routing and scheduling |
| 6. Media & composer | Players, PDF, basic image editor, rich text per channel | partial (lightbox, canvas editor, Giphy, emoji; channel send-preview still open) |
| 7. Backup | Encrypted export of config + secrets + session pointers | partial (pickers, optional media, restart prompt; in-place merge still needs restart) |
| 8. AI replies | OpenAI / Claude / OpenRouter, personality, delay, sanitization | planned |
| 9. More networks | X, iMessage, SMS, Google Chat, LinkedIn, then others | partial (Matrix added; rest planned) |
| 10. Calls | Audio first, then video — network-dependent | partial (protocol kept; UI is honest — no fake WebRTC) |
| 11. Platforms | amd64 + arm64 desktop on Windows, Linux, macOS; Android later | partial (desktop CI + docs; Android future) |
| 12. Telemetry | Opt-in Sentry / PostHog, `.env` vs GitHub Environments | done (0.0.9–0.1.0) |
| 13. Resource budget | Beat Ferdium by 50–70% RSS on a comparable multi-account idle load | partial (sleep + shared sidecars + `scripts/rss-sample.sh`; Shuttle numbers not yet measured) |
| 14. Daily-driver chrome | Tray, quick switch, shortcuts, account sleep, search polish | partial (must-haves landed; FTS polish and later QoL remain) |

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
- Downloaded media lives under `~/Documents/shuttle/<account>/`; message `metadata` JSON holds paths and MIME, not blobs.
- Account sleep / hibernation, wake on open, and one sidecar process per connector type (multi-account attach).
- System tray, Ctrl/Cmd+K quick switch, keyboard shortcuts; media lightbox + canvas image editor; backup pickers with optional media.
- Connector catalog is a list in `ConnectorManager::list_connectors()` plus SQLite seed rows (including Matrix).
- Telemetry commands and Privacy toggles; see [telemetry-events.md](telemetry-events.md).

---

## 1 — Desktop shell and account hygiene

**Landed.** In-app context menus, account disable/mute/remove, chat flags, and debug-only DevTools. Remaining from the original list: schedule follow-up and richer forward (track 5). Save media is in the message menu and lightbox.

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

**Landed** for workspaces, priority groups, notes/todos, and one-shot reminders. Not yet: recurring reminders, “if they have not replied by …” (also listed under [track 14](#14--daily-driver-chrome--qol)), per-account channel-style overrides, unread totals scoped to the workspace filter.

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

**Landed for viewers/editor:** in-thread players, lightbox (image/video/PDF attempt), canvas crop/rotate/pen/arrow/pixelate on attach, Giphy/emoji, save media. Remaining: channel-aware send preview, WhatsApp-native mark syntax.

### Viewers / players

In-thread, then a lightbox:

- Images (png/jpeg/webp/gif).
- Video (system codecs; don’t bundle a full ffmpeg if the OS player suffices).
- Audio (voice notes and files).
- PDF (embedded viewer; print/save).

Downloaded files go under `~/Documents/shuttle/<account>/media|avatars`; SQLite keeps paths and MIME, not blobs.

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

**Honest stub.** Protocol commands exist; the UI does not advertise `calls:audio` or show a fake in-call mixer. Real audio waits on a backend that can actually capture/play media.

Highest uncertainty. Do not fake a unified WebRTC layer that none of the backends share.

- **Telegram:** TDLib has call APIs — first candidate for 1:1 audio.
- **Signal:** depends on signal-cli (often messaging-only); may never match the official app.
- **WhatsApp:** GOWA is a Web multi-device gateway; calls may be impossible or poor.
- **Others:** treat as unsupported until proven.

Product shape when a backend allows it: in-thread call button, native audio device access via Tauri, optional video. Cross-network calls (WhatsApp user ↔ Telegram user) are **out of scope** — Shuttle is not a SIP bridge.

---

## 13 — Resource budget vs Ferdium

**Partial.** Account sleep (default 5 minutes idle) and `scripts/rss-sample.sh` landed. Re-run the 4 WhatsApp + 1 Telegram idle hour and put Shuttle numbers in the table below — do not invent them.

**Goal:** prove the north star in numbers. On a comparable multi-account idle load, Shuttle should use **50–70% less RSS** than Ferdium — so “which messaging hub is light?” has an obvious answer. Developer ease still wins: we get there via sleep + lean connectors, not by making sidecars harder to write.

### Reference benchmark (Ferdium)

Measured locally, **12:50:56 → 13:49:29** (60 min, 30 samples @ 2 min):

| Metric | Value |
| --- | --- |
| Workload | **4 WhatsApp + 1 Telegram + 1 ChatGPT**; **4 of 6** set to hibernate after **5 min** idle |
| CPU | avg **0.19%**, max **3.8%** (idle the whole hour) |
| RAM (RSS) | avg **2.67 GiB**, median **2.69 GiB**, min **2.11 GiB**, max **4.24 GiB** |
| Processes | **16–21** (typically ~18) |
| Notes | Peak 4.24 GiB around service spawn (~12:52); settled ~2.6 GiB as unused renderers exited. Start baseline ~3.2 GiB. |

Ferdium’s architecture (Electron + one Chromium renderer per active service) explains the floor: hibernation helps, but the shell still holds ~2.7 GiB with mostly idle services.

### Shuttle target (beat by 50–70%)

Comparable **messaging** load for Shuttle: **4 WhatsApp accounts + 1 Telegram** (no ChatGPT webview equivalent — that is out of scope for this bar). Idle / light use, with account sleep enabled where possible.

| Bar | Avg RSS target | vs Ferdium 2.67 GiB |
| --- | --- | --- |
| **Must hit (50% less)** | **≤ ~1.35 GiB** | 50% reduction |
| **Stretch (70% less)** | **≤ ~0.80 GiB** | 70% reduction |
| Spike ceiling | Prefer **&lt; ~2.0 GiB** on reconnect/sync | Ferdium spiked to 4.24 GiB |
| CPU | Stay near-idle when quiet (same order as Ferdium’s &lt;1% avg) | Not the differentiator |

**Pass criteria:** a 60-minute sample (same cadence as above) with 4 WA + 1 TG connected, majority sleeping after idle timeout, avg RSS in the **0.8–1.35 GiB** band. Document method (OS, sampler, process set) next to the numbers so re-runs stay comparable.

### How Shuttle wins structurally

| Ferdium | Shuttle |
| --- | --- |
| Bundled Chromium + N service webviews | One OS WebView (UI) + N connector processes |
| Hibernation unloads renderers (~5–10 MB stub each when it works) | Account **sleep** stops idle accounts; UI cost stays fixed; one process per network |
| Full web app parity per service | Normalized SQLite inbox; pay only for protocol clients |

### Implementation levers (do these for the budget)

1. **Account sleep / auto-hibernate** — **landed** (default 5 min idle; wake on open / quick-switch). Manual `disabled` remains permanent.
2. **Shared connector processes** — **landed** (one Python sidecar per network; attach multiple accounts). Native helpers (GOWA, TDLib, signal-cli) still scale with how those tools are spawned.
3. **Lazy connectors** — keep release installers slim; never keep Signal’s JVM warm unless a Signal account is active (Signal is the known heavy sidecar).
4. **No background webviews** — never embed ChatGPT / Slack / Gmail as webviews to “match Ferdium service count”; breadth comes from connectors or stays out of the resource budget.
5. **Measure** — `scripts/rss-sample.sh` samples `shuttle` + child PIDs the same way as the Ferdium run.

Re-run the 4 WhatsApp + 1 Telegram idle hour and put Shuttle numbers in the table above — do not invent them.

---

## 14 — Daily-driver chrome and QoL

**Must-haves landed:** system tray (close-to-tray, unread tooltip), Ctrl/Cmd+K quick switch, documented shortcuts, account sleep distinct from disable. Remaining in this track: FTS polish and the high-value QoL table below.

Lightweight alone is not enough — Shuttle should feel like the obvious daily client: responsive chrome, less friction, still easy to extend.

### Must-have for daily driver

| Item | Notes |
| --- | --- |
| **System tray + unread badge** | Minimize to tray; click restores; badge = `total_unread` (respect mute). |
| **Quick switch (⌘/Ctrl+K)** | Fuzzy jump to chat, account, or workspace. |
| **Keyboard shortcuts** | Navigate list, archive, mute, search (`/`), mark read — document in Settings. |
| **Account sleep** | Auto-stop sidecar after idle (default ~5 min to match the Ferdium comparison); wake on activity / notification path. Distinct from permanent **disable**. |
| **Full-text search polish** | Global / network / in-chat already started; keep indexing cheap and scoped to open inboxes. |

### High-value QoL (after chrome)

| Item | Notes |
| --- | --- |
| **Snooze conversation** | Hide until datetime; extends reminders. |
| **Contact linking** | Same person across WA / TG / … for smarter forward targets. |
| **Message templates / snippets** | Local-only canned replies. |
| **Per-account proxy** | Route sidecar traffic (restrictive networks). |
| **Workspace-scoped unread** | Unread totals respect workspace filter (called out under org track). |
| **“If no reply by …”** | Conditional reminder on top of one-shot nudges. |
| **Split pane** | Two threads on wide desktops. |
| **Density** | Compact vs comfortable conversation rows. |
| **Channel-aware send preview** | Show how formatting will land before send. |
| **Rules import/export** | Share forwarding setups across machines (pairs with backup). |

### Explicitly not copying from Ferdium

- Recipe / webview service zoo (100+ sites).
- Hibernating Chromium tabs as the primary memory strategy.
- Bundling a second browser engine inside the app.

---

## Suggested build order

```
1–5, 12, 14 must-haves, 6 viewers/editor, 7 pickers
                already landed
        ↓
13          Measure Shuttle RSS (sleep is in; numbers TBD)
        ↓
8           AI (after backup + sanitization)
        ↓
9           Extra connectors (ongoing; do not inflate idle RSS)
        ↓
10          Calls (only where a sidecar actually can)
```

Tracks 6 and 9 can proceed in parallel once sleep keeps idle cost honest. Track 10 waits on real backend support. **Do not add webview-based “extra services” to chase Ferdium’s service count** — that would blow track 13.

---

## Protocol / schema additions

Most of this already landed. Remaining when media, sleep, and AI ship:

- Connector capability flags: `calls`, `rich_text`, `read_receipts_optional`, `media`.
- Attachment metadata on messages (paths under `attachments/`, MIME) — not blobs in SQLite.
- Account sleep state: last activity, sleep after minutes, wake reason (user / notification / rule) — catalog columns `sleep_*` plus `config.sleep`; runtime last-activity in the connector manager.

Already in `app.sqlite` / the protocol: `forward_message`, `schedule_message`, conversation `workspace_id` / `priority_group` / notes, tables `workspaces`, `forwarding_rules`, `scheduled_messages`, `reminders`, `chat_todos`, `app_meta`.

---

## Explicitly later or no

- Shuttle-hosted accounts or a Shuttle relay for calls/AI.
- Encrypting the message DB at rest (still desirable; not in this feature list).
- Cross-network audio bridges.
- 32-bit desktop builds.
- Embedding arbitrary websites (ChatGPT, Gmail, …) as first-class “accounts” to match Ferdium’s recipe list.

signal-cli **is** downloaded on demand in release flows (GPL-3.0 sidecar, not linked into Rust). See [ATTRIBUTION.md](../ATTRIBUTION.md) and [licensing.md](licensing.md).

When a track ships, move it to [CHANGELOG.md](../CHANGELOG.md) and tick the snapshot table above.
