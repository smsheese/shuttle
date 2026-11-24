<script lang="ts">
  import { getEmbedHost, getLinkPreview, openExternal, type LinkPreview } from '$lib/api';
  import { resolveThumbUrl } from '$lib/messageMedia';
  import { videoEmbed } from '$lib/messageKinds';
  import { onceInView } from '$lib/inView';
  import Marquee from '$lib/components/Marquee.svelte';

  let { url }: { url: string } = $props();

  const embed = $derived(videoEmbed(url));
  let preview = $state<LinkPreview | null>(null);
  let image = $state<string | null>(null);
  let playing = $state(false);
  let playerSrc = $state('');

  async function play() {
    if (!embed) return;
    const host = await getEmbedHost();
    playerSrc = host
      ? `${host}/embed/${embed.provider}/${embed.id}`
      : `${embed.embedUrl}${embed.embedUrl.includes('?') ? '&' : '?'}autoplay=1`;
    playing = true;
  }

  async function load() {
    const got = await getLinkPreview(url);
    if (!got) return;
    preview = got;
    image = await resolveThumbUrl(got.image_path);
  }

  /** "Oct 25, 2009", in UTC so it matches the date the site itself shows. */
  function dateLabel(iso: string | undefined): string {
    if (!iso) return '';
    const d = new Date(iso);
    return Number.isNaN(d.getTime())
      ? ''
      : d.toLocaleDateString(undefined, { year: 'numeric', month: 'short', day: 'numeric', timeZone: 'UTC' });
  }
  const byline = $derived(
    [preview?.author ?? preview?.site ?? (embed ? (embed.provider === 'youtube' ? 'YouTube' : 'Vimeo') : ''), dateLabel(preview?.published)]
      .filter(Boolean)
      .join(' · ')
  );

  const title = $derived(preview?.title ?? (embed ? (embed.provider === 'youtube' ? 'YouTube video' : 'Vimeo video') : ''));
</script>

<div class="slot" use:onceInView={load}>
  {#if embed}
    <div class="video-card">
      {#if playing}
        <iframe
          class="player"
          src={playerSrc}
          title={title}
          allow="autoplay; encrypted-media; picture-in-picture; fullscreen"
          allowfullscreen
          referrerpolicy="strict-origin-when-cross-origin"
        ></iframe>
      {:else}
        <button type="button" class="poster" onclick={play} aria-label="Play {title}">
          {#if image}<img src={image} alt="" draggable="false" />{/if}
          <span class="play" aria-hidden="true">
            <svg width="26" height="26" viewBox="0 0 24 24" fill="currentColor"><path d="M8 5v14l11-7z" /></svg>
          </span>
        </button>
      {/if}
      <div class="info">
        <Marquee class="title" text={title} />
        {#if preview?.description}<Marquee class="desc" text={preview.description} />{/if}
        <span class="row">
          <span class="site">{byline}</span>
          <button type="button" class="open" onclick={() => void openExternal(embed.watchUrl)}>Open in browser</button>
        </span>
      </div>
    </div>
  {:else if preview}
    <button type="button" class="link-card" onclick={() => void openExternal(url)} title={url}>
      {#if image}<img src={image} alt="" draggable="false" />{/if}
      <span class="text">
        {#if byline}<span class="site">{byline}</span>{/if}
        {#if preview.title}<Marquee class="title" text={preview.title} />{/if}
        {#if preview.description}<Marquee class="desc" text={preview.description} />{/if}
      </span>
    </button>
  {/if}
</div>

<style>
  .link-card {
    display: flex;
    gap: 10px;
    align-items: stretch;
    width: 100%;
    max-width: 340px;
    margin-top: 6px;
    padding: 6px;
    border: 0;
    border-left: 3px solid color-mix(in srgb, currentColor 40%, transparent);
    border-radius: 6px;
    background: color-mix(in srgb, currentColor 9%, transparent);
    color: inherit;
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .link-card:hover {
    background: color-mix(in srgb, currentColor 15%, transparent);
  }
  .link-card img {
    flex: none;
    width: 64px;
    height: 64px;
    border-radius: 4px;
    object-fit: cover;
  }
  .text {
    display: flex;
    flex-direction: column;
    min-width: 0;
    flex: 1;
    justify-content: center;
    gap: 1px;
  }
  .site {
    font-size: 0.75em;
    opacity: 0.7;
  }
  .text :global(.title),
  .info :global(.title) {
    font-weight: 600;
    font-size: 0.92em;
  }
  .text :global(.desc),
  .info :global(.desc) {
    font-size: 0.82em;
    opacity: 0.8;
  }

  .video-card {
    width: 300px;
    max-width: 100%;
    margin-top: 6px;
    border-radius: 8px;
    overflow: hidden;
    background: color-mix(in srgb, currentColor 9%, transparent);
  }
  .poster,
  .player {
    display: block;
    width: 100%;
    aspect-ratio: 16 / 9;
    border: 0;
    padding: 0;
    background: #000;
  }
  .poster {
    position: relative;
    cursor: pointer;
    color: #fff;
  }
  .poster img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
  }
  .play {
    position: absolute;
    inset: 0;
    margin: auto;
    width: 52px;
    height: 52px;
    border-radius: 50%;
    display: grid;
    place-items: center;
    background: rgba(0, 0, 0, 0.65);
  }
  .poster:hover .play {
    background: rgba(200, 0, 0, 0.85);
  }
  .info {
    display: flex;
    flex-direction: column;
    gap: 1px;
    padding: 6px 8px 7px;
    min-width: 0;
  }
  .row {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 8px;
  }
  .open {
    padding: 0;
    border: 0;
    background: none;
    color: inherit;
    font: inherit;
    font-size: 0.75em;
    text-decoration: underline;
    opacity: 0.8;
    cursor: pointer;
  }
</style>
