# Release checklist (first public release)

What must be true before Shuttle is public. Tick items as they land. Shuttle is open source (AGPL-3.0-or-later, with a commercial license); users bring their own keys, so the build embeds none except PostHog.

## Repository secrets for `release.yml`

Signing runs only when its secrets exist; without them builds are unsigned and the workflow still passes.

- [ ] macOS: `APPLE_CERTIFICATE` (base64 of the Developer ID Application `.p12`), `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`, `APPLE_ID`, `APPLE_PASSWORD` (app-specific password), `APPLE_TEAM_ID`. Needs a paid Apple Developer account.
- [ ] Windows: `WINDOWS_CERTIFICATE` (base64 of a `.pfx`), `WINDOWS_CERTIFICATE_PASSWORD`. A normal certificate still shows SmartScreen warnings until it builds reputation; an EV certificate, or SignPath Foundation's free signing for open source, avoids that.
- [ ] Linux: `GPG_PRIVATE_KEY` (armored), `GPG_PASSPHRASE`. Publish the public key in the README so people can verify `.asc` files and `SHA256SUMS`.
- [ ] Telemetry: `POSTHOG_API_KEY`, `POSTHOG_HOST` in the `production` environment. Decide whether a public build should ship telemetry on by default, and say so in the README.
- [ ] Optional: `SHUTTLE_COMPONENTS_BASE_URL` only if components are not served from the GitHub release.

## Before tagging

- [x] Version is 0.6.0 in `package.json`, `package-lock.json`, `Cargo.toml`, `Cargo.lock` and `tauri.conf.json`.
- [x] `[Unreleased]` promoted to `## [0.6.0] - 2026-11-19` (sections merged). Tag `v0.6.0`.
- [ ] Both workflows run on the tag: `release.yml` (installers) and `publish-components.yml` (connector archives, attached to the same release). The app compiles in the components URL, so the component assets for the same version must be on the release before anyone adds a WhatsApp account.
- [ ] Clean-machine install on Linux, Windows and macOS (Intel and Apple Silicon): first launch, add a WhatsApp account, sidecar download from the release.
- [ ] Confirm the Intel macOS component set (`macos-x86_64`) was published, since the universal app asks for it on Intel Macs.
- [x] No updater for 0.6.0: users install new builds by hand (README says so). Add the Tauri updater later.

## Open source hygiene

- [ ] `LICENSE` (AGPL-3.0), `COMMERCIAL-LICENSE.md`, `CONTRIBUTING.md` and `CLA.md` at the root; `ATTRIBUTION.md` and third-party notices current. signal-cli stays a separate downloaded GPL-3.0 process.
- [x] `cargo deny check` passes with `deny.toml` (permissive licences only; `r-efi` and `self_cell` are dual-licensed and used under the permissive option). `cargo audit` and `npm audit` are clean except RUSTSEC-2026-0387 (`age` to `crypto-bigint`, timing only, no remote oracle), ignored with a reason in `deny.toml`. `chrono` is flagged unmaintained, not vulnerable.
- [x] Secret scan: regex pass over 287 commits, then `gitleaks` 8.30.1 over 282 commits (174 MB): no leaks. Both also run in CI.
- [x] README: install, build from source, supported networks, the unofficial-WhatsApp risk (GOWA can get numbers restricted; suggest a spare number), privacy and telemetry opt-out.
- [x] `CONTRIBUTING.md`, `SECURITY.md`, issue templates (`.github/ISSUE_TEMPLATE/`) asking for version, OS and steps. Never ask for GOWA, TDLib or signal-cli session files.

## Product limits to state plainly

- [x] WhatsApp calls cannot be placed from Shuttle (GOWA has no call route); the call buttons hand off to WhatsApp.
- [x] GIFs, stickers and translation need the user's own Giphy and OpenRouter keys (Settings). Keys are stored in the local config file, not in the OS keychain.

## Performance and backend (from the earlier plan)

- [ ] Idle RSS, CPU and wake latency with 1, 3 and 5 WhatsApp accounts against the Ferdium bar in `docs/roadmap.md` section 13; record the numbers.
- [ ] Open a 5,000-message chat and a sticker-heavy group: no memory growth, no dropped frames.
- [x] `EXPLAIN QUERY PLAN` on a synthetic 200,000-message chat. Measured: opening a chat is fine (1 ms, uses the timestamp index). Newest-message lookup sorted the whole chat (128 ms); an expression index `idx_messages_conv_epoch` makes it 0.02 ms, and the timestamp maximum and unread count were rewritten to use it with the same results. A test fails if the plan regresses. Still unmeasured: a real multi-account database on a low-end machine, which the new `operation_timing` telemetry will show.
- [ ] One GOWA and one connector for all accounts; orphan reclaim after force-kill on all three OSes.
- [ ] Reconnect after sleep and network loss; backup and restore round trip.
- [x] Tests green locally (132 Python, 139 Rust) and in `.github/workflows/ci.yml` (Linux only; first run on GitHub pending). Windows and macOS test runners are not set up.
- [ ] Pinned GOWA version and `connectors/gowa/checksums.txt` match what the release downloads.
