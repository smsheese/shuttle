# Platform and architecture support

Shuttle targets **64-bit desktop** on **amd64 (x86_64)** and **arm64 (aarch64)** across Windows, Linux, and macOS. Android is a possible future target but is **not** part of the current desktop build.

## Support matrix

| OS | amd64 / x86_64 | arm64 / aarch64 | Install formats |
| --- | --- | --- | --- |
| **Linux** | supported | supported | `.deb`, AppImage |
| **macOS** | supported (universal or Intel build) | supported (Apple Silicon) | `.dmg`, `.app` |
| **Windows** | supported | supported | `.msi`, `.exe` (NSIS) |
| **Android** | n/a | planned / future | not in scope yet |

Build each OS on a machine (or CI runner) with the matching CPU architecture. macOS can ship a **universal** `.app` that contains both Intel and Apple Silicon binaries in one bundle.

## What is portable vs native

| Layer | amd64 + arm64 | Notes |
| --- | --- | --- |
| Tauri shell + Rust core | yes | Built per target triple or macOS universal (~30 MB installer) |
| Svelte UI | yes | Same web assets everywhere |
| Connector sidecars + native helpers | per OS + arch | **Downloaded on demand** when you add an account (hosted on S3; see below) |
| Python runtime | per OS + arch | Downloaded when needed; system Python 3.12+ is used when compatible |

Release installers ship **core only**. Connector scripts, native binaries (GOWA, TDLib, signal-cli), slim CPython, and per-network Python deps are fetched lazily from a versioned manifest:

```
{SHUTTLE_COMPONENTS_BASE_URL}/v{app_version}/manifest.json
```

Installed under `~/.local/share/shuttle/components/` (or `$SHUTTLE_DATA_DIR/components/`).

Each install records its manifest version in `components/installed.json`, and Settings, About reads it back into a per-network list: installed or not, version, and the download size from the manifest. Nothing installs from that page; components still arrive only when you add or use an account on the network. When the manifest cannot be fetched the page shows sizes as unknown instead of failing.

Publish component archives with `./scripts/publish-components.sh` (CI: `.github/workflows/publish-components.yml` on `v*` tags).

Local development still uses repo `connectors/` when running `npm run tauri dev` (debug fallback).

| Component | Build / fetch helper |
| --- | --- |
| GOWA (WhatsApp) | `./connectors/gowa/fetch.sh` |
| TDLib (Telegram) | `./connectors/tdlib/fetch.sh` |
| signal-cli (Signal) | `./connectors/signal/fetch.sh` |
| Slim Python runtime | `SHUTTLE_PYTHON_SKIP_DEPS=1 ./scripts/fetch-python-runtime.sh` + `./scripts/slim-python-runtime.sh` |
| Messenger / Instagram deps | Built into separate tarballs during `./scripts/publish-components.sh` |

GOWA is pinned to one release (`GOWA_PINNED_VERSION` in `connectors/gowa/fetch.sh`), and each archive is checked against `connectors/gowa/checksums.txt`. `SHUTTLE_GOWA_VERSION` fetches another tag, which is installed without a checksum check. The pinned release has no Windows arm64 build, so `fetch.sh` cannot get GOWA on Windows arm64 and `publish-components.sh` skips it there. WhatsApp cannot run on Windows arm64 until the pin moves to a release that has that build.

## Media, saving and file-manager integration

| Concern | Linux | Windows | macOS |
| --- | --- | --- | --- |
| Media cache (`<data dir>/media_cache`) | `~/.local/share/shuttle` | `%APPDATA%\shuttle` (Roaming) | `~/Library/Application Support/shuttle` |
| Default save folder | `~/Documents/shuttle` (`XDG_DOCUMENTS_DIR`) | `Documents\shuttle` | `~/Documents/shuttle` |
| Reveal a saved file | `org.freedesktop.FileManager1.ShowItems` (D-Bus), fallback `xdg-open` on the folder | Explorer with the file selected (`explorer /select,`) | Finder reveal (`open -R`) |
| Save dialog (*Ask each time*) | native GTK/portal dialog | native Win32 dialog | native NSSavePanel |
| Retention timestamp | file mtime, touched on open | same (`SetFileTime` through `File::set_modified`) | same |

All three use the same Rust code (`media_store.rs`); only the reveal fallback differs by OS. Known gaps: on Windows the cache sits in the Roaming profile because that is where the app's data directory already lives, so a roaming-profile setup would sync it; on a Linux desktop with neither a FileManager1 service nor `xdg-open`, the reveal click shows an error toast instead. WhatsApp message previews exist only for messages that came in a history-sync dump (GOWA does not forward them live), see [storage.md](storage.md#media-cache-and-saved-media).

