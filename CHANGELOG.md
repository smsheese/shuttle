# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Versions **0.0.1–0.0.9** reconstruct the development history from Cursor agent sessions and the implementations that landed. **0.1.0** is the current release (environment-variable configuration).

## [Unreleased]

## [0.6.1] - 2026-11-23

### Fixed
- Installed builds could not download connectors: the components URL baked into 0.6.0 pointed at a bucket that never received files, so no WhatsApp connector started and no messages arrived. The upload step never ran (its condition could not see its own env), and the bucket layout did not match the manifest URLs.
- Component archives are no longer attached to the GitHub release when a bucket is configured, so the release holds only the installers.

### Changed
- Release CI reads `POSTHOG_PROJECT_TOKEN`, `POSTHOG_HOST`, `S3_BUCKET`, `S3_ENDPOINT` and `SHUTTLE_COMPONENTS_BASE_URL` from repository variables; only the S3 keys stay secrets. The build fails if the PostHog token is missing.

## [0.6.0] - 2026-11-20

### Added
- Settings, Feedback tab with the feedback form embedded. The form loads only after you press the button, and Shuttle sends no chat data with it. The form's own script is not loaded into the app.
- `PRIVACY.md`: every case where data leaves the computer, who receives it and how to turn it off. Settings, About shows the installation ID for deletion requests.
- Usage diagnostics now carry what is needed to tune Shuttle for weak computers and many accounts: real CPU and memory of Shuttle and its helper programs on all three OSes, account count ranges (1, 2-3, 4-5, 6-10, 11 or more), chat and message count ranges, and median, 95th percentile and slowest time for the chat list, opening a chat and search. Still no names, numbers, identifiers or message content.
- `.github/workflows/ci.yml`: Python and Rust tests, `svelte-check`, `gitleaks`, `cargo deny` and `npm audit` on every push and pull request, plus a weekly advisory run.
- `deny.toml`: licence allow list (permissive only, matching the AGPL plus commercial model) and advisory policy.

