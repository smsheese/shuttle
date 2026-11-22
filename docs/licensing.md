# Licensing

Shuttle is **free and open source** under the [GNU AGPL-3.0-or-later](../LICENSE), and also offered under a separate paid [commercial license](../COMMERCIAL-LICENSE.md).

## Why AGPL plus a commercial license

The goal is that the community can read, run, change and build on Shuttle for free, while anyone who builds a paid product on it either shares their changes or pays.

- **Free path (AGPL-3.0-or-later).** Use, modify and redistribute Shuttle at no cost, including selling it or running it as a paid service. If you distribute a modified version, or let people use one over a network, you must publish the complete source of your version under the AGPL.
- **Paid path (commercial license).** If you cannot or will not publish your source (a closed product, a proprietary fork, an embedded or white-label build), you buy a commercial license from the project owner, Mohammed Sheese Sheikh (iam@shee.se). Terms are in [COMMERCIAL-LICENSE.md](../COMMERCIAL-LICENSE.md). Fees can be a flat fee or a revenue share.

The AGPL on its own cannot force anyone to pay. Money comes only from the commercial license, which companies buy so they do not have to share their code.

Shuttle is a **local-first desktop app**, and the connectors it talks to are separate programs under their own licenses.

## Third-party components

Shuttle integrates with separate connector backends (GOWA, TDLib, signal-cli, Python libraries, etc.). Those projects keep their own licenses. Shuttle does not relicense them.

| Component | License | Shipped with installer? |
| --- | --- | --- |
| Shuttle app + wrappers | AGPL-3.0-or-later (or commercial) | yes (this repo) |
| signal-cli | GPL-3.0 | downloaded on demand when adding Signal |
| Embedded CPython (python-build-standalone) | MPL-2.0 / PSF | downloaded on demand when needed |
| GOWA / TDLib | MIT / BSL-1.0 | downloaded on demand when adding WhatsApp / Telegram |
| Matrix Client-Server API | Apache-2.0 (spec); server-dependent | HTTPS only, no extra binary |
| fbchat, instagrapi | BSD-3-Clause / MIT | downloaded on demand (Messenger / Instagram) |

Full credits: [ATTRIBUTION.md](../ATTRIBUTION.md) and **Settings → Attributions** in the app.

**Commercial license limit:** the commercial license covers only code the maintainers own or may relicense (see the CLA). It cannot cover third-party components. Their licenses still apply to whoever ships them.

## signal-cli (GPL-3.0)

There is no better maintained open option for Signal in a local sidecar today, so Shuttle **downloads signal-cli on demand** when you add a Signal account. The app shows a GPL acknowledgment before download. Shuttle talks to signal-cli over JSON-RPC; it is not linked into the Rust binary.

Shuttle's own GitHub releases host the signal-cli component, so the project is a distributor and must meet GPL-3.0: keep the license text and copyright notices, and make the corresponding source for the exact version shipped available. Upstream source: [github.com/AsamK/signal-cli](https://github.com/AsamK/signal-cli). License text is included in the Shuttle installer under `licenses/signal-cli-GPL-3.0.txt`. Anyone who re-hosts the component takes on the same duties.

Shuttle and the GPL signal-cli sidecar stay separate programs that talk over JSON-RPC; each license applies to its own program.

## Contributing

Contributions need a signed [CLA](../CLA.md) so the project can keep offering the commercial license. You keep your copyright. The maintainers record each signature (GitHub user, name, date, CLA version) before merging. See [CONTRIBUTING.md](../CONTRIBUTING.md).

New source files may carry `SPDX-License-Identifier: AGPL-3.0-or-later`; files without a header are covered by the root [LICENSE](../LICENSE).

## Dependencies

Linked dependencies must be compatible with AGPL-3.0 **and** with relicensing under the commercial license. Avoid GPL-only, LGPL-only, AGPL, SSPL, BUSL and proprietary code in the binary; MPL-2.0 is fine when unmodified. Run `cargo deny check licenses` before releases.

Last manual check (2026-11-19, from `Cargo.lock` and `package-lock.json`): every Rust crate and npm runtime package is under MIT, Apache-2.0, BSD, ISC, Zlib, BSL-1.0, Unicode-3.0, CDLA-Permissive-2.0, Unlicense or CC0, or offers one of those as an option. The MPL-2.0 crates (`cssparser`, `selectors`, `dtoa-short`, `option-ext`) are used unmodified.

## Name and logo

The AGPL covers Shuttle's code, not its name or logo (`shuttle-logov1.1.*`, `shuttle-app/static/shuttle-logo.*`, `shuttle-app/src-tauri/icons/`). Forks may keep the notices but should use their own name and icon so users can tell them apart from official Shuttle builds. A commercial license includes the name or logo only if the agreement says so.

## Not legal advice

This document summarizes project intent. For compliance questions about your specific distribution or product, consult a lawyer.