### Copying media to the clipboard

The media viewer's **Copy** button (also Ctrl/Cmd+C there), the message menu's **Copy** on a media message and its **Copy media** call the `copy_media_to_clipboard` command (`src-tauri/src/clipboard.rs`). It validates the path with `media_store::validated_media_path` (cache or legacy Documents root only), then writes through one long-lived `clipboard-rs` context. The webview clipboard is not used for files. A media message's body is a placeholder such as `[image]` when it has no caption, so **Copy** on it copies the file rather than that text; a caption is never added to the clipboard because a text target would make text fields paste the caption instead of the image.

**File names.** Cached files are named after the message id (older builds gave them no extension), so the clipboard never points at them. The copy stages a hard link (a copy where the filesystem cannot link, e.g. the legacy Documents folder) at `<data dir>/media_cache/clipboard/<n>/<name>` and references that. `<name>` is the document's own file name, or `<account>-<chat>-<yymmdd>-<hhmm>.<ext>` (e.g. `Work-Mum-261112-1325.jpg`) for photos, videos, stickers and audio. The account and chat halves are sanitised for every OS (no path separators, Windows-forbidden characters, reserved names, control characters, trailing dots or spaces; whitespace collapsed; capped at 40 characters each) and the date and time come from the message, in local time. The extension comes from the cached file's name, else its magic bytes (`media_store::effective_extension`), so extensionless files still paste as `.jpg` and are treated as images. Each copy prunes older staged folders (the newest 5 are kept) and startup keeps only the newest one; nothing else in that folder is touched. *Save media* uses the same naming.

### Dragging files onto a chat

The main window sets `dragDropEnabled: false` in `tauri.conf.json`, so the webview receives the OS drag events instead of Tauri's own drop handler. That is what lets one HTML5 `dragover`/`drop` path work on all three OSes and in `vite dev` with the mock data: Windows blocks HTML5 drag events while Tauri's handler is on, and `getCurrentWebview().onDragDropEvent` reports absolute paths, which the send path cannot take (it carries bytes, not a path). A dropped file is read in the webview and goes through the same `send_attachment` call as the file picker. Nothing in the chain limits the file size.

### Window and app icons

`bundle.icon` in `tauri.conf.json` lists `icons/icon.png` (512 px) first because Tauri's `default_window_icon()` is the *first* `.png` (Linux) or the first entry of the first `.ico` (Windows; `icon.ico` has its 256 px image first). On Linux the main window icon is additionally set from the embedded 512 px PNG, which is what X11 alt-tab switchers read (`_NET_WM_ICON`); macOS uses `icon.icns` (up to 1024 px) from the bundle. The `.deb` installs every listed PNG into the matching `hicolor/<size>/apps/` folder.

| | Linux (X11 and Wayland) | Windows | macOS |
|---|---|---|---|
| Image / sticker | `image/png` + `text/uri-list` + `x-special/gnome-copied-files` + KDE "copy, not cut" hint | DIB image + registered `PNG` format (keeps transparency) + `CF_HDROP` | PNG on the pasteboard + file URL |
| Video, audio, document | `text/uri-list` + `x-special/gnome-copied-files` + KDE hint + `text/plain` path | `CF_HDROP` + Unicode text path | file URL |
| Backend | X11: `x11rb` selection owner thread; Wayland: `wl-clipboard-rs` (wlr/ext data-control) | `clipboard-win` | `NSPasteboard` via `objc2` |

Behaviour notes:

- **Why `clipboard-rs`:** it is the small crate that can set image data, a file list and extra formats in one clipboard write on all three OSes (arboard can only set one kind at a time, and on Linux offers only `text/uri-list`, which Nautilus-style managers ignore). It is pure Rust on Linux (no GTK/xcb C libraries).
- **Memory:** nothing decoded is kept. PNG files are passed through as raw bytes; JPEG/WebP/GIF/BMP are decoded once and re-encoded to PNG inside the copy call; images over 64 megapixels are copied as a file only. On Linux the clipboard owner must keep the (compressed) PNG until another app replaces the clipboard, and the context is one idle thread created on first use.
- **Wayland:** the backend is chosen at runtime. Compositors without a data-control protocol (GNOME/Mutter) make `clipboard-rs` fall back to XWayland, which works for pasting into other apps while Shuttle runs.
- **Fallback:** if the native copy fails, only the file path is copied as text and the toast says so. The button never does nothing.
- **Windows paths:** `canonicalize` yields `\\?\C:\...`; the prefix is stripped before `CF_HDROP` because Explorer rejects it.
- **Unverified here:** Windows, macOS and native Wayland were written from the library sources and not run; the X11 path was checked with `xclip` (`cargo test live_ -- --ignored --nocapture` lists the targets and reads them back; on Wayland it uses `wl-paste`).