- Licensing: Shuttle stays AGPL-3.0-or-later, with a separate paid commercial license (`COMMERCIAL-LICENSE.md`) for products that cannot meet the AGPL, and a contributor license agreement (`CLA.md`, `CONTRIBUTING.md`) that lets the project offer that license. `docs/licensing.md` and `ATTRIBUTION.md` now state that signal-cli, GOWA, TDLib and the Python runtime are downloaded components rather than bundled, and cover dependency rules and the Shuttle name and logo. The installer ships the AGPL text once, as `licenses/Shuttle-AGPL-3.0.txt` (the duplicate `third-party/licenses/AGPL-3.0.txt` is gone).
- Typing `@` in a group chat opens a list of that group's members (not every contact) above the composer. Typing more letters filters it, Up/Down move through it, and Enter or Tab tags the person (Esc closes it). Picked names are sent as real WhatsApp mentions.
- Pasting an image, video or audio file into the composer asks, next to the input, whether to send it as media or as a file (document). A pasted document stages as a file straight away.
- `docs/beta-checklist.md`: keys to supply, distribution, optimization and risk checklist for the beta.
- Settings has fields for your OpenRouter API key and model id, and for your Giphy API key. GIF and sticker search works as soon as you add a Giphy key, with no rebuild.
- Release workflow signs builds when secrets are present: macOS (Developer ID and notarization), Windows (code-signing certificate with timestamp) and Linux (GPG detached signatures plus `SHA256SUMS`). Releases now include AppImage, rpm and NSIS installers as well as deb, dmg and msi.
- Connector components are attached to the same GitHub release as the installers, and the app downloads them from there by default (`SHUTTLE_COMPONENTS_BASE_URL` still overrides). An Intel macOS component set is published too.
- Files can be dropped onto a chat. Dragging one over the chat shows a `Drop to attach` overlay, and dropping one or more files puts them in a bar above the input with a picture preview, the file name, the size and a remove button; the caption goes in the normal input and each file is sent as its own message with the caption on the first. Photos go as photos, videos as videos, audio as audio, and anything else as a document under its own name and type. Pasting an image from the clipboard fills the same bar. A file dropped anywhere outside the chat no longer makes the window navigate to it.
- In search results across several accounts, each chat and message result shows its account name at the right end of the name line and the account's accent colour as a bar on its left edge. Picking one account hides the account name.
- In a WhatsApp group where only admins can send, the composer is replaced by "Only admins can send messages", with the attachment, emoji, GIF, schedule and reply controls hidden. Such chats are not offered as forward targets. The connector reports whether the group is admin-only and whether you are an admin, refreshed every 5 minutes and on connect. A chat that never reported this stays writable.
- On startup Shuttle preloads the last 15 messages of each account's 20 most recent chats so those chats open without waiting. Every open still re-reads the database and replaces the cached rows.
- A status post carries the link card WhatsApp attached to it (`url`, `title`, and a stored preview image), and every media status carries `thumbnail_path`. Channels report their real name from GOWA's newsletter info and from the history dumps, so `Newsletter 123456` is no longer the name a channel has to keep.
- A WhatsApp direct chat shows an audio and a video call button in the contact bar above the messages, with icons instead of emoji. GOWA v9.9.0 has no route that places a call and only rejects incoming ones, so the buttons open that chat in WhatsApp, where the phone or the desktop app starts the call, and a toast says so. When the chat has no phone number to link to, the toast says the call starts on the phone. The same two buttons in the contact details panel behave the same way for WhatsApp and keep calling directly on the connectors that advertise `calls:`.
- Message actions in the right-click menu: a quick bar of six reactions plus "+" for the full emoji picker (choosing your current reaction again removes it), Edit for your own text or caption within 15 minutes, Delete with "Delete for me" and, for your own messages within two days, "Delete for everyone", and Translate. Reactions show at once and the network's answer is the truth; a failed action shows a toast and rolls the reaction back.
- Edits and deletions made on the phone or by the other person now show in Shuttle: an edited message gets a small "edited" label, a message deleted for everyone stays in place as "This message was deleted", and one deleted for you is removed and never comes back through a later sync. Sent and received changes take the same path into the database.
- Opt-in translation through a LibreTranslate-compatible server you choose (Settings > Privacy > Translation, off by default). The translation appears under the message and is never saved. Without a server, Translate opens that setting.
- Connectors declare which message actions they support (`actions` on each connector: react, edit, delete, delete for everyone, and the edit and delete-for-everyone time limits). Only WhatsApp supports them today; the core enforces the same rules before anything is sent.
- URLs in message bodies are links: the accent colour on an inbound bubble and the bubble's own text colour on an outbound one, underlined on hover. They open in the system browser, never in the app window.
- A new Forward dialog: pick any number of chats on any account (they stay ticked while you change the account filter or search), each row shows the avatar, the account with its network icon and a group icon. Send forwards at once; the clock button schedules the forward instead. Failed chats are listed and stay selected, and Send retries only what failed. Forwarded messages carry WhatsApp's "Forwarded" mark and show a small "Forwarded" label in Shuttle.
- Select several messages: right-click a message and choose Select, then click messages (shift-click for a range) and Copy, Forward, Star, Translate or Delete them together from the bar above the thread. Esc leaves selection. Actions that do not apply to some messages say how many were skipped.
- View contact: in a group, right-click someone else's message and choose View contact. It opens your chat with that person on the same account, or, if you have none, their name and number without creating a chat.
- Scheduled forwards keep the original message, not a copy of its text, so a scheduled photo or file is fetched and sent with its caption when it is due.
- Replies are real WhatsApp replies: Reply shows a preview bar above the input (× or Esc cancels) and the next send is linked to that message, including in groups. Replies to media send the link too, but GOWA v9.9.0 may ignore it.
- Replies show a quote block (author, snippet or media label, image thumbnail) at the top of the bubble; click it to jump to and highlight the original. Incoming replies, replies made on the phone and history imports carry their link, and messages stored earlier are linked by a one-time background pass.
- Copy button in the media viewer (also Ctrl/Cmd+C) and a "Copy media" entry in the message menu put the downloaded media on the system clipboard (image data plus file reference for images, file reference for other files) so it pastes into other apps; where the OS can't take a file, the path is copied as text.

- Chat list previews start with an up or down arrow for sent or received, and group previews name the sender ("Samah: Hey test"). Existing chats are backfilled from their newest message.
- Group `@mentions` render as highlighted names (saved contact, push name or phone number; "You" for yourself); the WhatsApp connector reports mentions from history.

- Settings → Media & storage: save folder, "Ask where to save each time", keep downloaded media for N days (default 60, or forever) with daily cleanup, cache size and "Clear downloaded media now". Cleanup never touches saved files or chats on the network.
- After saving, a bottom-right "Saved to <path>" toast; clicking the path shows the file in the system file manager.
- Per-account media auto-download for images (on), videos (off) and files (off), with per-chat overrides from the chat list's right-click menu.
- Media that is not downloaded shows its WhatsApp preview where available, its size and a download button, including in the media viewer.

- WhatsApp reactions from live events and history are shown as a pill on the message bubble, with who reacted on hover.
- Per-chat note items, added from the chat panel or by right-clicking a message, each with an optional countdown timer or alarm.
- Todos and notes created from a message keep a quoted snippet; clicking it jumps to and highlights the message.
- In-app date and time picker with presets for scheduled messages, forwarding, reminders and note alarms; it closes on outside click or Escape and applies on Enter.
- The media viewer shows a thumbnail strip of the chat's images and videos, navigable by click, arrow keys or mouse wheel.

