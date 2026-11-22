<script lang="ts">
  import { revealInFileManager } from '$lib/api';
  import { dismissToast, toast, toasts } from '$lib/toast.svelte';

  async function reveal(path: string, id: number) {
    try {
      await revealInFileManager(path);
    } catch (e) {
      toast.error(`Could not show the file: ${e instanceof Error ? e.message : String(e)}`);
    }
    dismissToast(id);
  }
</script>

<div class="toasts" aria-live="polite">
  {#each toasts.items as t (t.id)}
    <div class="toast" class:error={t.kind === 'error'} role={t.kind === 'error' ? 'alert' : 'status'}>
      {#if t.kind === 'saved' && t.path}
        <span>{t.text}</span>
        <button type="button" class="path" title="Show in file manager" onclick={() => reveal(t.path!, t.id)}>{t.path}</button>
      {:else}
        <span>{t.text}</span>
      {/if}
    </div>
  {/each}
</div>

<style>
  .toasts {
    position: fixed;
    right: 16px;
    bottom: 16px;
    z-index: 400;
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    gap: 8px;
    max-width: min(520px, calc(100vw - 32px));
    pointer-events: none;
  }
  .toast {
    pointer-events: auto;
    display: flex;
    gap: 6px;
    align-items: baseline;
    min-width: 0;
    padding: 10px 14px;
    font-size: 0.85rem;
    color: var(--text);
    background: var(--bg-panel);
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    box-shadow: var(--shadow-md);
  }
  .toast.error {
    border-color: var(--danger, #e5484d);
  }
  .path {
    min-width: 0;
    padding: 0;
    color: var(--accent);
    font: inherit;
    text-align: left;
    overflow-wrap: anywhere;
    background: none;
    border: 0;
    cursor: pointer;
    text-decoration: underline;
  }
  .path:hover {
    color: var(--accent-hover);
  }
</style>