### Startup and idle cost

Shuttle opens **one** window and keeps one WebView process. The boot screen is inline HTML and CSS in `src/app.html` (`#boot-splash`), painted before any app CSS or JS runs and removed by `+layout.svelte` once the app config is applied (with a 15 s fallback if that round-trip hangs). There is no second transparent always-on-top window, which used to leave a second WebKitWebProcess running for the life of the app after the splash closed. The behaviour is the same on Linux, Windows and macOS.

Nothing animates while the app is idle: the sidebar's active-account ring is a static gradient, unread badges do not pulse, a chat preview scrolls its text only while its row is hovered or focused, and loading spinners run only while a real load is pending. `prefers-reduced-motion` stops the remaining transitions. The window title is set once at startup and again only when the unread count changes; there is no per-second redraw.

On Linux the WebView gets WebKit's smallest cache model (`DocumentViewer`), no page cache and no WebGL, since the UI draws 2D canvas only. `performance.low_memory_webview` in `config.json` (default `true`) turns that off when something needs WebGL (see [storage.md](storage.md#configjson)). Windows and macOS keep their engines' defaults, and the hardware-acceleration policy is unchanged on all three. Conversation rows carry `content-visibility: auto`, so rows below the fold skip layout and paint until they scroll into view.

Closing the window hides it to the tray, so for most of the day Shuttle's WebView is alive for a UI nobody is looking at. After `performance.unload_hidden_after_secs` hidden (default 120), `src-tauri/src/main_window.rs` destroys the window and its renderer process exits. The tray icon, a second launch and the macOS Dock icon all go through `main_window::show`, which rebuilds the window from the `main` entry in `tauri.conf.json` and re-runs the per-window setup (`prepare_main_window`). The frontend restores the open chat, its filters and each chat's unsent draft from one small `localStorage` key, and `current_call` puts a call panel back.

On Linux, destroying the window is not enough on its own: WebKitGTK keeps its `WebProcess` (the renderer) running, so the unload also calls `webkit_web_view_terminate_web_process`. Measured on this repo's dev build, a loaded window held about 137 MB of app PSS plus 191 MB of renderer and 40 MB of network process; with the window unloaded it was 165 MB of app PSS and 45 MB of network process, with no renderer process at all. A rebuild takes 0.1-0.2 s to map the window and about a second to paint the same chat. Windows (WebView2) and macOS (WKWebView) compile and follow the same lifecycle, but whether their renderer exits on unload was not measured here.

The unload is held off while a call is ringing or connected (`call.*` events tracked in Rust), and `RunEvent::ExitRequested` with no exit code is prevented, because losing the last window is not a quit: tray Quit and restart pass a code and still exit.

### Microphone access (voice notes)

The recorder calls `navigator.mediaDevices.getUserMedia`, which every webview engine treats as a permission request. Shuttle answers it for its own UI in `src-tauri/src/permissions.rs` (`allow_microphone`, called from `setup` once the main window exists) and leaves every other request on the engine's default answer.

| | Linux (WebKitGTK) | Windows (WebView2) | macOS (WKWebView) |
| --- | --- | --- | --- |
| Why it was denied | nothing handled the webview's `permission-request` signal, and WebKit denies an unhandled one | wry only answers clipboard requests, so WebView2 fell back to its own browser prompt | not denied: wry's UI delegate already grants media capture |
| Caveat | none | none | wry's delegate grants the camera as well, and overriding it would drop its file picker and window handling; Shuttle's UI never asks for a camera |
| What Shuttle does | connects the signal and allows audio capture from the app origin only, denying camera and everything else | adds a `PermissionRequested` handler that allows the microphone and denies the camera | nothing in code; the bundle needs a usage description |
| Packaging requirement | none; capture needs PipeWire or PulseAudio at runtime | none | `Info.plist` (`NSMicrophoneUsageDescription`) plus `entitlements.plist` (`com.apple.security.device.audio-input`, applied when the bundle is signed and hardened) |
| Verified | on Linux, by a real WebKitGTK webview: `cargo test --lib live_ -- --ignored --nocapture` | code review only | code review only |

The microphone is granted to `tauri://localhost`, `http://tauri.localhost`, `https://tauri.localhost` and the Vite dev server on `localhost:1420`. A page served from anywhere else is refused, so a link opened inside the app cannot record. On macOS the first recording shows the macOS prompt once and never again. The dialog text names the macOS or Windows switch on a real OS denial (`src/lib/micErrors.ts`), and says nothing about settings on Linux, which has no such switch.

