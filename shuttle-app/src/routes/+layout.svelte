<script lang="ts">
  import '../app.css';
  import { onMount } from 'svelte';
  import { fetchTweakcnTheme, getAppConfig, openDevtools, saveAppConfig } from '$lib/api';
  import { dismissSplashScreen } from '$lib/splash';
  import { applyAppConfig } from '$lib/theme';
  import { ensureThemeConfig } from '$lib/tweakcn';

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
        void dismissSplashScreen();
      });
    const mq = window.matchMedia('(prefers-color-scheme: dark)');
    const onScheme = () => {
      getAppConfig().then((cfg) => applyAppConfig(cfg));
    };
    mq.addEventListener('change', onScheme);
    return () => {
      document.removeEventListener('contextmenu', block, true);
      window.removeEventListener('keydown', onKey);
      mq.removeEventListener('change', onScheme);
    };
  });
</script>

<slot />
