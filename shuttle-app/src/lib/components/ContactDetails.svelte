<script lang="ts">
  import {
    avatarColor,
    conversationAvatar,
    fetchContactProfile,
    getInitials,
    readMessageMedia,
    updateConversation,
  } from '$lib/api';
  import NetworkIcon from '$lib/components/NetworkIcon.svelte';
  import MediaLightbox, { type LightboxItem } from '$lib/components/MediaLightbox.svelte';
  import { mediaDataFromMessage, mediaFilename, mediaKindFromMessage, mediaPathFromMessage } from '$lib/messageMedia';
  import type { Account, ContactProfileBundle, Conversation, MediaRetentionConfig, Message, PriorityGroup, Workspace } from '$lib/types';

  interface Props {
    open: boolean;
    conversation: Conversation | null;
    accounts: Account[];
    connectors: { id: string; capabilities: string[] }[];
    workspaces: Workspace[];
    priorityGroups: PriorityGroup[];
    globalMediaRetention: MediaRetentionConfig;
    onclose: () => void;
    onupdated: () => void;
    onstartcall?: (mode: 'audio' | 'video') => void;
    onsaveglobalmediaretention?: (cfg: MediaRetentionConfig) => void;
  }

  let {
    open, conversation, accounts, connectors, workspaces, priorityGroups,
    globalMediaRetention, onclose, onupdated, onstartcall, onsaveglobalmediaretention,
  }: Props = $props();

  let tab = $state<'media' | 'docs' | 'links' | 'starred' | 'account'>('media');
  let bundle = $state<ContactProfileBundle | null>(null);
  let loading = $state(false);
  let toast = $state<string | null>(null);
  let thumbUrls = $state<Record<string, string>>({});
  let retentionDraft = $state<MediaRetentionConfig>({});
  let lightboxOpen = $state(false);
  let lightboxIndex = $state(0);
  let lightboxItems = $state<LightboxItem[]>([]);

  const RETENTION_FIELDS: { key: keyof MediaRetentionConfig; label: string }[] = [
    { key: 'images_days', label: 'Images' },
    { key: 'videos_days', label: 'Videos' },
    { key: 'audio_days', label: 'Audio' },
    { key: 'voice_days', label: 'Voice messages' },
    { key: 'documents_days', label: 'Documents' },
    { key: 'stickers_days', label: 'Stickers' },
    { key: 'gifs_days', label: 'GIFs' },
  ];

  const account = $derived(conversation ? accounts.find((a) => a.id === conversation.account_id) : null);
  const connector = $derived(account ? connectors.find((c) => c.id === account.connector_id) : null);
  const caps = $derived(connector?.capabilities ?? []);
  const callsAudio = $derived(caps.includes('calls:audio'));
  const callsVideo = $derived(caps.includes('calls:video'));
  const canCall = $derived(callsAudio || callsVideo);
  const isDirect = $derived(conversation?.conversation_type === 'direct');
  const chatType = $derived(
    conversation?.conversation_type === 'group'
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

  $effect(() => {
    if (open && conversation) {
      loading = true;
      thumbUrls = {};
      void fetchContactProfile(conversation.account_id, conversation.id)
        .then((data) => { bundle = data; })
        .finally(() => { loading = false; });
    } else if (!open) {
      bundle = null;
      tab = 'media';
      thumbUrls = {};
    }
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
    }
  });

  $effect(() => {
    if (tab === 'account') {
      retentionDraft = { ...globalMediaRetention };
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
    onstartcall?.(mode);
  }

  function retentionValue(key: keyof MediaRetentionConfig): string {
    const v = retentionDraft[key];
    return v != null ? String(v) : '';
  }

  function setRetentionValue(key: keyof MediaRetentionConfig, raw: string) {
    const n = parseInt(raw, 10);
    retentionDraft = {
      ...retentionDraft,
      [key]: raw.trim() === '' || isNaN(n) || n <= 0 ? null : n,
    };
    onsaveglobalmediaretention?.(retentionDraft);
  }

  function thumbSrc(item: Message): string | null {
    return thumbUrls[item.id] ?? mediaDataFromMessage(item);
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
      if (!url) continue;
      const mime = msg.metadata?.mime;
      items.push({
        id: msg.id,
        url,
        kind,
        filename: mediaFilename(msg) ?? (tab === 'docs' ? docLabel(msg) : null),
        mime: typeof mime === 'string' ? mime : null,
      });
    }
    return items;
  }

  function openLightboxForItem(msg: Message) {
    const items = buildTabLightboxItems();
    const idx = items.findIndex((i) => i.id === msg.id);
    if (idx < 0) return;
    lightboxItems = items;
    lightboxIndex = idx;
    lightboxOpen = true;
  }

  function saveLightboxItem(item: LightboxItem) {
    const a = document.createElement('a');
    a.href = item.url;
    a.download = item.filename ?? `media-${item.id.slice(0, 8)}`;
    a.click();
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

    {#if isDirect && account && canCall}
      <div class="call-row">
        {#if callsAudio}
          <button type="button" class="call-btn" title="Audio call" onclick={() => handleCall('audio')}>
            🎧 Audio
          </button>
        {/if}
        {#if callsVideo}
          <button type="button" class="call-btn" title="Video call" onclick={() => handleCall('video')}>
            📹 Video
          </button>
        {/if}
      </div>
    {/if}

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

        <div class="section-head">Global media retention</div>
        <p class="retention-hint">Days to keep downloaded media locally. Leave blank to keep forever.</p>
        <div class="retention-grid">
          {#each RETENTION_FIELDS as field (field.key)}
            <label class="retention-row">
              <span class="retention-label">{field.label}</span>
              <input
                type="number"
                min="1"
                placeholder="∞"
                value={retentionValue(field.key)}
                oninput={(e) => setRetentionValue(field.key, e.currentTarget.value)}
                class="retention-input"
              />
              <span class="retention-unit">days</span>
            </label>
          {/each}
        </div>
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
                {@const src = thumbSrc(item)}
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
    onindex={(i) => (lightboxIndex = i)}
    onsave={saveLightboxItem}
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
  }
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

  .retention-grid {
    display: flex; flex-direction: column; gap: 6px;
  }

  .retention-row {
    display: grid;
    grid-template-columns: 1fr 72px auto;
    align-items: center;
    gap: 8px;
    font-size: 12px;
  }

  .retention-label { color: var(--text-secondary); }

  .retention-input {
    width: 100%;
    padding: 4px 6px;
    font-size: 12px;
    border: 1px solid var(--border-subtle);
    border-radius: 4px;
    background: var(--bg-input);
    color: var(--text);
    text-align: right;
  }

  .retention-unit {
    font-size: 11px;
    color: var(--text-muted);
    white-space: nowrap;
  }

  .toast {
    position: fixed; bottom: 24px; left: 50%;
    transform: translateX(-50%);
    background: var(--bg-panel); border: 1px solid var(--border-subtle);
    padding: 8px 14px; border-radius: 8px; font-size: 13px;
    z-index: 300; box-shadow: var(--shadow-sm);
  }
</style>
