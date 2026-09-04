<script lang="ts">
  import { avatarColor, conversationAvatar, getInitials } from '$lib/api';
  import type { Conversation, StatusItem, StatusPost } from '$lib/types';

  interface Props {
    open: boolean;
    feed: StatusItem[];
    startSenderId: string;
    conversations?: Conversation[];
    onclose: () => void;
    /** Resolve a playable media URL for a status post (data URL / blob). */
    requestMedia: (accountId: string, messageId: string) => Promise<string | null>;
  }

  let {
    open,
    feed,
    startSenderId,
    conversations = [],
    onclose,
    requestMedia,
  }: Props = $props();

  const IMAGE_MS = 5000;
  const TEXT_MS = 5500;

  let senderIndex = $state(0);
  let postIndex = $state(0);
  let progress = $state(0);
  let paused = $state(false);
  let loading = $state(false);
  let mediaUrl = $state<string | null>(null);
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

  function normalizePosts(item: StatusItem | null): StatusPost[] {
    if (!item) return [];
    if (item.posts && item.posts.length > 0) return item.posts;
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
    const conv = conversations.find((c) => c.remote_id === item.sender_id);
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

  async function loadCurrentMedia() {
    const token = ++mediaToken;
    mediaUrl = null;
    loadError = null;
    const current = post;
    const item = sender;
    if (!current || !item) return;

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

  $effect(() => {
    if (!open) {
      wasOpen = false;
      stopTimer();
      mediaToken += 1;
      mediaUrl = null;
      return;
    }
    // Only (re)start when opening, not when the feed refreshes mid-view.
    if (wasOpen) return;
    wasOpen = true;
    const idx = Math.max(
      0,
      feed.findIndex((s) => s.sender_id === startSenderId)
    );
    resetTo(idx >= 0 ? idx : 0, 0);
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
      {#if loading}
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
      {:else}
        <div class="text-card" style="background: {avatarColor(sender.sender_name)}">
          <p>{post.text || sender.preview || 'Status'}</p>
        </div>
      {/if}

      {#if post.text && mediaKind !== 'text' && mediaUrl}
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