- Switch the selected theme’s dark mode on a custom schedule, by system preference, or from sunset to sunrise at a selected location.
- Choose an accent colour for each account; the sidebar shows its colour and unread count, and the combined conversation list marks each account with a matching stripe.
- Show the unread total in the window title and as a desktop taskbar or Dock badge where supported.

### Changed
- Chat rules look up the newest message, newest timestamp and unread count through a new index (`idx_messages_conv_epoch`). Measured on a synthetic 200,000-message chat: newest-message lookup 128 ms to 0.02 ms, timestamp maximum 45 ms to under 1 ms. The index builds once on first launch (about 0.15 s for 200,000 messages).
- Dependencies updated for security advisories: `rusqlite` 0.41 (bundled SQLite fix), `zip` 9, `rustls`, `rustls-webpki`, `quinn-proto`, `tokio`, `zstd` and the npm `uuid` pulled in by the Giphy SDK. `cargo audit` and `npm audit` are clean except one accepted `age` advisory documented in `deny.toml`.
- Release builds ignore empty signing secrets, so unsigned builds work without any Apple, Windows or GPG setup. The README lists the first-launch warnings.

- The sidebar's active account keeps its own accent colour on the left bar and its picture gets a rotating two-colour ring; inactive accounts are 10% smaller. All Chats gets a rotating theme-accent ring when active.
- Status cards drop the picture circle; the name sits on one line and scrolls like pinned chat names.
- The audio and video call buttons on a WhatsApp chat open a panel beside the buttons instead of opening wa.me at once; "Open in WhatsApp" is one click inside it.
- Translation now runs through your own OpenRouter model instead of a LibreTranslate server, so the same key and model can serve other AI features later. Nothing is sent until a key is set.
- Unread counts in the sidebar skip channels and status updates.
- New > Contact shows names with small avatars instead of raw WhatsApp JIDs, lists saved contacts and people you already chat with, and has an "Include unknown group members" toggle (off by default). Typed numbers are normalised and checked against WhatsApp; errors show inline, and Esc or a click outside closes the dialog.
- Downloaded media is cached with a proper file extension (file name, then mime type, then file contents).
- Reply no longer prepends `> quoted text` to the draft; "Send later" is disabled while replying.

- New Shuttle logo (`shuttle-logov1.1.svg`) replaces the old emoji icon everywhere: app, window and tray icon, installer icons (`.ico`, `.icns`, Windows Store tiles), iOS and Android launcher icons, favicon, sidebar brand, and splash screen. iOS icons are opaque on white.
- Downloaded media, avatars and previews live in an app-private cache (`<data dir>/media_cache`) instead of `~/Documents/shuttle`; only media you explicitly save goes to your save folder. Existing files in the old folder are left in place and stay readable.
- Saving media never overwrites an existing file (`name (1).ext`), and backups with downloaded media include the new cache.
- GOWA's duplicate copy of each download is removed after transfer.

- Pinned chats are a horizontal avatar row at the top of the list pane (network and unread badges, marquee name for long titles, account accent bar under the name) instead of rows in the chat list. The row is compact: 36 px avatars with a proportionally smaller unread badge.
- Every message bubble has the same 10px rounding on all four corners; the run-based tail corners are gone (bubble gaps are unchanged).
- Saved and copied media files are named after the account, the chat and the message time, e.g. `Work-Mum-261112-1325.jpg` (local time, two-digit date and time, safe on Linux, Windows and macOS). Documents keep their own file name.
- When no chat is open, the main pane shows an Updates home with status cards and a Channels list; WhatsApp channels moved there from the chat list (still found by search). On phones it sits above the chat list.
- Reminders live on notes: the chat panel's separate "Remind me" section is gone, and existing chat notes and unfired reminders are migrated to note items.

- Use Shuttle’s PostHog project configuration instead of the unrelated TFS instance.

- Replace Sentry with opt-in PostHog error tracking; exception reports exclude raw errors and stack traces to protect private messaging data.

### Fixed

