<script lang="ts">
  import type { LocationPayload } from '$lib/messageKinds';
  import { osmUrl } from '$lib/messageKinds';
  import { getMapTile, openExternal } from '$lib/api';
  import { resolveThumbUrl } from '$lib/messageMedia';
  import { onceInView } from '$lib/inView';

  let { location }: { location: LocationPayload } = $props();

  /** The backend's cached map image; null keeps the static pin card (off, offline). */
  let tileUrl = $state<string | null>(null);

  async function loadTile() {
    const path = await getMapTile(location.lat, location.lng);
    tileUrl = await resolveThumbUrl(path);
  }

  const coords = $derived(`${location.lat.toFixed(5)}, ${location.lng.toFixed(5)}`);
  let copied = $state(false);

  async function copy() {
    try {
      await navigator.clipboard.writeText(`${location.lat},${location.lng}`);
      copied = true;
      setTimeout(() => (copied = false), 1500);
    } catch {
      copied = false;
    }
  }
</script>

<div class="location-card">
  <button
    type="button"
    class="map"
    use:onceInView={loadTile}
    onclick={() => void openExternal(osmUrl(location.lat, location.lng))}
    aria-label="Open this location on a map"
  >
    {#if tileUrl}
      <img src={tileUrl} alt="" draggable="false" />
    {/if}
    <span class="pin" aria-hidden="true">📍</span>
    {#if tileUrl}<span class="credit">© OpenStreetMap</span>{/if}
  </button>
  <div class="meta">
    <span class="title">{location.name ?? (location.live ? 'Live location' : 'Location')}</span>
    {#if location.address}<span class="addr">{location.address}</span>{/if}
    <span class="addr">{coords}</span>
  </div>
  <div class="actions">
    <button type="button" onclick={() => void openExternal(osmUrl(location.lat, location.lng))}>Open map</button>
    <button type="button" onclick={copy}>{copied ? 'Copied' : 'Copy coordinates'}</button>
  </div>
</div>

<style>
  .location-card {
    display: flex;
    flex-direction: column;
    gap: 6px;
    width: 260px;
    max-width: 100%;
  }
  .map {
    position: relative;
    height: 120px;
    padding: 0;
    border: 0;
    border-radius: 8px;
    overflow: hidden;
    cursor: pointer;
    color: inherit;
    background: color-mix(in srgb, currentColor 10%, transparent);
  }
  .map img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
  }
  .pin {
    position: absolute;
    left: 50%;
    top: 50%;
    transform: translate(-50%, -85%);
    font-size: 1.6rem;
    filter: drop-shadow(0 1px 2px rgba(0, 0, 0, 0.4));
  }
  .credit {
    position: absolute;
    right: 4px;
    bottom: 2px;
    font-size: 0.65rem;
    padding: 0 3px;
    border-radius: 3px;
    background: rgba(255, 255, 255, 0.75);
    color: #333;
  }
  .meta {
    display: flex;
    flex-direction: column;
  }
  .title {
    font-weight: 600;
  }
  .addr {
    font-size: 0.85em;
    opacity: 0.75;
    overflow-wrap: anywhere;
  }
  .actions {
    display: flex;
    gap: 6px;
  }
  .actions button {
    flex: 1 1 auto;
    padding: 5px 8px;
    border: 0;
    border-radius: 6px;
    background: color-mix(in srgb, currentColor 12%, transparent);
    color: inherit;
    font: inherit;
    font-size: 0.85em;
    cursor: pointer;
  }
  .actions button:hover {
    background: color-mix(in srgb, currentColor 22%, transparent);
  }
</style>
