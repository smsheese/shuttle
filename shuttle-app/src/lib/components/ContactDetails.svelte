<script lang="ts">
  import {
    avatarColor,
    conversationAvatar,
    downloadMessageMediaAndWait,
    fetchContactProfile,
    getInitials,
    openExternal,
    readMessageMedia,
    updateConversation,
  } from '$lib/api';
  import { mediaNameOwner, saveMessageMediaFile } from '$lib/mediaSave';
  import { whatsappCallFallback } from '$lib/whatsappLinks';
  import { toast as notify } from '$lib/toast.svelte';
  import NetworkIcon from '$lib/components/NetworkIcon.svelte';
  import MediaLightbox, { type LightboxItem } from '$lib/components/MediaLightbox.svelte';
  import {
    mediaDataFromMessage,
    mediaFilename,
    mediaKindFromMessage,
    mediaPathFromMessage,
    mediaSizeFromMessage,
    mediaThumbPathFromMessage,
    resolveThumbUrl,
  } from '$lib/messageMedia';
  import type { Account, ContactProfileBundle, Conversation, Message, PriorityGroup, Workspace } from '$lib/types';

  interface Props {
    open: boolean;
    conversation: Conversation | null;
    accounts: Account[];
    connectors: { id: string; capabilities: string[] }[];
    workspaces: Workspace[];
    priorityGroups: PriorityGroup[];
    onclose: () => void;
    onupdated: () => void;
    onstartcall?: (mode: 'audio' | 'video') => void;
    /** A group member with no chat on this account: `conversation` is not stored, so nothing
     * is loaded or saved for it. */
    transient?: boolean;
  }

  let {
    open, conversation, accounts, connectors, workspaces, priorityGroups,
    onclose, onupdated, onstartcall, transient = false,
  }: Props = $props();

  let tab = $state<'media' | 'docs' | 'links' | 'starred' | 'account'>('media');
  let bundle = $state<ContactProfileBundle | null>(null);
  let loading = $state(false);
  let toast = $state<string | null>(null);
  let thumbUrls = $state<Record<string, string>>({});
  let lightboxOpen = $state(false);
  let lightboxId = $state<string | null>(null);
  /** WhatsApp's own tiny previews for media that is not downloaded. */
  let previewUrls = $state<Record<string, string>>({});
  let downloading = $state<Set<string>>(new Set());

  const account = $derived(conversation ? accounts.find((a) => a.id === conversation.account_id) : null);
  const connector = $derived(account ? connectors.find((c) => c.id === account.connector_id) : null);
  const caps = $derived(connector?.capabilities ?? []);
  const callsAudio = $derived(caps.includes('calls:audio'));
  const callsVideo = $derived(caps.includes('calls:video'));
  const isDirect = $derived(conversation?.conversation_type === 'direct');
  /** A connector places the call itself; WhatsApp has no call API, so its buttons open the app. */
  const whatsappCalls = $derived(account?.connector_id === 'whatsapp');
  const canCall = $derived(callsAudio || callsVideo || whatsappCalls);
  const canVideoCall = $derived(callsVideo || whatsappCalls);
  const chatType = $derived(
    transient
      ? 'Group member'
      : conversation?.conversation_type === 'group'
      ? 'Group'
      : conversation?.conversation_type === 'channel'
        ? 'Channel'
        : 'Direct chat'
  );
  const profile = $derived(bundle?.profile ?? {});
  const phone = $derived(profile.phone || conversation?.remote_id.replace(/@.+$/, '') || '');
  const avatarSrc = $derived(conversation ? conversationAvatar(conversation) : null);

  const mediaItems = $derived(bundle?.media ?? []);
  const docItems = $derived(bundle?.docs ?? []);
  const tabItems = $derived(
    tab === 'media' ? mediaItems :
    tab === 'docs' ? docItems :
    tab === 'links' ? bundle?.links ?? [] :
    tab === 'starred' ? bundle?.starred ?? [] : []
  );
  const isGridTab = $derived(tab === 'media' || tab === 'docs');

  // Primitive ids: the parent hands over a fresh `conversation` object on every
  // live update, and only a change of chat should reload the profile.
  const accountId = $derived(conversation?.account_id ?? null);
  const conversationId = $derived(conversation?.id ?? null);

  $effect(() => {
    if (!open) {
      bundle = null;
      tab = 'media';
      thumbUrls = {};
      return;
    }
    if (!accountId || !conversationId || transient) return;
    let stale = false;
    loading = true;
    thumbUrls = {};
    void fetchContactProfile(accountId, conversationId)
      .then((data) => { if (!stale) bundle = data; })
      .finally(() => { if (!stale) loading = false; });
    return () => { stale = true; };
  });

  $effect(() => {
    if (!open || !bundle) return;
    const items = [...(bundle.media ?? []), ...(bundle.docs ?? [])];
    for (const item of items) {
      const inline = mediaDataFromMessage(item);
      if (inline) {
        thumbUrls[item.id] = inline;
        continue;
      }
      const path = mediaPathFromMessage(item);
      if (path && !thumbUrls[item.id]) {
        readMessageMedia(path)
          .then((url) => { thumbUrls = { ...thumbUrls, [item.id]: url }; })
          .catch(() => {});
      }
      const preview = mediaThumbPathFromMessage(item);
      if (!path && preview && !previewUrls[item.id]) {
        void resolveThumbUrl(preview).then((url) => {
          if (url) previewUrls = { ...previewUrls, [item.id]: url };
        });
      }
    }
  });

  async function copyPhone() {
    if (!phone) return;
    await navigator.clipboard.writeText(phone);
    toast = 'Phone copied';
    setTimeout(() => (toast = null), 1800);
  }

  async function setWorkspace(id: string) {
    if (!conversation) return;
    if (id === '') await updateConversation(conversation.id, { clear_workspace: true });
    else await updateConversation(conversation.id, { workspace_id: id });
    onupdated();
  }

  async function setPriority(id: string) {
    if (!conversation) return;
    if (id === '') await updateConversation(conversation.id, { clear_priority: true });
    else await updateConversation(conversation.id, { priority_group: id });
    onupdated();
  }

  function handleCall(mode: 'audio' | 'video') {
    if (!whatsappCalls) {
      onstartcall?.(mode);
      return;
    }
    const fallback = whatsappCallFallback(mode, conversation?.remote_id);
    notify.info(fallback.message);
    if (fallback.url) void openExternal(fallback.url);
  }

  /** Downloaded media URL (what the viewer plays), if the file is cached. */
  function thumbSrc(item: Message): string | null {
    return thumbUrls[item.id] ?? mediaDataFromMessage(item);
  }

  /** What a grid cell shows: the media itself, else WhatsApp's preview. */
  function cellSrc(item: Message): string | null {
    return thumbSrc(item) ?? previewUrls[item.id] ?? null;
  }

  function docLabel(item: Message): string {
    return mediaFilename(item) || item.body?.replace(/^\[.*\]$/, '') || 'Document';
  }

  function lightboxKindForMessage(msg: Message): LightboxItem['kind'] | null {
    const kind = mediaKindFromMessage(msg);
    if (kind === 'image' || kind === 'sticker') return 'image';
    if (kind === 'video') return 'video';
    if (kind === 'audio') return 'audio';
    if (kind === 'document') return 'document';
    return null;
  }

  function buildTabLightboxItems(): LightboxItem[] {
    const items: LightboxItem[] = [];
    for (const msg of tabItems) {
      const kind = lightboxKindForMessage(msg);
      if (!kind) continue;
      const url = thumbSrc(msg);
      const mime = msg.metadata?.mime;
      items.push({
        id: msg.id,
        url: url ?? '',
        kind,
        filename: mediaFilename(msg) ?? (tab === 'docs' ? docLabel(msg) : null),
        mime: typeof mime === 'string' ? mime : null,
        path: url ? mediaPathFromMessage(msg) : null,
        thumb: previewUrls[msg.id] ?? null,
        status: url ? 'ready' : mediaFailedFor(msg) ? 'unavailable' : downloading.has(msg.id) ? 'loading' : 'idle',
        size: mediaSizeFromMessage(msg),
      });
    }
    return items;
  }

  function mediaFailedFor(msg: Message): boolean {
    return msg.metadata?.media_error != null;
  }

  const lightboxItems = $derived<LightboxItem[]>(lightboxOpen ? buildTabLightboxItems() : []);
  const lightboxIndex = $derived(Math.max(0, lightboxItems.findIndex((i) => i.id === lightboxId)));

  function openLightboxForItem(msg: Message) {
    lightboxId = msg.id;
    lightboxOpen = true;
  }

  function findMessage(id: string): Message | undefined {
    return [...(bundle?.media ?? []), ...(bundle?.docs ?? [])].find((m) => m.id === id);
  }

  function saveLightboxItem(item: LightboxItem) {
    const msg = findMessage(item.id);
    if (msg && conversation)
      void saveMessageMediaFile(conversation.account_id, conversation.id, msg, mediaNameOwner(conversation, accounts));
  }

  async function downloadLightboxItem(item: LightboxItem) {
    const msg = findMessage(item.id);
    if (!msg || !conversation || downloading.has(msg.id)) return;
    downloading = new Set([...downloading, msg.id]);
    try {
      const done = await downloadMessageMediaAndWait(conversation.account_id, conversation.id, msg.id);
      if (bundle) {
        const swap = (list: Message[]) => list.map((m) => (m.id === done.id ? done : m));
        bundle = { ...bundle, media: swap(bundle.media ?? []), docs: swap(bundle.docs ?? []) };
      }
    } catch (e) {
      notify.error(e instanceof Error ? e.message : 'Download failed');
    } finally {
      downloading = new Set([...downloading].filter((id) => id !== msg.id));
    }
  }
