import type { TelemetryEventName, TelemetryProps } from './types';

/** Allowed property keys per event (must match Rust registry). */
export const EVENT_ALLOWLIST: Partial<Record<TelemetryEventName, readonly string[]>> = {
  app_ready: ['duration_ms'],
  account_add_completed: ['connector_type'],
  account_add_failed: ['connector_type', 'error_category'],
  account_removed: ['connector_type'],
  connector_sync_completed: ['connector_type', 'duration_ms', 'items_processed', 'errors'],
  connector_sync_failed: ['connector_type', 'duration_ms', 'error_category'],
  connector_crashed: ['connector_type', 'error_category'],
  database_initialized: ['database_size_bucket', 'accounts_total', 'connector_count'],
  database_error: ['error_category'],
  performance_snapshot: [
    'foreground',
    'sample_count',
    'cpu_avg',
    'cpu_p95',
    'memory_avg_mb',
    'memory_p95_mb',
    'operation',
  ],
  command_failed: ['operation', 'error_category', 'connector_type'],
};

const GLOBAL_KEYS = new Set([
  'app_version',
  'build_channel',
  'environment',
  'release',
  'git_commit',
  'os',
  'os_version',
  'architecture',
  'cpu_core_count',
  'ram_bucket',
  'accounts_total',
  'connector_count',
  'database_size_bucket',
  'message_count_bucket',
  'duration_ms',
  'items_processed',
  'errors',
  'connector_type',
  'operation',
  'error_category',
  'foreground',
  'sample_count',
  'cpu_avg',
  'cpu_p95',
  'memory_avg_mb',
  'memory_p95_mb',
]);

const SENSITIVE_KEY =
  /(phone|email|username|password|token|secret|cookie|auth|message|body|content|account_id|conversation_id|chat_id|remote_id|sender|credential|api_key|authorization|session|identity|hostname|home_dir|ip_addr)/i;

export function filterTelemetryProps(
  event: TelemetryEventName,
  props: TelemetryProps
): TelemetryProps {
  const allowed = new Set([...(EVENT_ALLOWLIST[event] ?? []), ...GLOBAL_KEYS]);
  const out: TelemetryProps = {};
  for (const [key, value] of Object.entries(props)) {
    if (SENSITIVE_KEY.test(key)) continue;
    if (!allowed.has(key)) continue;
    if (typeof value === 'string' && SENSITIVE_KEY.test(value)) continue;
    out[key] = value;
  }
  return out;
}
