# Privacy policy

Last updated: 2026-11-20. Applies to Shuttle 0.6.0 and later.

Shuttle is a local-first desktop app. Your messages, contacts and sessions stay on your computer. The project has no server that receives your chats and no account system. This page lists every case where data leaves your machine, who receives it and how to turn it off.

## What stays on your computer

- Messages, media, contacts, notes, settings and the sessions of every linked account, under the OS application data directory (on Linux `~/.local/share/shuttle`). See [docs/storage.md](docs/storage.md).
- Your API keys (OpenRouter, Giphy), in the local settings file.
- The message database is not encrypted at rest. Use full-disk encryption.

We never receive or read any of this.

## Messaging networks

Shuttle connects to WhatsApp, Telegram, Signal, Messenger, Instagram, Matrix and email directly from your computer, through local helper programs. Those networks receive and process your messages under their own terms and privacy policies. Shuttle is not affiliated with them. WhatsApp access uses an unofficial client, and WhatsApp may restrict numbers that use one.

## Anonymous diagnostics (off by default)

Settings, Privacy has two switches. Both are off until you turn them on, and you can turn them off at any time. Turning one off stops sending and clears anything queued.

- **Crash reports.** When Shuttle crashes or hits an unhandled error, it sends a fixed error category and, for a Rust crash, the source file name and line number inside Shuttle's own code. Crash messages, stack traces and any text from your chats are discarded before sending.
- **Usage and performance diagnostics.** Sent in batches every 30 seconds or so. Contents:
  - App version, build channel (testing or production), OS name, CPU architecture, CPU core count and a RAM size range.
  - Start-up time, and whether an account was added or removed, with the network type (for example "telegram").
  - CPU and memory use of Shuttle and its helper programs, averaged over several minutes.
  - How long the chat list, opening a chat and search take (median, 95th percentile and slowest, per batch).
  - Size ranges, never exact numbers: number of linked accounts (1, 2-3, 4-5, 6-10, 11 or more), number of chats and messages (for example "1k-10k"), and database size (for example "10-100 MB").
  - Whether a sync worked, how long it took and how many items it handled, plus a fixed error category when it failed.

The full list of events and fields is in [docs/telemetry-events.md](docs/telemetry-events.md). The code allows only the fields in that list, so a field that is not listed cannot be sent.

**Never collected:** message text or previews, names of people, chats or groups, phone numbers, email addresses, usernames, who sent or received anything, account, chat or message identifiers, contact lists, media, credentials, tokens, cookies, QR codes, your computer's name, your OS user name or file paths. Session replay and automatic click tracking are not used.

**Who receives it.** PostHog, in its EU Cloud region (`eu.i.posthog.com`), in a project owned by Shuttle. Events carry a random installation ID (a UUID generated on first run) so we can see how one installation behaves over time. It is not linked to your name, email or accounts. Crash reports carry no installation ID. Like any network request, the request reveals your IP address to the receiving server. The Shuttle PostHog project is set to discard client IP addresses and to keep data for a limited period. PostHog's own policy: <https://posthog.com/privacy>.

**What we use it for.** Finding crashes and making Shuttle faster and lighter, especially on low-end computers and for people who link many accounts.

**Deleting your data.** Turn the switches off. To remove data already sent, open an issue or contact us with your installation ID (Settings, About) and we will delete it.

## Other services Shuttle talks to

Only when you use the related feature.

| Service | When | What is sent |
| --- | --- | --- |
| OpenRouter (<https://openrouter.ai>) | You press Translate | The text you chose to translate, your OpenRouter key and the model id you set. OpenRouter and the model provider handle it under their policies. |
| Giphy (<https://giphy.com>) | You open the GIF or sticker picker | Your search words and your Giphy key. GIFs and stickers load from Giphy's servers. |
| Feedback form (`forms.tfs.qa`) | You press "Open the feedback form" in Settings, Feedback | Shuttle opens the form in your system browser. The form page receives what you type into it. Shuttle does not add any chat data or identifiers. |
| GitHub API (`api.github.com`) | You open Settings, About, or press "Check for updates" | A request for the latest release of Shuttle: the app version only. No account, installation ID, chat data or other identifiers. GitHub sees your IP address. Shuttle checks at most once an hour. |
| GitHub (<https://github.com>) | You install a network component, such as WhatsApp or Telegram support | A download request for that component. GitHub sees your IP address. |
| Open-Meteo (<https://open-meteo.com>) | You search for a location for automatic dark mode | The place name you type. |
| tweakcn (<https://tweakcn.com>) and Google Fonts | You apply a tweakcn theme | A request for the theme and its fonts. |
| WhatsApp (`wa.me`, web.whatsapp.com) | You choose "Open in WhatsApp" | Your system browser or WhatsApp opens the link. |

Shuttle contains no advertising and no other analytics, and does not sell or share data.

## Children

Shuttle is not directed at children under 13.

## Changes

Material changes are listed in [CHANGELOG.md](CHANGELOG.md) and this page's date is updated.

## Contact

Questions or deletion requests: open an issue at <https://github.com/smsheese/shuttle/issues> and leave out anything private, or use the feedback form in Settings. Security problems: [SECURITY.md](SECURITY.md).
