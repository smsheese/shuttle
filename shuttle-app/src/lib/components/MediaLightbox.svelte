<script lang="ts" module>
  export type LightboxItem = {
    id: string;
    /** Playable/viewable URL. Empty string while the media is not downloaded yet. */
    url: string;
    kind: 'image' | 'video' | 'audio' | 'document';
    filename?: string | null;
    mime?: string | null;
    /** Local file path of the downloaded media (what Copy puts on the clipboard); null when unknown. */
    path?: string | null;
    /** Small preview used by the thumbnail strip (defaults to `url` for images). */
    thumb?: string | null;
    /**
     * `ready` (default) when `url` is usable; `idle` when not downloaded yet (thumbnail plus a
     * Download button); `loading` while downloading; `unavailable` if the download failed.
     */
    status?: 'ready' | 'idle' | 'loading' | 'unavailable';
    /** Message time (RFC 3339); names a copied file like "Work-Mum-261112-1325.jpg". */
    timestamp?: string | null;
    /** The chat's title and the account's name, the two halves before the time. */
    accountName?: string | null;
    chatName?: string | null;
    /** Seconds, shown on video thumbnails when known. */
    duration?: number | null;
    /** Size in bytes when WhatsApp reported it (shown next to the Download button). */
    size?: number | null;
  };
</script>

