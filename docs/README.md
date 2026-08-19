# Shuttle documentation

Product landing page and contributor setup live in the [root README](../README.md). This folder is the in-repo reference for how Shuttle is built, where data lives, and what is still planned.

| Document | What it covers |
| --- | --- |
| [overview.md](overview.md) | Product, principles, stack, and architecture |
| [core.md](core.md) | Rust core, SQLite model, connector protocol, Tauri commands |
| [storage.md](storage.md) | On-disk layout, keyring, security guarantees and gaps |
| [platforms.md](platforms.md) | Windows / Linux / macOS, amd64 + arm64, packaging and CI |
| [licensing.md](licensing.md) | AGPL-3.0 intent and third-party connector licenses |
| [roadmap.md](roadmap.md) | Landed tracks vs remaining work |
| [matrix-mautrix-feasibility.md](matrix-mautrix-feasibility.md) | Closed assessment: stay native; do not use mautrix as a transport layer |
| [telemetry-events.md](telemetry-events.md) | Live telemetry contract, privacy rules, env vars |
| [telemetry-implementation-plan.md](telemetry-implementation-plan.md) | Original telemetry design (implemented; use the events doc day-to-day) |

Third-party credits: [ATTRIBUTION.md](../ATTRIBUTION.md). Release history: [CHANGELOG.md](../CHANGELOG.md).