- WhatsApp messages no longer wait behind a long history sync or avatar backlog: sends run on their own lane in the connector, so they go out at once and stay in order.
- A sticker no longer shows a stored timestamp such as `2026-11-19T06:03:15Z` under it.
- Shuttle no longer freezes and eats gigabytes of memory when a chat with hundreds of media messages (a sticker-heavy group) is open. The thread's media effect watched the maps it filled, so each finished file read re-ran it over every message and started the reads still in flight again, a quadratic pile-up of IPC calls and pending promises. Reads in flight are now tracked and the effect no longer subscribes to the result maps.
- Recording a voice note in the installed app no longer reports that Shuttle is not allowed to use the microphone. The webview was left to deny its own microphone request: WebKitGTK denies a `permission-request` signal nobody handles, and WebView2 fell back to a browser prompt a desktop app cannot draw. Shuttle grants the microphone to its own window on Linux and Windows, keeps the camera and every other request on the engine's default, and ships the macOS usage description and audio-input entitlement so the one-time macOS prompt can appear. When a real OS denial happens the dialog now names the macOS or Windows switch to open instead of blaming the app.
- Videos and other media from WhatsApp groups no longer show the word `unknown`. The connector read the sender's verification level as the message text, and live media was not recognised as media. Live videos, images, audio, documents and stickers now arrive as media with their caption, and `unknown` bodies already stored are repaired from the caption on startup. Messages stored before this keep only their caption as text, since they hold no media details.
- Right-click Copy on a photo, video or file copies the file itself, downloading it first when needed, instead of the text `[image]`. A caption is not copied with it, so text fields do not paste the caption in place of the picture.
- Opening a chat, including one you just left, no longer flashes "No messages synced for this chat yet". The thread now keeps the last messages of each chat in memory and shows them immediately, shows a spinner when a chat has nothing cached yet, and only shows the empty state once the read has finished. The account wake-up no longer delays reading messages.
- The Updates view no longer prints a connector placeholder as a chat name. A channel or group stored as `Group`, `Channel` or `Newsletter 123456` shows the name the connector reported, then the same chat seen through another account, and only then a neutral label such as `Unnamed channel`. Chat list avatars use the same name.
- A status card shows the picture of the first slide rather than of the newest post, and the story viewer draws that cached preview while the full media downloads. A status no longer opens onto a blank stage or `Could not load media`, and a feed refresh no longer leaves the viewer on an empty slide.
- An Instagram reel or Facebook video status shows a preview card with its picture, title and host name, plus a link chip that opens in the browser, instead of the raw URL as the story body. A link status with no picture shows the host name and title alone. Emoji-only statuses draw large and centred.
- WhatsApp chats show their picture previews again. Previews extracted from the history dumps were marked done while an earlier build wrote them to the Documents folder, so no chat had one and the backfill never retried. The sync state now records where previews went and re-extracts them when they are missing, and previews an older build left in the Documents folder are copied into the account cache. On the test account 671 previews came back.
- WhatsApp statuses no longer stop at the newest 80 posts, and the statuses the phone had already synced are included. GOWA was read one page deep and the history dumps' `statusV3Messages` were never read; both are paged and imported now, and an author known only by LID is reported as their phone number.
- WhatsApp messages show their formatting instead of the raw markers. `*bold*`, `_italic_`, `~strikethrough~`, `` `monospaced` ``, triple-backtick blocks, `> quotes`, `- ` and `* ` bullets and `1. ` lists are drawn as the reader expects, nested marks included, and links and mentions still work inside them. A lone marker, `snake_case` and `2*3*4` stay literal. Other networks render their text exactly as before.
- A sticker is drawn on the chat background, at its own size, with no bubble, border or shadow around it and no time under it. Its time appears only while the pointer is over it. Stickers download by themselves whatever the auto-download setting says, since the sticker is the message, and the cached preview is shown until the file lands.
- The voice note is recorded in the composer, not in a dialog. The microphone button after Send starts a take at once and its face becomes a pause button above a stop button, while the text input becomes a running timer with a level meter. Pause freezes the timer and the same button resumes the take; stop shows a player with the take's own waveform, a playhead you can click or arrow through, the duration and a discard button, and Send then sends it. Escape or discard drops the take, and the microphone is released on every path.
- The attach menu's Document entry offers every file type again, including zip, apk and bin, and takes several at once. A file whose type is not recognised is sent as `application/octet-stream` under its own name. The narrow `.pdf,.doc,.docx` list it had hidden most of those files from the picker.
- The location and poll forms stay inside the window at any width and scroll their contents when the window is short, instead of running off the right edge when the attach menu sits at the right of the composer.
- The `Forwarded` marker is a small pill in the top right corner of the message with a forward icon, on its own line beside the sender name, so it never covers the name or the reply it quotes. It reads on both the received and the sent bubble, in both themes.
- A WhatsApp poll is drawn the way WhatsApp draws it: the question in bold, `Select one` or `Select multiple`, one row per option with its vote count, a voter chip, a bar sized by that option's share of the votes and a tick on the option this account picked, then the time and ticks and a `View votes` button that lists who picked what. Options are display only, because GOWA v9.9.0 cannot cast a vote.
- A poll vote folds into the poll instead of arriving as its own `Poll vote: <option>` chat message. The connector reads the `poll` object GOWA reports beside a poll message: a creation becomes one message carrying `metadata.poll`, and a vote, a poll edit or an added option becomes a fact the core folds into that message by voter, so a replayed vote or a changed mind converges on the same counts and never moves the chat's preview or unread count. A vote for a poll that is not stored yet is dropped rather than invented as a message.
- Forwarding photos, videos, voice notes, stickers and documents sends the file with its caption. It used to send only the caption, or the media's internal id when there was no caption, behind a "Forwarded:" text prefix. Media that is not downloaded yet is downloaded first; locations and polls are reported as not forwardable instead of being sent as text.
- Forwarding a photo, video or other media that has no caption no longer sends its file hash (a string like `nDXT4hDD…QJs=`) as the caption. The core and the WhatsApp connector now treat a base64 file hash as no caption, the same rule the thread already used to hide it.
- Contact details no longer flickers between "Loading…" and the media grid while chats update in the background. The profile reloads only when you open a different chat, so thumbnails stay put and the dialog keeps its height.
- Linux: the Shuttle logo now shows in the panel and alt-tab. The 512 px window icon was silently dropped on X11 (no `_NET_WM_ICON`), so Linux now sets the 128 px icon; the icon is also re-applied when the window is first focused.
- Starting a chat from New > Contact (picking a contact or typing a number) now opens the thread; the new empty chat used to fall outside the loaded list page and never opened.
- Copy media from the message menu copies the actual file instead of its path; copied and saved media get readable names with extensions (e.g. `Work-Mum-261112-1325.jpg`, documents keep their name) via a small self-pruning staging folder.
- A pinned chat that receives a message brings the pinned row back to its left end, so the newest chat is visible. It used to scroll only when the first pinned chat changed, which never happened when that chat was the one updated.
- The window icon uses the large artwork instead of the 32 px icon, so Alt-Tab and the taskbar are no longer pixelated (Linux and Windows).
- The list-pane search field fills its whole pill and no longer shows an inner border or focus ring.

