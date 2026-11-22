import { isFutureLocalDateTime } from '$lib/localDateTime';
import type { NoteSchedule } from '$lib/types';

export type NoteScheduleMode = 'none' | 'timer' | 'alarm';

/** Countdown presets offered for a note timer. */
export const TIMER_PRESETS: { label: string; secs: number }[] = [
  { label: '10 min', secs: 600 },
  { label: '30 min', secs: 1800 },
  { label: '1 hour', secs: 3600 },
  { label: '3 hours', secs: 10800 },
];

export const TIMER_UNITS = { m: 60, h: 3600, d: 86400 } as const;

/**
 * The schedule to send for the chosen mode: `null` for none, `undefined` when the choice is
 * incomplete or invalid (empty timer, alarm missing or in the past).
 */
export function buildSchedule(
  mode: NoteScheduleMode,
  timerSecs: number,
  alarmAt: string
): NoteSchedule | null | undefined {
  if (mode === 'none') return null;
  if (mode === 'timer') {
    return Number.isFinite(timerSecs) && timerSecs >= 1 ? { timer_secs: Math.round(timerSecs) } : undefined;
  }
  return isFutureLocalDateTime(alarmAt) ? { alarm_at: new Date(alarmAt).toISOString() } : undefined;
}
