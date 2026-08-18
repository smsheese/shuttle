# Telemetry Event Reference

Shuttle uses a **typed, allowlisted** telemetry registry. Application code should call `$lib/telemetry` (frontend) or the Rust `TelemetryManager` — never Sentry/PostHog SDKs directly.

Telemetry is **opt-in** via Settings → Privacy → Anonymous diagnostics:

| Toggle | Backend | When enabled |
| --- | --- | --- |
| Crash reports | Sentry | Rust panics/errors + frontend unhandled exceptions (sanitized) |
| Usage diagnostics | PostHog | Product/performance events (batched ~30s) |

Both default to **off**.

## Privacy guarantees

Never collected:

- Message text or previews
- Contact names, phone numbers, emails, usernames
- Account/conversation/message IDs
- Credentials, tokens, cookies, QR payloads
- Hostname, OS username, home directory paths

Allowed global context:

- `app_version`, `build_channel`, `release`, `git_commit`
- `os`, `architecture`, `cpu_core_count`, `ram_bucket`
- `environment` (`testing` or `production`) and `build_channel` (same values)
- Aggregated buckets only (`database_size_bucket`, `message_count_bucket`, `accounts_total`)

Anonymous identity: random `installation_id` (UUIDv4) stored in `app_meta` inside `app.sqlite`.

## App events

| Event | Allowed properties |
| --- | --- |
| `app_started` | *(global context only)* |
| `app_ready` | `duration_ms` |
| `app_closed` | — |
| `onboarding_started` | — |
| `onboarding_completed` | — |
| `account_add_completed` | `connector_type` |
| `account_add_failed` | `connector_type`, `error_category` |
| `account_removed` | `connector_type` |
| `connector_sync_started` | `connector_type`, `duration_ms` |
| `connector_sync_completed` | `connector_type`, `duration_ms`, `items_processed`, `errors` |
| `connector_sync_failed` | `connector_type`, `duration_ms`, `error_category` |
| `connector_crashed` | `connector_type`, `error_category` |
| `database_initialized` | `database_size_bucket`, `accounts_total`, `connector_count` |
| `database_migration_completed` | — |
| `database_error` | `error_category` |
| `performance_snapshot` | `foreground`, `sample_count`, `cpu_avg`, `cpu_p95`, `memory_avg_mb`, `memory_p95_mb`, `operation` |
| `search_used` | — |
| `command_failed` | `operation`, `error_category`, `connector_type` |

## Connector telemetry protocol

Sidecars may emit one line on stdout:

```json
{
  "type": "telemetry",
  "event": "sync_completed",
  "connector_type": "telegram",
  "duration_ms": 3821,
  "items_processed": 1421,
  "errors": 0
}
```

Allowed connector events: `sync_started`, `sync_completed`, `sync_failed`, `crashed`.

The Rust host validates and maps these to app events before forwarding.

## Performance cadence

| Mode | Sample interval | Snapshot send interval |
| --- | --- | --- |
| Foreground | 60s | 15 minutes |
| Background | 180s | 30 minutes |

When usage diagnostics are disabled, the sampler sleeps and queues are cleared.

## Environment configuration

Values resolve in this order: **process env** → **`.env` file** (local) → **compile-time baked values** (CI production/testing builds).

| Variable | Purpose |
| --- | --- |
| `SENTRY_DSN` | Sentry project DSN (crash reports) |
| `POSTHOG_API_KEY` | PostHog project API key |
| `POSTHOG_HOST` | PostHog ingest host (default `https://us.i.posthog.com`) |
| `SHUTTLE_BUILD_CHANNEL` | `testing` or `production` |
| `SHUTTLE_GIT_COMMIT` | Optional release metadata |

### Local testing

```bash
cp .env.example .env
# fill SENTRY_DSN and POSTHOG_API_KEY
```

`SHUTTLE_BUILD_CHANNEL=testing` is the default in `.env.example`. The app loads `.env` from the repo root (or `shuttle-app/`) at startup. Do not commit `.env`.

Sentry and PostHog both receive an **environment** of `testing` or `production` (from `SHUTTLE_BUILD_CHANNEL`). Use **separate projects** plus that tag so test noise never sits in production dashboards.

### Sentry: keep testing out of production

1. Create two Sentry projects (or one project with Environments enabled):
   - **shuttle-testing**
   - **shuttle-production**
2. Put the **testing** DSN in local `.env`.
3. Put the **production** DSN in the GitHub Environment named `production`.
4. In Sentry, open the **Environment** dropdown (`testing` / `production`). Issues are tagged automatically.

If you only have one Sentry project, the Environment filter still separates events. Separate DSNs/projects are stronger isolation.

### PostHog: keep testing out of production

1. Create two PostHog projects:
   - **Shuttle testing**
   - **Shuttle production**
2. Put the **testing** project API key in `.env`.
3. Put the **production** project API key in the GitHub `production` environment.
4. Every event also includes `$environment` and `environment` (`testing` | `production`) so you can filter inside a single project if needed.

Do **not** reuse the production PostHog key in `.env`.

### GitHub production / testing

Create GitHub Environments named **`production`** and **`testing`**, each with **its own** secrets (different Sentry DSN and PostHog key):

- `SENTRY_DSN`
- `POSTHOG_API_KEY`
- `POSTHOG_HOST` (optional)

The release workflow uses:

| Trigger | GitHub Environment | `SHUTTLE_BUILD_CHANNEL` |
| --- | --- | --- |
| Push to `main` | `production` | `production` |
| Tag `v*` | `production` | `production` |
| Manual `workflow_dispatch` | chosen in the UI (default `testing`) | same as environment |

Those secrets are passed into `tauri build` and **baked into the binary**, so installed apps do not need a `.env` file. Users still must enable Settings → Privacy toggles.

See also [telemetry-implementation-plan.md](telemetry-implementation-plan.md).