- A captionless media reply could take the quoted text or GOWA's reply JSON as its body.

- Scrolling up in a chat with the wheel or PageUp no longer snaps back to the latest message; the view follows new messages only when you are at the bottom or just sent one, and stays put while older messages or media load.
- The reaction pill sits at the bottom-left of the bubble so it no longer covers the time and ticks.
- Media and files no longer repeat the sender's name, a MIME type or the file name as a caption; captionless media from history keeps a proper placeholder.
- When a pinned chat gets a new message while the pinned row is scrolled, the row scrolls back so it is visible at the left end.

- "Save media" wrote nothing and left the window stuck on a full-screen preview; it now saves to the save folder (or asks) without navigating away.
- A successful media retry clears the earlier download error.

- Group messages imported from WhatsApp history were stored under the group's number; they keep their real author (repaired on next sync), and every member's first message in a run shows their name in a stable colour.
- The media viewer and image editor close on Escape or a click outside the media, and the viewer always has a close button.

- Prevent image message bubbles from expanding beyond the displayed image width.
- Keep account photos visible when switching account filters, and show account accents independently of network icon colours.
- Look up WhatsApp profile names for unsaved conversations and retain reported business names; long previews restart after reaching the end.
- Make status updates clickable and keep playback and account feeds separate.
- Reorganize conversation previews, unread badges, timestamps and pin markers; show available WhatsApp profile and business names for unsaved contacts.
- Accept tweakcn theme share URLs as well as theme IDs, and show progress while a theme is applied.

- WhatsApp media above the former 2.5 MB preview / 12 MB document cutoffs now downloads in bounded chunks to an atomic file (up to 2 GB for messages; statuses retain 30 MB). Interrupted downloads do not leave usable partial files. Local previews use Tauri's scoped file protocol instead of full-file base64 copies.
- Media failures show a safe reason and a retry button; successful retries clear the stored error. Save media waits for the download result instead of looking at stale message metadata immediately after queuing it.
- WhatsApp waits for authenticated status rather than treating a connected socket as a logged-in device.
- Aligned JavaScript Tauri packages with the Rust minor versions, and removed the quick-switch accessibility and unused-style warnings.

- Use the bundled icon for the native desktop window and display the sidebar logo at the full rail width without a colored container.

- WhatsApp retries contacts and pinned/archived settings after initial linking, refreshes group subjects from the groups endpoint, and updates older chats outside the catch-up page. An initially empty address book no longer leaves names and pins missing until reconnect.
- WhatsApp imports history dumps that arrive after the initial chat listing. Saved names also appear on the account's own chat.

### Removed

- The unused per-kind media retention setting in contact details, and the old reminder commands (replaced by note commands).

## [0.5.0] - 2026-11-11

### Added

- WhatsApp read receipts reach WhatsApp. When read receipts are on for a chat, opening it sends a receipt for its newest message. The read call used to do nothing.
- A chat read on another device (phone or WhatsApp Web) clears its unread badge in Shuttle.
- `SHUTTLE_GOWA_VERSION` overrides the GOWA release that `connectors/gowa/fetch.sh` downloads. The default is the pinned release.
- Automated tests for the WhatsApp connector (`python3 -m unittest discover -s connectors/tests`) and for the chat-state rules (`cargo test` in `shuttle-app/src-tauri`).

### Changed

