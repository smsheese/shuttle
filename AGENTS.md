# Shuttle — Agent notes

## Product north star

Prefer designs that keep Shuttle the **obvious low-resource** messaging hub: lightweight and responsive, cross-platform, strong QoL — without making connectors harder to write. Do not trade idle RSS or wake latency for Electron-style webview parity. Details and the Ferdium resource bar: [`docs/roadmap.md`](docs/roadmap.md), principles in [`docs/overview.md`](docs/overview.md).

## Changelog

Update [`CHANGELOG.md`](CHANGELOG.md) in the same change set whenever you:

- implement a feature, or
- resolve an issue or bug from a previous commit.

Do not leave release notes for a follow-up. Use Keep a Changelog sections (`Added`, `Changed`, `Fixed`, `Removed`) under `[Unreleased]` until the user asks to commit.

## Environment variables

Whenever you add or use a variable that belongs in `.env` (runtime config, API keys, feature flags, URLs, etc.):

- Add it to `.env.example` in the relevant section with a comment explaining what it is.
- Use an empty value (`KEY=`) for secrets and a sensible default for non-secret settings.
- Never commit `.env` itself.

## Commit workflow

When the user asks to **commit**, do this in order (do not commit first and version later):

1. **Bump the version** from the current number, based on how many changes there are and how complex they are:
   - **Patch** (`0.1.x`): few, small, or isolated fixes/tweaks.
   - **Minor** (`0.x.0`): several changes, a new feature, or anything more involved.
   - **Major** (`x.0.0`): breaking public/packaging changes, or the first stable `1.0.0`.
2. **Write that version into all version files** so they stay in sync:
   - `shuttle-app/package.json`
   - `shuttle-app/src-tauri/Cargo.toml`
   - `shuttle-app/src-tauri/tauri.conf.json`
   - `shuttle-app/package-lock.json` (the top-level `version` / package entry)
3. **Promote `[Unreleased]` in `CHANGELOG.md`** to `## [X.Y.Z] - YYYY-MM-DD` (today’s date), with concise bullets that match the actual diff. Leave an empty `[Unreleased]` heading at the top.
4. **Commit all relevant files** together: code, version files, lockfiles if they changed, and `CHANGELOG.md`. Follow the user’s git commit rules (no commit unless they asked; no push unless they asked).
