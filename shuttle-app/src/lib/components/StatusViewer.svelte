<script lang="ts">
  import { untrack } from 'svelte';
  import { avatarColor, conversationAvatar, getInitials, openExternal } from '$lib/api';
  import { inlineSrc, isBareUrl, isEmojiOnlyText, linkPreviewHost, linkPreviewTitle, orderedPosts, safeExternalUrl } from '$lib/statusPreview';
  import type { Conversation, StatusItem, StatusPost } from '$lib/types';

  interface Props {
    open: boolean;
    feed: StatusItem[];
    startSenderId: string;
    startAccountId?: string;
    conversations?: Conversation[];
    onclose: () => void;
    /** Resolve a playable media URL for a status post (data URL / blob). */
    requestMedia: (accountId: string, messageId: string) => Promise<string | null>;
    /** Resolve a post's cached preview image, drawn while the full media loads. */
    resolveThumb?: (path: string) => Promise<string | null>;
  }

  let {
    open,
    feed,
    startSenderId,
    startAccountId,
    conversations = [],
    onclose,
    requestMedia,
    resolveThumb,
  }: Props = $props();

  const IMAGE_MS = 5000;
  const TEXT_MS = 5500;

  let senderIndex = $state(0);
  let postIndex = $state(0);
  let progress = $state(0);
  let paused = $state(false);
  let loading = $state(false);
  let mediaUrl = $state<string | null>(null);
  let previewUrl = $state<string | null>(null);
  let loadError = $state<string | null>(null);
  let videoEl = $state<HTMLVideoElement | null>(null);

  let raf = 0;
  let startedAt = 0;
  let elapsedBeforePause = 0;
  let holdTimer: ReturnType<typeof setTimeout> | undefined;
  let holding = false;
  let mediaToken = 0;
  let wasOpen = false;

  const sender = $derived(feed[senderIndex] ?? null);
  const posts = $derived(normalizePosts(sender));
  const post = $derived(posts[postIndex] ?? null);
  const mediaKind = $derived(normalizeKind(post?.media_type));
  const link = $derived(post?.link_preview ?? null);
  const linkUrl = $derived(safeExternalUrl(link?.url));
  const linkHost = $derived(linkPreviewHost(link?.url));
  const linkTitle = $derived(linkPreviewTitle(link));
  const linkOnly = $derived(!!link && isBareUrl(post?.text));
  const emojiOnly = $derived(isEmojiOnlyText(post?.text));

  function normalizePosts(item: StatusItem | null): StatusPost[] {
    if (!item) return [];
    const ordered = orderedPosts(item);
    if (ordered.length > 0) return ordered;
    return [
      {
        id: '',
        media_type: item.media_type || 'text',
        text: item.preview || '',
        timestamp: item.timestamp,
      },
    ];
  }

  function normalizeKind(raw: string | undefined): 'image' | 'video' | 'text' | 'other' {
    const k = (raw || 'text').toLowerCase();
    if (k === 'image' || k === 'photo' || k === 'sticker') return 'image';
    if (k === 'video') return 'video';
    if (k === 'text' || !k) return 'text';
    return 'other';
  }

  function senderAvatar(item: StatusItem): string | null {
    const conv = conversations.find((c) => c.remote_id === item.sender_id && (!item.account_id || c.account_id === item.account_id));
    return conv ? conversationAvatar(conv) : null;
  }

  function stopTimer() {
    if (raf) cancelAnimationFrame(raf);
    raf = 0;
  }

  function durationFor(kind: string): number {
    return kind === 'text' || kind === 'other' ? TEXT_MS : IMAGE_MS;
  }

  function tick() {
    if (paused || !open) return;
    const kind = mediaKind;
    if (kind === 'video') return;
    const total = durationFor(kind);
    const elapsed = elapsedBeforePause + (performance.now() - startedAt);
    progress = Math.min(1, elapsed / total);
    if (progress >= 1) {
      void goNext();
      return;
    }
    raf = requestAnimationFrame(tick);
  }

  function startProgress() {
    stopTimer();
    progress = 0;
    elapsedBeforePause = 0;
    startedAt = performance.now();
    if (mediaKind === 'video') return;
    raf = requestAnimationFrame(tick);
  }

  function pause() {
    if (paused) return;
    paused = true;
    if (mediaKind !== 'video') {
      elapsedBeforePause += performance.now() - startedAt;
      stopTimer();
    } else {
      videoEl?.pause();
    }
  }

  function resume() {
    if (!paused) return;
    paused = false;
    if (mediaKind !== 'video') {
      startedAt = performance.now();
      raf = requestAnimationFrame(tick);
    } else {
      void videoEl?.play().catch(() => {});
    }
  }

  /** Draw the cached preview at once, so a status is never a blank stage while media loads. */
  function loadPreview(token: number, current: StatusPost) {
    previewUrl = null;
    const path = current.thumbnail_path ?? current.link_preview?.thumbnail_path;
    const inline = inlineSrc(path);
    if (inline) {
      previewUrl = inline;
      return;
    }
    if (!path || !resolveThumb) return;
    void resolveThumb(path).then((url) => {
      if (token === mediaToken) previewUrl = url;
    });
  }

  async function loadCurrentMedia() {
    const token = ++mediaToken;
    mediaUrl = null;
    loadError = null;
    const current = post;
    const item = sender;
    if (!current || !item) {
      previewUrl = null;
      return;
    }
    loadPreview(token, current);

    const kind = normalizeKind(current.media_type);
    if (kind === 'text' || !current.id) {
      loading = false;
      startProgress();
      return;
    }

    loading = true;
    const accountId = item.account_id;
    if (!accountId) {
      loading = false;
      loadError = 'Missing account';
      startProgress();
      return;
    }

    try {
      const url = await requestMedia(accountId, current.id);
      if (token !== mediaToken) return;
      if (!url) {
        loadError = 'Could not load media';
        loading = false;
        startProgress();
        return;
      }
      mediaUrl = url;
      loading = false;
      if (kind === 'video') {
        progress = 0;
      } else {
        startProgress();
      }
    } catch {
      if (token !== mediaToken) return;
      loadError = 'Could not load media';
      loading = false;
      startProgress();
    }
  }

  function resetTo(senderIdx: number, postIdx: number) {
    stopTimer();
    senderIndex = senderIdx;
    postIndex = postIdx;
    progress = 0;
    paused = false;
    void loadCurrentMedia();
  }

  async function goNext() {
    stopTimer();
    if (postIndex + 1 < posts.length) {
      resetTo(senderIndex, postIndex + 1);
      return;
    }
    if (senderIndex + 1 < feed.length) {
      resetTo(senderIndex + 1, 0);
      return;
    }
    onclose();
  }

  function goPrev() {
    stopTimer();
    if (progress > 0.15 || (mediaKind === 'video' && (videoEl?.currentTime ?? 0) > 1)) {
      // Restart current story if already progressed.
      resetTo(senderIndex, postIndex);
      return;
    }
    if (postIndex > 0) {
      resetTo(senderIndex, postIndex - 1);
      return;
    }
    if (senderIndex > 0) {
      const prevPosts = normalizePosts(feed[senderIndex - 1]);
      resetTo(senderIndex - 1, Math.max(0, prevPosts.length - 1));
      return;
    }
    resetTo(senderIndex, postIndex);
  }

  function onVideoMeta() {
    if (!videoEl || mediaKind !== 'video') return;
    void videoEl.play().catch(() => {});
  }

  function onVideoTime() {
    if (!videoEl || mediaKind !== 'video' || paused) return;
    const dur = videoEl.duration;
    if (!Number.isFinite(dur) || dur <= 0) return;
    progress = Math.min(1, videoEl.currentTime / dur);
  }

  function onVideoEnded() {
    void goNext();
  }

  function onPointerDown(e: PointerEvent) {
    if (e.button !== 0) return;
    holding = false;
    clearTimeout(holdTimer);
    holdTimer = setTimeout(() => {
      holding = true;
      pause();
    }, 180);
  }

  function onPointerUp(e: PointerEvent) {
    clearTimeout(holdTimer);
    if (holding) {
      holding = false;
      resume();
      return;
    }
    const target = e.currentTarget as HTMLElement;
    const rect = target.getBoundingClientRect();
    const x = e.clientX - rect.left;
    if (x < rect.width * 0.33) goPrev();
    else void goNext();
  }

  function onPointerCancel() {
    clearTimeout(holdTimer);
    if (holding) {
      holding = false;
      resume();
    }
  }

  // A feed refresh can drop the post being shown; land on the last one instead of a blank viewer.
  $effect(() => {
    const count = posts.length;
    if (count > 0 && postIndex > count - 1) resetTo(senderIndex, count - 1);
  });

  $effect(() => {
    if (!open) {
      wasOpen = false;
      stopTimer();
      mediaToken += 1;
      mediaUrl = null;
      previewUrl = null;
      return;
    }
    // Only (re)start when opening, not when the feed refreshes mid-view.
    if (wasOpen) return;
    wasOpen = true;
    untrack(() => {
      const idx = feed.findIndex((s) => s.sender_id === startSenderId && s.account_id === startAccountId);
      resetTo(Math.max(0, idx), 0);
    });
    return () => stopTimer();
  });

  $effect(() => {
    if (!open) return;
    function onKey(e: KeyboardEvent) {
      if (e.key === 'Escape') onclose();
      else if (e.key === 'ArrowRight') void goNext();
      else if (e.key === 'ArrowLeft') goPrev();
      else if (e.key === ' ') {
        e.preventDefault();
        if (paused) resume();
        else pause();
      }
    }
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  });
</script>