- WhatsApp accounts are kept apart. Each account imports only its own GOWA history dumps, reads pinned and archived flags only for its own number, and stores media and avatars in its own folder under `Documents/shuttle/<account id>/`.
- Each WhatsApp account has its own worker thread in the connector, so one account connecting or syncing no longer delays sends from another.
- Chat order is the same everywhere: pinned first, then newest chat time, then title. Chat time only moves forward, and the preview follows it.
- Unread badges come from a per-chat read marker instead of a counter. Opening a chat, sending from any device, or reading on another device clears the badge. History sync no longer changes badges. Existing badges are kept on upgrade.
- "Mark as unread" keeps at least one unread until the chat is read again.
- Desktop notifications fire only for new messages that arrive live or through catch-up, are newer than the chat's read marker, and are at most 10 minutes old. Old history never notifies.
- WhatsApp catch-up makes one chat-list request per cycle and fetches only messages newer than each chat's last seen time. It emits events only when something changed.
- The WhatsApp message backfill runs once per chat, not continuously. The record of finished chats is kept in `connectors/whatsapp/<account id>/sync-state.json` under the data directory.
- Opening a chat reads only that chat's recent messages from GOWA. History dumps are imported once each, never when a chat is opened.
- The connector's unread counts and sort hints are ignored. The app decides unread and order itself.
- The conversation list is sorted and paged in the database. Refreshes are debounced, keep the pages already loaded, and no longer jump back to the first 30 chats.
- Idle use is lighter: a repeated WhatsApp event writes nothing, and the database uses `synchronous=NORMAL`.
- The GOWA release is pinned to v9.9.0, and each download is checked against its SHA-256. A checksum mismatch stops `scripts/publish-components.sh`. The pinned release has no Windows ARM build, so WhatsApp cannot be fetched on Windows ARM with this pin.
- **Upgrade note:** the first WhatsApp start after upgrading imports history dumps again and runs the one-time message backfill once for each chat.

### Fixed

- One person no longer appears as two chats when WhatsApp reports the contact by phone number and by LID. Duplicates merge into one chat, and messages, notes, todos, reminders, forwarding rules and scheduled messages move with it.
- WhatsApp chat names follow one order: saved contact, business verified name, push name, stored title, then the phone number. Groups use the group subject. A new chat is never named after its sender, and the account's own name is never used as a contact name.
- Messages without a valid timestamp or ID are dropped and logged. They used to be stamped with the current time, which put old messages at the top of the list.
- Sent messages get their WhatsApp ID and the `sent` status, and the echo of a sent message updates the local row instead of adding a duplicate. A failed send marks only that message `failed`. It no longer puts the whole account into an error state.
- A failed chat fetch no longer moves that chat's cursor, so messages it missed are fetched on the next catch-up.
- A message that arrives in the same second as the last one seen is recovered by a slower sweep.
- A failed WhatsApp contacts fetch keeps the previous contact names instead of clearing them.
- `scripts/publish-components.sh` had a syntax error (a stray quote in the TDLib check). It is fixed.

### Removed

- The continuous WhatsApp message backfill added in 0.4.0.
- Re-reading history dumps each time a chat is opened.
- A leftover debug logger that wrote to a hard-coded path on conversation and message events.

## [0.4.0] - 2026-09-04

### Added

- WhatsApp status updates appear as a horizontal stories row at the top of the conversation list. Tapping a status opens a full-screen viewer (progress bars, hold to pause, tap sides to seek).
- Paginated conversation list: the UI loads ~30 chats at a time with scroll-to-load-more; search still queries the full SQLite inbox.

### Changed

- WhatsApp connector continuously backfills message text for every chat (up to 500 messages each, throttled) so search and opening older threads hit local data sooner.
- Account hibernation is **off by default**; enable it globally under Notifications or per account in Accounts.
- Conversation list order follows WhatsApp chat list rank when synced, then recency.
- Status row avatars use neutral rings (not accent) so business profiles are not mistaken for unread; status count badge shows number of posts.

### Fixed

