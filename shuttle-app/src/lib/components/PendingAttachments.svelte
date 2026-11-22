<script lang="ts">
  import type { PendingAttachment } from '$lib/attachments';
  import { formatFileSize } from '$lib/messageMedia';

  interface Props {
    items: PendingAttachment[];
    onremove: (id: string) => void;
  }

  let { items, onremove }: Props = $props();
</script>

<div class="pending-files">
  {#each items as item (item.id)}
    <div class="pending-file">
      {#if item.preview}
        <img class="thumb" src={item.preview} alt="" />
      {:else}
        <span class="glyph" aria-hidden="true">
          {#if item.kind === 'video'}
            <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor"><path d="M8 5v14l11-7z"/></svg>
          {:else if item.kind === 'audio'}
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M9 18V5l12-2v13"/><circle cx="6" cy="18" r="3"/><circle cx="18" cy="16" r="3"/></svg>
          {:else}
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"/><path d="M14 2v6h6"/></svg>
          {/if}
        </span>
      {/if}
      <span class="text">
        <span class="name" title={item.filename}>{item.filename}</span>
        {#if formatFileSize(item.size)}
          <span class="size">{formatFileSize(item.size)}</span>
        {/if}
      </span>
      <button
        type="button"
        class="remove"
        onclick={() => onremove(item.id)}
        aria-label="Remove {item.filename}"
        title="Remove"
      >×</button>
    </div>
  {/each}
</div>

<style>
  .pending-files {
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin-bottom: 8px;
  }

  .pending-file {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 6px 8px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    background: var(--bg-input);
    color: var(--text);
    min-width: 0;
  }

  .thumb {
    width: 34px;
    height: 34px;
    flex: none;
    border-radius: var(--radius-sm);
    object-fit: cover;
    background: var(--bg-panel);
  }

  .glyph {
    width: 34px;
    height: 34px;
    flex: none;
    display: grid;
    place-items: center;
    border-radius: var(--radius-sm);
    background: var(--bg-panel);
    color: var(--text-muted);
  }

  .text {
    display: flex;
    flex-direction: column;
    gap: 1px;
    min-width: 0;
    flex: 1;
  }

  .name {
    font-size: 12px;
    font-weight: 500;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .size {
    font-size: 11px;
    color: var(--text-muted);
  }

  .remove {
    flex: none;
    width: 24px;
    height: 24px;
    border: none;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--text-secondary);
    font-size: 17px;
    line-height: 1;
    cursor: pointer;
  }

  .remove:hover {
    background: var(--bg-hover);
    color: var(--text);
  }

  .remove:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 1px;
  }
</style>
