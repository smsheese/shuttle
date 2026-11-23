<script lang="ts">
  import '../app.css';
  import { onMount } from 'svelte';
  import { fetchTweakcnTheme, getAppConfig, openDevtools, saveAppConfig } from '$lib/api';
  import { applyAppConfig } from '$lib/theme';
  import { ensureThemeConfig } from '$lib/tweakcn';
  import { startWindowTitle } from '$lib/windowTitle';
  import Toasts from '$lib/components/Toasts.svelte';

  /** Longest the static boot screen may stay up if the config round-trip hangs. */
  const BOOT_SPLASH_FALLBACK_MS = 15_000;

  function removeBootSplash() {
    document.getElementById('boot-splash')?.remove();
  }

  onMount(() => {
    const block = (e: Event) => e.preventDefault();
    document.addEventListener('contextmenu', block, true);
    const onKey = (e: KeyboardEvent) => {
      if (e.ctrlKey && e.shiftKey && e.key.toLowerCase() === 'i') {
        e.preventDefault();
        openDevtools();
      }
    };
    window.addEventListener('keydown', onKey);
    void startWindowTitle();
    const bootFallback = window.setTimeout(removeBootSplash, BOOT_SPLASH_FALLBACK_MS);
    getAppConfig()
      .then(async (cfg) => {
        const withTheme = await ensureThemeConfig(cfg, fetchTweakcnTheme);
        const next =
          withTheme.appearance.tweakcn_css !== cfg.appearance.tweakcn_css
            ? await saveAppConfig(withTheme)
            : withTheme;
        applyAppConfig(next);
      })
      .finally(() => {
        window.clearTimeout(bootFallback);
        removeBootSplash();
      });
    const mq = window.matchMedia('(prefers-color-scheme: dark)');
    const onScheme = () => {
      getAppConfig().then((cfg) => applyAppConfig(cfg));
    };
    mq.addEventListener('change', onScheme);
    return () => {
      window.clearTimeout(bootFallback);
      document.removeEventListener('contextmenu', block, true);
      window.removeEventListener('keydown', onKey);
      mq.removeEventListener('change', onScheme);
    };
  });
</script>

<slot />
<Toasts />
