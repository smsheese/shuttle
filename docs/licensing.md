# Licensing

Shuttle is **free and open source** under the [GNU Affero General Public License v3.0 (AGPL-3.0)](../LICENSE).

## Why AGPL-3.0

AGPL-3.0 was chosen so Shuttle can grow with community contributions while discouraging proprietary forks that hide modified code — especially **SaaS wrappers** that run a modified Shuttle backend for users without publishing those changes.

Compared to MIT or Apache-2.0:

- **Copyleft:** derivatives and combined works that you distribute must stay under AGPL (or a compatible license) and include source.
- **Network copyleft (Section 13):** if you modify Shuttle and run it so users interact with it over a network (for example a hosted unified-inbox service), you must offer those users the corresponding source of your modified version.

Shuttle is a **local-first desktop app**. Most users run it on their own machine and AGPL behaves like GPL for them. AGPL matters when someone turns Shuttle into a multi-user hosted service without sharing changes.

## Third-party components

Shuttle integrates with separate connector backends (GOWA, TDLib, signal-cli, Python libraries, etc.). Those projects keep their own licenses. Shuttle does not relicense them.

| Component | License | Shipped with installer? |
| --- | --- | --- |
| Shuttle app + wrappers | AGPL-3.0 | yes (this repo) |
| signal-cli | GPL-3.0 | downloaded on demand when adding Signal |
| Embedded CPython (python-build-standalone) | MPL-2.0 / PSF | downloaded on demand when needed |
| GOWA / TDLib | MIT / BSL-1.0 | downloaded on demand when adding WhatsApp / Telegram |
| Matrix Client-Server API | Apache-2.0 (spec); server-dependent | HTTPS only, no extra binary |
| fbchat, instagrapi | BSD-3-Clause / MIT | downloaded on demand (Messenger / Instagram) |

Full credits: [ATTRIBUTION.md](../ATTRIBUTION.md) and **Settings → Attributions** in the app.

## signal-cli (GPL-3.0)

There is no better maintained open option for Signal in a local sidecar today, so Shuttle **downloads signal-cli on demand** when you add a Signal account. The app shows a GPL acknowledgment before download. Shuttle talks to signal-cli over JSON-RPC; it is not linked into the Rust binary.

GPL-3.0 obligations still apply when signal-cli is installed on your device: preserve license text, copyright notices, and make corresponding source available. Upstream source: [github.com/AsamK/signal-cli](https://github.com/AsamK/signal-cli). License text is included in the Shuttle installer under `licenses/signal-cli-GPL-3.0.txt`.

AGPL Shuttle + GPL signal-cli sidecar is a standard combined-distribution arrangement; each license applies to its respective program.

## Contributing

By contributing to this repository you agree that your contributions are licensed under AGPL-3.0 (same as the project), unless you explicitly state otherwise in the contribution.

## Not legal advice

This document summarizes project intent. For compliance questions about your specific distribution or product, consult a lawyer.
