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

Publish component archives with `./scripts/publish-components.sh` (CI: `.github/workflows/publish-components.yml` on `v*` tags).

Local development still uses repo `connectors/` when running `npm run tauri dev` (debug fallback).

| Component | Build / fetch helper |
| --- | --- |
| GOWA (WhatsApp) | `./connectors/gowa/fetch.sh` |
| TDLib (Telegram) | `./connectors/tdlib/fetch.sh` |
| signal-cli (Signal) | `./connectors/signal/fetch.sh` |
| Slim Python runtime | `SHUTTLE_PYTHON_SKIP_DEPS=1 ./scripts/fetch-python-runtime.sh` + `./scripts/slim-python-runtime.sh` |
| Messenger / Instagram deps | Built into separate tarballs during `./scripts/publish-components.sh` |

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

## Known gaps

- **32-bit** (i686, armv7) is still out of scope. Tauri/WebKit packaging plus native helper coverage makes it non-trivial rather than a quick win.
- **Linux i386 / musl-only** distros are untested; glibc-based builds are the default.
- **GPL signal-cli** is downloaded on demand when you add a Signal account (license text ships with the app; see [licensing.md](licensing.md)).
