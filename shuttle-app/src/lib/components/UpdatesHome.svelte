<script lang="ts">
  import { avatarColor, conversationAvatar, formatTime, getInitials } from '$lib/api';
  import { displayNameFor } from '$lib/displayName';
  import { isBareUrl, isEmojiOnlyText, linkPreviewHost, orderedPosts } from '$lib/statusPreview';
  import { listPreview } from '$lib/messageMedia';
  import PreviewDirection from '$lib/components/PreviewDirection.svelte';
  import NetworkIcon from '$lib/components/NetworkIcon.svelte';
  function marquee(node: HTMLElement) {
    const text = node.firstElementChild as HTMLElement;
    const update = () => {
      const distance = Math.max(0, text.scrollWidth - node.clientWidth);
      node.style.setProperty('--marquee-distance', `${distance}px`);
      node.style.setProperty('--marquee-duration', `${Math.max(5, distance / 28 + 3)}s`);
      node.classList.toggle('overflowing', distance > 1);
    };
    const observer = new ResizeObserver(update);
    observer.observe(node);
    observer.observe(text);
    update();
    return { destroy: () => observer.disconnect() };
  }
  import { CONNECTOR_COLORS, type Account, type Conversation, type StatusItem } from '$lib/types';

  interface Props {
    statuses: StatusItem[];
    /** Channel conversations (conversation_type === 'channel'). */
    channels: Conversation[];
    /** All loaded conversations, used to find a status sender's avatar. */
    conversations: Conversation[];
    accounts: Account[];
    /** Show the network badge on channel avatars (all-accounts view). */
    showNetwork?: boolean;
    datetimeFormat?: string;
    channelColor?: (connectorId: string) => string;
    /** Resolve an already-downloaded image URL for the sender's latest status, if any. */
    statusThumb?: (item: StatusItem) => string | null;
    onstatus: (item: StatusItem) => void;
    onselect: (id: string) => void;
    oncontext?: (conv: Conversation, x: number, y: number) => void;
    /** Show the subtle "Select a conversation" hint. */
    showHint?: boolean;
  }

  let {
    statuses,
    channels,
    conversations,
    accounts,
    showNetwork = false,
    datetimeFormat = '12h_full',
    channelColor,
    statusThumb,
    onstatus,
    onselect,
    oncontext,
    showHint = true,
  }: Props = $props();

  let cardsEl = $state<HTMLDivElement | undefined>();

  function senderAvatar(item: StatusItem): string | null {
    const conv = conversations.find(
      (c) => c.remote_id === item.sender_id && (!item.account_id || c.account_id === item.account_id)
    );
    return conv ? conversationAvatar(conv) : null;
  }

  /** Text of a text-only status, shown on the card when there is no image behind it. */
  function textBody(item: StatusItem): string {
    const post = orderedPosts(item).find((p) => (p.text ?? '').trim());
    const kind = (post?.media_type ?? item.media_type ?? 'text').toLowerCase();
    if (kind !== 'text') return '';
    const text = (post?.text || item.preview || '').trim();
    if (!text) return '';
    return isBareUrl(text) ? linkPreviewHost(post?.link_preview?.url ?? text) : text.slice(0, 90);
  }

  function onCardsWheel(e: WheelEvent) {
    if (!cardsEl) return;
    if (Math.abs(e.deltaY) <= Math.abs(e.deltaX)) return;
    e.preventDefault();
    cardsEl.scrollLeft += e.deltaY;
  }

  function accountFor(conv: Conversation): Account | undefined {
    return accounts.find((a) => a.id === conv.account_id);
  }
</script>

