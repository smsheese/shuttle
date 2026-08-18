export type TelemetryEventName =
  | 'app_started'
  | 'app_ready'
  | 'app_closed'
  | 'onboarding_started'
  | 'onboarding_completed'
  | 'account_add_completed'
  | 'account_add_failed'
  | 'account_removed'
  | 'connector_sync_started'
  | 'connector_sync_completed'
  | 'connector_sync_failed'
  | 'connector_crashed'
  | 'database_initialized'
  | 'database_migration_completed'
  | 'database_error'
  | 'performance_snapshot'
  | 'search_used'
  | 'command_failed';

export type TelemetryProps = Record<string, string | number | boolean | null>;

export interface TelemetryErrorContext {
  operation?: string;
  error_category?: string;
  connector_type?: string;
}
