# Shuttle — Agent notes

## Product north star

Prefer designs that keep Shuttle the **obvious low-resource** messaging hub: lightweight and responsive, cross-platform, strong QoL — without making connectors harder to write. Do not trade idle RSS or wake latency for Electron-style webview parity. Details and the Ferdium resource bar: [`docs/roadmap.md`](docs/roadmap.md), principles in [`docs/overview.md`](docs/overview.md).

## Cross-platform targets

Shuttle ships for **Linux, Windows, and macOS**. Any change that touches process lifecycle, filesystem paths, IPC, notifications, single-instance behavior, connectors, or OS integration must be implemented (or explicitly guarded) for **all three** — do not land Linux-only shortcuts without matching behavior on Windows and macOS.

When adding platform-specific code:

- Prefer shared abstractions in Rust (`connectors/process_tree.rs`, `connectors/process_lock.rs`) or Python (`connectors/shuttle_ipc.py`) over scattered `if linux` branches in feature code.
- Test the logic path mentally (or manually) on each OS: clean quit, force-kill, orphan reclaim, and second-instance launch.
- Document OS-specific limitations in code comments only when a true platform gap remains.

## Modals and popovers

A click that opens a modal, popover, menu or chooser must open it **next to the element that launched it** (anchored to the button, input or message), never centred on the window or in a far corner, so the person can answer without moving the pointer. Examples: the call buttons' panel opens under the header buttons, and the paste-as-media-or-file chooser opens above the composer. Use a full-window modal only when the task is genuinely window-wide (settings, media lightbox).

## WhatsApp sync rules

- Connectors report facts. `shuttle-app/src-tauri/src/db/chat.rs` is the only place that decides titles, chat time, previews, unread and chat merges. Do not add per-path flags that change stored state (the old `force_recency`, `replace_title` and `list_rank` are gone; `history` only gates notifications and UI events) or a second writer for chat rows.
- One connector process and one GOWA serve all WhatsApp accounts. Every reply and event must carry `account_id`. Never read shared GOWA storage (`gowa/storages`, the WhatsApp chat-settings table, history dumps) without filtering by the account's own number.
- After changing `connectors/whatsapp-connector.py` or the chat rules, run `python3 -m unittest discover -s connectors/tests` and `cargo test` (in `shuttle-app/src-tauri`).
- The GOWA version is pinned in `connectors/gowa/fetch.sh`. Move it deliberately, together with `connectors/gowa/checksums.txt` (see [`connectors/gowa/README.md`](connectors/gowa/README.md)).

## Documentation

Keep docs aligned with the code in the **same change set** when behavior, setup, packaging, or agent conventions change:

- [`CHANGELOG.md`](CHANGELOG.md) — always for features and bug fixes (see below).
- [`README.md`](README.md) — install steps, supported platforms, headline features, or quick-start changes.
- [`docs/`](docs/) — deeper behavior when APIs, paths, env vars, or workflows change (e.g. `platforms.md`, `core.md`, `storage.md`).
- [`AGENTS.md`](AGENTS.md) — repo rules and agent workflow when process changes.

Skip doc updates only when the change is purely internal with no user-visible or operator-visible effect.

## Changelog

Update [`CHANGELOG.md`](CHANGELOG.md) in the same change set whenever you:

- implement a feature, or
- resolve an issue or bug from a previous commit.

Do not leave release notes for a follow-up. Use Keep a Changelog sections (`Added`, `Changed`, `Fixed`, `Removed`) under `[Unreleased]` while work is in progress.

## Commit workflow

When the user asks to **commit** or to produce a **release build**, finish documentation and versioning **before** `git commit` (do not commit first and version later):

1. **Update docs** — changelog (required), plus README / `docs/` / `AGENTS.md` when the change warrants it (see **Documentation** above).
2. **Bump the version** from the current number, based on how many changes there are and how complex they are:
   - **Patch** (`0.1.x`): few, small, or isolated fixes/tweaks.
   - **Minor** (`0.x.0`): several changes, a new feature, or anything more involved.
   - **Major** (`x.0.0`): breaking public/packaging changes, or the first stable `1.0.0`.
3. **Write that version into all version files** so they stay in sync:
   - `shuttle-app/package.json`
   - `shuttle-app/src-tauri/Cargo.toml`
   - `shuttle-app/src-tauri/tauri.conf.json`
   - `shuttle-app/package-lock.json` (the top-level `version` / package entry)
4. **Promote `[Unreleased]` in `CHANGELOG.md`** to `## [X.Y.Z] - YYYY-MM-DD` (today’s date), with concise bullets that match the actual diff. Leave an empty `[Unreleased]` heading at the top.
5. **Commit all relevant files** together: code, version files, lockfiles if they changed, docs, and `CHANGELOG.md`. Follow the user’s git commit rules (no commit unless they asked; no push unless they asked).
6. **Release build** (when requested) — e.g. `./scripts/build-release.sh -b deb` on Linux; artifacts under `shuttle-app/src-tauri/target/release/bundle/`.

## Licensing

Shuttle is AGPL-3.0-or-later plus a paid commercial license ([`docs/licensing.md`](docs/licensing.md)). New linked dependencies (Rust crates, npm runtime packages) must be AGPL-compatible **and** relicensable under the commercial license: no GPL-only, LGPL-only, AGPL, SSPL, BUSL or proprietary code in the binary. GPL helpers stay separate downloaded processes, like signal-cli. Outside contributions need the signed [CLA](CLA.md). Do not copy code or assets without recording their source and license.

## Environment variables

Whenever you add or use a variable that belongs in `.env` (runtime config, API keys, feature flags, URLs, etc.):

- Add it to `.env.example` in the relevant section with a comment explaining what it is.
- Use an empty value (`KEY=`) for secrets and a sensible default for non-secret settings.
- Never commit `.env` itself.

## PostHog project

Shuttle uses its own PostHog project, not the shared TFS PostHog instance. Use Shuttle’s configured project ingestion token and EU Cloud ingest host (`https://eu.i.posthog.com`). Keep the token out of tracked files; see [`docs/telemetry-events.md`](docs/telemetry-events.md).
