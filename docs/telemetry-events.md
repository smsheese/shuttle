# Telemetry Event Reference

Shuttle uses a **typed, allowlisted** telemetry registry. Application code should call `$lib/telemetry` (frontend) or the Rust `TelemetryManager` — never PostHog SDKs directly.

Telemetry is **opt-in** via Settings → Privacy → Anonymous diagnostics:

| Toggle | Backend | When enabled |
| --- | --- | --- |
| Crash reports | PostHog | Rust panics and frontend unhandled exceptions (safe categories only) |
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

- `product`, `app`, `platform`, `app_version`, `build_channel`, `release`, `git_commit`
- `os`, `architecture`, `cpu_core_count`, `ram_bucket`
- `environment` (`testing` or `production`) and `build_channel` (same values)
- Aggregated buckets only (`database_size_bucket`, `message_count_bucket`, `accounts_total`)

Usage diagnostics use a random `installation_id` (UUIDv4) stored in `app_meta`. Error reports are personless and contain a fixed error category, a crate-relative Rust panic location when available, and the global context above. Panic payloads, frontend error messages, and stack traces are discarded before sending. This limits debugging detail. Panic delivery is best-effort. Session replay and automatic interaction capture are not enabled. PostHog receives the network request IP; configure the instance to discard client IP data.

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
| `performance_snapshot` | `foreground`, `sample_count`, `cpu_avg`, `cpu_p95`, `memory_avg_mb`, `memory_p95_mb` (Shuttle itself), `sidecar_cpu_avg`, `sidecar_memory_avg_mb`, `sidecar_memory_p95_mb`, `sidecar_count` (its helper programs), plus the load buckets below |
| `search_used` | — |
| `command_failed` | `operation`, `error_category`, `connector_type` |
| `operation_timing` | `operation` (`chat_list`, `chat_open`, `search_chats`, `search_messages`), `count`, `p50_ms`, `p95_ms`, `max_ms`, plus the load buckets below |

## Load buckets

`performance_snapshot`, `operation_timing` and `database_initialized` carry ranges that describe the load, so a slow number can be read against it. They are ranges, never counts.

| Field | Values |
| --- | --- |
| `accounts_total` | `0`, `1`, `2_3`, `4_5`, `6_10`, `11_plus` |
| `conversation_count_bucket`, `message_count_bucket`, `largest_account_message_bucket` | `0`, `1_100`, `101_1k`, `1k_10k`, `10k_100k`, `100k_1m`, `gt_1m` |
| `database_size_bucket` | `le_1mb`, `le_10mb`, `le_100mb`, `gt_100mb` |

Timings are collected in memory per operation (last 200 calls) and sent as one summary per batch. Only the four listed operation names are recorded.

See [PRIVACY.md](../PRIVACY.md) for the user-facing policy.

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
| `POSTHOG_API_KEY` | PostHog project ingestion token (not a personal API key) |
| `POSTHOG_HOST` | PostHog ingest host (default `https://eu.i.posthog.com` (EU Cloud)) |
| `SHUTTLE_BUILD_CHANNEL` | `testing` or `production` |
| `SHUTTLE_GIT_COMMIT` | Optional release metadata |

### Local testing

```bash
cp .env.example .env
# fill POSTHOG_API_KEY with the testing project ingestion token
```

`SHUTTLE_BUILD_CHANNEL=testing` is the default in `.env.example`. The app loads `.env` from the repo root (or `shuttle-app/`) at startup. Do not commit `.env`.

PostHog events receive an **environment** of `testing` or `production` from `SHUTTLE_BUILD_CHANNEL`. Use a separate testing project if available.

### PostHog: keep testing out of production

1. Create two PostHog projects:
   - **Shuttle testing**
   - **Shuttle production**
2. Put the **testing** project API key in `.env`.
3. Put the **production** project API key in the GitHub `production` environment.
4. Every event also includes `$environment` and `environment` (`testing` | `production`) so you can filter inside a single project if needed.

Do **not** reuse the production PostHog key in `.env`.

### GitHub production / testing

Create GitHub Environments named **`production`** and **`testing`**, each with its own project ingestion token:

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