- Incoming WhatsApp messages show up live: the connector registers a local GOWA webhook (websocket often never carries chat events), normalizes chat JIDs, and catch-up polling is tighter.
- Opening an unread chat stays pinned to the latest messages instead of landing further up after an empty-then-load layout race.
- Conversation recency follows WhatsApp’s chat list even when GOWA has not stored the newest message body yet (history preview refresh no longer rewinds timestamps).
- WhatsApp history JSON ingest reads every `history-*.json` dump (not only the newest eight), merges chats by phone JID, and re-applies that dump when you open a chat so existing backup bodies show without waiting for a new inbound message. Threads whose WhatsApp list time is ahead of stored bodies say so instead of implying a live message is required.
- Active conversations sort by message recency instead of stale sync-time list ranks.
- WhatsApp business/verified names take precedence over incorrect saved/self-account labels, and outgoing websocket events resolve the recipient JID.
- WhatsApp Business chats no longer show your own business brand name; titles prefer the address book and reject GOWA’s leaked self display name.
- Conversation list pins sync from WhatsApp (whatsmeow chat settings) so pinned chats like Message yourself stay at the top like WhatsApp Web.
- Opening a chat (and sending a message) snaps the thread to the latest message.
- Status/stories row sits above the scrollable conversation list so it is not covered while scrolling.
- Unread chats use a light row tint only (no left accent bar), so business chats are not mistaken for a special border.
- Conversation list unread badges no longer stretch into full-width bars (Settings `.row > *` flex rule was leaking globally).
- Desktop shell uses CSS grid with a fixed viewport height chain so the rail, list, and thread stay side-by-side down to 640px.
- Hibernation and quit no longer panic: connector shutdown uses sync `kill_process()` instead of Tokio `start_kill()` / `kill_on_drop` (which require a runtime on the main thread).
- WhatsApp waits through GOWA `SESSION_SAVED_ERROR` while the saved session reconnects instead of bailing early.
- Contact names are restored from the address book and no longer overwritten by phone-number placeholders from backup sync.
- Unread chats get a tinted row highlight; WhatsApp history/backup `unreadCount` is ignored so stale badges (e.g. business chats already read in WhatsApp) are not reapplied.
- Status row scrolls with mouse wheel and click-drag.
- Desktop three-pane layout fills the window (no stacked half-height panes) for tiled widths down to 640px.
- Conversation list hides `status@broadcast` as a normal chat.
- WhatsApp catch-up that recovers messages missed by the websocket now counts as live (bumps unread + UI), not as history sync.
- Status/stories row stays visible: feed parsing accepts GOWA `messages` payloads, catch-up refreshes the feed, empty snapshots no longer wipe it, and the row stays above the scrollable list.
- Sidebar account buttons show the account profile photo (circular) with the network icon inset at the bottom-left.
- Empty WhatsApp protocol stubs are no longer stored as blank messages; threads with no synced content show an explanation instead of an empty pane.

## [0.3.2] - 2026-08-22

### Changed

- Connector/GOWA process lifecycle is cross-platform: Linux `PR_SET_PDEATHSIG`, Windows job objects (`KILL_ON_JOB_CLOSE`), macOS/Windows parent watchdog threads, and platform-specific orphan/cmdline checks (Linux `/proc`, macOS `ps`, Windows PowerShell CIM).

## [0.3.1] - 2026-08-21

### Added

- Single-instance lock: launching Shuttle again focuses the existing window instead of opening a second copy.
- Window title shows app version and a live locale date/time (`Shuttle 0.3.1 — …`).
- Hard cap of **one OS process per connector** (pidfile reclaim of orphans) and **one GOWA** instance (lock + stray kill before start).
- Connector and GOWA children are parented to Shuttle (Linux `PR_SET_PDEATHSIG`, `kill_on_drop`, no detached GOWA session) so killing/quitting Shuttle tears them down too.

### Changed

- Outbound chat bubbles, send button, search highlight, and related accents follow the active tweakcn theme (`--bg-bubble-out` / `--text-on-accent`) instead of a hardcoded blue gradient.
- Regenerated desktop/mobile app icons from the in-love emoji logo (including Android launcher assets and a real `.icns`).

### Fixed

- Sending no longer silently succeeds when the connector isn’t running: start/send errors are surfaced, failed messages are marked `failed`, and the composer draft is restored when nothing was queued.

## [0.3.0] - 2026-08-21

### Added

- Account **hibernation**: idle sidecars stop after a configurable timeout (default 5 minutes), with optional periodic wake to sync (default 15 minutes). Opening a chat or sending wakes that account. Per-account overrides in Settings (use default, always on, or custom times). Sleeping is distinct from permanent disable.
- One sidecar **process per network**, not per account: multiple WhatsApp / Telegram / … accounts share a Python process. `disconnect` detaches one account; `shutdown` exits when the last account leaves.
- System tray with close-to-tray, unread tooltip, Ctrl/Cmd+K quick switch, and documented keyboard shortcuts (Settings → Keyboard).
- In-thread media lightbox (images, video, PDF attempt), canvas image editor on photo attach (crop, rotate, pen, arrow, pixelate), and Save media from the message menu.
- Backup file pickers, optional downloaded media in the export bundle, and a restart prompt after restore.
- Account workspace assignment in Settings and the account context menu; conversation Notes and Remind menu items.
- `scripts/rss-sample.sh` to sample Shuttle + child RSS the same way as the Ferdium reference run.

### Changed

- Product north star documented across README, overview, and roadmap: lightweight + responsive as the no-thought answer for resource use, plus cross-platform QoL and contributor-friendly sidecars; Ferdium ~2.7 GiB idle benchmark remains the 50–70% RSS bar to beat (~0.8–1.35 GiB on 4 WhatsApp + 1 Telegram).
- Unread totals skip muted accounts and muted chats.
- Connector capability flags match reality (no fake `calls:audio`; Messenger and Instagram no media cap). Call UI is an honest notice rather than a fake in-call mixer.
- Live notifications pause while an account is asleep.