</script>

{#if open && conversation}
  <div class="backdrop" role="presentation" onclick={onclose}></div>
  <div class="modal" role="dialog" aria-modal="true" aria-label="Contact details">
    <header class="head">
      <h2>Contact details</h2>
      <button type="button" class="close" onclick={onclose} aria-label="Close">×</button>
    </header>

    <div class="hero">
      <div class="avatar" style="background: {avatarColor(conversation.id)}">
        {#if avatarSrc}
          <img src={avatarSrc} alt="" />
        {:else}
          <span>{getInitials(conversation.title)}</span>
        {/if}
      </div>
      <div class="hero-text">
        <div class="name">{conversation.title}</div>
        {#if profile.username && profile.username !== conversation.title}
          <div class="sub">@{profile.username}</div>
        {/if}
        <div class="sub">{chatType}</div>
        {#if profile.business_name}
          <div class="sub biz">{profile.business_name}</div>
        {/if}
      </div>
    </div>

    {#if phone}
      <button type="button" class="phone-row" onclick={copyPhone} title="Copy phone number">
        <span class="phone">{phone}</span>
        <span class="copy-hint">Click to copy</span>
      </button>
    {/if}

    {#if profile.about}
      <div class="about">
        <span class="label">About</span>
        <p>{profile.about}</p>
      </div>
    {/if}

    {#if isDirect && account && canCall && onstartcall}
      <div class="call-row">
        <button type="button" class="call-btn" title="Audio call" onclick={() => handleCall('audio')}>
          <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <path d="M22 16.92v3a2 2 0 0 1-2.18 2 19.79 19.79 0 0 1-8.63-3.07 19.5 19.5 0 0 1-6-6A19.79 19.79 0 0 1 2.12 4.18 2 2 0 0 1 4.11 2h3a2 2 0 0 1 2 1.72c.13.96.36 1.9.7 2.81a2 2 0 0 1-.45 2.11L8.09 9.91a16 16 0 0 0 6 6l1.27-1.27a2 2 0 0 1 2.11-.45c.9.34 1.85.57 2.81.7A2 2 0 0 1 22 16.92z"/>
          </svg>
          Audio
        </button>
        {#if canVideoCall}
          <button type="button" class="call-btn" title="Video call" onclick={() => handleCall('video')}>
            <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
              <rect x="2" y="6" width="13" height="12" rx="2"/><path d="m15 12 6-3.5v11L15 16z"/>
            </svg>
            Video
          </button>
        {/if}
      </div>
    {/if}

    {#if transient}
      <p class="muted">You have no chat with this person on this account.</p>
    {:else}
    <div class="tabs">
      {#each (['media', 'docs', 'links', 'starred', 'account'] as const) as t (t)}
        <button type="button" class:active={tab === t} onclick={() => (tab = t)}>{t}</button>
      {/each}
    </div>

    {#if tab === 'account'}
      <div class="account-settings">
        {#if account}
          <div class="account-row">
            <NetworkIcon connectorId={account.connector_id} size={16} />
            <span>{account.name}</span>
          </div>
        {/if}

        <div class="section-head">Media</div>
        <p class="retention-hint">
          Downloads, saving and how long downloaded media is kept are in Settings → Media &amp; storage.
          Right-click this chat in the list to change what downloads automatically here.
        </p>
      </div>
    {:else if isGridTab}
      <div class="grid-scroll">
        {#if loading}
          <p class="muted">Loading…</p>
        {:else if tabItems.length === 0}
          <p class="muted">Nothing here yet</p>
        {:else}
          <div class="item-grid">
            {#each tabItems as item (item.id)}
              {#if tab === 'media'}
                {@const src = cellSrc(item)}
                {@const kind = mediaKindFromMessage(item)}
                <button type="button" class="grid-cell" title={kind ?? 'media'} onclick={() => openLightboxForItem(item)}>
                  {#if src}
                    <img src={src} alt="" loading="lazy" />
                  {:else}
                    <div class="cell-placeholder">
                      <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8"><rect x="3" y="5" width="18" height="14" rx="2"/><circle cx="8.5" cy="10" r="1.5"/><path d="m21 15-5-5-4 4-2-2-5 5"/></svg>
                    </div>
                  {/if}
                </button>
              {:else}
                <button type="button" class="grid-cell doc-cell" title={docLabel(item)} onclick={() => openLightboxForItem(item)}>
                  <span class="doc-icon">📄</span>
                  <span class="doc-name">{docLabel(item)}</span>
                </button>
              {/if}
            {/each}
          </div>
        {/if}
      </div>
    {:else}
      <div class="list-scroll">
        {#if loading}
          <p class="muted">Loading…</p>
        {:else if tabItems.length === 0}
          <p class="muted">Nothing here yet</p>
        {:else}
          {#each tabItems as item (item.id)}
            <div class="tab-item">
              <span class="tab-preview">{item.body || ''}</span>
              <span class="tab-time">{new Date(item.timestamp).toLocaleDateString()}</span>
            </div>
          {/each}
        {/if}
      </div>
    {/if}

    <h3>Organize</h3>
    <div class="organize-row">
      <label>
        Workspace
        <select value={conversation.workspace_id ?? ''} onchange={(e) => setWorkspace(e.currentTarget.value)}>
          <option value="">Account default</option>
          {#each workspaces as ws (ws.id)}
            <option value={ws.id}>{ws.name}</option>
          {/each}
        </select>
      </label>
      <label>
        Priority
        <select value={conversation.priority_group ?? ''} onchange={(e) => setPriority(e.currentTarget.value)}>
          <option value="">None</option>
          {#each priorityGroups as g (g.id)}
            <option value={g.id}>{g.name}</option>
          {/each}
        </select>
      </label>
    </div>
    {/if}

    {#if tab !== 'account' && account}
      <div class="account-row bottom">
        <NetworkIcon connectorId={account.connector_id} size={16} />
        <span>{account.name}</span>
      </div>
    {/if}

    {#if toast}
      <div class="toast">{toast}</div>
    {/if}
  </div>
  <MediaLightbox
    open={lightboxOpen}
    items={lightboxItems}
    index={lightboxIndex}
    onclose={() => (lightboxOpen = false)}
    onindex={(i) => (lightboxId = lightboxItems[i]?.id ?? lightboxId)}
    onsave={saveLightboxItem}
    ondownload={downloadLightboxItem}
  />
{/if}

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.45);
    z-index: 200;
  }

  .modal {
    position: fixed;
    left: 50%;
    top: 50%;
    transform: translate(-50%, -50%);
    z-index: 201;
    width: min(480px, calc(100vw - 32px));
    max-height: min(90vh, 760px);
    overflow: auto;
    background: var(--bg-panel);
    border: 1px solid var(--border-subtle);
    border-radius: 12px;
    padding: 18px;
    display: flex;
    flex-direction: column;
    gap: 10px;
    box-shadow: 0 16px 48px rgba(0, 0, 0, 0.2);
  }

  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }

  h2 { margin: 0; font-size: 18px; }

  h3 {
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--text-muted);
    margin-top: 4px;
  }

  .close {
    border: none;
    background: transparent;
    color: var(--text-muted);
    font-size: 22px;
    line-height: 1;
    cursor: pointer;
    padding: 0 4px;
  }

  .hero { display: flex; gap: 12px; align-items: center; }

  .avatar {
    width: 56px; height: 56px;
    border-radius: 50%;
    overflow: hidden;
    display: flex; align-items: center; justify-content: center;
    color: #fff; font-weight: 600; flex-shrink: 0;
  }
  .avatar img { width: 100%; height: 100%; object-fit: cover; }

  .name { font-size: 16px; font-weight: 600; }
  .sub { font-size: 12px; color: var(--text-muted); }
  .sub.biz { color: var(--text-primary); font-weight: 500; }

  .phone-row {
    display: flex; flex-direction: column; align-items: flex-start; gap: 2px;
    padding: 8px 10px; border: 1px solid var(--border-subtle); border-radius: 8px;
    background: var(--bg-hover); cursor: pointer; text-align: left; width: 100%;
  }
  .phone { font-family: ui-monospace, monospace; font-size: 14px; }
  .copy-hint { font-size: 11px; color: var(--text-muted); }

  .about .label { font-size: 11px; text-transform: uppercase; color: var(--text-muted); }
  .about p { margin: 4px 0 0; font-size: 13px; white-space: pre-wrap; }

  .call-row { display: flex; gap: 8px; }
  .call-btn {
    flex: 1; padding: 8px; border-radius: 8px;
    border: 1px solid var(--border-subtle); background: var(--bg-hover);
    cursor: pointer; font-size: 13px;
    display: inline-flex; align-items: center; justify-content: center; gap: 6px;
  }
  .call-btn svg { flex: none; opacity: 0.85; }
  .call-btn:disabled { opacity: 0.45; cursor: not-allowed; }

  .tabs {
    display: flex; gap: 4px;
    border-bottom: 1px solid var(--border-subtle); padding-bottom: 4px;
  }
  .tabs button {
    flex: 1; border: none; background: transparent;
    padding: 6px 4px; font-size: 12px; text-transform: capitalize;
    color: var(--text-muted); cursor: pointer; border-radius: 6px;
  }
  .tabs button.active {
    background: var(--bg-hover); color: var(--text-primary); font-weight: 600;
  }

  /* Grid scroll: 6 cols, max 3 rows visible, scroll for more */
  .grid-scroll {
    max-height: calc(3 * 52px + 2 * 4px);
    overflow-y: auto;
    overscroll-behavior: contain;
  }

  .item-grid {
    display: grid;
    grid-template-columns: repeat(6, 1fr);
    gap: 4px;
  }

  .grid-cell {
    aspect-ratio: 1;
    border-radius: 4px;
    overflow: hidden;
    background: var(--bg-hover);
    min-width: 0;
    border: none;
    padding: 0;
    cursor: pointer;
    text-align: inherit;
    color: inherit;
    font: inherit;
  }

  .grid-cell:hover {
    outline: 2px solid var(--accent);
    outline-offset: -2px;
  }

  .grid-cell img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
  }

  .cell-placeholder {
    width: 100%;
    height: 100%;
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--text-muted);
  }

  .doc-cell {
    aspect-ratio: auto;
    min-height: 52px;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 2px;
    padding: 4px;
  }

  .doc-icon { font-size: 16px; line-height: 1; }
  .doc-name {
    font-size: 8px;
    color: var(--text-muted);
    text-align: center;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    width: 100%;
  }

  .list-scroll {
    max-height: 180px;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 6px;
    overscroll-behavior: contain;
  }

  .tab-item {
    display: flex; justify-content: space-between; gap: 8px;
    font-size: 12px; padding: 6px 8px; border-radius: 6px;
    background: var(--bg-hover);
  }
  .tab-preview { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; flex: 1; }
  .tab-time { color: var(--text-muted); flex-shrink: 0; }

  .muted { font-size: 12px; color: var(--text-muted); margin: 0; padding: 8px 0; }

  .organize-row {
    display: flex; gap: 8px;
  }
  .organize-row label {
    flex: 1; display: flex; flex-direction: column; gap: 4px;
    font-size: 12px; color: var(--text-muted);
  }

  .account-row {
    display: flex; align-items: center; gap: 6px;
    font-size: 12px; color: var(--text-muted);
  }
  .account-row.bottom { margin-top: 2px; }

  .account-settings {
    display: flex; flex-direction: column; gap: 8px;
  }

  .section-head {
    font-size: 12px; font-weight: 600; color: var(--text-primary);
  }

  .retention-hint {
    font-size: 11px; color: var(--text-muted); margin: 0;
  }

  .toast {
    position: fixed; bottom: 24px; left: 50%;
    transform: translateX(-50%);
    background: var(--bg-panel); border: 1px solid var(--border-subtle);
    padding: 8px 14px; border-radius: 8px; font-size: 13px;
    z-index: 300; box-shadow: var(--shadow-sm);
  }
</style>
