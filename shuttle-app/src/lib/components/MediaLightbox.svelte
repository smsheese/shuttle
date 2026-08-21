<script lang="ts">
  import { openExternal } from '$lib/api';

  export type LightboxItem = {
    id: string;
    url: string;
    kind: 'image' | 'video' | 'audio' | 'document';
    filename?: string | null;
    mime?: string | null;
  };

  interface Props {
    open: boolean;
    items: LightboxItem[];
    index: number;
    onclose: () => void;
    onindex: (i: number) => void;
    onsave?: (item: LightboxItem) => void;
    onedit?: (item: LightboxItem) => void;
  }

  let { open, items, index, onclose, onindex, onsave, onedit }: Props = $props();

  let pdfFailed = $state(false);

  const item = $derived(items[index] ?? null);
  const hasNav = $derived(items.length > 1);
  const isPdf = $derived(
    item?.kind === 'document' &&
      (item.filename?.toLowerCase().endsWith('.pdf') ||
        item.mime?.toLowerCase().includes('pdf') ||
        item.url.toLowerCase().includes('.pdf'))
  );
  const canOpenExternal = $derived(item?.url.startsWith('http://') || item?.url.startsWith('https://'));

  $effect(() => {
    if (open) pdfFailed = false;
  });

  $effect(() => {
    if (!open) return;
    function onKey(e: KeyboardEvent) {
      if (e.key === 'Escape') onclose();
      else if (e.key === 'ArrowLeft' && hasNav) onindex((index - 1 + items.length) % items.length);
      else if (e.key === 'ArrowRight' && hasNav) onindex((index + 1) % items.length);
    }
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  });

  function saveCurrent() {
    if (!item) return;
    if (onsave) {
      onsave(item);
      return;
    }
    const a = document.createElement('a');
    a.href = item.url;
    a.download = item.filename ?? `media-${item.id.slice(0, 8)}`;
    a.click();
  }

  async function openWithSystem() {
    if (!item || !canOpenExternal) return;
    await openExternal(item.url);
  }
</script>

{#if open && item}
  <div class="backdrop" role="presentation" onclick={onclose}></div>
  <div class="lightbox" role="dialog" aria-modal="true" aria-label="Media viewer">
    <header class="toolbar">
      <span class="title">{item.filename ?? item.kind}</span>
      <div class="actions">
        {#if item.kind === 'image' && onedit}
          <button type="button" class="btn" onclick={() => onedit?.(item)}>Edit</button>
        {/if}
        <button type="button" class="btn" onclick={saveCurrent}>Save</button>
        <button type="button" class="btn close" onclick={onclose} aria-label="Close">×</button>
      </div>
    </header>

    <div class="stage">
      {#if hasNav}
        <button type="button" class="nav prev" onclick={() => onindex((index - 1 + items.length) % items.length)} aria-label="Previous">‹</button>
      {/if}

      <div class="content">
        {#if item.kind === 'image'}
          <img src={item.url} alt={item.filename ?? 'Image'} />
        {:else if item.kind === 'video'}
          <!-- svelte-ignore a11y_media_has_caption -->
          <video src={item.url} controls></video>
        {:else if item.kind === 'audio'}
          <div class="audio-wrap">
            <span class="audio-label">{item.filename ?? 'Audio'}</span>
            <audio src={item.url} controls></audio>
          </div>
        {:else if isPdf && !pdfFailed}
          <iframe
            src={item.url}
            title={item.filename ?? 'PDF'}
            onerror={() => (pdfFailed = true)}
          ></iframe>
        {:else if isPdf && pdfFailed}
          <div class="doc-fallback">
            <p>Could not preview this PDF in Shuttle.</p>
            {#if canOpenExternal}
              <button type="button" class="btn primary" onclick={openWithSystem}>Open with system</button>
            {:else}
              <button type="button" class="btn primary" onclick={saveCurrent}>Save</button>
            {/if}
          </div>
        {:else}
          <div class="doc-fallback">
            <span class="doc-icon">📄</span>
            <p>{item.filename ?? 'Document'}</p>
            <button type="button" class="btn primary" onclick={saveCurrent}>Save</button>
          </div>
        {/if}
      </div>

      {#if hasNav}
        <button type="button" class="nav next" onclick={() => onindex((index + 1) % items.length)} aria-label="Next">›</button>
      {/if}
    </div>

    {#if hasNav}
      <footer class="counter">{index + 1} / {items.length}</footer>
    {/if}
  </div>
{/if}

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.82);
    z-index: 220;
  }

  .lightbox {
    position: fixed;
    inset: 16px;
    z-index: 221;
    display: flex;
    flex-direction: column;
    gap: 8px;
    pointer-events: none;
  }

  .toolbar,
  .stage,
  .counter {
    pointer-events: auto;
  }

  .toolbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 8px 12px;
    border-radius: var(--radius-md);
    background: var(--bg-panel);
    border: 1px solid var(--border-subtle);
    color: var(--text);
  }

  .title {
    font-size: 13px;
    font-weight: 500;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
  }

  .actions {
    display: flex;
    align-items: center;
    gap: 6px;
    flex-shrink: 0;
  }

  .btn {
    border: 1px solid var(--border);
    background: var(--bg-input);
    color: var(--text);
    border-radius: var(--radius-sm);
    padding: 5px 10px;
    font-size: 12px;
    cursor: pointer;
  }

  .btn.primary {
    background: var(--accent);
    border-color: var(--accent);
    color: white;
  }

  .btn.close {
    font-size: 20px;
    line-height: 1;
    padding: 2px 8px;
  }

  .stage {
    flex: 1;
    min-height: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
  }

  .content {
    flex: 1;
    min-width: 0;
    min-height: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    max-height: 100%;
  }

  .content img,
  .content video {
    max-width: 100%;
    max-height: calc(100vh - 120px);
    border-radius: var(--radius-sm);
    object-fit: contain;
  }

  .content iframe {
    width: min(900px, 100%);
    height: calc(100vh - 140px);
    border: none;
    border-radius: var(--radius-sm);
    background: white;
  }

  .audio-wrap {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 12px;
    padding: 24px;
    background: var(--bg-panel);
    border-radius: var(--radius-md);
    border: 1px solid var(--border-subtle);
    min-width: min(360px, 90vw);
  }

  .audio-label {
    font-size: 14px;
    color: var(--text-muted);
  }

  .audio-wrap audio {
    width: 100%;
  }

  .doc-fallback {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 12px;
    padding: 32px;
    background: var(--bg-panel);
    border-radius: var(--radius-md);
    border: 1px solid var(--border-subtle);
    color: var(--text);
    text-align: center;
  }

  .doc-icon {
    font-size: 40px;
  }

  .doc-fallback p {
    margin: 0;
    font-size: 14px;
    word-break: break-all;
  }

  .nav {
    flex-shrink: 0;
    width: 40px;
    height: 40px;
    border: none;
    border-radius: 50%;
    background: color-mix(in srgb, var(--bg-panel) 85%, transparent);
    color: var(--text);
    font-size: 28px;
    line-height: 1;
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .nav:hover {
    background: var(--bg-panel);
  }

  .counter {
    text-align: center;
    font-size: 12px;
    color: rgba(255, 255, 255, 0.75);
  }
</style>
