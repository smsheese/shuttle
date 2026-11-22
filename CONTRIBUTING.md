# Contributing to Shuttle

Thanks for helping. Shuttle is AGPL-3.0-or-later and also offered under a [commercial license](COMMERCIAL-LICENSE.md), so contributions need a signed CLA.

1. Read the [CLA](CLA.md) and agree to it in your pull request (once per contributor).
2. Sign off your commits: `git commit -s`.
3. Run the tests before opening a PR: `python3 -m unittest discover -s connectors/tests` and `cargo test` (in `shuttle-app/src-tauri`).
4. Update `CHANGELOG.md` for features and fixes, and keep docs aligned (see [AGENTS.md](AGENTS.md)).
5. New dependencies linked into the app must be compatible with AGPL-3.0 **and** allow relicensing under the commercial license: MIT, BSD, ISC, Apache-2.0, Zlib, BSL-1.0, Unicode and MPL-2.0 (unmodified) are fine. Do not add GPL-only, LGPL-only, AGPL, SSPL, BUSL or proprietary code to the binary. Separate processes (like signal-cli) are listed in [docs/licensing.md](docs/licensing.md).
6. Do not copy code, icons, fonts or images from elsewhere without naming the source and its license in the PR.

## Source file headers

New source files may start with an SPDX line in the file's comment syntax, for example:

```rust
// SPDX-License-Identifier: AGPL-3.0-or-later
```

Do not mass-edit existing files to add headers. The root [LICENSE](LICENSE) covers every file that has none.

Architecture and rules: [docs/overview.md](docs/overview.md), [AGENTS.md](AGENTS.md).
