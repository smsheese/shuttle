/** The desktop OS the webview runs on, from the user agent it reports. */
export type DesktopOs = 'macos' | 'windows' | 'linux';

const DENIED = new Set(['NotAllowedError', 'PermissionDeniedError', 'SecurityError']);
const MISSING = new Set(['NotFoundError', 'DevicesNotFoundError']);
const BUSY = new Set(['NotReadableError', 'TrackStartError']);

/** Only macOS and Windows have an OS switch a person can find; Linux has no such setting. */
const BLOCKED_BY_OS: Record<DesktopOs, string | null> = {
  macos:
    'macOS has blocked the microphone for Shuttle. Open System Settings > Privacy & Security > Microphone, switch Shuttle on, then try again.',
  windows:
    'Windows has blocked the microphone for Shuttle. Open Settings > Privacy & security > Microphone, turn on "Let desktop apps access your microphone", then try again.',
  linux: null,
};

export function desktopOs(userAgent: string): DesktopOs {
  if (/Windows/i.test(userAgent)) return 'windows';
  if (/Mac OS X|Macintosh|MacIntel/i.test(userAgent)) return 'macos';
  return 'linux';
}

/**
 * What the voice-note dialog says when `getUserMedia` fails. The app grants its own UI the
 * microphone, so a denial is the OS holding it back, and only macOS and Windows can say which
 * setting to open.
 */
export function microphoneErrorMessage(error: unknown, userAgent: string): string {
  const name = error instanceof Error ? error.name : '';
  if (DENIED.has(name)) {
    return (
      BLOCKED_BY_OS[desktopOs(userAgent)] ??
      'The microphone did not start. Check that it is plugged in and switched on, then try again.'
    );
  }
  if (MISSING.has(name)) return 'No microphone was found. Plug one in, then try again.';
  if (BUSY.has(name)) return 'Another app is using the microphone. Close it, then try again.';
  return `The microphone could not start: ${error instanceof Error ? error.message : String(error)}`;
}
