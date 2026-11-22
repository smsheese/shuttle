/** Shared 12h/24h clock preference, set from the appearance setting (`datetime_format`). */
export const datePrefs = $state({ hour12: true });

/** Formats whose id starts with `24h`, plus the day-first/ISO styles, use a 24-hour clock. */
export function setDatetimeFormat(id: string | null | undefined): void {
  datePrefs.hour12 = !(id && (id.startsWith('24h') || id === 'eu' || id === 'iso'));
}
