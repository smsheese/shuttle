/** An incomplete local date/time cannot be converted to a scheduled instant. */
export function isCompleteLocalDateTime(value: string): boolean {
  return /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}$/.test(value) && Number.isFinite(new Date(value).getTime());
}

const pad2 = (n: number) => String(n).padStart(2, '0');

/** `YYYY-MM-DDTHH:mm` in local time, the value format the date-time picker binds to. */
export function toLocalDateTime(d: Date): string {
  return `${d.getFullYear()}-${pad2(d.getMonth() + 1)}-${pad2(d.getDate())}T${pad2(d.getHours())}:${pad2(d.getMinutes())}`;
}

/** Local `YYYY-MM-DDTHH:mm` for an ISO instant, or '' when it does not parse. */
export function isoToLocalDateTime(iso: string): string {
  const d = new Date(iso);
  return Number.isNaN(d.getTime()) ? '' : toLocalDateTime(d);
}

/** The first minute that is still in the future (picker minimum for schedule/reminder flows). */
export function nextMinute(now = new Date()): Date {
  const d = new Date(now);
  d.setSeconds(0, 0);
  d.setMinutes(d.getMinutes() + 1);
  return d;
}

/** A default pick: `offsetMs` from now, rounded up to the next whole minute. */
export function defaultLocalDateTime(offsetMs = 3600_000, now = new Date()): string {
  return toLocalDateTime(nextMinute(new Date(now.getTime() + offsetMs - 60_000)));
}

/** Is this local date-time at or after the first future minute? */
export function isFutureLocalDateTime(value: string, now = new Date()): boolean {
  return isCompleteLocalDateTime(value) && new Date(value).getTime() >= nextMinute(now).getTime();
}

/** "1h 05m", "4m 09s", "12s" — remaining time for a countdown. */
export function formatCountdown(ms: number): string {
  const total = Math.max(0, Math.ceil(ms / 1000));
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  if (h >= 24) return `${Math.floor(h / 24)}d ${h % 24}h`;
  if (h > 0) return `${h}h ${pad2(m)}m`;
  if (m > 0) return `${m}m ${pad2(s)}s`;
  return `${s}s`;
}