<script lang="ts">
  import { openExternal } from '$lib/api';
  import { copyCachedMediaToClipboard } from '$lib/mediaSave';
  import { formatFileSize } from '$lib/messageMedia';

  interface Props {
    open: boolean;
    items: LightboxItem[];
    index: number;
    onclose: () => void;
    onindex: (i: number) => void;
    /** Saves the item (downloading it first when needed). Must never navigate the webview. */
    onsave: (item: LightboxItem) => void;
    onedit?: (item: LightboxItem) => void;
    /** Starts downloading a not-downloaded item. */
    ondownload?: (item: LightboxItem) => void;
  }

  let { open, items, index, onclose, onindex, onsave, onedit, ondownload }: Props = $props();

  let pdfFailed = $state(false);
  let stripEl = $state<HTMLElement | null>(null);

  const item = $derived(items[index] ?? null);
  const hasNav = $derived(items.length > 1);
  const ready = $derived(!!item && (item.status ?? 'ready') === 'ready' && !!item.url);
  const isPdf = $derived(
    !!item &&
      item.kind === 'document' &&
      (item.filename?.toLowerCase().endsWith('.pdf') ||
        item.mime?.toLowerCase().includes('pdf') ||
        item.url.toLowerCase().includes('.pdf'))
  );
  const canSave = $derived(!!item && (ready || item.status === 'idle'));
  // Copy needs the file on disk (unlike Save it never downloads first).
  const canCopy = $derived(!!item && ready && !!item.path);
  const canOpenExternal = $derived(
    !!item && (item.url.startsWith('http://') || item.url.startsWith('https://'))
  );

  $effect(() => {
    if (open) pdfFailed = false;
  });

  function step(delta: number) {
    if (items.length < 2) return;
    onindex((index + delta + items.length) % items.length);
  }

  // Keys are handled in the capture phase and stopped so the chat list / page
  // shortcuts underneath never react to Escape or arrows while the viewer is open.
  $effect(() => {
    if (!open) return;
    function onKey(e: KeyboardEvent) {
      if (e.key === 'Escape') {
        e.preventDefault();
        e.stopImmediatePropagation();
        onclose();
      } else if ((e.ctrlKey || e.metaKey) && !e.altKey && !e.shiftKey && e.key.toLowerCase() === 'c') {
        // Ctrl/Cmd+C copies the item unless the user selected text (e.g. the filename).
        if (window.getSelection()?.toString()) return;
        e.preventDefault();
        e.stopImmediatePropagation();
        void copyCurrent();
      } else if ((e.key === 'ArrowLeft' || e.key === 'ArrowRight') && hasNav) {
        if (e.altKey || e.ctrlKey || e.metaKey) return;
        e.preventDefault();
        e.stopImmediatePropagation();
        step(e.key === 'ArrowLeft' ? -1 : 1);
      }
    }
    window.addEventListener('keydown', onKey, true);
    return () => window.removeEventListener('keydown', onKey, true);
  });

  // Keep the active thumbnail visible.
  $effect(() => {
    if (!open || !stripEl) return;
    const id = item?.id;
    if (!id) return;
    const el = stripEl.querySelector<HTMLElement>('[data-active="true"]');
    el?.scrollIntoView({ inline: 'center', block: 'nearest' });
  });

  // Wheel over the strip scrolls it sideways (non-passive so the page behind never scrolls).
  function wheelScroll(node: HTMLElement) {
    function onWheel(e: WheelEvent) {
      if (node.scrollWidth <= node.clientWidth) return;
      const delta = Math.abs(e.deltaX) > Math.abs(e.deltaY) ? e.deltaX : e.deltaY;
      node.scrollLeft += delta;
      e.preventDefault();
    }
    node.addEventListener('wheel', onWheel, { passive: false });
    return { destroy: () => node.removeEventListener('wheel', onWheel) };
  }

  // A click only closes when it both started and ended on the empty backdrop,
  // so a drag/pan that finishes outside the media never counts.
  let downOnBackdrop = false;

  function isBackdrop(target: EventTarget | null): boolean {
    return target instanceof Element && !target.closest('[data-keep]');
  }

  function onPointerDown(e: PointerEvent) {
    downOnBackdrop = isBackdrop(e.target);
  }

  function onPointerUp(e: PointerEvent) {
    const close = downOnBackdrop && isBackdrop(e.target);
    downOnBackdrop = false;
    if (close) onclose();
  }

  function saveCurrent() {
    if (!item || !canSave) return;
    onsave(item);
  }

  async function copyCurrent() {
    if (!item?.path || !canCopy) return;
    await copyCachedMediaToClipboard(item.path, {
      kind: item.kind,
      filename: item.filename,
      timestamp: item.timestamp,
      accountName: item.accountName,
      chatName: item.chatName,
    });
  }

  async function openWithSystem() {
    if (!item || !canOpenExternal) return;
    await openExternal(item.url);
  }

  function formatDuration(sec: number | null | undefined): string {
    if (!sec || sec <= 0 || !Number.isFinite(sec)) return '';
    const s = Math.round(sec);
    return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`;
  }

  function thumbSrc(it: LightboxItem): string | null {
    if (it.thumb) return it.thumb;
    if (it.kind === 'image' && (it.status ?? 'ready') === 'ready' && it.url) return it.url;
    return null;
  }
</script>

{#if open}
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div
    class="lightbox"
    role="dialog"
    aria-modal="true"
    aria-label="Media viewer"
    tabindex="-1"
    onpointerdown={onPointerDown}
    onpointerup={onPointerUp}
  >
    <header class="toolbar" data-keep>
      <span class="title">{item?.filename ?? item?.kind ?? 'Media'}</span>
      {#if hasNav}
        <span class="counter">{index + 1} / {items.length}</span>
      {/if}
      <div class="actions">
        {#if item && ready && item.kind === 'image' && onedit}
          <button type="button" class="btn" onclick={() => onedit?.(item)}>Edit</button>
        {/if}
        <button type="button" class="btn" onclick={copyCurrent} disabled={!canCopy} title="Copy to clipboard (Ctrl/Cmd+C)">Copy</button>
        <button type="button" class="btn" onclick={saveCurrent} disabled={!canSave}>Save</button>
        <button type="button" class="btn close" onclick={onclose} aria-label="Close">×</button>
      </div>
    </header>

    <div class="stage">
      {#if hasNav}
        <button type="button" class="nav prev" data-keep onclick={() => step(-1)} aria-label="Previous">‹</button>
      {/if}

      <div class="content">
        {#if !item}
          <div class="doc-fallback" data-keep>
            <p>Nothing to show.</p>
            <button type="button" class="btn primary" onclick={onclose}>Close</button>
          </div>
        {:else if !ready}
          <div class="pending-media" data-keep>
            {#if item.thumb}
              <img class="pending-thumb" src={item.thumb} alt="" draggable="false" />
            {:else}
              <span class="doc-icon">{item.kind === 'video' ? '🎬' : item.kind === 'image' ? '🖼️' : '📄'}</span>
            {/if}
            <div class="pending-bar">
              {#if item.status === 'loading'}
                <span class="spin" aria-hidden="true"></span>
                <span>Downloading…</span>
              {:else if item.status === 'unavailable'}
                <span>This media could not be downloaded.</span>
                {#if ondownload}
                  <button type="button" class="btn primary" onclick={() => item && ondownload?.(item)}>Retry</button>
                {/if}
              {:else if ondownload}
                <button type="button" class="btn primary" onclick={() => item && ondownload?.(item)}>
                  Download{formatFileSize(item.size) ? ` · ${formatFileSize(item.size)}` : ''}
                </button>
              {:else}
                <span>Not downloaded yet</span>
              {/if}
            </div>
          </div>
        {:else if item.kind === 'image'}
          {#key item.id}
            <img src={item.url} alt={item.filename ?? 'Image'} data-keep draggable="false" />
          {/key}
        {:else if item.kind === 'video'}
          {#key item.id}
            <!-- svelte-ignore a11y_media_has_caption -->
            <video src={item.url} controls data-keep></video>
          {/key}
        {:else if item.kind === 'audio'}
          <div class="audio-wrap" data-keep>
            <span class="audio-label">{item.filename ?? 'Audio'}</span>
            <audio src={item.url} controls></audio>
          </div>
        {:else if isPdf && !pdfFailed}
          <iframe
            src={item.url}
            title={item.filename ?? 'PDF'}
            data-keep
            onerror={() => (pdfFailed = true)}
          ></iframe>
        {:else if isPdf && pdfFailed}
          <div class="doc-fallback" data-keep>
            <p>Could not preview this PDF in Shuttle.</p>
            {#if canOpenExternal}
              <button type="button" class="btn primary" onclick={openWithSystem}>Open with system</button>
            {:else}
              <button type="button" class="btn primary" onclick={saveCurrent}>Save</button>
            {/if}
          </div>
        {:else}
          <div class="doc-fallback" data-keep>
            <span class="doc-icon">📄</span>
            <p>{item.filename ?? 'Document'}</p>
            <button type="button" class="btn primary" onclick={saveCurrent}>Save</button>
          </div>
        {/if}
      </div>

      {#if hasNav}
        <button type="button" class="nav next" data-keep onclick={() => step(1)} aria-label="Next">›</button>
      {/if}
    </div>

    {#if hasNav}
      <div class="strip" data-keep bind:this={stripEl} use:wheelScroll role="listbox" aria-label="Chat media" aria-orientation="horizontal">
        {#each items as it, i (it.id)}
          {@const src = thumbSrc(it)}
          {@const dur = it.kind === 'video' ? formatDuration(it.duration) : ''}
          <button
            type="button"
            class="thumb"
            class:active={i === index}
            class:pending={(it.status ?? 'ready') !== 'ready'}
            data-active={i === index}
            role="option"
            aria-selected={i === index}
            aria-label={it.filename ?? `${it.kind} ${i + 1}`}
            onclick={() => onindex(i)}
          >
            {#if src}
              <img {src} alt="" loading="lazy" decoding="async" draggable="false" />
            {:else}
              <span class="tile" aria-hidden="true">
                {#if it.kind === 'video'}
                  <svg width="18" height="18" viewBox="0 0 24 24" fill="currentColor"><path d="M8 5v14l11-7z" /></svg>
                {:else if it.kind === 'image'}
                  <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8"><rect x="3" y="5" width="18" height="14" rx="2" /><circle cx="8.5" cy="10" r="1.5" /><path d="m21 15-5-5-4 4-2-2-5 5" /></svg>
                {:else}
                  <span>{it.kind === 'audio' ? '🎵' : '📄'}</span>
                {/if}
              </span>
            {/if}
            {#if it.kind === 'video'}
              <span class="badge" aria-hidden="true">
                {#if src}<svg width="10" height="10" viewBox="0 0 24 24" fill="currentColor"><path d="M8 5v14l11-7z" /></svg>{/if}
                {#if dur}<span>{dur}</span>{/if}
              </span>
            {/if}
          </button>
        {/each}
      </div>
    {/if}
  </div>
{/if}

<style>
  .lightbox {
    position: fixed;
    inset: 0;
    z-index: 221;
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 12px;
    background: rgba(0, 0, 0, 0.82);
    outline: none;
    touch-action: manipulation;
  }

  .toolbar {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 8px 12px;
    border-radius: var(--radius-md);
    background: var(--bg-panel);
    border: 1px solid var(--border-subtle);
    color: var(--text);
  }

  .title {
    flex: 1;
    font-size: 13px;
    font-weight: 500;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
  }

  .counter {
    flex-shrink: 0;
    font-size: 12px;
    color: var(--text-muted);
    font-variant-numeric: tabular-nums;
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

  .btn:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .btn.primary {
    background: var(--accent);
    border-color: var(--accent);
    color: var(--text-on-accent, white);
  }

  .btn.close {
    font-size: 20px;
    line-height: 1;
    padding: 2px 10px;
  }

  .stage {
    flex: 1;
    min-height: 0;
    display: flex;
    align-items: stretch;
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
  }

  .content img,
  .content video {
    max-width: 100%;
    max-height: 100%;
    border-radius: var(--radius-sm);
    object-fit: contain;
    user-select: none;
  }

  .content iframe {
    width: min(900px, 100%);
    height: 100%;
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
    min-width: min(360px, 80vw);
  }

  .audio-label {
    font-size: 14px;
    color: var(--text-muted);
  }

  .audio-wrap audio {
    width: 100%;
  }

  .pending-media {
    position: relative;
    display: grid;
    place-items: center;
    min-width: min(360px, 80vw);
    min-height: 240px;
    max-width: 80vw;
    max-height: 70vh;
    overflow: hidden;
    border-radius: var(--radius-md);
    background: var(--bg-panel);
    color: var(--text);
  }
  .pending-thumb {
    max-width: 80vw;
    max-height: 70vh;
    min-width: 240px;
    object-fit: contain;
    /* WhatsApp previews are tiny; upscaling them blurry looks intentional rather than broken. */
    filter: blur(6px);
    transform: scale(1.04);
  }
  .pending-bar {
    position: absolute;
    inset: auto 0 0 0;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 10px;
    padding: 12px;
    background: linear-gradient(transparent, rgba(0, 0, 0, 0.65));
    color: #fff;
    font-size: 0.9rem;
  }
  .spin {
    width: 16px;
    height: 16px;
    border: 2px solid rgba(255, 255, 255, 0.35);
    border-top-color: #fff;
    border-radius: 50%;
    animation: lb-spin 0.8s linear infinite;
  }
  @keyframes lb-spin {
    to {
      transform: rotate(360deg);
    }
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
    max-width: 100%;
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
    align-self: center;
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

  .strip {
    flex-shrink: 0;
    display: flex;
    gap: 6px;
    align-self: center;
    max-width: 100%;
    padding: 6px;
    overflow-x: auto;
    overflow-y: hidden;
    border-radius: var(--radius-md);
    background: color-mix(in srgb, var(--bg-panel) 85%, transparent);
    border: 1px solid var(--border-subtle);
    scrollbar-width: thin;
    overscroll-behavior: contain;
  }

  .thumb {
    position: relative;
    flex: 0 0 auto;
    width: 56px;
    height: 56px;
    padding: 0;
    overflow: hidden;
    border: 2px solid transparent;
    border-radius: var(--radius-sm);
    background: var(--bg-input);
    color: var(--text-muted);
    cursor: pointer;
    opacity: 0.7;
  }

  .thumb:hover {
    opacity: 1;
  }

  .thumb.active {
    border-color: var(--accent);
    opacity: 1;
  }

  .thumb.pending img {
    opacity: 0.5;
  }

  .thumb img {
    display: block;
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  .tile {
    display: flex;
    width: 100%;
    height: 100%;
    align-items: center;
    justify-content: center;
    font-size: 18px;
  }

  .badge {
    position: absolute;
    left: 3px;
    bottom: 3px;
    display: inline-flex;
    align-items: center;
    gap: 3px;
    padding: 1px 4px;
    border-radius: 4px;
    background: rgba(0, 0, 0, 0.65);
    color: #fff;
    font-size: 10px;
    line-height: 1.2;
    font-variant-numeric: tabular-nums;
  }

  .badge:empty {
    display: none;
  }

  @media (max-width: 640px) {
    .lightbox {
      padding: 8px;
      gap: 6px;
    }

    .nav {
      width: 32px;
      height: 32px;
      font-size: 22px;
    }

    .stage {
      gap: 4px;
    }

    .thumb {
      width: 48px;
      height: 48px;
    }
  }
</style>
