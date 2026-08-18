<script lang="ts">
  import '../app.css';
  import { onMount } from 'svelte';
  import { getAppConfig, openDevtools } from '$lib/api';
  import { applyAppConfig } from '$lib/theme';

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
    getAppConfig().then(applyAppConfig);
    const mq = window.matchMedia('(prefers-color-scheme: dark)');
    const onScheme = () => getAppConfig().then(applyAppConfig);
    mq.addEventListener('change', onScheme);
    return () => {
      document.removeEventListener('contextmenu', block, true);
      window.removeEventListener('keydown', onKey);
      mq.removeEventListener('change', onScheme);
    };
  });
</script>

<slot />
