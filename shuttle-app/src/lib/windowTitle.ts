/** Keep the native window title as `Shuttle <version> — <locale date/time>`. */
export async function startWindowTitleClock(): Promise<() => void> {
  try {
    const { isTauri } = await import('@tauri-apps/api/core');
    if (!isTauri()) return () => {};

    const { getCurrentWindow } = await import('@tauri-apps/api/window');
    const { getVersion } = await import('@tauri-apps/api/app');
    const win = getCurrentWindow();
    if (win.label !== 'main') return () => {};

    const version = await getVersion();

    const tick = async () => {
      const when = new Date().toLocaleString(undefined, {
        dateStyle: 'medium',
        timeStyle: 'medium',
      });
      await win.setTitle(`Shuttle ${version} — ${when}`);
    };

    await tick();
    const id = window.setInterval(() => {
      void tick();
    }, 1000);
    return () => window.clearInterval(id);
  } catch {
    return () => {};
  }
}
