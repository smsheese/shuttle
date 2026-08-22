<script lang="ts">
  import { avatarColor, conversationAvatar, downloadMessageMedia, getInitials, readMessageMedia } from '$lib/api';
  import NetworkIcon from '$lib/components/NetworkIcon.svelte';
  import EmojiPickerPop from '$lib/components/EmojiPickerPop.svelte';
  import GiphyPicker from '$lib/components/GiphyPicker.svelte';
  import ImageEditor from '$lib/components/ImageEditor.svelte';
  import MediaLightbox, { type LightboxItem } from '$lib/components/MediaLightbox.svelte';
  import { giphyUrlToBase64, type GiphyItem } from '$lib/giphy';
  import {
    captionText,
    looksLikeJid,
    mediaDataFromMessage,
    mediaFailed,
    mediaFilename,
    mediaIconLabel,
    mediaKindFromMessage,
    mediaPathFromMessage,
    shouldDownloadMedia,
  } from '$lib/messageMedia';
  import { wrapSelection } from '$lib/richText';
  import { CONNECTOR_COLORS, type Account, type AttachmentKind, type AttachmentPayload, type Conversation, type Message } from '$lib/types';

  interface Props {
    conversation: Conversation | null;
    messages: Message[];
    accounts: Account[];
    draft: string;
    ondraft: (v: string) => void;
    onsend: () => void;
    onsendattachment?: (payload: AttachmentPayload) => void;
    onsendlater?: (sendAt: string) => void;
    onback?: () => void;
    showBack?: boolean;
    onmsgmenu?: (msg: Message, x: number, y: number) => void;
    ontextmenu?: (text: string, x: number, y: number) => void;
    onheaderclick?: () => void;
    ontogglepanel?: () => void;
    onstartcall?: (mode: 'audio' | 'video') => void;
    connectors?: { id: string; capabilities: string[] }[];
    panelOpen?: boolean;
    channelColor?: string;
  }

  let {
    conversation,
    messages,
    accounts,
    draft,
    ondraft,
    onsend,
    onsendattachment,
    onsendlater,
    onback,
    showBack = false,
    onmsgmenu,
    ontextmenu,
    onheaderclick,
    ontogglepanel,
    onstartcall,
    connectors = [],
    panelOpen = false,
    channelColor,
  }: Props = $props();
  let composerEl = $state<HTMLTextAreaElement | null>(null);
  let mediaUrls = $state<Record<string, string>>({});
  let picker = $state<null | 'emoji' | 'sticker' | 'gif' | 'attach' | 'location' | 'poll' | 'later'>(null);
  let laterAt = $state('');
  let pendingFile = $state<null | { kind: AttachmentKind; filename: string; mime: string; data_base64: string }>(null);
  let locLat = $state('');
  let locLng = $state('');
  let locBusy = $state(false);
  let pollQuestion = $state('');
  let pollOptions = $state(['', '']);
  let recording = $state(false);
  let recorder = $state<MediaRecorder | null>(null);
  let recordChunks = $state<Blob[]>([]);
  let fileImageEl = $state<HTMLInputElement | null>(null);
  let fileVideoEl = $state<HTMLInputElement | null>(null);
  let fileAudioEl = $state<HTMLInputElement | null>(null);
  let fileDocEl = $state<HTMLInputElement | null>(null);
  let fileGifEl = $state<HTMLInputElement | null>(null);
  let fileStickerEl = $state<HTMLInputElement | null>(null);
  let chatSearchOpen = $state(false);
  let chatSearchQuery = $state('');
  let messagesEl = $state<HTMLDivElement | null>(null);
  let lightboxOpen = $state(false);
  let lightboxIndex = $state(0);
  let lightboxItems = $state<LightboxItem[]>([]);
  let editorOpen = $state(false);
  let editorSrc = $state('');
  let editorFilename = $state('');
  let editorMime = $state('');
  let editorOriginal = $state<null | { kind: AttachmentKind; filename: string; mime: string; data_base64: string }>(null);

  const displayMessages = $derived(
    [...messages].sort((a, b) => {
      if (a.pinned !== b.pinned) return a.pinned ? -1 : 1;
      return new Date(a.timestamp).getTime() - new Date(b.timestamp).getTime();
    })
  );

  const account = $derived(
    conversation ? accounts.find((a) => a.id === conversation.account_id) : null
  );

  const connectorCaps = $derived(
    account ? connectors.find((c) => c.id === account.connector_id)?.capabilities ?? [] : []
  );
  const canCall = $derived(
    conversation?.conversation_type === 'direct' &&
      connectorCaps.some((c) => c.startsWith('calls:'))
  );

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      submitComposer();
    }
    if (e.key === 'Escape') picker = null;
  }

  function formatMsgTime(iso: string): string {
    const d = new Date(iso);
    return d.toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' });
  }

  function dateKey(iso: string): string {
    const d = new Date(iso);
    return `${d.getFullYear()}-${d.getMonth()}-${d.getDate()}`;
  }

  function formatDateLabel(iso: string): string {
    const d = new Date(iso);
    const now = new Date();
    const today = new Date(now.getFullYear(), now.getMonth(), now.getDate()).getTime();
    const that = new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
    const diff = Math.round((today - that) / 86400000);
    if (diff === 0) return 'Today';
    if (diff === 1) return 'Yesterday';
    return d.toLocaleDateString([], {
      weekday: 'short',
      month: 'short',
      day: 'numeric',
      year: d.getFullYear() !== now.getFullYear() ? 'numeric' : undefined,
    });
  }

  const requestedMedia = new Set<string>();
  const mediaTimedOut = new Set<string>();
  const mediaTimeouts = new Map<string, ReturnType<typeof setTimeout>>();

  function mediaUnavailable(msg: Message): boolean {
    return mediaFailed(msg) || mediaTimedOut.has(msg.id);
  }

  function mediaUrlFor(msg: Message): string | null {
    return mediaUrls[msg.id] ?? mediaDataFromMessage(msg);
  }

  function resizeComposer() {
    if (!composerEl) return;
    composerEl.style.height = 'auto';
    const maxHeight = parseFloat(getComputedStyle(composerEl).lineHeight) * 4 + 12;
    composerEl.style.height = `${Math.min(composerEl.scrollHeight, maxHeight)}px`;
  }

  $effect(() => {
    draft;
    queueMicrotask(resizeComposer);
  });

  // Scroll to bottom when conversation changes (always) or new messages arrive (if near bottom)
  let lastConversationId = $state<string | null>(null);
  $effect(() => {
    const convId = conversation?.id ?? null;
    const changed = convId !== lastConversationId;
    displayMessages; // track
    if (!messagesEl) return;
    const el = messagesEl;
    if (changed) {
      lastConversationId = convId;
      // Always scroll to bottom on conversation switch
      queueMicrotask(() => { el.scrollTop = el.scrollHeight; });
    } else {
      // Only scroll if near bottom
      const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 120;
      if (atBottom) {
        queueMicrotask(() => { el.scrollTop = el.scrollHeight; });
      }
    }
  });

  $effect(() => {
    if (!picker) return;
    function onDoc(e: PointerEvent) {
      const t = e.target as HTMLElement | null;
      if (t?.closest('.picker-wrap') || t?.closest('emoji-picker')) return;
      picker = null;
    }
    document.addEventListener('pointerdown', onDoc);
    return () => document.removeEventListener('pointerdown', onDoc);
  });

  $effect(() => {
    if (!conversation) return;
    for (const msg of messages) {
      const kind = mediaKindFromMessage(msg);
      if (!shouldDownloadMedia(kind)) continue;
      if (mediaFailed(msg)) continue;
      mediaTimedOut.delete(msg.id);

      const inline = mediaDataFromMessage(msg);
      if (inline) {
        mediaUrls[msg.id] = inline;
        continue;
      }

      const path = mediaPathFromMessage(msg);
      if (path && !mediaUrls[msg.id]) {
        readMessageMedia(path)
          .then((url) => {
            mediaUrls = { ...mediaUrls, [msg.id]: url };
          })
          .catch(() => {});
      }

      if (!inline && !path && !mediaFailed(msg) && !requestedMedia.has(msg.id)) {
        requestedMedia.add(msg.id);
        if (!mediaTimeouts.has(msg.id)) {
          mediaTimeouts.set(
            msg.id,
            setTimeout(() => {
              mediaTimeouts.delete(msg.id);
              requestedMedia.delete(msg.id);
              if (!mediaUrlFor(msg) && !mediaFailed(msg)) {
                mediaTimedOut.add(msg.id);
              }
            }, 20000)
          );
        }
        downloadMessageMedia(conversation.account_id, conversation.id, msg.id).catch(() => {
          requestedMedia.delete(msg.id);
        });
      }
    }
  });

  $effect(() => {
    for (const msg of messages) {
      if (mediaFailed(msg) || mediaUrlFor(msg)) {
        const t = mediaTimeouts.get(msg.id);
        if (t) {
          clearTimeout(t);
          mediaTimeouts.delete(msg.id);
        }
      }
    }
  });

  type GroupPos = 'single' | 'first' | 'middle' | 'last';

  function groupPos(msgs: Message[], i: number): GroupPos {
    const msg = msgs[i];
    const sameAsPrev = i > 0 && msgs[i - 1].direction === msg.direction;
    const sameAsNext = i < msgs.length - 1 && msgs[i + 1].direction === msg.direction;
    if (!sameAsPrev && !sameAsNext) return 'single';
    if (!sameAsPrev && sameAsNext) return 'first';
    if (sameAsPrev && sameAsNext) return 'middle';
    return 'last';
  }

  function applyMark(mark: 'bold' | 'italic' | 'strike' | 'code') {
    if (!composerEl) return;
    const start = composerEl.selectionStart ?? draft.length;
    const end = composerEl.selectionEnd ?? draft.length;
    const next = wrapSelection(draft, start, end, mark);
    ondraft(next.text);
    queueMicrotask(() => {
      composerEl?.focus();
      composerEl?.setSelectionRange(next.start, next.end);
    });
  }

  function localDateTimeValue(offsetMs = 3600_000): string {
    const d = new Date(Date.now() + offsetMs);
    const pad = (n: number) => String(n).padStart(2, '0');
    return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}T${pad(d.getHours())}:${pad(d.getMinutes())}`;
  }

  function togglePicker(next: typeof picker) {
    picker = picker === next ? null : next;
    if (picker === 'later') laterAt = localDateTimeValue();
  }

  function submitSendLater() {
    if (!laterAt || !draft.trim() || !onsendlater) return;
    onsendlater(new Date(laterAt).toISOString());
    picker = null;
  }

  function insertEmoji(emoji: string) {
    const start = composerEl?.selectionStart ?? draft.length;
    const end = composerEl?.selectionEnd ?? draft.length;
    ondraft(draft.slice(0, start) + emoji + draft.slice(end));
    picker = null;
    queueMicrotask(() => {
      const pos = start + emoji.length;
      composerEl?.focus();
      composerEl?.setSelectionRange(pos, pos);
    });
  }

  function fileToBase64(file: File): Promise<string> {
    return new Promise((resolve, reject) => {
      const reader = new FileReader();
      reader.onload = () => {
        const result = String(reader.result ?? '');
        const comma = result.indexOf(',');
        resolve(comma >= 0 ? result.slice(comma + 1) : result);
      };
      reader.onerror = () => reject(reader.error);
      reader.readAsDataURL(file);
    });
  }

  function fileToDataUrl(file: File): Promise<string> {
    return new Promise((resolve, reject) => {
      const reader = new FileReader();
      reader.onload = () => resolve(String(reader.result ?? ''));
      reader.onerror = () => reject(reader.error);
      reader.readAsDataURL(file);
    });
  }

  function lightboxKind(kind: string | null): LightboxItem['kind'] | null {
    if (!kind) return null;
    if (kind === 'image' || kind === 'sticker') return 'image';
    if (kind === 'video') return 'video';
    if (kind === 'audio') return 'audio';
    if (kind === 'document') return 'document';
    return null;
  }

  function buildThreadLightboxItems(): LightboxItem[] {
    const items: LightboxItem[] = [];
    for (const msg of displayMessages) {
      const kind = mediaKindFromMessage(msg);
      const lbKind = lightboxKind(kind);
      if (!lbKind) continue;
      const url = mediaUrlFor(msg);
      if (!url) continue;
      const mime = msg.metadata?.mime;
      items.push({
        id: msg.id,
        url,
        kind: lbKind,
        filename: mediaFilename(msg),
        mime: typeof mime === 'string' ? mime : null,
      });
    }
    return items;
  }

  function openLightboxForMessage(msg: Message) {
    const items = buildThreadLightboxItems();
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

  function editLightboxItem(item: LightboxItem) {
    if (item.kind !== 'image') return;
    lightboxOpen = false;
    editorSrc = item.url;
    editorFilename = item.filename ?? 'edited.jpg';
    editorMime = item.mime ?? 'image/jpeg';
    editorOriginal = null;
    editorOpen = true;
  }

  function closeEditor() {
    editorOpen = false;
    editorOriginal = null;
  }

  function onEditorSend(result: { data_base64: string; mime: string; filename: string }) {
    sendPayload({
      kind: 'image',
      ...result,
      caption: draft.trim() || undefined,
    });
    ondraft('');
    closeEditor();
  }

  function onEditorSkip() {
    if (editorOriginal) pendingFile = { ...editorOriginal };
    closeEditor();
  }

  async function stageFile(kind: AttachmentKind, file: File) {
    pendingFile = {
      kind,
      filename: file.name,
      mime: file.type || 'application/octet-stream',
      data_base64: await fileToBase64(file),
    };
    picker = null;
  }

  async function onPickFile(kind: AttachmentKind, e: Event) {
    const input = e.currentTarget as HTMLInputElement;
    const file = input.files?.[0];
    input.value = '';
    if (!file) return;
    if (kind === 'image' && file.type.startsWith('image/') && file.type !== 'image/gif') {
      editorOriginal = {
        kind,
        filename: file.name,
        mime: file.type || 'image/jpeg',
        data_base64: await fileToBase64(file),
      };
      editorSrc = await fileToDataUrl(file);
      editorFilename = file.name;
      editorMime = file.type || 'image/jpeg';
      editorOpen = true;
      picker = null;
      return;
    }
    await stageFile(kind, file);
  }

  function canSend(): boolean {
    return Boolean(draft.trim() || pendingFile);
  }

  function submitComposer() {
    if (pendingFile && onsendattachment) {
      onsendattachment({
        ...pendingFile,
        caption: draft.trim() || undefined,
      });
      pendingFile = null;
      ondraft('');
      return;
    }
    if (draft.trim()) onsend();
  }

  function sendPayload(payload: AttachmentPayload) {
    onsendattachment?.(payload);
    picker = null;
    pendingFile = null;
  }

  async function sendGiphyItem(item: GiphyItem, kind: 'gif' | 'sticker') {
    try {
      const { data_base64, mime } = await giphyUrlToBase64(item.fullUrl);
      sendPayload({
        kind: kind === 'gif' ? 'image' : 'sticker',
        filename: `${item.id}.${mime.includes('webp') ? 'webp' : 'gif'}`,
        mime,
        data_base64,
        caption: draft.trim() || undefined,
      });
      ondraft('');
    } catch (err) {
      console.error('[GiphyPicker] send failed:', err, 'url:', item.fullUrl);
    }
  }

  async function useMyLocation() {
    locBusy = true;
    try {
      const pos = await new Promise<GeolocationPosition>((resolve, reject) => {
        navigator.geolocation.getCurrentPosition(resolve, reject, { timeout: 12000 });
      });
      locLat = String(pos.coords.latitude);
      locLng = String(pos.coords.longitude);
    } catch {
      locBusy = false;
    }
    locBusy = false;
  }

  function sendLocation() {
    const latitude = Number(locLat);
    const longitude = Number(locLng);
    if (!Number.isFinite(latitude) || !Number.isFinite(longitude)) return;
    sendPayload({
      kind: 'location',
      latitude,
      longitude,
      caption: draft.trim() || undefined,
    });
    ondraft('');
  }

  function sendPoll() {
    const options = pollOptions.map((o) => o.trim()).filter(Boolean);
    if (!pollQuestion.trim() || options.length < 2) return;
    sendPayload({
      kind: 'poll',
      question: pollQuestion.trim(),
      caption: pollQuestion.trim(),
      options,
      max_answer: 1,
    });
    pollQuestion = '';
    pollOptions = ['', ''];
  }

  async function toggleRecord() {
    if (recording && recorder) {
      recorder.stop();
      return;
    }
    const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
    const rec = new MediaRecorder(stream);
    recordChunks = [];
    rec.ondataavailable = (ev) => {
      if (ev.data.size > 0) recordChunks = [...recordChunks, ev.data];
    };
    rec.onstop = async () => {
      stream.getTracks().forEach((t) => t.stop());
      recording = false;
      recorder = null;
      const blob = new Blob(recordChunks, { type: rec.mimeType || 'audio/webm' });
      const file = new File([blob], 'voice.webm', { type: blob.type });
      const data_base64 = await fileToBase64(file);
      sendPayload({
        kind: 'ptt',
        filename: file.name,
        mime: file.type,
        data_base64,
      });
    };
    recorder = rec;
    recording = true;
    rec.start();
    picker = null;
  }
</script>

<div class="thread">
  {#if conversation}
    <header class="thread-header">
      {#if showBack}
        <button class="back-btn" onclick={onback} aria-label="Back">
          <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <path d="m15 18-6-6 6-6"/>
          </svg>
        </button>
      {/if}

      <div class="header-avatar-wrap">
        <div class="header-avatar" style="background: {avatarColor(conversation.title)}">
          {#if conversationAvatar(conversation)}
            <img class="avatar-img" src={conversationAvatar(conversation) ?? ''} alt="" />
          {:else}
            {getInitials(conversation.title)}
          {/if}
        </div>
        {#if account}
          <span
            class="header-network-badge"
            style="background: {channelColor ?? CONNECTOR_COLORS[account.connector_id] ?? '#888'}"
          >
            <NetworkIcon connectorId={account.connector_id} size={9} />
          </span>
        {/if}
      </div>

      <button class="header-info" type="button" onclick={() => onheaderclick?.()} aria-label="Contact details">
        <h2>{conversation.title}</h2>
        {#if account}
          <span class="subtitle">{account.name}</span>
        {:else if conversation.conversation_type === 'group'}
          <span class="subtitle">Group</span>
        {/if}
      </button>
      {#if canCall && onstartcall}
        <button type="button" class="panel-btn" title="Audio call" onclick={() => onstartcall?.('audio')} aria-label="Audio call">🎧</button>
        {#if connectorCaps.includes('calls:video')}
          <button type="button" class="panel-btn" title="Video call" onclick={() => onstartcall?.('video')} aria-label="Video call">📹</button>
        {/if}
      {/if}
      <button type="button" class="panel-btn" class:active={chatSearchOpen} onclick={() => (chatSearchOpen = !chatSearchOpen)} aria-label="Search in chat">
        <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <circle cx="11" cy="11" r="7"/><path d="m21 21-4.3-4.3"/>
        </svg>
      </button>
      {#if ontogglepanel}
        <button class="panel-btn" class:active={panelOpen} type="button" onclick={ontogglepanel} aria-label="Chat details">
          <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <circle cx="12" cy="12" r="10"/><path d="M12 16v-4M12 8h.01"/>
          </svg>
        </button>
      {/if}
    </header>

    {#if chatSearchOpen}
      <div class="chat-search">
        <input
          type="search"
          placeholder="Search in this chat"
          bind:value={chatSearchQuery}
        />
        <button type="button" onclick={() => { chatSearchOpen = false; chatSearchQuery = ''; }} aria-label="Close search">×</button>
      </div>
    {/if}

    <div class="messages" bind:this={messagesEl}>
      {#each displayMessages as msg, i (msg.id)}
        {@const prev = displayMessages[i - 1]}
        {@const pos = groupPos(displayMessages, i)}
        {@const showGap = !prev || prev.direction !== msg.direction}
        {@const showDate = !prev || dateKey(prev.timestamp) !== dateKey(msg.timestamp)}
        {@const kind = mediaKindFromMessage(msg)}
        {@const media = mediaUrlFor(msg)}
        {@const caption = captionText(msg)}
        {@const filename = mediaFilename(msg)}
        {@const hasContent = !!kind || !!caption}
        {@const searchHit = chatSearchQuery.trim() && (msg.body || '').toLowerCase().includes(chatSearchQuery.trim().toLowerCase())}
        {#if hasContent && !(chatSearchQuery.trim() && !searchHit)}
        {#if showDate}
          <div class="date-sep"><span>{formatDateLabel(msg.timestamp)}</span></div>
        {/if}
        <div
          class="msg-row"
          role="group"
          class:outbound={msg.direction === 'outbound'}
          class:gap={showGap}
          class:group-single={pos === 'single'}
          class:group-first={pos === 'first'}
          class:group-middle={pos === 'middle'}
          class:group-last={pos === 'last'}
          class:search-hit={searchHit}
          oncontextmenu={(e) => {
            e.preventDefault();
            const sel = window.getSelection()?.toString();
            if (sel && sel.trim()) ontextmenu?.(sel, e.clientX, e.clientY);
            else onmsgmenu?.(msg, e.clientX, e.clientY);
          }}
        >
          <div class="bubble">
            {#if msg.direction === 'inbound' && conversation.conversation_type === 'group' && msg.sender_name && (pos === 'single' || pos === 'first')}
              <span class="sender">{msg.sender_name}</span>
            {/if}
            {#if kind === 'image' || kind === 'sticker'}
              {#if media}
                <button type="button" class="media-open" onclick={() => openLightboxForMessage(msg)} aria-label="View {mediaIconLabel(kind)}">
                  <img class="msg-media" class:sticker={kind === 'sticker'} src={media} alt={caption || mediaIconLabel(kind)} />
                </button>
              {:else}
                <div class="media-fallback" class:failed={mediaUnavailable(msg)}>
                  <svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true">
                    <rect x="3" y="5" width="18" height="14" rx="2"/>
                    <circle cx="8.5" cy="10" r="1.5"/>
                    <path d="m21 15-5-5-4 4-2-2-5 5"/>
                  </svg>
                  <span>{mediaUnavailable(msg) ? `${mediaIconLabel(kind)} unavailable` : `Loading ${mediaIconLabel(kind).toLowerCase()}…`}</span>
                </div>
              {/if}
            {:else if kind === 'video'}
              {#if media}
                <div class="video-wrap">
                  <!-- svelte-ignore a11y_media_has_caption -->
                  <video class="msg-media" src={media} controls></video>
                  <button type="button" class="video-expand" onclick={() => openLightboxForMessage(msg)} aria-label="Expand video">⛶</button>
                </div>
              {:else}
                <div class="media-fallback" class:failed={mediaUnavailable(msg)}>
                  <span>{mediaUnavailable(msg) ? 'Video unavailable' : 'Loading video…'}</span>
                </div>
              {/if}
            {:else if kind === 'audio'}
              {#if media}
                <audio class="msg-audio" src={media} controls></audio>
              {:else}
                <div class="media-fallback" class:failed={mediaUnavailable(msg)}>
                  <span>{mediaUnavailable(msg) ? 'Audio unavailable' : 'Loading audio…'}</span>
                </div>
              {/if}
            {:else if kind === 'document'}
              {#if media}
                <a class="doc-link" href={media} download={filename ?? 'document'}>
                  📎 {filename ?? 'Document'}
                </a>
              {:else}
                <div class="media-fallback" class:failed={mediaUnavailable(msg)}>
                  <span>{mediaUnavailable(msg) ? 'Document unavailable' : `📎 ${filename ?? 'Loading document…'}`}</span>
                </div>
              {/if}
            {:else if kind === 'contact' || kind === 'poll' || kind === 'event' || kind === 'location'}
              <div class="media-card">
                <span class="media-card-label">{mediaIconLabel(kind)}</span>
                {#if caption}
                  <p>{caption}</p>
                {/if}
              </div>
            {/if}
            {#if caption && kind !== 'contact' && kind !== 'poll' && kind !== 'event' && kind !== 'location'}
              <p class:highlight={searchHit}>{caption}</p>
            {/if}
            <span class="msg-time">
              {#if msg.pinned}<span class="msg-pin" title="Pinned">📌</span>{/if}
              {#if msg.starred}<span class="msg-star" title="Starred">★</span>{/if}
              {formatMsgTime(msg.timestamp)}
              {#if msg.direction === 'outbound'}
                <span
                  class="status"
                  class:read={msg.status === 'read'}
                  class:failed={msg.status === 'failed'}
                  aria-label={msg.status === 'failed' ? 'Failed' : msg.status === 'read' ? 'Read' : 'Sent'}
                >
                  {#if msg.status === 'failed'}!
                  {:else if msg.status === 'read' || msg.status === 'delivered'}✓✓
                  {:else}✓{/if}
                </span>
              {/if}
            </span>
          </div>
        </div>
        {/if}
      {/each}
    </div>

    <footer class="composer">
      <input bind:this={fileImageEl} class="hidden-file" type="file" accept="image/*" onchange={(e) => onPickFile('image', e)} />
      <input bind:this={fileVideoEl} class="hidden-file" type="file" accept="video/*" onchange={(e) => onPickFile('video', e)} />
      <input bind:this={fileAudioEl} class="hidden-file" type="file" accept="audio/*" onchange={(e) => onPickFile('audio', e)} />
      <input bind:this={fileDocEl} class="hidden-file" type="file" accept=".pdf,.doc,.docx,.xls,.xlsx,.ppt,.pptx,.txt,.zip,.csv,application/pdf" onchange={(e) => onPickFile('document', e)} />
      <input bind:this={fileGifEl} class="hidden-file" type="file" accept="image/gif,image/webp,.gif" onchange={(e) => onPickFile('gif', e)} />
      <input bind:this={fileStickerEl} class="hidden-file" type="file" accept="image/webp,image/png,.webp,.png" onchange={(e) => onPickFile('sticker', e)} />

      <div class="composer-toolbar">
        <div class="format-bar">
          <button type="button" class="format-btn" onclick={() => applyMark('bold')} aria-label="Bold">B</button>
          <button type="button" class="format-btn" onclick={() => applyMark('italic')} aria-label="Italic"><em>I</em></button>
          <button type="button" class="format-btn" onclick={() => applyMark('strike')} aria-label="Strikethrough">S</button>
          <button type="button" class="format-btn code" onclick={() => applyMark('code')} aria-label="Code">{'</>'}</button>
          <span class="fmt-sep" aria-hidden="true"></span>
          <div class="picker-wrap">
            <button type="button" class="format-btn" class:active={picker === 'emoji'} onclick={() => togglePicker('emoji')} aria-label="Emoji">😀</button>
            {#if picker === 'emoji'}
              <div class="picker-pop emoji-pop" role="dialog" aria-label="Emoji">
                <EmojiPickerPop onpick={insertEmoji} />
              </div>
            {/if}
          </div>
          <div class="picker-wrap">
            <button type="button" class="format-btn" class:active={picker === 'sticker'} onclick={() => togglePicker('sticker')} aria-label="Stickers">🎭</button>
            {#if picker === 'sticker'}
              <div class="picker-pop giphy-pop" role="dialog" aria-label="Stickers">
                <GiphyPicker mode="sticker" onpick={(item) => void sendGiphyItem(item, 'sticker')} />
              </div>
            {/if}
          </div>
          <div class="picker-wrap">
            <button type="button" class="format-btn" class:active={picker === 'gif'} onclick={() => togglePicker('gif')} aria-label="GIFs">GIF</button>
            {#if picker === 'gif'}
              <div class="picker-pop giphy-pop" role="dialog" aria-label="GIFs">
                <GiphyPicker mode="gif" onpick={(item) => void sendGiphyItem(item, 'gif')} />
              </div>
            {/if}
          </div>
        </div>
        <div class="toolbar-right">
          <div class="picker-wrap">
            <button type="button" class="format-btn attach-btn" class:active={picker === 'attach'} onclick={() => togglePicker('attach')} aria-label="Attach">
              <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <path d="M21.44 11.05l-9.19 9.19a6 6 0 0 1-8.49-8.49l9.19-9.19a4 4 0 0 1 5.66 5.66l-9.2 9.19a2 2 0 0 1-2.83-2.83l8.49-8.48"/>
              </svg>
            </button>
            {#if picker === 'attach'}
              <div class="picker-pop attach-pop" role="menu" aria-label="Attach">
                <button type="button" role="menuitem" onclick={() => fileImageEl?.click()}>
                  <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><rect x="3" y="5" width="18" height="14" rx="2"/><circle cx="8.5" cy="10" r="1.5"/><path d="m21 15-5-5-4 4-2-2-5 5"/></svg>
                  Photo
                </button>
                <button type="button" role="menuitem" onclick={() => fileVideoEl?.click()}>
                  <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><rect x="2" y="5" width="20" height="14" rx="2"/><path d="m10 9 6 3-6 3V9z"/></svg>
                  Video
                </button>
                <button type="button" role="menuitem" onclick={() => fileAudioEl?.click()}>
                  <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><path d="M9 18V5l12-2v13"/><circle cx="6" cy="18" r="3"/><circle cx="18" cy="16" r="3"/></svg>
                  Audio
                </button>
                <button type="button" role="menuitem" onclick={() => fileDocEl?.click()}>
                  <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"/><path d="M14 2v6h6M16 13H8M16 17H8M10 9H8"/></svg>
                  Document
                </button>
                <button type="button" role="menuitem" onclick={() => { picker = 'location'; void useMyLocation(); }}>
                  <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><path d="M12 21s7-4.5 7-11a7 7 0 1 0-14 0c0 6.5 7 11 7 11z"/><circle cx="12" cy="10" r="2.5"/></svg>
                  Location
                </button>
                <button type="button" role="menuitem" onclick={() => void toggleRecord()}>
                  <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><path d="M12 14a3 3 0 0 0 3-3V6a3 3 0 1 0-6 0v5a3 3 0 0 0 3 3z"/><path d="M19 11a7 7 0 0 1-14 0M12 18v3"/></svg>
                  {recording ? 'Stop recording' : 'Voice note'}
                </button>
                <button type="button" role="menuitem" onclick={() => (picker = 'poll')}>
                  <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><path d="M4 19h16M6 16V9M12 16V5M18 16v-4"/></svg>
                  Poll
                </button>
              </div>
            {/if}
            {#if picker === 'location'}
              <div class="picker-pop form-pop" role="dialog" aria-label="Send location">
                <p class="picker-label">Location pin</p>
                <label>Latitude <input type="text" bind:value={locLat} /></label>
                <label>Longitude <input type="text" bind:value={locLng} /></label>
                <div class="form-actions">
                  <button type="button" class="picker-action ghost" onclick={() => void useMyLocation()} disabled={locBusy}>
                    {locBusy ? 'Locating…' : 'Use my location'}
                  </button>
                  <button type="button" class="picker-action" onclick={sendLocation}>Send pin</button>
                </div>
              </div>
            {/if}
            {#if picker === 'poll'}
              <div class="picker-pop form-pop" role="dialog" aria-label="Create poll">
                <p class="picker-label">Poll</p>
                <label>Question <input type="text" bind:value={pollQuestion} placeholder="Ask something" /></label>
                {#each pollOptions as _opt, i (i)}
                  <label>Option {i + 1} <input type="text" bind:value={pollOptions[i]} /></label>
                {/each}
                {#if pollOptions.length < 12}
                  <button type="button" class="picker-action ghost" onclick={() => (pollOptions = [...pollOptions, ''])}>Add option</button>
                {/if}
                <button type="button" class="picker-action" onclick={sendPoll}>Send poll</button>
              </div>
            {/if}
          </div>
          {#if onsendlater}
            <div class="picker-wrap">
              <button
                class="later-btn"
                class:active={picker === 'later'}
                type="button"
                onclick={() => togglePicker('later')}
                disabled={!draft.trim()}
                aria-label="Send later"
              >
                <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                  <circle cx="12" cy="12" r="9"/>
                  <path d="M12 7v5l3 2"/>
                </svg>
              </button>
              {#if picker === 'later'}
                <div class="picker-pop form-pop later-pop" role="dialog" aria-label="Send later">
                  <p class="picker-label">Send later</p>
                  <label>Date and time <input type="datetime-local" bind:value={laterAt} /></label>
                  <div class="form-actions">
                    <button type="button" class="picker-action ghost" onclick={() => (picker = null)}>Cancel</button>
                    <button type="button" class="picker-action" onclick={submitSendLater} disabled={!laterAt}>Schedule</button>
                  </div>
                </div>
              {/if}
            </div>
          {/if}
        </div>
      </div>
      {#if pendingFile}
        <div class="pending-chip">
          <span>{pendingFile.kind}: {pendingFile.filename}</span>
          <button type="button" onclick={() => (pendingFile = null)} aria-label="Remove attachment">×</button>
        </div>
      {/if}
      {#if recording}
        <div class="pending-chip recording">
          <span>Recording voice note…</span>
          <button type="button" onclick={() => void toggleRecord()}>Stop</button>
        </div>
      {/if}
      <div class="composer-inner">
        <textarea
          bind:this={composerEl}
          placeholder="Type a message…"
          rows="1"
          value={draft}
          oninput={(e) => {
            ondraft(e.currentTarget.value);
            resizeComposer();
          }}
          onkeydown={handleKeydown}
          aria-label="Message input"
        ></textarea>
        <button class="send-btn" onclick={submitComposer} disabled={!canSend()} aria-label="Send">
          <svg width="18" height="18" viewBox="0 0 24 24" fill="currentColor">
            <path d="M2.01 21L23 12 2.01 3 2 10l15 2-15 2z"/>
          </svg>
        </button>
      </div>
    </footer>
    <MediaLightbox
      open={lightboxOpen}
      items={lightboxItems}
      index={lightboxIndex}
      onclose={() => (lightboxOpen = false)}
      onindex={(i) => (lightboxIndex = i)}
      onsave={saveLightboxItem}
      onedit={editLightboxItem}
    />
    <ImageEditor
      open={editorOpen}
      src={editorSrc}
      filename={editorFilename}
      mime={editorMime}
      oncancel={closeEditor}
      onsend={onEditorSend}
      onskip={editorOriginal ? onEditorSkip : undefined}
    />
  {:else}
    <div class="empty-thread">
      <div class="empty-icon">
        <svg width="56" height="56" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.25">
          <path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z"/>
        </svg>
      </div>
      <h2>Select a conversation</h2>
      <p>Pick a chat from the sidebar to start messaging across all your networks.</p>
    </div>
  {/if}
</div>

<style>
  .thread {
    flex: 1;
    display: flex;
    flex-direction: column;
    background: var(--bg-main);
    min-width: 0;
    min-height: 0;
  }

  .thread-header {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 14px 20px;
    border-bottom: 1px solid var(--border-subtle);
    background: var(--bg-panel);
    flex-shrink: 0;
    min-width: 0;
  }

  .back-btn {
    border: none;
    background: transparent;
    color: var(--text);
    cursor: pointer;
    padding: 6px;
    border-radius: var(--radius-sm);
    display: none;
    margin-left: -6px;
  }

  .back-btn:hover {
    background: var(--bg-hover);
  }

  .header-avatar-wrap {
    position: relative;
    flex-shrink: 0;
  }

  .header-avatar {
    width: 36px;
    height: 36px;
    border-radius: 50%;
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 13px;
    font-weight: 600;
    color: white;
    overflow: hidden;
  }

  .avatar-img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
  }

  .header-network-badge {
    position: absolute;
    bottom: -2px;
    right: -2px;
    width: 15px;
    height: 15px;
    border-radius: 50%;
    color: white;
    display: flex;
    align-items: center;
    justify-content: center;
    border: 2px solid var(--bg-panel);
  }

  .header-info {
    min-width: 0;
    flex: 1 1 auto;
    border: none;
    background: transparent;
    text-align: left;
    cursor: pointer;
    padding: 2px 4px;
    border-radius: var(--radius-sm);
    color: inherit;
    overflow: hidden;
  }

  .header-info:hover {
    background: var(--bg-hover);
  }

  .header-info h2 {
    font-size: 15px;
    font-weight: 600;
    letter-spacing: -0.02em;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .subtitle {
    font-size: 12px;
    color: var(--text-muted);
    display: block;
    margin-top: 1px;
  }

  .panel-btn {
    flex: 0 0 auto;
    width: 34px;
    height: 34px;
    margin-left: 4px;
    border: none;
    background: transparent;
    color: var(--text-muted);
    cursor: pointer;
    padding: 0;
    border-radius: var(--radius-sm);
    display: inline-flex;
    align-items: center;
    justify-content: center;
  }
  .panel-btn:hover, .panel-btn.active {
    background: var(--bg-hover);
    color: var(--accent);
  }

  .messages {
    flex: 1;
    overflow-y: auto;
    padding: 20px 24px;
    display: flex;
    flex-direction: column;
    gap: 1px;
  }

  .date-sep {
    display: flex;
    justify-content: center;
    margin: 14px 0 10px;
  }

  .date-sep span {
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    color: var(--text-muted);
    background: var(--bg-panel);
    border: 1px solid var(--border-subtle);
    border-radius: 999px;
    padding: 3px 10px;
  }

  .msg-row {
    display: flex;
    justify-content: flex-start;
    margin-bottom: 2px;
  }

  .msg-row.gap {
    margin-top: 10px;
  }

  .msg-row.gap:first-child {
    margin-top: 0;
  }

  .msg-row.outbound {
    justify-content: flex-end;
  }

  .bubble {
    max-width: min(520px, 68%);
    padding: 5px 9px 4px;
    border-radius: 18px;
    background: var(--bg-bubble-in);
    position: relative;
    box-shadow: var(--shadow-sm);
  }

  /* Inbound grouping — tail on bottom-left */
  .msg-row:not(.outbound).group-single .bubble,
  .msg-row:not(.outbound).group-last .bubble {
    border-radius: 18px 18px 18px 5px;
  }

  .msg-row:not(.outbound).group-first .bubble {
    border-radius: 18px 18px 5px 5px;
  }

  .msg-row:not(.outbound).group-middle .bubble {
    border-radius: 5px 18px 5px 5px;
  }

  .outbound .bubble {
    background: var(--bg-bubble-out, var(--primary, var(--accent)));
    color: var(--text-on-accent, var(--primary-foreground, #ffffff));
    box-shadow: var(--shadow-sm);
  }

  /* Outbound grouping — tail on bottom-right */
  .msg-row.outbound.group-single .bubble {
    border-radius: 18px 18px 5px 18px;
  }

  .msg-row.outbound.group-first .bubble {
    border-radius: 18px 18px 5px 18px;
  }

  .msg-row.outbound.group-middle .bubble {
    border-radius: 18px 5px 5px 18px;
  }

  .msg-row.outbound.group-last .bubble {
    border-radius: 5px 18px 5px 18px;
  }

  .msg-row.group-middle .bubble,
  .msg-row.group-first:not(.group-single) .bubble {
    padding-bottom: 3px;
  }

  .msg-row.group-first:not(.group-single) .bubble,
  .msg-row.group-middle .bubble {
    padding-top: 3px;
  }

  .sender {
    display: block;
    font-size: 11px;
    font-weight: 600;
    color: var(--accent);
    margin-bottom: 3px;
    letter-spacing: 0.01em;
  }

  .bubble p {
    font-size: 1rem;
    line-height: 1.4;
    word-wrap: break-word;
    white-space: pre-wrap;
    letter-spacing: -0.01em;
  }

  .outbound .bubble p {
    color: inherit;
  }

  .msg-time {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 3px;
    font-size: 9px;
    line-height: 1;
    color: var(--text-muted);
    margin-top: 1px;
    letter-spacing: 0.01em;
    opacity: 0.85;
  }

  .outbound .msg-time {
    color: color-mix(
      in oklch,
      var(--text-on-accent, var(--primary-foreground, #ffffff)) 62%,
      transparent
    );
  }

  .media-open {
    display: block;
    padding: 0;
    border: none;
    background: transparent;
    cursor: zoom-in;
    line-height: 0;
  }

  .video-wrap {
    position: relative;
    display: inline-block;
    max-width: min(240px, 100%);
  }

  .video-expand {
    position: absolute;
    top: 6px;
    right: 6px;
    width: 28px;
    height: 28px;
    border: none;
    border-radius: 6px;
    background: rgba(0, 0, 0, 0.55);
    color: white;
    font-size: 14px;
    cursor: pointer;
    line-height: 1;
    opacity: 0.85;
  }

  .video-expand:hover {
    opacity: 1;
  }

  .msg-media {
    display: block;
    max-width: min(240px, 100%);
    max-height: 280px;
    border-radius: 10px;
    margin: 0;
    object-fit: cover;
  }

  .media-fallback {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 10px 8px;
    margin-bottom: 0;
    border-radius: 8px;
    background: color-mix(in srgb, currentColor 8%, transparent);
    font-size: 12px;
    color: var(--text-muted);
  }

  .outbound .media-fallback {
    color: color-mix(
      in oklch,
      var(--text-on-accent, var(--primary-foreground, #ffffff)) 80%,
      transparent
    );
  }

  .media-fallback.failed {
    opacity: 0.85;
  }

  .status {
    font-size: 10px;
    opacity: 0.7;
  }

  .status.read {
    color: var(--accent);
    opacity: 1;
  }

  .outbound .status.read {
    color: var(--text-on-accent);
    opacity: 0.95;
  }

  .status.failed {
    color: var(--destructive, #ef4444);
    opacity: 1;
  }

  .composer {
    padding: 12px 20px 18px;
    border-top: 1px solid var(--border-subtle);
    background: var(--bg-panel);
    flex-shrink: 0;
  }

  .composer-toolbar {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 8px;
    min-width: 0;
    overflow: visible;
  }

  .format-bar {
    display: flex;
    align-items: center;
    gap: 4px;
    flex-wrap: nowrap;
    overflow: visible;
  }

  .format-btn {
    border: 1px solid var(--border);
    background: var(--bg-input);
    color: var(--text);
    border-radius: var(--radius-sm);
    width: 28px;
    height: 28px;
    min-width: 28px;
    padding: 0;
    cursor: pointer;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    font-size: 12px;
    line-height: 1;
  }

  .format-btn.code {
    font-size: 11px;
  }

  .later-btn.active,
  .format-btn.active {
    border-color: var(--accent);
    background: var(--accent-muted);
  }

  .fmt-sep {
    width: 1px;
    height: 16px;
    background: var(--border);
    margin: 0 4px;
  }

  .toolbar-right {
    margin-left: auto;
    display: flex;
    align-items: center;
    gap: 4px;
  }

  .picker-wrap {
    position: relative;
  }

  .picker-pop {
    position: absolute;
    bottom: calc(100% + 8px);
    left: 0;
    z-index: 200;
    width: min(280px, 70vw);
    max-height: 260px;
    overflow: auto;
    padding: 10px;
    border-radius: var(--radius-md);
    border: 1px solid var(--border);
    background: var(--bg-panel);
    box-shadow: var(--shadow-md, 0 8px 24px rgba(0, 0, 0, 0.18));
  }

  .picker-pop.emoji-pop,
  .picker-pop.giphy-pop {
    width: min(320px, 92vw);
    max-height: none;
    overflow: visible;
    padding: 0;
  }

  .attach-pop {
    left: auto;
    right: 0;
    width: 210px;
    padding: 6px;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .attach-pop button[role='menuitem'] {
    display: flex;
    align-items: center;
    gap: 10px;
    text-align: left;
    padding: 8px 10px;
    border: none;
    background: transparent;
    border-radius: 8px;
    cursor: pointer;
    font-size: 13px;
    color: var(--text-primary);
  }

  .attach-pop button[role='menuitem']:hover {
    background: var(--bg-hover);
  }

  .attach-pop svg {
    flex-shrink: 0;
    opacity: 0.75;
  }

  .attach-pop button,
  .picker-action {
    border: none;
    background: transparent;
    color: var(--text);
    text-align: left;
    padding: 8px 10px;
    border-radius: var(--radius-sm);
    cursor: pointer;
    font-size: 13px;
    width: 100%;
  }

  .attach-pop button:hover,
  .picker-action:hover {
    background: var(--bg-hover);
  }

  .picker-action {
    background: var(--bg-input);
    border: 1px solid var(--border);
    margin-top: 8px;
    text-align: center;
  }

  .picker-action.ghost {
    background: transparent;
  }

  .picker-label {
    margin: 0 0 8px;
    font-size: 12px;
    font-weight: 600;
  }

  .picker-hint {
    margin: 0;
    font-size: 12px;
    color: var(--text-muted);
  }

  .form-pop {
    width: min(260px, 72vw);
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .form-pop label {
    display: flex;
    flex-direction: column;
    gap: 4px;
    font-size: 11px;
    color: var(--text-muted);
  }

  .form-pop input {
    border: 1px solid var(--border);
    background: var(--bg-input);
    color: var(--text);
    border-radius: var(--radius-sm);
    padding: 6px 8px;
    font-size: 13px;
  }

  .form-actions {
    display: flex;
    gap: 6px;
  }

  .pending-chip {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 8px;
    padding: 6px 10px;
    border-radius: var(--radius-sm);
    background: var(--bg-input);
    font-size: 12px;
    color: var(--text-secondary);
  }

  .pending-chip button {
    margin-left: auto;
    border: none;
    background: transparent;
    color: var(--text);
    cursor: pointer;
    font-size: 16px;
  }

  .pending-chip.recording {
    color: #ef4444;
  }

  .hidden-file {
    display: none;
  }

  .composer-inner {
    display: flex;
    align-items: flex-end;
    gap: 10px;
    background: var(--bg-input);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    padding: 6px 6px 6px 16px;
    transition: border-color 0.15s ease, box-shadow 0.15s ease;
  }

  .composer-inner:focus-within {
    border-color: var(--accent);
    box-shadow: 0 0 0 3px var(--accent-muted);
  }

  textarea {
    flex: 1;
    resize: none;
    border: none;
    background: transparent;
    color: var(--text);
    font-size: 1rem;
    line-height: 1.45;
    max-height: calc(1.45em * 4 + 12px);
    overflow-y: auto;
    outline: none;
    padding: 6px 0;
    min-height: 1.45em;
  }

  textarea::selection {
    background: color-mix(in srgb, var(--accent) 35%, transparent);
    color: var(--text);
  }

  textarea::placeholder {
    color: var(--text-muted);
  }

  .send-btn {
    width: 36px;
    height: 36px;
    border: none;
    border-radius: var(--radius-md);
    background: var(--accent);
    color: var(--text-on-accent, var(--primary-foreground, #ffffff));
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
    transition: background 0.15s ease, transform 0.1s ease, opacity 0.15s ease;
  }

  .msg-media.sticker {
    max-width: 160px;
    max-height: 160px;
    background: transparent;
  }

  .msg-audio {
    width: min(260px, 100%);
    height: 36px;
    margin-bottom: 4px;
  }

  .doc-link {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 8px 10px;
    margin-bottom: 4px;
    border-radius: 8px;
    background: color-mix(in srgb, currentColor 8%, transparent);
    color: inherit;
    text-decoration: none;
    font-size: 13px;
    word-break: break-all;
  }

  .media-card {
    padding: 8px 10px;
    margin-bottom: 4px;
    border-radius: 8px;
    background: color-mix(in srgb, currentColor 8%, transparent);
  }

  .media-card-label {
    display: block;
    font-size: 12px;
    font-weight: 600;
    margin-bottom: 4px;
  }

  .media-card p {
    margin: 0;
    font-size: 13px;
    white-space: pre-wrap;
  }

  .chat-search {
    display: flex;
    gap: 8px;
    padding: 8px 12px;
    border-bottom: 1px solid var(--border-subtle);
    background: var(--bg-panel);
  }

  .chat-search input {
    flex: 1;
    border: 1px solid var(--border-subtle);
    border-radius: 8px;
    padding: 6px 10px;
    background: var(--bg-main);
    color: var(--text-primary);
    font-size: 13px;
  }

  .chat-search button {
    border: none;
    background: transparent;
    color: var(--text-muted);
    font-size: 20px;
    cursor: pointer;
  }

  .msg-row.search-hit .bubble {
    outline: 1px solid var(--accent);
  }

  .highlight {
    background: var(--accent-muted);
    border-radius: 4px;
  }

  .msg-star {
    color: #eab308;
  }

  .msg-pin {
    font-size: 9px;
  }

  .later-pop {
    left: auto;
    right: 0;
    width: min(260px, 72vw);
  }

  .later-btn {
    flex: 0 0 auto;
    width: 28px;
    height: 28px;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--text-secondary);
    cursor: pointer;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: 0;
  }

  .later-btn:disabled {
    opacity: 0.35;
    cursor: default;
  }

  .send-btn:disabled {
    opacity: 0.3;
    cursor: default;
  }

  .send-btn:not(:disabled):hover {
    background: var(--accent-hover);
  }

  .send-btn:not(:disabled):active {
    transform: scale(0.95);
  }

  .empty-thread {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    color: var(--text-muted);
    text-align: center;
    padding: 32px;
    background: radial-gradient(ellipse at center, var(--accent-muted) 0%, transparent 70%);
  }

  .empty-icon {
    opacity: 0.2;
    margin-bottom: 20px;
    color: var(--accent);
  }

  .empty-thread h2 {
    font-size: 20px;
    font-weight: 600;
    color: var(--text);
    margin-bottom: 8px;
    letter-spacing: -0.02em;
  }

  .empty-thread p {
    font-size: 14px;
    max-width: 320px;
    line-height: 1.5;
    color: var(--text-muted);
  }

  @media (max-width: 768px) {
    .thread {
      width: 100%;
    }

    .thread-header {
      padding: calc(10px + var(--safe-top)) 12px 10px 8px;
      gap: 10px;
    }

    .back-btn {
      display: flex;
      align-items: center;
      justify-content: center;
      width: 44px;
      height: 44px;
      min-width: 44px;
      min-height: 44px;
      padding: 0;
      margin-left: 0;
      flex-shrink: 0;
      touch-action: manipulation;
      -webkit-tap-highlight-color: transparent;
    }

    .back-btn:active {
      background: var(--bg-hover);
    }

    .header-avatar {
      width: 40px;
      height: 40px;
      font-size: 14px;
    }

    .header-info h2 {
      font-size: 16px;
    }

    .messages {
      padding: 16px 12px;
      padding-bottom: calc(16px + var(--safe-bottom));
    }

    .bubble {
      max-width: min(520px, 82%);
    }

    .composer {
      padding: 10px 12px calc(10px + var(--safe-bottom));
    }

    .composer-inner {
      min-height: 44px;
      padding: 4px 4px 4px 14px;
    }

    .send-btn {
      width: 44px;
      height: 44px;
      min-width: 44px;
      min-height: 44px;
      touch-action: manipulation;
    }
    .later-btn {
      width: 28px;
      height: 28px;
      min-height: 28px;
    }

    .empty-thread {
      display: none;
    }
  }
</style>