## Rust target triples

| Platform | Typical triple | Bundle command |
| --- | --- | --- |
| Linux amd64 | `x86_64-unknown-linux-gnu` | `npm run tauri build` |
| Linux arm64 | `aarch64-unknown-linux-gnu` | `npm run tauri build` (on arm64 runner) |
| macOS universal | `universal-apple-darwin` | `npm run tauri build -- --target universal-apple-darwin` |
| macOS Intel only | `x86_64-apple-darwin` | `npm run tauri build -- --target x86_64-apple-darwin` |
| macOS Apple Silicon | `aarch64-apple-darwin` | `npm run tauri build -- --target aarch64-apple-darwin` |
| Windows amd64 | `x86_64-pc-windows-msvc` | `npm run tauri build` |
| Windows arm64 | `aarch64-pc-windows-msvc` | `npm run tauri build` (on arm64 runner) |

On Windows, prefer the **MSVC** toolchain (Visual Studio Build Tools). Cross-compiling Windows arm64 from amd64 is possible but not the default CI path.

## Local release build

```bash
# From repo root — builds for the host OS/arch
./scripts/build-release.sh

# macOS fat binary (Intel + Apple Silicon)
./scripts/build-release.sh -- --target universal-apple-darwin
```

Artifacts land under `shuttle-app/src-tauri/target/release/bundle/`.

## CI

`.github/workflows/release.yml` builds on:

- `ubuntu-latest` (Linux amd64)
- `ubuntu-24.04-arm` (Linux arm64)
- `macos-latest` (macOS universal)
- `windows-latest` (Windows amd64)
- `windows-11-arm` (Windows arm64)

Tag a release as `v*` to upload bundles as GitHub release assets.

Pushes to **`main`** also run this workflow using the GitHub **`production`** environment (telemetry secrets baked into the binary). Manual runs default to **`testing`**. See [telemetry-events.md](telemetry-events.md).

## Prerequisites by OS

### Linux (Debian / Mint / Ubuntu)

```bash
sudo apt install \
  build-essential pkg-config python3 \
  libglib2.0-dev libgtk-3-dev libwebkit2gtk-4.1-dev \
  libjavascriptcoregtk-4.1-dev libsoup-3.0-dev librsvg2-dev \
  libdbus-1-dev libssl-dev libayatana-appindicator3-dev libxdo-dev
```

### macOS

- Xcode Command Line Tools
- Rust via [rustup](https://rustup.rs)

### Windows

- [Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/) (MSVC, Windows SDK)
- [WebView2](https://developer.microsoft.com/microsoft-edge/webview2/) (usually preinstalled on Windows 11)
- Rust via rustup with `x86_64-pc-windows-msvc` or `aarch64-pc-windows-msvc`

## Android (future)

Shuttle is a **Tauri 2 desktop** app today. A future Android build would likely use [Tauri mobile](https://v2.tauri.app/start/) (or a companion app) and would require:

- Replacing or rethinking **sidecar connectors** (no arbitrary child processes on mobile)
- Network-specific **SDK integrations** instead of Python subprocesses
- Separate signing, Play Store policy, and background-sync constraints

Until that work lands, treat Android as **roadmap only**. The `cdylib` / `staticlib` crate types in `Cargo.toml` are kept compatible with a future mobile shell but nothing is wired up yet.

## Message actions

Reacting, editing, deleting and translating are the same on Linux, Windows and macOS: the rules live in Rust (`connectors/message_ops.rs`, `db/chat.rs`), the WhatsApp calls go through the shared GOWA HTTP API, and translation is a plain HTTPS request from the core to the server set in Settings (it honours the `HTTPS_PROXY` / `HTTP_PROXY` environment variables). Nothing here touches OS APIs or file paths. Which actions a network offers comes from its `actions` capabilities, so the menu is the same on every OS; today only WhatsApp has any.

## Known gaps

- **macOS 11 and earlier** cannot capture audio in WKWebView; WebKit only added `getUserMedia` on macOS 12. Everything newer works.
- **32-bit** (i686, armv7) is still out of scope. Tauri/WebKit packaging plus native helper coverage makes it non-trivial rather than a quick win.
- **Linux i386 / musl-only** distros are untested; glibc-based builds are the default.
- **GPL signal-cli** is downloaded on demand when you add a Signal account (license text ships with the app; see [licensing.md](licensing.md)).

Desktop unread badges use the operating system’s taskbar or Dock support. On Linux, badge visibility depends on the desktop shell and its launcher integration. The window title also shows the unread total; muted accounts and muted or archived chats are excluded, matching the sidebar count.
