/** Close the native splash window and show the main window (Tauri only). */
export async function dismissSplashScreen(): Promise<void> {
  try {
    const { isTauri } = await import('@tauri-apps/api/core');
    if (!isTauri()) return;
    const { getCurrentWindow } = await import('@tauri-apps/api/window');
    const { WebviewWindow } = await import('@tauri-apps/api/webviewWindow');
    const main = getCurrentWindow();
    if (main.label !== 'main') return;
    await main.show();
    await main.setFocus();
    const splash = await WebviewWindow.getByLabel('splashscreen');
    await splash?.close();
  } catch {
    // Non-fatal: browser dev or missing splash window.
  }
}