### Fixed

- Workspace fallback uses seeded `default` instead of stale `others`.
- Reminder `datetime-local` shows local time.
- Backup export/restore surfaces errors in Settings.
- Scheduled sends stop retrying after repeated failures.

## [0.2.1] - 2026-08-19

### Fixed

- Release CI now passes `VITE_GIPHY_API_KEY` into `tauri build` so Giphy works in production installers when set as a GitHub Environment secret.
- Release CI: install real macOS Rust targets (`aarch64` + `x86_64`) instead of invalid `universal-apple-darwin` rustup triple; pass universal target only to Tauri.
- Release CI: install `xdg-utils` on Linux so AppImage bundling succeeds on ARM runners.
- Release CI: install `libfuse2` on Linux for reliable AppImage bundling via linuxdeploy.
- Release CI: pass `--` before Windows bundle flags on the default `tauri build` path so `--bundles nsis` is parsed correctly.

### Changed

- `.env.example` documents that `VITE_GIPHY_API_KEY` can be set locally or via GitHub Environment secrets for release builds.
- Release CI reads `.github/ci-rebuild-platforms.txt` to rebuild only failed platforms instead of the full matrix every push (`all` / platform names / `none` to skip). Tag pushes always build the full matrix and publish GitHub Releases with changelog notes and one primary installer per platform (`.deb`, `.dmg`, `.msi`).

## [0.2.0] - 2026-08-19

### Added

- Rich **Contact details** modal: profile fetch, copy phone, media/docs/links/starred tabs, call buttons, workspace/priority organize row.
- **Star and pin** messages: DB migration, context menu actions, badges in thread.
- **Search scopes**: global, per-network, and in-chat message search with jump-to-hit.
- **Call protocol** and **CallPanel** UI; Signal/Telegram audio call paths; WhatsApp calls shown disabled with tooltip.
- **Giphy** GIF and sticker picker (`@giphy/js-fetch-api`) with infinite scroll; Rust `fetch_url_bytes` for reliable media download.
- **Emoji picker** (`emoji-picker-element`) in composer.
- Attachment menu icons; Giphy/emoji/sticker composer buttons.
- **Media retention** settings (per-type expiry days) in Contact details → Account tab; stored in app config.
- New Shuttle **app icon**, favicon, and window/taskbar icon (in-love emoji SVG).
- `VITE_GIPHY_API_KEY` in `.env.example`; Vite `envDir` points to repo root.
- Component publishing workflow and bundled connector runtime scripts.

### Changed

- Conversation list sorting uses GOWA `list_rank` and `force_recency` for WhatsApp parity.
- Message bubble spacing: no inner margin between content and time; 2px gap after bubble.
- Sent images/GIFs embed as data URLs locally; GIF sends use `compress: false` for WhatsApp.
- Contact details media/docs tabs use 6-column scrollable grids; thumbnails load from disk cache.
- App shell layout hardened: fixed sidebar rail, list pane, and thread pane with overflow clipping.
- Auto-scroll to bottom on conversation open and when new messages arrive (if near bottom).
- Default organization: single **Default** workspace; workspace/priority filters appear only after custom entries.
- **Settings → Workspaces & priorities**: add and remove priority labels.

### Fixed

- Empty message bubbles and garbage bodies (hex keys, JIDs) hidden in thread.
- `[Image]`/`[Sticker]` placeholder text no longer shown below media bubbles.
- WhatsApp: saved contact names via GOWA contacts and `sender_display_name`.
- WhatsApp: GOWA `POST /devices` “already exists” treated as success on resume.
- CSP allows `https:` images for Giphy previews; Giphy env loaded from repo-root `.env`.
- `tauri dev` compile warnings and unused CSS selectors cleaned up.

## [0.1.3] - 2026-08-18

### Added

- Full [tweakcn](https://github.com/jnsahaj/tweakcn) theme integration: fetch by URL/id, apply colours and Google Fonts, reload app.
- Default theme: tweakcn **Light Green** (`cmlhfpjhw000004l4f4ax3m7z`) with light colour scheme.

### Fixed

- Light mode form controls (dropdowns, inputs) now use theme `--input` / `--text` instead of staying dark grey.

## [0.1.2] - 2026-08-18

### Added

- Startup splash screen with Shuttle logo while the app loads.
- Tabbed Settings layout (Appearance, Notifications, Privacy, and more) instead of one long page.
- Color scheme picker buttons in Settings → Appearance.

### Changed

- App icons and favicon regenerated from the Shuttle logo.
- Settings cog moved to the bottom-left rail; highlights when Settings is open.
- Installer metadata: author **Sheese Sheikh** ([shee.se](https://shee.se), [github.com/smsheese](https://github.com/smsheese)), Shuttle description, and Cursor Agent credit.

### Fixed

- Settings toggles and checkboxes no longer broken by global `appearance: none` on inputs.

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