{#if open && sender && post}
  <div class="viewer" role="dialog" aria-modal="true" aria-label="Status viewer">
    <div class="chrome">
      <div class="segments" aria-hidden="true">
        {#each posts as _, i}
          <div class="seg">
            <div
              class="seg-fill"
              style="width: {i < postIndex ? 100 : i === postIndex ? progress * 100 : 0}%"
            ></div>
          </div>
        {/each}
      </div>

      <header class="header">
        <div class="who">
          <span class="avatar" style="background: {avatarColor(sender.sender_name)}">
            {#if senderAvatar(sender)}
              <img src={senderAvatar(sender) ?? ''} alt="" />
            {:else}
              {getInitials(sender.sender_name)}
            {/if}
          </span>
          <div class="meta">
            <span class="name">{sender.sender_name}</span>
            {#if post.timestamp}
              <span class="time">{new Date(post.timestamp).toLocaleString()}</span>
            {/if}
          </div>
        </div>
        <button type="button" class="close" onclick={onclose} aria-label="Close">×</button>
      </header>
    </div>

    <div
      class="stage"
      role="presentation"
      onpointerdown={onPointerDown}
      onpointerup={onPointerUp}
      onpointercancel={onPointerCancel}
      onpointerleave={onPointerCancel}
    >
      {#if loading && !previewUrl}
        <div class="state">Loading…</div>
      {:else if loadError && !mediaUrl && mediaKind !== 'text'}
        <div class="state">
          <p>{loadError}</p>
          {#if post.text}
            <p class="caption-fallback">{post.text}</p>
          {/if}
        </div>
      {:else if mediaKind === 'video' && mediaUrl}
        <!-- svelte-ignore a11y_media_has_caption -->
        <video
          bind:this={videoEl}
          class="media"
          src={mediaUrl}
          playsinline
          autoplay
          onloadedmetadata={onVideoMeta}
          ontimeupdate={onVideoTime}
          onended={onVideoEnded}
        ></video>
      {:else if mediaKind === 'image' && mediaUrl}
        <img class="media" src={mediaUrl} alt={post.text || 'Status'} />
      {:else if previewUrl && link}
        <div class="link-card">
          <div class="link-shot">
            <img class="link-backdrop" src={previewUrl} alt="" aria-hidden="true" />
            <img class="link-image" src={previewUrl} alt={linkTitle} />
          </div>
          <div class="link-body">
            <span class="link-host">{linkHost}</span>
            {#if linkTitle !== linkHost}
              <p class="link-title">{linkTitle}</p>
            {/if}
          </div>
        </div>
      {:else if previewUrl}
        <img class="media" src={previewUrl} alt={post.text || 'Status'} />
      {:else if link}
        <div class="link-card link-card-bare">
          <div class="link-body">
            <span class="link-host">{linkHost}</span>
            {#if linkTitle !== linkHost}
              <p class="link-title">{linkTitle}</p>
            {/if}
          </div>
        </div>
      {:else if loadError && mediaKind !== 'text'}
        <div class="state">
          <p>{loadError}</p>
          {#if post.text}
            <p class="caption-fallback">{post.text}</p>
          {/if}
        </div>
      {:else}
        <div class="text-card" class:emoji={emojiOnly} style="background: {avatarColor(sender.sender_name)}">
          <p>{post.text || sender.preview || 'Status'}</p>
        </div>
      {/if}

      {#if linkUrl}
        <button
          type="button"
          class="link-chip"
          title={link?.url}
          onpointerdown={(e) => e.stopPropagation()}
          onpointerup={(e) => e.stopPropagation()}
          onclick={(e) => {
            e.stopPropagation();
            if (linkUrl) void openExternal(linkUrl);
          }}
        >
          <svg viewBox="0 0 24 24" width="13" height="13" aria-hidden="true">
            <path
              d="M10.6 13.4a1 1 0 0 1 0-1.4l3.5-3.5a1 1 0 0 1 1.4 1.4l-3.5 3.5a1 1 0 0 1-1.4 0Zm-2 6.6a4.6 4.6 0 0 1-3.3-7.8l2.1-2.1a1 1 0 1 1 1.4 1.4l-2.1 2.1a2.6 2.6 0 0 0 3.7 3.7l2.1-2.1a1 1 0 1 1 1.4 1.4l-2.1 2.1a4.6 4.6 0 0 1-3.2 1.3Zm10.6-3.3a1 1 0 0 1-.7-.3 1 1 0 0 1 0-1.4l2.1-2.1a2.6 2.6 0 0 0-3.7-3.7l-2.1 2.1a1 1 0 0 1-1.4-1.4l2.1-2.1a4.6 4.6 0 0 1 6.5 6.5l-2.1 2.1a1 1 0 0 1-.7.3Z"
              fill="currentColor"
            />
          </svg>
          <span>{linkHost}</span>
        </button>
      {/if}

      {#if post.text && mediaKind !== 'text' && mediaUrl && !linkOnly}
        <div class="caption">{post.text}</div>
      {/if}
    </div>
  </div>
{/if}

<style>
  .viewer {
    position: fixed;
    inset: 0;
    z-index: 260;
    background: #0a0a0b;
    color: #fff;
    display: flex;
    flex-direction: column;
    user-select: none;
    touch-action: none;
  }

  .chrome {
    position: absolute;
    top: 0;
    left: 0;
    right: 0;
    z-index: 2;
    padding: calc(10px + env(safe-area-inset-top, 0px)) 12px 0;
    background: linear-gradient(to bottom, rgba(0, 0, 0, 0.55), transparent);
    pointer-events: none;
  }

  .segments {
    display: flex;
    gap: 4px;
    margin-bottom: 10px;
    pointer-events: none;
  }

  .seg {
    flex: 1;
    height: 3px;
    border-radius: 999px;
    background: rgba(255, 255, 255, 0.28);
    overflow: hidden;
  }

  .seg-fill {
    height: 100%;
    background: #fff;
    border-radius: inherit;
  }

  .header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    pointer-events: auto;
    padding-bottom: 12px;
  }

  .who {
    display: flex;
    align-items: center;
    gap: 10px;
    min-width: 0;
  }

  .avatar {
    width: 36px;
    height: 36px;
    border-radius: 50%;
    display: grid;
    place-items: center;
    font-size: 12px;
    font-weight: 700;
    overflow: hidden;
    flex-shrink: 0;
  }

  .avatar img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  .meta {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }

  .name {
    font-size: 14px;
    font-weight: 600;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .time {
    font-size: 11px;
    color: rgba(255, 255, 255, 0.7);
  }

  .close {
    border: none;
    background: rgba(255, 255, 255, 0.12);
    color: #fff;
    width: 36px;
    height: 36px;
    border-radius: 50%;
    font-size: 22px;
    line-height: 1;
    cursor: pointer;
    flex-shrink: 0;
  }

  .stage {
    flex: 1;
    min-height: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    position: relative;
    cursor: pointer;
  }

  .media {
    max-width: 100%;
    max-height: 100%;
    width: 100%;
    height: 100%;
    object-fit: contain;
    background: #000;
  }

  .text-card {
    width: min(420px, 92vw);
    min-height: min(520px, 70vh);
    border-radius: 16px;
    display: grid;
    place-items: center;
    padding: 28px;
    text-align: center;
  }

  .text-card p {
    margin: 0;
    font-size: clamp(22px, 4.5vw, 32px);
    font-weight: 650;
    line-height: 1.3;
    color: #fff;
    text-shadow: 0 1px 2px rgba(0, 0, 0, 0.25);
    word-break: break-word;
  }

  .text-card.emoji {
    min-height: 0;
    background: transparent !important;
  }

  .text-card.emoji p {
    font-size: clamp(64px, 18vw, 140px);
    line-height: 1.05;
  }

  .link-card {
    width: min(380px, 88vw);
    max-height: min(78vh, 620px);
    display: flex;
    flex-direction: column;
    border-radius: 18px;
    overflow: hidden;
    background: rgba(255, 255, 255, 0.06);
    border: 1px solid rgba(255, 255, 255, 0.14);
    box-shadow: 0 18px 48px rgba(0, 0, 0, 0.45);
  }

  .link-card-bare {
    padding: 26px 22px;
    justify-content: center;
    min-height: 150px;
  }

  .link-shot {
    position: relative;
    flex: 1 1 auto;
    min-height: 0;
    aspect-ratio: 4 / 5;
    max-height: min(58vh, 480px);
    overflow: hidden;
    background: #000;
  }

  .link-backdrop {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    object-fit: cover;
    filter: blur(18px) brightness(0.5);
    transform: scale(1.12);
  }

  .link-image {
    position: relative;
    width: 100%;
    height: 100%;
    object-fit: contain;
  }

  .link-body {
    flex: 0 0 auto;
    padding: 12px 16px 16px;
    text-align: left;
  }

  .link-host {
    font-size: 11px;
    letter-spacing: 0.04em;
    text-transform: lowercase;
    color: rgba(255, 255, 255, 0.62);
  }

  .link-title {
    margin: 4px 0 0;
    font-size: 15px;
    font-weight: 600;
    line-height: 1.3;
    color: #fff;
    display: -webkit-box;
    -webkit-line-clamp: 3;
    line-clamp: 3;
    -webkit-box-orient: vertical;
    overflow: hidden;
    word-break: break-word;
  }

  .link-chip {
    position: absolute;
    left: 50%;
    bottom: calc(18px + env(safe-area-inset-bottom, 0px));
    transform: translateX(-50%);
    display: inline-flex;
    align-items: center;
    gap: 6px;
    max-width: calc(100vw - 32px);
    padding: 8px 14px;
    border-radius: 999px;
    border: 1px solid rgba(255, 255, 255, 0.22);
    background: rgba(255, 255, 255, 0.16);
    backdrop-filter: blur(10px);
    color: #fff;
    font-size: 13px;
    font-weight: 550;
    cursor: pointer;
    z-index: 3;
  }

  .link-chip span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .link-chip:hover {
    background: rgba(255, 255, 255, 0.28);
  }

  .caption {
    position: absolute;
    left: 0;
    right: 0;
    bottom: 0;
    padding: 48px 18px calc(20px + env(safe-area-inset-bottom, 0px));
    background: linear-gradient(to top, rgba(0, 0, 0, 0.65), transparent);
    font-size: 15px;
    line-height: 1.35;
    text-align: center;
    pointer-events: none;
  }

  .state {
    color: rgba(255, 255, 255, 0.85);
    font-size: 15px;
    text-align: center;
    padding: 24px;
  }

  .caption-fallback {
    margin-top: 12px;
    opacity: 0.85;
  }
</style>
