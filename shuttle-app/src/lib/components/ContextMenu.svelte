<script lang="ts">
  import type { MenuItem } from '$lib/types';

  interface Props {
    open: boolean;
    x: number;
    y: number;
    items: MenuItem[];
    onclose: () => void;
    onselect: (id: string) => void;
  }

  let { open, x, y, items, onclose, onselect }: Props = $props();
  let el = $state<HTMLDivElement | undefined>();

  $effect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onclose();
    };
    const onDown = (e: MouseEvent) => {
      if (el && !el.contains(e.target as Node)) onclose();
    };
    window.addEventListener('keydown', onKey);
    window.addEventListener('mousedown', onDown);
    return () => {
      window.removeEventListener('keydown', onKey);
      window.removeEventListener('mousedown', onDown);
    };
  });

  const style = $derived.by(() => {
    const pad = 8;
    const w = 240;
    const h = items.length * 32 + 12;
    const left = Math.min(x, window.innerWidth - w - pad);
    const top = Math.min(y, window.innerHeight - h - pad);
    return `left:${Math.max(pad, left)}px;top:${Math.max(pad, top)}px`;
  });
</script>

{#if open}
  <div class="menu" bind:this={el} style={style} role="menu">
    {#each items as item (item.id)}
      {#if item.separator}
        <div class="sep" role="separator"></div>
      {:else}
        <button
          class="item"
          class:danger={item.danger}
          disabled={item.disabled}
          role="menuitem"
          type="button"
          onclick={() => {
            if (!item.disabled) {
              onselect(item.id);
              onclose();
            }
          }}
        >
          {item.label}
        </button>
      {/if}
    {/each}
  </div>
{/if}

<style>
  .menu {
    position: fixed;
    z-index: 80;
    min-width: 220px;
    max-width: 280px;
    padding: 6px;
    background: var(--bg-panel);
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    box-shadow: var(--shadow-md);
  }
  .item {
    display: block;
    width: 100%;
    text-align: left;
    border: none;
    background: transparent;
    color: var(--text);
    font-size: 13px;
    padding: 8px 10px;
    border-radius: var(--radius-sm);
    cursor: pointer;
  }
  .item:hover:not(:disabled) {
    background: var(--bg-hover);
  }
  .item:disabled {
    color: var(--text-muted);
    cursor: default;
  }
  .item.danger {
    color: #ef4444;
  }
  .sep {
    height: 1px;
    background: var(--border-subtle);
    margin: 4px 6px;
  }
</style>
