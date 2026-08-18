import {
  telemetryError as apiError,
  telemetryPerformance as apiPerformance,
  telemetrySetForeground as apiSetForeground,
  telemetryTrack as apiTrack,
} from '$lib/api';
import { filterTelemetryProps } from './privacy';
import type { TelemetryErrorContext, TelemetryEventName, TelemetryProps } from './types';

let handlersInstalled = false;
let appReadySent = false;

export async function initTelemetry(): Promise<void> {
  if (handlersInstalled || typeof window === 'undefined') return;
  handlersInstalled = true;

  window.addEventListener('error', (event) => {
    void reportError('frontend_exception', {
      operation: 'window.error',
      error_category: event.message.slice(0, 120),
    });
  });

  window.addEventListener('unhandledrejection', (event) => {
    const reason =
      event.reason instanceof Error
        ? event.reason.message
        : typeof event.reason === 'string'
          ? event.reason
          : 'unhandled_rejection';
    void reportError('frontend_unhandled_rejection', {
      operation: 'window.unhandledrejection',
      error_category: reason.slice(0, 120),
    });
  });

  document.addEventListener('visibilitychange', () => {
    void setForeground(document.visibilityState === 'visible');
  });

  await setForeground(document.visibilityState === 'visible');
}

export async function markAppReady(durationMs?: number): Promise<void> {
  if (appReadySent) return;
  appReadySent = true;
  await track('app_ready', durationMs == null ? {} : { duration_ms: durationMs });
}

export async function track(event: TelemetryEventName, props: TelemetryProps = {}): Promise<void> {
  const filtered = filterTelemetryProps(event, props);
  try {
    await apiTrack(event, filtered);
  } catch {
    // Telemetry must never break app workflows.
  }
}

export async function reportError(message: string, context: TelemetryErrorContext = {}): Promise<void> {
  try {
    await apiError(message, context);
  } catch {
    // ignore
  }
}

export async function trackPerformance(operation: string, props: TelemetryProps = {}): Promise<void> {
  try {
    await apiPerformance(operation, { ...props, operation });
  } catch {
    // ignore
  }
}

export async function setForeground(foreground: boolean): Promise<void> {
  try {
    await apiSetForeground(foreground);
  } catch {
    // ignore
  }
}

export async function markAppClosed(): Promise<void> {
  await track('app_closed');
}