<section class="updates" aria-label="Updates">
  <div class="inner">
    <h1>Updates</h1>

    {#if statuses.length > 0}
      <section class="block" aria-labelledby="updates-status-h">
        <h2 id="updates-status-h">Status</h2>
        <div class="cards" bind:this={cardsEl} role="group" aria-label="Status updates" onwheel={onCardsWheel}>
          {#each statuses as item (`${item.account_id ?? ''}:${item.sender_id}`)}
            {@const thumb = statusThumb?.(item) ?? null}
            {@const avatar = senderAvatar(item)}
            {@const text = textBody(item)}
            {@const emojiOnly = isEmojiOnlyText(text)}
            <button
              type="button"
              class="card"
              class:mine={item.from_me}
              style="--card-color: {avatarColor(item.sender_name)}"
              title={item.sender_name}
              onclick={() => onstatus(item)}
            >
              {#if thumb}
                <img class="card-bg" src={thumb} alt="" loading="lazy" decoding="async" />
              {:else if avatar}
                <img class="card-bg card-bg-blur" src={avatar} alt="" loading="lazy" decoding="async" />
              {/if}
              {#if text && !thumb}
                <span class="card-text" class:emoji={emojiOnly}>{text}</span>
              {/if}
              <span class="card-shade" aria-hidden="true"></span>
              {#if (item.count ?? 0) > 1}
                <span class="card-count">{item.count}</span>
              {/if}
              <span class="card-name" use:marquee><span>{item.from_me ? 'My status' : item.sender_name}</span></span>
            </button>
          {/each}
        </div>
      </section>
    {/if}

    {#if channels.length > 0}
      <section class="block" aria-labelledby="updates-channels-h">
        <h2 id="updates-channels-h">Channels</h2>
        <div class="channels" role="group" aria-label="Channels">
          {#each channels as conv (conv.id)}
            {@const account = accountFor(conv)}
            {@const name = displayNameFor(conv, conversations)}
            <button
              type="button"
              class="channel"
              class:unread={conv.unread_count > 0}
              class:muted={conv.muted}
              onclick={() => onselect(conv.id)}
              oncontextmenu={(e) => {
                e.preventDefault();
                oncontext?.(conv, e.clientX, e.clientY);
              }}
            >
              <span class="ch-avatar-wrap">
                <span class="ch-avatar" style="background: {avatarColor(name)}">
                  {#if conversationAvatar(conv)}
                    <img src={conversationAvatar(conv) ?? ''} alt="" loading="lazy" decoding="async" />
                  {:else}
                    {getInitials(name)}
                  {/if}
                </span>
                {#if showNetwork && account}
                  <span
                    class="ch-network"
                    style="background: {channelColor?.(account.connector_id) ?? CONNECTOR_COLORS[account.connector_id] ?? '#888'}"
                    title={account.name}
                  >
                    <NetworkIcon connectorId={account.connector_id} size={10} />
                  </span>
                {/if}
              </span>
              <span class="ch-body">
                <span class="ch-top">
                  <span class="ch-title">{name}</span>
                  <span class="ch-time">{formatTime(conv.last_message_at, datetimeFormat)}</span>
                </span>
                <span class="ch-bottom">
                  {#if listPreview(conv.last_message_preview) && conv.last_message_from_me != null}
                    <PreviewDirection fromMe={conv.last_message_from_me} />
                  {/if}
                  <span class="ch-preview">{listPreview(conv.last_message_preview)}</span>
                  {#if conv.unread_count > 0}
                    <span class="ch-badge">{conv.unread_count > 99 ? '99+' : conv.unread_count}</span>
                  {/if}
                </span>
              </span>
            </button>
          {/each}
        </div>
      </section>
    {/if}

    {#if showHint}
      <p class="hint">Select a conversation from the sidebar to start messaging.</p>
    {/if}
  </div>
</section>

<style>
  .updates {
    flex: 1 1 auto;
    min-width: 0;
    min-height: 0;
    overflow-y: auto;
    overflow-x: hidden;
    background: var(--bg-main);
    color: var(--text);
  }

  .inner {
    width: 100%;
    max-width: 900px;
    margin: 0 auto;
    padding: 28px 24px 32px;
    box-sizing: border-box;
  }

  h1 {
    margin: 0 0 18px;
    font-size: 22px;
    font-weight: 700;
    letter-spacing: -0.02em;
    color: var(--text);
  }

  .block {
    margin-bottom: 26px;
    min-width: 0;
  }

  h2 {
    margin: 0 0 10px;
    font-size: 14px;
    font-weight: 600;
    color: var(--text-secondary);
  }

  /* Status cards */
  .cards {
    display: flex;
    gap: 10px;
    overflow-x: auto;
    overflow-y: hidden;
    padding: 2px 2px 10px;
    scrollbar-width: thin;
    overscroll-behavior-x: contain;
    touch-action: pan-x;
  }

  .card {
    position: relative;
    flex: 0 0 auto;
    width: 110px;
    height: 170px;
    padding: 0;
    border: 1px solid var(--border-subtle);
    border-radius: 16px;
    overflow: hidden;
    cursor: pointer;
    color: #fff;
    text-align: left;
    background: linear-gradient(
      160deg,
      color-mix(in srgb, var(--card-color) 85%, #000) 0%,
      color-mix(in srgb, var(--card-color) 45%, #000) 100%
    );
    transition: transform 0.12s ease, box-shadow 0.12s ease;
    -webkit-tap-highlight-color: transparent;
  }

  .card:hover {
    transform: translateY(-2px);
    box-shadow: var(--shadow-md);
  }

  .card:active {
    transform: scale(0.98);
  }

  .card:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }

  .card-bg {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  /* A small avatar scaled up is soft anyway; the blur hides the pixelation. */
  .card-bg-blur {
    filter: blur(14px) saturate(1.2);
    transform: scale(1.35);
    opacity: 0.85;
  }

  .card-text {
    position: absolute;
    inset: 52px 10px 40px;
    overflow: hidden;
    font-size: 12px;
    line-height: 1.3;
    font-weight: 600;
    text-align: center;
    display: flex;
    align-items: center;
    justify-content: center;
    overflow-wrap: anywhere;
  }

  .card-text.emoji {
    font-size: 34px;
    line-height: 1.1;
  }

  .card-shade {
    position: absolute;
    inset: 0;
    background: linear-gradient(180deg, rgba(0, 0, 0, 0.28) 0%, transparent 32%, transparent 52%, rgba(0, 0, 0, 0.72) 100%);
  }

  .card-count {
    position: absolute;
    top: 8px;
    right: 8px;
    min-width: 18px;
    height: 18px;
    padding: 0 5px;
    border-radius: var(--radius-full);
    background: rgba(0, 0, 0, 0.5);
    color: #fff;
    font-size: 10px;
    font-weight: 700;
    display: flex;
    align-items: center;
    justify-content: center;
    line-height: 1;
  }

  .card-name {
    position: absolute;
    left: 8px;
    right: 8px;
    bottom: 8px;
    font-size: 12px;
    font-weight: 600;
    line-height: 1.25;
    display: block;
    overflow: hidden;
    white-space: nowrap;
    text-shadow: 0 1px 2px rgba(0, 0, 0, 0.6);
  }

  .card-name > span {
    display: inline-block;
    min-width: max-content;
  }

  .card-name:global(.overflowing) {
    -webkit-mask-image: linear-gradient(90deg, #000 80%, transparent);
    mask-image: linear-gradient(90deg, #000 80%, transparent);
  }

  .card:hover .card-name:global(.overflowing),
  .card:focus-visible .card-name:global(.overflowing) {
    -webkit-mask-image: none;
    mask-image: none;
  }

  .card:hover .card-name:global(.overflowing) > span,
  .card:focus-visible .card-name:global(.overflowing) > span {
    animation: marquee var(--marquee-duration) linear infinite;
  }

  @keyframes marquee {
    0%, 20% { transform: translateX(0); }
    95%, 100% { transform: translateX(calc(-1 * var(--marquee-distance))); }
  }

  /* Channels */
  .channels {
    display: flex;
    flex-direction: column;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-lg);
    background: var(--bg-panel);
    overflow: hidden;
  }

  .channel {
    display: flex;
    align-items: center;
    gap: 12px;
    width: 100%;
    padding: 10px 14px;
    border: none;
    border-bottom: 1px solid var(--border-subtle);
    background: transparent;
    color: var(--text);
    text-align: left;
    cursor: pointer;
    min-width: 0;
    -webkit-tap-highlight-color: transparent;
  }

  .channel:last-child {
    border-bottom: none;
  }

  .channel:hover {
    background: var(--bg-hover);
  }

  .channel:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: -2px;
  }

  .ch-avatar-wrap {
    position: relative;
    flex-shrink: 0;
  }

  .ch-avatar {
    width: 46px;
    height: 46px;
    border-radius: 50%;
    display: grid;
    place-items: center;
    overflow: hidden;
    font-size: 15px;
    font-weight: 600;
    color: #fff;
  }

  .ch-avatar img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
  }

  .ch-network {
    position: absolute;
    bottom: -1px;
    right: -2px;
    width: 17px;
    height: 17px;
    border-radius: 50%;
    color: #fff;
    display: flex;
    align-items: center;
    justify-content: center;
    border: 2px solid var(--bg-panel);
  }

  .ch-body {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 3px;
  }

  .ch-top,
  .ch-bottom {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    min-width: 0;
  }

  .ch-bottom :global(.dir) { margin-right: -6px; font-size: 13px; }

  .ch-title {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 14px;
    font-weight: 600;
    color: var(--text-primary);
  }

  .ch-time {
    flex: 0 0 auto;
    font-size: 11px;
    color: var(--text-muted);
  }

  .channel.unread .ch-time {
    color: var(--accent);
  }

  .ch-preview {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 13px;
    color: var(--text-muted);
  }

  .channel.unread .ch-preview {
    color: var(--text-secondary);
    font-weight: 500;
  }

  .channel.muted .ch-title,
  .channel.muted .ch-preview {
    opacity: 0.75;
  }

  .ch-badge {
    flex: 0 0 auto;
    min-width: 19px;
    height: 19px;
    padding: 0 5px;
    border-radius: var(--radius-full);
    background: var(--accent);
    color: var(--text-on-accent, #fff);
    font-size: 10px;
    font-weight: 700;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .hint {
    margin: 8px 0 0;
    text-align: center;
    font-size: 12px;
    color: var(--text-muted);
    opacity: 0.8;
  }

  @media (max-width: 639px) {
    .inner {
      padding: 14px 16px 12px;
    }

    h1 {
      display: none;
    }

    .block {
      margin-bottom: 16px;
    }

    .channels {
      border-radius: var(--radius-md);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .card {
      transition: none;
    }

    .card:hover,
    .card:active {
      transform: none;
    }
  }
</style>
