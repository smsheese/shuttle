<script lang="ts">
  import 'emoji-picker-element';

  interface Props {
    onpick: (emoji: string) => void;
  }

  let { onpick }: Props = $props();
  let host = $state<HTMLElement | null>(null);

  $effect(() => {
    const el = host?.querySelector('emoji-picker') as HTMLElement & {
      addEventListener: (type: string, fn: (e: CustomEvent) => void) => void;
      removeEventListener: (type: string, fn: (e: CustomEvent) => void) => void;
    };
    if (!el) return;
    const handler = (e: CustomEvent<{ unicode: string }>) => onpick(e.detail.unicode);
    el.addEventListener('emoji-click', handler as (e: CustomEvent) => void);
    return () => el.removeEventListener('emoji-click', handler as (e: CustomEvent) => void);
  });
</script>

<div class="emoji-host" bind:this={host}>
  <emoji-picker class="light"></emoji-picker>
</div>

<style>
  .emoji-host :global(emoji-picker) {
    --num-columns: 8;
    --border-size: 0;
    --background: var(--bg-panel);
    --category-emoji-size: 1.125rem;
    --emoji-size: 1.375rem;
    --input-border-color: var(--border-subtle);
    --input-font-color: var(--text-primary);
    --input-placeholder-color: var(--text-muted);
    width: min(300px, 70vw);
    height: 320px;
  }
</style>
