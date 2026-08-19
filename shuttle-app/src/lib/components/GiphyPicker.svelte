<script lang="ts">
  import {
    giphyConfigured,
    searchGifs,
    searchStickers,
    PAGE_SIZE,
    type GiphyItem,
  } from '$lib/giphy';

  interface Props {
    mode: 'gif' | 'sticker';
    onpick: (item: GiphyItem) => void;
  }

  let { mode, onpick }: Props = $props();

  let query = $state('');
  let items = $state<GiphyItem[]>([]);
  let loading = $state(false);
  let loadingMore = $state(false);
  let hasMore = $state(true);
  let offset = $state(0);
  let timer: ReturnType<typeof setTimeout> | undefined;
  let scrollEl: HTMLDivElement | undefined = $state();

  async function fetchPage(q: string, off: number): Promise<GiphyItem[]> {
    return mode === 'gif' ? await searchGifs(q, off) : await searchStickers(q, off);
  }

  async function loadInitial(q: string) {
    loading = true;
    offset = 0;
    hasMore = true;
    try {
      const data = await fetchPage(q, 0);
      items = data;
      offset = data.length;
      hasMore = data.length >= PAGE_SIZE;
    } finally {
      loading = false;
    }
    if (scrollEl) scrollEl.scrollTop = 0;
  }

  async function loadMore() {
    if (loadingMore || !hasMore) return;
    loadingMore = true;
    try {
      const data = await fetchPage(query, offset);
      items = [...items, ...data];
      offset += data.length;
      if (data.length < PAGE_SIZE) hasMore = false;
    } finally {
      loadingMore = false;
    }
  }

  function onScroll(e: Event) {
    const el = e.target as HTMLDivElement;
    if (el.scrollTop + el.clientHeight >= el.scrollHeight - 40) {
      loadMore();
    }
  }

  $effect(() => {
    mode;
    clearTimeout(timer);
    timer = setTimeout(() => void loadInitial(query), query ? 280 : 0);
    return () => clearTimeout(timer);
  });
</script>

<div class="giphy-picker">
  {#if !giphyConfigured()}
    <p class="hint">Add <code>VITE_GIPHY_API_KEY</code> to enable Giphy search.</p>
  {:else}
    <input
      type="search"
      class="giphy-search"
      placeholder={mode === 'gif' ? 'Search GIFs' : 'Search stickers'}
      bind:value={query}
    />
    {#if loading}
      <p class="hint">Loading…</p>
    {:else if items.length === 0}
      <p class="hint">No results</p>
    {:else}
      <div class="giphy-scroll" bind:this={scrollEl} onscroll={onScroll}>
        <div class="giphy-grid">
          {#each items as item (item.id)}
            <button type="button" class="giphy-cell" onclick={() => onpick(item)} aria-label={item.title}>
              <img src={item.previewUrl} alt={item.title} loading="lazy" />
            </button>
          {/each}
        </div>
        {#if loadingMore}
          <p class="hint">Loading more…</p>
        {/if}
      </div>
    {/if}
  {/if}
</div>

<style>
  .giphy-picker {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .giphy-search {
    width: 100%;
    border: 1px solid var(--border-subtle);
    border-radius: 8px;
    padding: 5px 8px;
    font-size: 12px;
    background: var(--bg-input, var(--bg-main));
    color: var(--text-primary);
    box-sizing: border-box;
  }

  .giphy-scroll {
    max-height: 260px;
    overflow-y: auto;
    overscroll-behavior: contain;
  }

  .giphy-grid {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 4px;
  }

  .giphy-cell {
    border: none;
    padding: 0;
    background: var(--bg-hover);
    border-radius: 6px;
    overflow: hidden;
    cursor: pointer;
    aspect-ratio: 1;
  }

  .giphy-cell img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
  }

  .hint {
    margin: 0;
    font-size: 12px;
    color: var(--text-muted);
    text-align: center;
    padding: 12px 4px;
  }

  .hint code {
    font-size: 11px;
  }
</style>
