/** The native window title: the unread count in front when there is one, then the app name. */
export function windowTitle(version: string, unread: number): string {
  return `${unread > 0 ? `(${unread}) ` : ''}Shuttle ${version}`;
}

let unreadCount = 0;
let applyTitle: ((count: number) => void) | undefined;

/** Set the unread count the title shows. The title changes only when the count does. */
export function setWindowTitleUnread(count: number): void {
  unreadCount = count;
  applyTitle?.(count);
}

/** Set the title once at start (Tauri only). Nothing here runs on a timer. */
export async function startWindowTitle(): Promise<void> {
  try {
    const { isTauri } = await import('@tauri-apps/api/core');
    if (!isTauri()) return;

    const { getCurrentWindow } = await import('@tauri-apps/api/window');
    const { getVersion } = await import('@tauri-apps/api/app');
    const win = getCurrentWindow();
    if (win.label !== 'main') return;

    const version = await getVersion();
    let shown = -1;
    applyTitle = (count) => {
      if (count === shown) return;
      shown = count;
      void win.setTitle(windowTitle(version, count));
    };
    applyTitle(unreadCount);
  } catch {
    // Browser dev has no window handle.
  }
}
