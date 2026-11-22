<script lang="ts">
  import { untrack } from 'svelte';
  import { SvelteSet } from 'svelte/reactivity';
  import DateTimePopover from '$lib/components/DateTimePopover.svelte';
  import { nextMinute, toLocalDateTime } from '$lib/localDateTime';
  import { avatarColor, conversationAvatar, downloadMessageMedia, downloadMessageMediaAndWait, fetchContactProfile, getInitials, listContacts, openExternal, readMessageMedia } from '$lib/api';
  import { mediaNameOwner, saveMessageMediaFile, shouldAutoDownload } from '$lib/mediaSave';
  import NetworkIcon from '$lib/components/NetworkIcon.svelte';
  import EmojiPickerPop from '$lib/components/EmojiPickerPop.svelte';
  import GiphyPicker from '$lib/components/GiphyPicker.svelte';
  import ImageEditor from '$lib/components/ImageEditor.svelte';
  import MediaLightbox, { type LightboxItem } from '$lib/components/MediaLightbox.svelte';
  import PollCard from '$lib/components/PollCard.svelte';
  import { pollDataFromMessage } from '$lib/poll';
  import { giphyUrlToBase64, type GiphyItem } from '$lib/giphy';
  import {
    captionText,
    buildLightboxItems,
    formatFileSize,
    mediaDataFromMessage,
    mediaFailed,
    mediaFilename,
    mediaIconLabel,
    mediaKindFromMessage,
    mediaPathFromMessage,
    mediaSizeFromMessage,
    mediaThumbPathFromMessage,
    resolveThumbUrl,
    shouldDownloadMedia,
  } from '$lib/messageMedia';
  import {
    activeMention,
    buildMentionResolver,
    expandMentions,
    filterMentionCandidates,
    mentionCandidates,
    type GroupMember,
    type MentionCandidate,
  } from '$lib/mentions';
  import { whatsappBlocks } from '$lib/whatsappMarkup';
  import { whatsappCallFallback } from '$lib/whatsappLinks';
  import { SELECTION_ACTIONS, isEdited, isRevoked, type SelectionAction, type TranslationView } from '$lib/messageActions';
  import { bodySegments } from '$lib/structuredText';
  import MessageQuote from '$lib/components/MessageQuote.svelte';
  import { messageSnippet, quoteLabel } from '$lib/messageSnippet';
  import { wrapSelection } from '$lib/richText';
  import { composeLocked } from '$lib/composeLock';
  import { senderHue, senderKey, senderLabel } from '$lib/senderIdentity';
  import { CONNECTOR_COLORS, type Account, type AttachmentKind, type AttachmentPayload, type Contact, type Conversation, type Message, type ReactionGroup } from '$lib/types';
  import {
    classifyFile,
    removeAttachment,
    type ComposerFileKind,
    type PendingAttachment,
  } from '$lib/attachments';
  import {
    formatClock,
    peaksFromChannelData,
    recorderControls,
    recorderNext,
    recordErrorMessage,
    type RecorderAction,
    type RecorderState,
  } from '$lib/recorder';
  import PendingAttachments from '$lib/components/PendingAttachments.svelte';
  import VoiceClip from '$lib/components/VoiceClip.svelte';

  /** The take the player is showing, once stop produced audio to send. */
  interface RecordedTake {
    state: RecorderState;
    error: string;
    elapsedMs: number;
    level: number;
    clip: null | { blob: Blob; mime: string; filename: string; url: string; durationMs: number; peaks: number[] };
  }

  interface Props {
    conversation: Conversation | null;
    messages: Message[];
    /** The first read of this chat is still running: show a spinner, not the empty state. */
    loading?: boolean;
    accounts: Account[];
    draft: string;
    ondraft: (v: string) => void;
    /** `text` is the draft with picked @names written as the @<number> tokens the network reads. */
    onsend: (text?: string) => void;
    onsendattachment?: (payload: AttachmentPayload) => void | Promise<void>;
    onsendlater?: (sendAt: string) => void;
    /** The message the next send answers (shown in a bar above the input). */
    replyingTo?: Message | null;
    oncancelreply?: () => void;
    /** Jump to a message by its local id (the quote of a reply links to its original). */
    onjumptomessage?: (messageId: string) => void;
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
    /** Translations shown under messages, by message id. */
    translations?: Record<string, TranslationView>;
    ondismisstranslation?: (messageId: string) => void;
    /** An action of the selection bar, with the selected messages in chat order. */
    onselectionaction?: (action: SelectionAction, msgs: Message[]) => void;
  }

  let {
    conversation,
    messages,
    loading = false,
    accounts,
    draft,
    ondraft,
    onsend,
    onsendattachment,
    onsendlater,
    replyingTo = null,
    oncancelreply,
    onjumptomessage,
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
    translations = {},
    ondismisstranslation,
    onselectionaction,
  }: Props = $props();
  let composerEl = $state<HTMLTextAreaElement | null>(null);
  /** This chat refuses messages from this account (an admin-only group). */
  let composerLocked = $derived(conversation ? composeLocked(conversation) : false);
  let mediaUrls = $state<Record<string, string>>({});
  let picker = $state<null | 'emoji' | 'sticker' | 'gif' | 'attach' | 'location' | 'poll' | 'later'>(null);
  let laterBtn = $state<HTMLButtonElement | null>(null);
  let pending = $state<PendingAttachment[]>([]);
  let dropActive = $state(false);
  let locLat = $state('');
  let locLng = $state('');
  let locBusy = $state(false);
  let pollQuestion = $state('');
  let pollOptions = $state(['', '']);
  let rec = $state<RecordedTake>({ state: 'idle', error: '', elapsedMs: 0, level: 0, clip: null });
  let recPauseEl = $state<HTMLButtonElement | null>(null);
  let recRecorder: MediaRecorder | null = null;
  let recStream: MediaStream | null = null;
  let recChunks: Blob[] = [];
  let recClock: ReturnType<typeof setInterval> | null = null;
  let recMeter: number | null = null;
  let recAudio: AudioContext | null = null;
  let recStartedAt = 0;
  let recPausedAt = 0;
  let recPausedTotal = 0;
  /** How many nested dragenter events are open, so leaving one child does not clear the overlay. */
  let dragDepth = 0;
  let fileImageEl = $state<HTMLInputElement | null>(null);
  let fileVideoEl = $state<HTMLInputElement | null>(null);
  let fileAudioEl = $state<HTMLInputElement | null>(null);
  let fileDocEl = $state<HTMLInputElement | null>(null);
  let chatSearchOpen = $state(false);
  let chatSearchQuery = $state('');
  let messagesEl = $state<HTMLDivElement | null>(null);
  let lightboxOpen = $state(false);
  let lightboxId = $state<string | null>(null);
  let editorOpen = $state(false);
  let editorSrc = $state('');
  let editorFilename = $state('');
  let editorMime = $state('');
  let editorOriginal = $state<null | { kind: AttachmentKind; filename: string; mime: string; data_base64: string }>(null);
  /** The picked image itself, kept so "send as is" puts the real file back in the bar. */
  let editorSourceFile: File | null = null;

  const displayMessages = $derived(
    [...messages].sort((a, b) => {
      if (a.pinned !== b.pinned) return a.pinned ? -1 : 1;
      return new Date(a.timestamp).getTime() - new Date(b.timestamp).getTime();
    })
  );

  const visibleMessages = $derived(
    displayMessages.filter((msg) => {
      const kind = mediaKindFromMessage(msg);
      const caption = captionText(msg);
      return Boolean(kind || (caption && caption.trim()) || isRevoked(msg));
    })
  );

  const listAheadOfBodies = $derived.by(() => {
    const listAt = conversation?.last_message_at
      ? Date.parse(conversation.last_message_at)
      : NaN;
    if (!Number.isFinite(listAt) || visibleMessages.length === 0) return false;
    let newest = 0;
    for (const msg of visibleMessages) {
      const t = Date.parse(msg.timestamp);
      if (Number.isFinite(t) && t > newest) newest = t;
    }
    return listAt - newest > 120_000;
  });

  const account = $derived(
    conversation ? accounts.find((a) => a.id === conversation.account_id) : null
  );

  /** Selected message ids while in selection mode; null when not selecting. */
  let selected = $state<SvelteSet<string> | null>(null);
  /** Where a shift-click range starts. */
  let selectAnchor: string | null = null;
  const selectedMessages = $derived(
    selected
      ? messages
          .filter((m) => selected?.has(m.id))
          .sort((a, b) => new Date(a.timestamp).getTime() - new Date(b.timestamp).getTime())
      : []
  );

  $effect(() => {
    void conversation?.id;
    untrack(() => cancelSelection());
  });

  export function startSelection(msg: Message) {
    selected = new SvelteSet([msg.id]);
    selectAnchor = msg.id;
  }

  /** Leave selection mode. False when it was not on. */
  export function cancelSelection(): boolean {
    if (!selected) return false;
    selected = null;
    selectAnchor = null;
    return true;
  }

  function toggleSelected(msg: Message, range: boolean) {
    if (!selected) return;
    const ids = visibleMessages.map((m) => m.id);
    const from = range && selectAnchor ? ids.indexOf(selectAnchor) : -1;
    const to = ids.indexOf(msg.id);
    if (from >= 0 && to >= 0) {
      for (const id of ids.slice(Math.min(from, to), Math.max(from, to) + 1)) selected.add(id);
    } else if (selected.has(msg.id)) {
      selected.delete(msg.id);
    } else {
      selected.add(msg.id);
    }
    selectAnchor = msg.id;
  }

  const connectorCaps = $derived(
    account ? connectors.find((c) => c.id === account.connector_id)?.capabilities ?? [] : []
  );
  /**
   * How the contact bar's call buttons act. A connector that advertises `calls:` places the call
   * itself; WhatsApp cannot (GOWA v9.9.0 has no call route), so its buttons open WhatsApp on the
   * chat instead, where the phone or the desktop app starts the call.
   */
  const callTarget = $derived.by(() => {
    if (conversation?.conversation_type !== 'direct') return null;
    if (connectorCaps.some((c) => c.startsWith('calls:'))) return 'connector' as const;
    if (account?.connector_id === 'whatsapp') return 'whatsapp' as const;
    return null;
  });
  const canVideoCall = $derived(
    callTarget === 'whatsapp' || (callTarget === 'connector' && connectorCaps.includes('calls:video'))
  );
  /** WhatsApp writes `*bold*`; the other networks do not, so only WhatsApp gets the markup renderer. */
  const whatsappMarkup = $derived(account?.connector_id === 'whatsapp');

  /** WhatsApp cannot place the call from here, so the buttons open a small panel under them. */
  let callMenu = $state<'audio' | 'video' | null>(null);
  const callFallback = $derived(
    callMenu ? whatsappCallFallback(callMenu, conversation?.remote_id) : null
  );

  function startCall(mode: 'audio' | 'video') {
    if (callTarget === 'connector') {
      onstartcall?.(mode);
      return;
    }
    callMenu = callMenu === mode ? null : mode;
  }

  function openCallInWhatsApp() {
    const url = callFallback?.url;
    callMenu = null;
    if (url) void openExternal(url);
  }
  function handleKeydown(e: KeyboardEvent) {
    if (mentionOpen) {
      if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
        e.preventDefault();
        const n = mentionMatches.length;
        mentionIndex = (mentionIndex + (e.key === 'ArrowDown' ? 1 : n - 1)) % n;
        return;
      }
      if (e.key === 'Enter' || e.key === 'Tab') {
        e.preventDefault();
        pickMention(mentionMatches[mentionIndex]);
        return;
      }
      if (e.key === 'Escape') {
        e.preventDefault();
        mentionDismissed = mention?.start ?? null;
        return;
      }
    }
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      void submitComposer();
    }
    if (e.key === 'Escape') {
      if (pasteChoice) pasteChoice = null;
      else if (picker) picker = null;
      else if (replyingTo) oncancelreply?.();
    }
  }

  // ---- Replies ------------------------------------------------------------------------------

  /** Preview images of quoted messages, by the path the quote names. */
  let quoteThumbs = $state<Record<string, string>>({});

  function quoteThumbPath(msg: Message): string | null {
    const thumb = mediaThumbPathFromMessage(msg);
    if (thumb) return thumb;
    const kind = mediaKindFromMessage(msg);
    return kind === 'image' || kind === 'photo' || kind === 'sticker' ? mediaPathFromMessage(msg) : null;
  }

  function loadQuoteThumb(path: string | null | undefined) {
    if (!path || quoteThumbs[path]) return;
    void resolveThumbUrl(path).then((url) => {
      if (url) quoteThumbs = { ...quoteThumbs, [path]: url };
    });
  }

  $effect(() => {
    for (const m of messages) loadQuoteThumb(m.reply?.thumbnail_path);
  });

  $effect(() => {
    if (!replyingTo) return;
    loadQuoteThumb(quoteThumbPath(replyingTo));
    // Choosing Reply moves the caret to the input, ready to type the answer.
    untrack(() => composerEl?.focus());
  });

  const replyingToWho = $derived(
    replyingTo
      ? replyingTo.direction === 'outbound'
        ? 'You'
        : senderLabel(replyingTo)
      : ''
  );
  const replyBarThumb = $derived.by(() => {
    const path = replyingTo ? quoteThumbPath(replyingTo) : null;
    return path ? (quoteThumbs[path] ?? null) : null;
  });

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
  /** Local file reads under way, so a re-run of the media effect never repeats one. */
  const readsInFlight = new Set<string>();
  const mediaTimeouts = new Map<string, ReturnType<typeof setTimeout>>();
  // Audio the user pressed play on while it was not downloaded: play it once it arrives.
  const playWhenReady = new Set<string>();
  let mediaTimedOut = $state<Set<string>>(new Set());
  /** Downloads in flight (started by the user or automatically). */
  let downloading = $state<Set<string>>(new Set());
  /** Resolved preview images of media that is not downloaded. */
  let thumbUrls = $state<Record<string, string>>({});

  function toggleIn(set: Set<string>, id: string, on: boolean): Set<string> {
    if (set.has(id) === on) return set;
    const next = new Set(set);
    if (on) next.add(id);
    else next.delete(id);
    return next;
  }

  /** The user asked for this media (thumbnail click, Download or Retry, play on audio). */
  async function startDownload(msg: Message, playAfter = false) {
    if (!conversation || downloading.has(msg.id)) return;
    if (playAfter) playWhenReady.add(msg.id);
    mediaTimedOut = toggleIn(mediaTimedOut, msg.id, false);
    downloading = toggleIn(downloading, msg.id, true);
    try {
      await downloadMessageMediaAndWait(conversation.account_id, conversation.id, msg.id);
    } catch {
      // The connector event contains the safe, actionable reason displayed below.
    } finally {
      downloading = toggleIn(downloading, msg.id, false);
    }
  }

  function mediaUnavailable(msg: Message): boolean {
    return mediaFailed(msg) || mediaTimedOut.has(msg.id);
  }

  function playOnReady(node: HTMLAudioElement, id: string) {
    if (playWhenReady.delete(id)) void node.play().catch(() => {});
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
    // Reading the draft is the dependency here, not a value: a new draft resizes the box.
    // eslint-disable-next-line no-unused-expressions, @typescript-eslint/no-unused-expressions
    draft;
    queueMicrotask(resizeComposer);
  });

  // ---- Scroll: follow the newest message only while the reader is at the bottom -----------
  //
  // The reader's position lives in plain variables, not `$state`: the follow-up effects must
  // never re-run because the reader scrolled, and nothing here may move the view unless a
  // message arrived, the chat changed or the reader sent something. (The old effect tracked a
  // `stickToBottom` state and snapped to the end whenever the last message was outbound, so
  // the first wheel tick away from the bottom flipped that state, re-ran the effect and put
  // the view back; every reaction, receipt or thumbnail update re-ran it the same way.)
  const BOTTOM_THRESHOLD = 80;
  const AT_END = 24;
  let atBottom = true;
  /** The reader moved up on purpose (wheel, touch): stays set until they are back at the end. */
  let leftBottom = false;
  let pendingSnapToLatest = false;
  let prevConvId: string | null = null;
  let prevFirstId: string | null = null;
  let prevLastId: string | null = null;
  let anchorHeight = 0;
  let anchorTop = 0;

  function scrollMessagesToLatest() {
    const el = messagesEl;
    if (!el) return;
    el.scrollTop = el.scrollHeight;
  }

  function snapToLatest() {
    atBottom = true;
    leftBottom = false;
    scrollMessagesToLatest();
    queueMicrotask(scrollMessagesToLatest);
    requestAnimationFrame(() => {
      if (atBottom) scrollMessagesToLatest();
      requestAnimationFrame(() => {
        if (atBottom) scrollMessagesToLatest();
        pendingSnapToLatest = false;
      });
    });
  }

  function onMessagesScroll() {
    const el = messagesEl;
    if (!el) return;
    const distance = el.scrollHeight - el.scrollTop - el.clientHeight;
    if (distance <= AT_END) {
      atBottom = true;
      leftBottom = false;
    } else {
      atBottom = !leftBottom && distance < BOTTOM_THRESHOLD;
    }
  }

  function onMessagesWheel(e: WheelEvent) {
    if (e.deltaY < 0) {
      leftBottom = true;
      atBottom = false;
      pendingSnapToLatest = false;
    }
  }

  function onMessagesTouchMove() {
    leftBottom = true;
    pendingSnapToLatest = false;
  }

  /** Images, stickers and video posters finish loading after render and grow the thread. */
  function onContentLoaded() {
    if (atBottom && !leftBottom) scrollMessagesToLatest();
  }

  // Remember where the view was just before the messages change, to anchor it afterwards.
  $effect.pre(() => {
    void visibleMessages;
    const el = messagesEl;
    if (!el) return;
    anchorHeight = el.scrollHeight;
    anchorTop = el.scrollTop;
  });

  $effect(() => {
    const el = messagesEl;
    const convId = conversation?.id ?? null;
    const count = visibleMessages.length;
    const first = visibleMessages[0];
    const last = visibleMessages[count - 1];
    if (!el) return;
    untrack(() => {
      const firstId = first?.id ?? null;
      const lastId = last?.id ?? null;
      if (convId !== prevConvId) {
        prevConvId = convId;
        pendingSnapToLatest = true;
        atBottom = true;
        leftBottom = false;
      }
      const newTail = lastId !== null && prevLastId !== null && lastId !== prevLastId;
      const prepended = !newTail && firstId !== prevFirstId && prevFirstId !== null && lastId === prevLastId;
      const justSent =
        newTail &&
        last?.direction === 'outbound' &&
        Date.now() - Date.parse(last.timestamp) < 30_000;
      prevFirstId = firstId;
      prevLastId = lastId;
      if (!convId) return;

      if (pendingSnapToLatest) {
        if (count > 0) snapToLatest();
      } else if (newTail && ((atBottom && !leftBottom) || justSent)) {
        snapToLatest();
      } else if (prepended) {
        // Older messages were added above: keep the same message under the reader's eyes.
        // (Engines with native scroll anchoring have already done it; then there is no gap.)
        const grown = el.scrollHeight - anchorHeight;
        if (grown > 0 && Math.abs(el.scrollTop - anchorTop) < 1) el.scrollTop = anchorTop + grown;
      }
    });
  });

  // The pane itself resizing (window, composer growing) keeps a reader at the bottom there.
  $effect(() => {
    const el = messagesEl;
    if (!el) return;
    const ro = new ResizeObserver(() => {
      if (pendingSnapToLatest || (atBottom && !leftBottom)) scrollMessagesToLatest();
    });
    ro.observe(el);
    return () => ro.disconnect();
  });

  // ---- @mentions: names come from saved contacts, then message senders ---------------------
  let contacts = $state<Contact[]>([]);
  const hasMentionTokens = $derived(
    conversation?.conversation_type === 'group' &&
      messages.some((m) => m.body.includes('@') && /@\d{5,16}/.test(m.body))
  );

  $effect(() => {
    const accountId = conversation?.account_id;
    if (!accountId || !(hasMentionTokens || mentionWanted === conversation?.id)) return;
    let cancelled = false;
    void listContacts(accountId)
      .then((rows) => {
        if (!cancelled) contacts = rows;
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  });

  // ---- Typing @: a list of the chat's people above the composer ----------------------------
  /** Chat in which an @ was typed; its contacts and members are loaded on demand. */
  let mentionWanted = $state('');
  let mention = $state<{ start: number; query: string } | null>(null);
  let mentionIndex = $state(0);
  /** Start of an @ the person closed with Escape; it stays closed until they type another. */
  let mentionDismissed = $state<number | null>(null);
  let pickedMentions = $state<MentionCandidate[]>([]);


  function updateMention() {
    const el = composerEl;
    if (!el || conversation?.conversation_type !== 'group') {
      mention = null;
      return;
    }
    const next = activeMention(el.value, el.selectionStart ?? el.value.length);
    if (next?.start !== mention?.start || next?.query !== mention?.query) mentionIndex = 0;
    if (next && conversation) mentionWanted = conversation.id;
    mention = next;
  }

  function pickMention(c: MentionCandidate | undefined) {
    const el = composerEl;
    if (!c || !el || !mention) return;
    const caret = el.selectionStart ?? el.value.length;
    const text = `${el.value.slice(0, mention.start)}@${c.name} ${el.value.slice(caret)}`;
    const at = mention.start + c.name.length + 2;
    if (!pickedMentions.some((p) => p.user === c.user)) pickedMentions = [...pickedMentions, c];
    mention = null;
    ondraft(text);
    el.value = text;
    el.setSelectionRange(at, at);
    resizeComposer();
    el.focus();
  }

  // The first @ in a group asks the connector for its members; they arrive on the chat's metadata.
  let membersAsked = '';
  $effect(() => {
    const conv = conversation;
    if (!conv || mentionWanted !== conv.id || conv.conversation_type !== 'group' || membersAsked === conv.id) return;
    membersAsked = conv.id;
    void fetchContactProfile(conv.account_id, conv.id).catch(() => {});
  });

  $effect(() => {
    if (!draft) {
      pickedMentions = [];
      mention = null;
    }
  });

  const mentionResolver = $derived(buildMentionResolver(contacts, messages, account?.identity));
  const groupMembers = $derived.by(() => {
    const profile = conversation?.metadata?.contact_profile as { members?: GroupMember[] } | undefined;
    return Array.isArray(profile?.members) ? profile.members : [];
  });
  const mentionPeople = $derived(mentionCandidates(groupMembers, mentionResolver));
  const mentionMatches = $derived(mention ? filterMentionCandidates(mentionPeople, mention.query) : []);
  const mentionOpen = $derived(
    mention !== null && mention.start !== mentionDismissed && mentionMatches.length > 0
  );

  $effect(() => {
    if (!picker) return;
    function onDoc(e: PointerEvent) {
      const t = e.target as HTMLElement | null;
      if (t?.closest('.picker-wrap') || t?.closest('emoji-picker') || t?.closest('.dt-popover')) return;
      picker = null;
    }
    document.addEventListener('pointerdown', onDoc);
    return () => document.removeEventListener('pointerdown', onDoc);
  });

  $effect(() => {
    const conv = conversation;
    if (!conv) return;
    const chatPrefs = conv.auto_download;
    const accountPrefs = account?.auto_download;
    for (const msg of messages) {
      const kind = mediaKindFromMessage(msg);
      if (!shouldDownloadMedia(kind)) continue;

      const inline = mediaDataFromMessage(msg);
      if (inline) {
        mediaUrls[msg.id] = inline;
        continue;
      }

      const path = mediaPathFromMessage(msg);
      // `mediaUrls` / `thumbUrls` are read untracked: every resolved read replaces them, and an
      // effect subscribed to them re-ran over all messages, re-issuing every read still in
      // flight (quadratic invokes and a growing promise backlog in big sticker groups).
      const haveUrl = untrack(() => mediaUrls[msg.id]);
      if (path && !haveUrl && !readsInFlight.has(`m:${msg.id}:${path}`)) {
        const key = `m:${msg.id}:${path}`;
        readsInFlight.add(key);
        readMessageMedia(path)
          .then((url) => {
            mediaUrls = { ...mediaUrls, [msg.id]: url };
          })
          .catch(() => {})
          .finally(() => readsInFlight.delete(key));
      }
      if (!path && haveUrl) {
        // The cached file was removed by retention: back to thumbnail + Download.
        const { [msg.id]: _gone, ...rest } = mediaUrls;
        mediaUrls = rest;
        requestedMedia.delete(msg.id);
      }
      const thumb = mediaThumbPathFromMessage(msg);
      if (!path && thumb && !untrack(() => thumbUrls[msg.id]) && !readsInFlight.has(`t:${msg.id}:${thumb}`)) {
        const key = `t:${msg.id}:${thumb}`;
        readsInFlight.add(key);
        void resolveThumbUrl(thumb)
          .then((url) => {
            if (url) thumbUrls = { ...thumbUrls, [msg.id]: url };
          })
          .finally(() => readsInFlight.delete(key));
      }

      if (mediaFailed(msg) || path) continue;
      // Only media the effective setting (chat override, else account default) allows is fetched
      // by itself; everything else waits for the user. Voice notes still download on play.
      if (!shouldAutoDownload(kind, chatPrefs, accountPrefs) || requestedMedia.has(msg.id)) continue;
      requestedMedia.add(msg.id);
      downloading = toggleIn(downloading, msg.id, true);
      if (!mediaTimeouts.has(msg.id)) {
        mediaTimeouts.set(
          msg.id,
          setTimeout(() => {
            mediaTimeouts.delete(msg.id);
            downloading = toggleIn(downloading, msg.id, false);
            if (!mediaUrlFor(msg) && !mediaFailed(msg)) {
              mediaTimedOut = toggleIn(mediaTimedOut, msg.id, true);
            }
          }, 20000)
        );
      }
      // A sticker is the message, not a file the reader chooses to fetch, so it is requested as an
      // explicit download: the core skips `auto = true` for any kind its auto-download setting
      // excludes, and the sticker would otherwise never arrive.
      downloadMessageMedia(conv.account_id, conv.id, msg.id, kind !== 'sticker')
        .then((started) => {
          if (!started) downloading = toggleIn(downloading, msg.id, false);
        })
        .catch(() => {
          requestedMedia.delete(msg.id);
          downloading = toggleIn(downloading, msg.id, false);
        });
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
        if (downloading.has(msg.id)) downloading = toggleIn(downloading, msg.id, false);
      }
    }
  });

  type GroupPos = 'single' | 'first' | 'middle' | 'last';

  /**
   * Two neighbouring messages form one run when the same side sent them, and in a group chat
   * the same member: a different sender starts a new run (new gap, own corners, name shown).
   */
  function sameRun(a: Message, b: Message): boolean {
    if (a.direction !== b.direction) return false;
    if (a.direction === 'outbound' || conversation?.conversation_type !== 'group') return true;
    return senderKey(a) === senderKey(b);
  }

  function groupPos(msgs: Message[], i: number): GroupPos {
    const msg = msgs[i];
    const sameAsPrev = i > 0 && sameRun(msgs[i - 1], msg);
    const sameAsNext = i < msgs.length - 1 && sameRun(msg, msgs[i + 1]);
    if (!sameAsPrev && !sameAsNext) return 'single';
    if (!sameAsPrev && sameAsNext) return 'first';
    if (sameAsPrev && sameAsNext) return 'middle';
    return 'last';
  }

  /** One pill per message: the emojis used (most used first) and the total when above 1. */
  function reactionPill(groups: ReactionGroup[]): { emojis: string; total: number; title: string; mine: boolean } {
    const total = groups.reduce((n, g) => n + g.count, 0);
    return {
      emojis: groups.slice(0, 3).map((g) => g.emoji).join(''),
      total,
      title: groups.map((g) => `${g.emoji} ${g.names.join(', ')}`).join('\n'),
      mine: groups.some((g) => g.mine),
    };
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

  function togglePicker(next: typeof picker) {
    picker = picker === next ? null : next;
  }

  function submitSendLater(localValue: string) {
    picker = null;
    if (!draft.trim() || !onsendlater) return;
    onsendlater(new Date(localValue).toISOString());
  }

  /**
   * Scroll to a message in the thread and flash it briefly (the same jump reply links use).
   * Returns false when that message is not rendered, e.g. because it is not loaded yet.
   */
  export function scrollToMessage(messageId: string): boolean {
    const row = messagesEl?.querySelector<HTMLElement>(`[data-msg-id="${CSS.escape(messageId)}"]`);
    if (!row) return false;
    // The jump is the reader's choice: nothing may pull the view back while it animates.
    // (The scroll handler re-enables following if the message sits at the very end.)
    pendingSnapToLatest = false;
    leftBottom = true;
    atBottom = false;
    row.scrollIntoView({ block: 'center', behavior: 'smooth' });
    row.classList.remove('jump-flash');
    void row.offsetWidth; // restart the animation when the same message is jumped to twice
    row.classList.add('jump-flash');
    setTimeout(() => row.classList.remove('jump-flash'), 1800);
    return true;
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

  // Account and chat names for a saved or copied file; the core builds the name from them.
  const nameOwner = $derived(mediaNameOwner(conversation, accounts));

  function openBodyLink(e: MouseEvent) {
    e.preventDefault();
    const href = (e.currentTarget as HTMLAnchorElement).getAttribute('href');
    if (href) void openExternal(href);
  }

  // Live list (images + videos of this chat, chronological) so thumbnails/media that finish
  // loading while the viewer is open update in place; the open item is tracked by id.
  const lightboxItems = $derived<LightboxItem[]>(
    lightboxOpen
      ? buildLightboxItems(visibleMessages, mediaUrlFor, mediaUnavailable, {
          thumbFor: (m) => thumbUrls[m.id] ?? null,
          downloading: (m) => downloading.has(m.id),
          owner: nameOwner,
        })
      : []
  );
  const lightboxIndex = $derived(lightboxItems.findIndex((i) => i.id === lightboxId));

  function openLightboxForMessage(msg: Message) {
    lightboxId = msg.id;
    lightboxOpen = true;
  }

  function saveLightboxItem(item: LightboxItem) {
    const msg = messages.find((m) => m.id === item.id);
    if (msg && conversation) void saveMessageMediaFile(conversation.account_id, conversation.id, msg, nameOwner);
  }

  function downloadLightboxItem(item: LightboxItem) {
    const msg = messages.find((m) => m.id === item.id);
    if (msg) void startDownload(msg);
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
    editorSourceFile = null;
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
    if (editorSourceFile) void stageFiles([editorSourceFile], 'image');
    closeEditor();
  }

  /** Puts files in the pending bar, already read, so Send does no work. */
  async function stageFiles(files: File[], hint?: ComposerFileKind) {
    const staged: PendingAttachment[] = [];
    for (const file of files) {
      const { kind, mime } = classifyFile(file.name, file.type, hint);
      const data_base64 = await fileToBase64(file);
      staged.push({
        id: `${file.name}:${file.size}:${staged.length}:${Math.random().toString(36).slice(2, 8)}`,
        kind,
        filename: file.name,
        mime,
        size: file.size,
        data_base64,
        preview: kind === 'image' ? `data:${mime};base64,${data_base64}` : null,
      });
    }
    pending = [...pending, ...staged];
  }

  function removePending(id: string) {
    pending = removeAttachment(pending, id);
  }

  async function onPickFile(kind: ComposerFileKind, e: Event) {
    const input = e.currentTarget as HTMLInputElement;
    const files = Array.from(input.files ?? []);
    input.value = '';
    if (!files.length) return;
    picker = null;
    const [first] = files;
    if (kind === 'image' && files.length === 1 && first.type.startsWith('image/') && first.type !== 'image/gif') {
      editorOriginal = {
        kind: 'image',
        filename: first.name,
        mime: first.type || 'image/jpeg',
        data_base64: await fileToBase64(first),
      };
      editorSrc = await fileToDataUrl(first);
      editorFilename = first.name;
      editorMime = first.type || 'image/jpeg';
      editorSourceFile = first;
      editorOpen = true;
      return;
    }
    await stageFiles(files, kind);
  }

  // ---- Drops and pastes ---------------------------------------------------------------------

  /** Only a drag carrying files is ours: a dragged link or selection keeps its usual meaning. */
  function carriesFiles(event: DragEvent): boolean {
    const types = event.dataTransfer?.types;
    return Boolean(types && Array.from(types).includes('Files'));
  }

  function onThreadDragEnter(event: DragEvent) {
    if (!carriesFiles(event) || recControls.inputDisabled) return;
    event.preventDefault();
    dragDepth += 1;
    dropActive = true;
  }

  function onThreadDragOver(event: DragEvent) {
    if (!carriesFiles(event) || recControls.inputDisabled) return;
    event.preventDefault();
    if (event.dataTransfer) event.dataTransfer.dropEffect = 'copy';
    dropActive = true;
  }

  function onThreadDragLeave(event: DragEvent) {
    if (!carriesFiles(event)) return;
    dragDepth = Math.max(0, dragDepth - 1);
    if (dragDepth === 0) dropActive = false;
  }

  async function onThreadDrop(event: DragEvent) {
    const files = Array.from(event.dataTransfer?.files ?? []);
    dragDepth = 0;
    dropActive = false;
    if (!files.length || recControls.inputDisabled) return;
    event.preventDefault();
    await stageFiles(files);
  }

  /** A drop anywhere else would make the webview open the file, so swallow it. */
  function onWindowDragOver(event: DragEvent) {
    if (carriesFiles(event)) event.preventDefault();
  }

  function onWindowDrop(event: DragEvent) {
    if (carriesFiles(event)) event.preventDefault();
  }

  /** Pasted image, video or audio files wait here while the person picks media or document. */
  let pasteChoice = $state<File[] | null>(null);

  function onComposerPaste(event: ClipboardEvent) {
    const files = Array.from(event.clipboardData?.files ?? []);
    if (!files.length || recControls.inputDisabled) return;
    event.preventDefault();
    const mediaFiles = files.every((f) => classifyFile(f.name, f.type).kind !== 'document');
    if (mediaFiles) {
      picker = null;
      pasteChoice = files;
    } else {
      void stageFiles(files);
    }
  }

  function resolvePaste(as: 'media' | 'file' | null) {
    const files = pasteChoice;
    pasteChoice = null;
    if (!files || !as) return;
    void stageFiles(files, as === 'file' ? 'document' : undefined);
    composerEl?.focus();
  }

  function canSend(): boolean {
    return Boolean(draft.trim() || pending.length || rec.clip);
  }

  /** One message per file, with the caption on the first, so a multi-file drop arrives in order. */
  async function submitComposer() {
    if (recControls.stack) return;
    if (rec.clip) {
      await sendClip();
      return;
    }
    const caption = draft.trim();
    if (pending.length && onsendattachment) {
      const items = pending;
      pending = [];
      ondraft('');
      for (const [at, item] of items.entries()) {
        await onsendattachment({
          kind: item.kind,
          filename: item.filename,
          mime: item.mime,
          data_base64: item.data_base64,
          caption: at === 0 && caption ? caption : undefined,
        });
      }
      return;
    }
    if (caption) onsend(expandMentions(caption, pickedMentions));
  }

  function sendPayload(payload: AttachmentPayload) {
    onsendattachment?.(payload);
    picker = null;
    pending = [];
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
  // ---- Voice notes --------------------------------------------------------------------------

  const recControls = $derived(recorderControls(rec.state));

  /** Every state change goes through the table, so no path can reach an impossible state. */
  function recSet(action: RecorderAction): boolean {
    const next = recorderNext(rec.state, action);
    if (!next) return false;
    rec.state = next;
    return true;
  }

  /** Turns the microphone off and drops the meter and the clock. Safe to call in any state. */
  function releaseRecorder() {
    if (recClock) {
      clearInterval(recClock);
      recClock = null;
    }
    if (recMeter !== null) {
      cancelAnimationFrame(recMeter);
      recMeter = null;
    }
    if (recAudio) {
      void recAudio.close().catch(() => {});
      recAudio = null;
    }
    recStream?.getTracks().forEach((track) => track.stop());
    recStream = null;
    if (recRecorder) {
      recRecorder.onstop = null;
      recRecorder.ondataavailable = null;
      if (recRecorder.state !== 'inactive') {
        try {
          recRecorder.stop();
        } catch {
          // Already stopped, so there is nothing left to release.
        }
      }
      recRecorder = null;
    }
    recChunks = [];
    rec.level = 0;
  }

  $effect(() => () => releaseRecorder());

  /** The failure path every start pays: release the device, then show the reason. */
  function failRecording(message: string) {
    releaseRecorder();
    rec.error = message;
    recSet('fail');
  }

  /** A moving level for the meter. It is cosmetic, so a missing AudioContext keeps recording. */
  function watchLevel(stream: MediaStream) {
    try {
      const context = new AudioContext();
      recAudio = context;
      const analyser = context.createAnalyser();
      analyser.fftSize = 512;
      context.createMediaStreamSource(stream).connect(analyser);
      const samples = new Uint8Array(analyser.fftSize);
      const tick = () => {
        analyser.getByteTimeDomainData(samples);
        let peak = 0;
        for (const sample of samples) peak = Math.max(peak, Math.abs(sample - 128));
        rec.level = Math.min(1, peak / 80);
        recMeter = requestAnimationFrame(tick);
      };
      recMeter = requestAnimationFrame(tick);
    } catch {
      rec.level = 0;
    }
  }

  async function startRecording() {
    rec.error = '';
    if (!recSet('start')) return;
    const devices = typeof navigator !== 'undefined' ? navigator.mediaDevices : undefined;
    if (!devices?.getUserMedia || typeof MediaRecorder === 'undefined') {
      failRecording('This window cannot reach a microphone. Voice notes need the Shuttle desktop app.');
      return;
    }
    let stream: MediaStream;
    try {
      stream = await devices.getUserMedia({ audio: true });
    } catch (error) {
      failRecording(recordErrorMessage(error));
      return;
    }
    let recorder: MediaRecorder;
    try {
      recorder = new MediaRecorder(stream);
    } catch (error) {
      stream.getTracks().forEach((track) => track.stop());
      failRecording(recordErrorMessage(error));
      return;
    }
    recStream = stream;
    recRecorder = recorder;
    recChunks = [];
    recorder.ondataavailable = (event) => {
      if (event.data.size > 0) recChunks.push(event.data);
    };
    recorder.onstop = () => void finishRecording(recorder.mimeType);
    // A one second timeslice means a crash costs at most the second it was in.
    recorder.start(1000);
    recStartedAt = Date.now();
    recPausedTotal = 0;
    recPausedAt = 0;
    rec.elapsedMs = 0;
    recClock = setInterval(() => {
      if (rec.state === 'recording') rec.elapsedMs = Date.now() - recStartedAt - recPausedTotal;
    }, 200);
    watchLevel(stream);
    queueMicrotask(() => recPauseEl?.focus());
  }

  function togglePauseRecording() {
    const recorder = recRecorder;
    if (!recorder) return;
    if (rec.state === 'paused') {
      if (!recSet('resume')) return;
      recorder.resume();
      if (recPausedAt) recPausedTotal += Date.now() - recPausedAt;
      recPausedAt = 0;
      return;
    }
    if (!recSet('pause')) return;
    recorder.pause();
    recPausedAt = Date.now();
  }

  function stopRecording() {
    const recorder = recRecorder;
    if (!recorder || recorder.state === 'inactive') return;
    recorder.stop();
  }

  /** Reads the take once it is closed, then shows the player in place of the text input. */
  async function finishRecording(mime: string) {
    const chunks = recChunks;
    const durationMs = rec.elapsedMs;
    releaseRecorder();
    const blob = new Blob(chunks, { type: mime || 'audio/webm' });
    if (!blob.size) {
      failRecording('The recording was empty. Try again.');
      return;
    }
    const peaks = await peaksForBlob(blob);
    if (!recSet('stop')) return;
    const type = blob.type || 'audio/webm';
    const extension = type.includes('ogg') ? 'ogg' : type.includes('mp4') ? 'm4a' : 'webm';
    rec.clip = {
      blob,
      mime: type,
      filename: `voice.${extension}`,
      url: URL.createObjectURL(blob),
      durationMs,
      peaks,
    };
  }

  /** Bars for the waveform. A take the browser cannot decode still plays, so this can fail. */
  async function peaksForBlob(blob: Blob): Promise<number[]> {
    if (typeof AudioContext === 'undefined') return [];
    let context: AudioContext | null = null;
    try {
      context = new AudioContext();
      const buffer = await context.decodeAudioData(await blob.arrayBuffer());
      const channels = Array.from({ length: buffer.numberOfChannels }, (_, at) =>
        buffer.getChannelData(at)
      );
      return peaksFromChannelData(channels);
    } catch {
      return [];
    } finally {
      void context?.close().catch(() => {});
    }
  }

  /** Leaves the take behind from any state and turns the microphone off. */
  function discardRecording() {
    releaseRecorder();
    if (rec.clip) URL.revokeObjectURL(rec.clip.url);
    rec.clip = null;
    rec.elapsedMs = 0;
    rec.error = '';
    recSet('discard');
  }

  async function sendClip() {
    const clip = rec.clip;
    if (!clip || !onsendattachment) return;
    const file = new File([clip.blob], clip.filename, { type: clip.mime });
    discardRecording();
    const data_base64 = await fileToBase64(file);
    await onsendattachment({ kind: 'ptt', filename: file.name, mime: file.type, data_base64 });
  }

  /** Escape drops a recording, wherever the focus sits. */
  function composerEscape(event: KeyboardEvent) {
    if (event.key !== 'Escape') return;
    if (callMenu) {
      callMenu = null;
      return;
    }
    if (!recControls.stack && !rec.clip) return;
    event.preventDefault();
    discardRecording();
  }
</script>

{#snippet messageRun(msg: Message, text: string)}{#each bodySegments(msg, text, mentionResolver) as seg}{#if 'mention' in seg}<span class="mention" class:self={seg.self}>{seg.mention}</span>{:else if 'url' in seg}<a class="body-link" href={seg.url} onclick={openBodyLink}>{seg.url}</a>{:else}{seg.text}{/if}{/each}{/snippet}

{#snippet messageBody(msg: Message, text: string, highlight: boolean)}
  {#if whatsappMarkup}
    <div class="wa-body" class:highlight>
      {#each whatsappBlocks(text) as block, at (at)}
        {#if block.kind === 'code'}
          <pre class="wa-code">{block.text}</pre>
        {:else if block.kind === 'quote'}
          <div class="wa-line wa-quote">{@render messageRun(msg, block.runs.map((run) => run.text).join(''))}</div>
        {:else if block.kind === 'bullet' || block.kind === 'ordered'}
          <div class="wa-line wa-item"><span class="wa-marker" aria-hidden="true">{block.kind === 'bullet' ? '•' : block.marker}</span><span class="wa-item-text">{#each block.runs as run, runAt (runAt)}{#if run.styles.length}<span class="wa-styled {run.styles.map((style) => `wa-${style}`).join(' ')}">{@render messageRun(msg, run.text)}</span>{:else}{@render messageRun(msg, run.text)}{/if}{/each}</span></div>
        {:else}
          <div class="wa-line">{#each block.runs as run, runAt (runAt)}{#if run.styles.length}<span class="wa-styled {run.styles.map((style) => `wa-${style}`).join(' ')}">{@render messageRun(msg, run.text)}</span>{:else}{@render messageRun(msg, run.text)}{/if}{/each}</div>
        {/if}
      {/each}
    </div>
  {:else}
    <p class:highlight>{@render messageRun(msg, text)}</p>
  {/if}
{/snippet}

<svelte:window onkeydown={composerEscape} ondragover={onWindowDragOver} ondrop={onWindowDrop} />

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="thread"
  ondragenter={onThreadDragEnter}
  ondragover={onThreadDragOver}
  ondragleave={onThreadDragLeave}
  ondrop={onThreadDrop}
>
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
      {#if callTarget}
        <div class="call-anchor">
        <button type="button" class="panel-btn" class:active={callMenu === 'audio'} title="Audio call" onclick={() => startCall('audio')} aria-label="Audio call">
          <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <path d="M22 16.92v3a2 2 0 0 1-2.18 2 19.79 19.79 0 0 1-8.63-3.07 19.5 19.5 0 0 1-6-6A19.79 19.79 0 0 1 2.12 4.18 2 2 0 0 1 4.11 2h3a2 2 0 0 1 2 1.72c.13.96.36 1.9.7 2.81a2 2 0 0 1-.45 2.11L8.09 9.91a16 16 0 0 0 6 6l1.27-1.27a2 2 0 0 1 2.11-.45c.9.34 1.85.57 2.81.7A2 2 0 0 1 22 16.92z"/>
          </svg>
        </button>
        {#if canVideoCall}
          <button type="button" class="panel-btn" class:active={callMenu === 'video'} title="Video call" onclick={() => startCall('video')} aria-label="Video call">
            <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
              <rect x="2" y="6" width="13" height="12" rx="2"/><path d="m15 12 6-3.5v11L15 16z"/>
            </svg>
          </button>
        {/if}
        {#if callMenu && callFallback}
          <div class="call-pop" role="dialog" aria-label="{callMenu === 'video' ? 'Video' : 'Audio'} call">
            <p>{callFallback.message}</p>
            <div class="call-pop-actions">
              {#if callFallback.url}
                <button type="button" class="call-pop-primary" onclick={openCallInWhatsApp}>Open in WhatsApp</button>
              {/if}
              <button type="button" onclick={() => (callMenu = null)}>Close</button>
            </div>
          </div>
        {/if}
        </div>
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

    {#if selected}
      {@const count = selectedMessages.length}
      <div class="select-bar" role="toolbar" aria-label="Selected messages">
        <button type="button" class="panel-btn" onclick={cancelSelection} aria-label="Leave selection" title="Leave selection (Esc)">×</button>
        <span class="select-count" aria-live="polite">{count} selected</span>
        {#each SELECTION_ACTIONS as action (action.id)}
          <button
            type="button"
            class="select-action"
            class:danger={action.id === 'delete'}
            disabled={count === 0}
            onclick={() => onselectionaction?.(action.id, selectedMessages)}
          >
            {action.id === 'star' && count > 0 && selectedMessages.every((m) => m.starred) ? 'Unstar' : action.label}
          </button>
        {/each}
      </div>
    {/if}

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

    <!-- The time and ticks row. A poll card draws it inside its own frame, above "View votes". -->
    {#snippet msgFooter(m: Message)}
      <span class="msg-time">
        {#if isEdited(m)}<span class="msg-edited">edited</span>{/if}
        {#if m.pinned}<span class="msg-pin" title="Pinned">📌</span>{/if}
        {#if m.starred}<span class="msg-star" title="Starred">★</span>{/if}
        {formatMsgTime(m.timestamp)}
        {#if m.direction === 'outbound'}
          <span
            class="status"
            class:read={m.status === 'read'}
            class:failed={m.status === 'failed'}
            aria-label={m.status === 'failed' ? 'Failed' : m.status === 'read' ? 'Read' : 'Sent'}
          >
            {#if m.status === 'failed'}!
            {:else if m.status === 'read' || m.status === 'delivered'}✓✓
            {:else}✓{/if}
          </span>
        {/if}
      </span>
    {/snippet}

    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="messages"
      bind:this={messagesEl}
      onscroll={onMessagesScroll}
      onwheel={onMessagesWheel}
      ontouchmove={onMessagesTouchMove}
      onloadcapture={onContentLoaded}
    >
      {#if visibleMessages.length === 0 && loading}
        <div class="thread-loading" role="status" aria-label="Loading messages"><span class="thread-spinner"></span></div>
      {:else if visibleMessages.length === 0}
        <div class="thread-empty-msgs">
          <p>No messages synced for this chat yet.</p>
          <p class="hint">
            The chat list comes from WhatsApp immediately. Message bodies only appear after WhatsApp
            sends a history dump to this linked device, or after a live message. Opening a chat
            cannot pull older threads the way WhatsApp Web does.
          </p>
        </div>
      {/if}
      {#if listAheadOfBodies && !loading}
        <div class="thread-empty-msgs history-gap">
          <p class="hint">
            WhatsApp lists this chat as newer than the messages stored on this linked device.
            Shuttle cannot download the missing bodies until WhatsApp sends them.
          </p>
        </div>
      {/if}
      {#each visibleMessages as msg, i (msg.id)}
        {@const prev = visibleMessages[i - 1]}
        {@const pos = groupPos(visibleMessages, i)}
        {@const showGap = !prev || !sameRun(prev, msg)}
        {@const showDate = !prev || dateKey(prev.timestamp) !== dateKey(msg.timestamp)}
        {@const kind = mediaKindFromMessage(msg)}
        {@const poll = pollDataFromMessage(msg)}
        {@const pollInCard = kind === 'poll' && !!poll}
        {@const media = mediaUrlFor(msg)}
        {@const caption = captionText(msg)}
        {@const filename = mediaFilename(msg)}
        {@const revoked = isRevoked(msg)}
        {@const translation = translations[msg.id]}
        {@const hasContent = !!kind || !!caption || revoked}
        {@const searchHit = chatSearchQuery.trim() && (msg.body || '').toLowerCase().includes(chatSearchQuery.trim().toLowerCase())}
        {@const showsSender = msg.direction === 'inbound' && conversation.conversation_type === 'group' && (pos === 'single' || pos === 'first')}
        {@const forwarded = !!msg.metadata?.forwarded && !revoked}
        {#if hasContent && !(chatSearchQuery.trim() && !searchHit)}
        {#if showDate}
          <div class="date-sep"><span>{formatDateLabel(msg.timestamp)}</span></div>
        {/if}
        <!-- svelte-ignore a11y_click_events_have_key_events (the row's checkbox is the keyboard path) -->
        <div
          class="msg-row"
          data-msg-id={msg.id}
          role="group"
          class:selecting={!!selected}
          class:selected={!!selected?.has(msg.id)}
          onclickcapture={(e) => {
            if (!selected) return;
            e.preventDefault();
            e.stopPropagation();
            toggleSelected(msg, e.shiftKey);
          }}
          class:outbound={msg.direction === 'outbound'}
          class:gap={showGap}
          class:group-single={pos === 'single'}
          class:group-first={pos === 'first'}
          class:group-middle={pos === 'middle'}
          class:group-last={pos === 'last'}
          class:search-hit={searchHit}
          class:has-reactions={!revoked && !!msg.reactions?.length}
          class:sticker-row={kind === 'sticker' && !caption && !msg.reply}
          oncontextmenu={(e) => {
            e.preventDefault();
            const sel = window.getSelection()?.toString();
            if (sel && sel.trim()) ontextmenu?.(sel, e.clientX, e.clientY);
            else onmsgmenu?.(msg, e.clientX, e.clientY);
          }}
        >
          {#if selected}
            <input type="checkbox" class="select-box" checked={selected.has(msg.id)} aria-label="Select message" />
          {/if}
          <div
            class="bubble"
            class:image-bubble={(kind === 'image' || kind === 'sticker') && !!media}
            class:sticker-bubble={kind === 'sticker' && !caption && !msg.reply}
          >
            {#if showsSender || forwarded}
              <div class="bubble-top" class:forward-only={!showsSender}>
                {#if showsSender}
                  <span class="sender" style="--sender-h: {senderHue(senderKey(msg))}">{senderLabel(msg)}</span>
                {/if}
                {#if forwarded}
                  <span class="forwarded">
                    <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                      <path d="m14 5 7 7-7 7"/><path d="M21 12H8a5 5 0 0 0-5 5v2"/>
                    </svg>
                    Forwarded
                  </span>
                {/if}
              </div>
            {/if}
            {#if revoked}
              <p class="tombstone"><span aria-hidden="true">⊘</span> This message was deleted</p>
            {:else}
            {#if msg.reply}
              {@const rp = msg.reply}
              {@const target = rp.available ? rp.message_id : undefined}
              <MessageQuote
                variant={msg.direction === 'outbound' ? 'out' : 'in'}
                sender={rp.sender_name ?? null}
                snippet={quoteLabel(rp.media_type, rp.snippet, rp.filename) || 'Message'}
                thumb={rp.thumbnail_path ? (quoteThumbs[rp.thumbnail_path] ?? null) : null}
                onclick={target && onjumptomessage ? () => onjumptomessage(target) : undefined}
                title="Go to the original message"
              />
            {/if}
            {#if kind === 'image' || kind === 'sticker' || kind === 'video'}
              {#if media && kind !== 'video'}
                <button type="button" class="media-open" onclick={() => openLightboxForMessage(msg)} aria-label="View {mediaIconLabel(kind)}">
                  <img class="msg-media" class:sticker={kind === 'sticker'} src={media} alt={caption || mediaIconLabel(kind)} />
                </button>
              {:else if media}
                <div class="video-wrap">
                  <!-- svelte-ignore a11y_media_has_caption -->
                  <video class="msg-media" src={media} controls></video>
                  <button type="button" class="video-expand" onclick={() => openLightboxForMessage(msg)} aria-label="Expand video">⛶</button>
                </div>
              {:else}
                {@const state = mediaUnavailable(msg) ? 'failed' : downloading.has(msg.id) ? 'downloading' : 'idle'}
                {@const thumb = thumbUrls[msg.id]}
                {@const size = formatFileSize(mediaSizeFromMessage(msg))}
                <button
                  type="button"
                  class="media-pending"
                  class:sticker={kind === 'sticker'}
                  class:has-thumb={!!thumb}
                  disabled={state === 'downloading'}
                  onclick={() => startDownload(msg)}
                  aria-label={state === 'failed' ? `Retry downloading ${mediaIconLabel(kind)}` : `Download ${mediaIconLabel(kind)}${size ? ` (${size})` : ''}`}
                >
                  {#if thumb}
                    <img class="pending-thumb" src={thumb} alt="" draggable="false" />
                  {:else}
                    <span class="pending-icon" aria-hidden="true">{kind === 'video' ? '🎬' : kind === 'sticker' ? '🏷️' : '🖼️'}</span>
                  {/if}
                  <span class="dl-badge" class:failed={state === 'failed'}>
                    {#if state === 'downloading'}
                      <span class="dl-spin" aria-hidden="true"></span>
                    {:else}
                      <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" aria-hidden="true"><path d="M12 4v11m0 0-4-4m4 4 4-4M5 20h14" /></svg>
                    {/if}
                    {#if state === 'failed'}<span>Retry</span>{:else if size}<span>{size}</span>{/if}
                  </span>
                </button>
              {/if}
            {:else if kind === 'audio'}
              {#if media}
                <audio class="msg-audio" src={media} controls use:playOnReady={msg.id}></audio>
              {:else}
                {@const state = mediaUnavailable(msg) ? 'failed' : downloading.has(msg.id) ? 'downloading' : 'idle'}
                {@const size = formatFileSize(mediaSizeFromMessage(msg))}
                <div class="media-fallback audio-pending" class:failed={state === 'failed'}>
                  <button type="button" class="audio-play" disabled={state === 'downloading'} onclick={() => startDownload(msg, true)} aria-label={state === 'failed' ? 'Retry downloading audio' : 'Play audio'}>
                    {#if state === 'downloading'}
                      <span class="dl-spin" aria-hidden="true"></span>
                    {:else}
                      <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><path d="M8 5v14l11-7z" /></svg>
                    {/if}
                  </button>
                  <span>{state === 'failed' ? 'Audio unavailable' : state === 'downloading' ? 'Downloading…' : mediaIconLabel(kind)}{size && state === 'idle' ? ` · ${size}` : ''}</span>
                </div>
              {/if}
            {:else if kind === 'document'}
              {@const state = media ? 'ready' : mediaUnavailable(msg) ? 'failed' : downloading.has(msg.id) ? 'downloading' : 'idle'}
              {@const size = formatFileSize(mediaSizeFromMessage(msg))}
              {@const thumb = thumbUrls[msg.id]}
              <div class="doc-card" class:failed={state === 'failed'}>
                {#if thumb && !media}
                  <img class="doc-thumb" src={thumb} alt="" draggable="false" />
                {:else}
                  <span class="doc-icon" aria-hidden="true">📎</span>
                {/if}
                <span class="doc-meta">
                  <span class="doc-name">{filename ?? 'Document'}</span>
                  {#if size}<span class="doc-size">{size}</span>{/if}
                </span>
                {#if state === 'ready'}
                  <button type="button" class="doc-action" onclick={() => conversation && saveMessageMediaFile(conversation.account_id, conversation.id, msg, nameOwner)} title="Save to your save folder" aria-label="Save {filename ?? 'document'}">
                    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" aria-hidden="true"><path d="M12 4v11m0 0-4-4m4 4 4-4M5 20h14" /></svg>
                  </button>
                {:else}
                  <button type="button" class="doc-action" disabled={state === 'downloading'} onclick={() => startDownload(msg)} title={state === 'failed' ? 'Retry download' : 'Download'} aria-label="{state === 'failed' ? 'Retry downloading' : 'Download'} {filename ?? 'document'}">
                    {#if state === 'downloading'}
                      <span class="dl-spin" aria-hidden="true"></span>
                    {:else}
                      <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" aria-hidden="true"><path d="M12 4v11m0 0-4-4m4 4 4-4M5 20h14" /></svg>
                    {/if}
                  </button>
                {/if}
              </div>
            {:else if pollInCard}
              <PollCard {poll} {msg} trailing={msgFooter} onAccent={msg.direction === 'outbound'} />
            {:else if kind === 'contact' || kind === 'poll' || kind === 'event' || kind === 'location'}
              <div class="media-card">
                <span class="media-card-label">{mediaIconLabel(kind)}</span>
                {#if caption}
                  <p>{caption}</p>
                {/if}
              </div>
            {/if}
            {#if kind && shouldDownloadMedia(kind) && mediaUnavailable(msg) && !media}
              <p class="hint">{typeof msg.metadata?.media_error === 'string' ? msg.metadata.media_error : 'Media download timed out.'}</p>
            {/if}
            {#if caption && kind !== 'contact' && kind !== 'poll' && kind !== 'event' && kind !== 'location'}
              {@render messageBody(msg, caption, !!searchHit)}
            {/if}
            {#if translation}
              <div class="translation" class:failed={translation.state === 'error'} aria-live="polite">
                <span class="translation-text">
                  {#if translation.state === 'loading'}Translating…{:else if translation.state === 'done'}{translation.text}{:else}{translation.message}{/if}
                </span>
                <button type="button" class="translation-close" aria-label="Hide translation" onclick={() => ondismisstranslation?.(msg.id)}>×</button>
              </div>
            {/if}
            {/if}
            {#if !pollInCard}
              {@render msgFooter(msg)}
            {/if}
            {#if !revoked && msg.reactions?.length}
              {@const pill = reactionPill(msg.reactions)}
              <span class="reactions" class:mine={pill.mine} title={pill.title} aria-label="Reactions: {pill.title.split('\n').join('; ')}">
                <span class="reaction-emojis">{pill.emojis}</span>{#if pill.total > 1}<span class="reaction-count">{pill.total}</span>{/if}
              </span>
            {/if}
          </div>
        </div>
        {/if}
      {/each}
    </div>

    <footer class="composer" class:locked={composerLocked}>
      <input bind:this={fileImageEl} class="hidden-file" type="file" accept="image/*" onchange={(e) => onPickFile('image', e)} />
      <input bind:this={fileVideoEl} class="hidden-file" type="file" accept="video/*" onchange={(e) => onPickFile('video', e)} />
      <input bind:this={fileAudioEl} class="hidden-file" type="file" accept="audio/*" onchange={(e) => onPickFile('audio', e)} />
      <input bind:this={fileDocEl} class="hidden-file" type="file" multiple onchange={(e) => onPickFile('document', e)} />

      <div class="composer-toolbar" inert={recControls.inputDisabled}>
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
                bind:this={laterBtn}
                class:active={picker === 'later'}
                type="button"
                onclick={() => togglePicker('later')}
                disabled={!draft.trim() || !!replyingTo}
                title={replyingTo ? 'A reply cannot be scheduled yet: cancel the reply to send later' : undefined}
                aria-label="Send later"
              >
                <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                  <circle cx="12" cy="12" r="9"/>
                  <path d="M12 7v5l3 2"/>
                </svg>
              </button>
              {#if picker === 'later'}
                <DateTimePopover
                  anchor={laterBtn}
                  min={toLocalDateTime(nextMinute())}
                  title="Send later"
                  confirmLabel="Schedule"
                  onconfirm={submitSendLater}
                  oncancel={() => (picker = null)}
                />
              {/if}
            </div>
          {/if}
        </div>
      </div>
      {#if replyingTo}
        <div class="reply-bar">
          <MessageQuote
            variant="composer"
            sender={replyingToWho}
            snippet={messageSnippet(replyingTo)}
            thumb={replyBarThumb}
          />
          <button type="button" class="reply-cancel" onclick={() => oncancelreply?.()} aria-label="Cancel reply" title="Cancel reply (Esc)">×</button>
        </div>
      {/if}
      {#if composerLocked}
        <p class="composer-locked">Only <strong>admins</strong> can send messages</p>
      {:else}
        {#if pending.length}
          <PendingAttachments items={pending} onremove={removePending} />
        {/if}
        {#if rec.state === 'error'}
          <div class="rec-error" role="alert">
            <span>{rec.error}</span>
            <button type="button" onclick={discardRecording} aria-label="Dismiss">×</button>
          </div>
        {/if}
        <div class="composer-inner" class:recording={recControls.strip}>
          {#if pasteChoice}
            <div class="picker-pop paste-pop" role="dialog" aria-label="Send pasted file">
              <p class="paste-title">
                Send pasted {pasteChoice.length === 1 ? classifyFile(pasteChoice[0].name, pasteChoice[0].type).kind : 'files'} as
              </p>
              <button type="button" class="picker-action" onclick={() => resolvePaste('media')}>Media</button>
              <button type="button" class="picker-action" onclick={() => resolvePaste('file')}>File (document)</button>
              <button type="button" class="picker-action muted" onclick={() => resolvePaste(null)}>Cancel</button>
            </div>
          {/if}
          {#if mentionOpen}
            <ul class="picker-pop mention-pop" role="listbox" aria-label="Mention someone">
              {#each mentionMatches as c, i (c.user)}
                <li
                  role="option"
                  aria-selected={i === mentionIndex}
                  class:active={i === mentionIndex}
                  onmousedown={(e) => {
                    e.preventDefault();
                    pickMention(c);
                  }}
                  onmouseenter={() => (mentionIndex = i)}
                >
                  <span class="mention-name">{c.name}</span>
                </li>
              {/each}
            </ul>
          {/if}
          {#if recControls.strip}
            <div class="rec-strip" class:paused={rec.state === 'paused'}>
              <span class="rec-dot" class:held={rec.state === 'paused'} aria-hidden="true"></span>
              <span class="rec-clock">{formatClock(rec.elapsedMs)}</span>
              {#if rec.state === 'paused'}
                <span class="rec-label">Paused</span>
              {/if}
              <div
                class="rec-meter"
                role="meter"
                aria-label="Microphone level"
                aria-valuemin="0"
                aria-valuemax="100"
                aria-valuenow={Math.round(rec.level * 100)}
              >
                <span class="rec-meter-fill" style="width: {Math.round(rec.level * 100)}%"></span>
              </div>
            </div>
          {:else if rec.clip}
            <VoiceClip
              src={rec.clip.url}
              peaks={rec.clip.peaks}
              durationMs={rec.clip.durationMs}
              ondiscard={discardRecording}
            />
          {:else}
            <textarea
              bind:this={composerEl}
              placeholder="Type a message…"
              rows="1"
              value={draft}
              oninput={(e) => {
                ondraft(e.currentTarget.value);
                resizeComposer();
                updateMention();
              }}
              onkeydown={handleKeydown}
              onkeyup={(e) => {
                if (e.key === 'ArrowLeft' || e.key === 'ArrowRight' || e.key === 'Home' || e.key === 'End') updateMention();
              }}
              onclick={updateMention}
              onblur={() => (mention = null)}
              onpaste={onComposerPaste}
              aria-label="Message input"
            ></textarea>
          {/if}
          <button class="send-btn" onclick={() => void submitComposer()} disabled={!canSend()} aria-label="Send">
            <svg width="18" height="18" viewBox="0 0 24 24" fill="currentColor">
              <path d="M2.01 21L23 12 2.01 3 2 10l15 2-15 2z"/>
            </svg>
          </button>
          {#if recControls.stack}
            <div class="rec-stack">
              <button
                type="button"
                class="rec-stack-btn pause"
                bind:this={recPauseEl}
                onclick={togglePauseRecording}
                aria-label={rec.state === 'paused' ? 'Resume recording' : 'Pause recording'}
                title={rec.state === 'paused' ? 'Resume' : 'Pause'}
              >
                {#if rec.state === 'paused'}
                  <svg width="10" height="10" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><path d="M8 5v14l11-7z"/></svg>
                {:else}
                  <svg width="10" height="10" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><path d="M7 4h4v16H7zM13 4h4v16h-4z"/></svg>
                {/if}
              </button>
              <button
                type="button"
                class="rec-stack-btn stop"
                onclick={stopRecording}
                aria-label="Stop recording"
                title="Stop"
              >
                <svg width="12" height="12" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><rect x="5" y="5" width="14" height="14" rx="2"/></svg>
              </button>
            </div>
          {:else if recControls.mic}
            <button
              type="button"
              class="mic-btn"
              onclick={() => void startRecording()}
              aria-label="Record a voice note"
              title="Record a voice note"
            >
              <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><path d="M12 14a3 3 0 0 0 3-3V6a3 3 0 1 0-6 0v5a3 3 0 0 0 3 3z"/><path d="M19 11a7 7 0 0 1-14 0M12 18v3"/></svg>
            </button>
          {/if}
        </div>
      {/if}
    </footer>
    {#if dropActive}
      <div class="drop-overlay">
        <div class="drop-card">
          <svg width="26" height="26" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" aria-hidden="true"><path d="M12 16V4m0 0L7 9m5-5 5 5"/><path d="M4 16v2a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-2"/></svg>
          <span>Drop to attach</span>
        </div>
      </div>
    {/if}
    <MediaLightbox
      open={lightboxOpen}
      items={lightboxItems}
      index={lightboxIndex}
      onclose={() => (lightboxOpen = false)}
      onindex={(i) => (lightboxId = lightboxItems[i]?.id ?? lightboxId)}
      onsave={saveLightboxItem}
      onedit={editLightboxItem}
      ondownload={downloadLightboxItem}
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
    flex: 1 1 auto;
    display: flex;
    flex-direction: column;
    background: var(--bg-main);
    min-width: 0;
    min-height: 0;
    overflow: hidden;
    /* The drop overlay covers the thread, so it needs a positioned box. */
    position: relative;
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

  .call-anchor {
    position: relative;
    display: flex;
    align-items: center;
  }

  .call-pop {
    position: absolute;
    top: calc(100% + 8px);
    right: 0;
    z-index: 300;
    width: min(260px, 80vw);
    padding: 12px;
    border-radius: var(--radius-md);
    border: 1px solid var(--border);
    background: var(--bg-panel);
    box-shadow: var(--shadow-md, 0 8px 24px rgba(0, 0, 0, 0.18));
  }

  .call-pop p {
    margin: 0 0 10px;
    font-size: 13px;
    line-height: 1.4;
    color: var(--text);
  }

  .call-pop-actions {
    display: flex;
    gap: 6px;
    justify-content: flex-end;
  }

  .call-pop-actions button {
    border: 1px solid var(--border);
    background: transparent;
    color: var(--text);
    border-radius: var(--radius-sm);
    padding: 6px 10px;
    font-size: 12px;
    cursor: pointer;
  }

  .call-pop-actions .call-pop-primary {
    background: var(--accent);
    border-color: var(--accent);
    color: #fff;
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
    flex: 1 1 auto;
    min-height: 0;
    overflow-y: auto;
    overscroll-behavior: contain;
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

  .msg-row.selecting {
    align-items: center;
    gap: 10px;
    cursor: pointer;
    user-select: none;
    border-radius: var(--radius-sm);
  }

  .msg-row.selected {
    background: var(--accent-muted);
  }

  .select-box {
    flex: 0 0 auto;
    width: 16px;
    height: 16px;
    margin: 0 0 0 4px;
    accent-color: var(--accent);
    cursor: pointer;
  }

  .msg-row.outbound .select-box {
    margin-right: auto;
  }

  .select-bar {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
    padding: 6px 12px;
    border-bottom: 1px solid var(--border-subtle);
    background: var(--bg-panel);
  }

  .select-bar .panel-btn {
    margin-left: 0;
    font-size: 20px;
  }

  .select-count {
    flex: 1;
    min-width: 90px;
    font-size: 13px;
    font-weight: 600;
    color: var(--text);
  }

  .select-action {
    padding: 5px 10px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    background: var(--bg-input);
    color: var(--text);
    font-size: 12px;
    cursor: pointer;
  }

  .select-action:hover:not(:disabled) {
    background: var(--bg-hover);
  }

  .select-action:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .select-action.danger {
    color: var(--destructive, #ef4444);
  }

  /* The sender name and the Forwarded pill share the top line, so neither can cover the other. */
  .bubble-top {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    margin-bottom: 3px;
  }

  .bubble-top .sender {
    margin-bottom: 0;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .bubble-top.forward-only {
    justify-content: flex-end;
  }

  .forwarded {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    flex: none;
    margin-left: auto;
    padding: 1px 6px 1px 5px;
    border-radius: 999px;
    /* currentColor tracks the bubble's own text colour, so the pill reads on both sides. */
    background: color-mix(in srgb, currentColor 14%, transparent);
    color: inherit;
    font-size: 10px;
    font-weight: 600;
    line-height: 1.4;
    letter-spacing: 0.01em;
    opacity: 0.9;
  }

  .forwarded svg {
    opacity: 0.8;
  }

  .bubble {
    max-width: min(520px, 68%);
    padding: 5px 9px 4px;
    border-radius: var(--radius-bubble);
    background: var(--bg-bubble-in);
    position: relative;
    box-shadow: var(--shadow-sm);
  }

  /* Keep the flex item from expanding to the image’s uncapped intrinsic width. */
  .bubble.image-bubble {
    max-width: min(258px, 68%);
  }

  .outbound .bubble {
    background: var(--bg-bubble-out, var(--primary, var(--accent)));
    color: var(--text-on-accent, var(--primary-foreground, #ffffff));
    box-shadow: var(--shadow-sm);
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
    /* Hue comes from a hash of the sender id; lightness is tuned per theme so the name
       reads on both the dark and the light bubble. */
    color: oklch(0.8 0.12 var(--sender-h, 290));
    margin-bottom: 3px;
    letter-spacing: 0.01em;
  }

  :global(:root[data-theme='light']) .sender {
    color: oklch(0.47 0.16 var(--sender-h, 290));
  }

  /* Room under a bubble for the pill that hangs off its bottom edge. */
  .msg-row.has-reactions {
    margin-bottom: 16px;
  }

  /* The pill sits bottom-left (inbound and outbound alike), the time and ticks bottom-right:
     a bubble with reactions is wide enough that the two never share a spot. */
  .msg-row.has-reactions .bubble {
    min-width: 124px;
  }

  .msg-row.has-reactions.outbound .bubble {
    min-width: 140px;
  }

  .reactions {
    position: absolute;
    bottom: -12px;
    left: 8px;
    display: inline-flex;
    align-items: center;
    gap: 3px;
    padding: 1px 6px 1px 5px;
    border-radius: 999px;
    background: var(--bg-bubble-in);
    color: var(--text);
    border: 2px solid var(--bg-main);
    box-shadow: var(--shadow-sm);
    font-size: 12px;
    line-height: 1.35;
    cursor: default;
    user-select: none;
  }

  .reactions.mine {
    outline: 1px solid var(--accent);
  }

  .reaction-count {
    font-size: 11px;
    font-weight: 600;
    color: var(--text-muted);
  }

  /* @mention: the person's name in the accent colour, a touch heavier. On an outbound bubble
     the accent is the bubble itself, so it keeps the text colour and gets a soft chip. */
  .mention {
    color: var(--accent);
    font-weight: 600;
    border-radius: 4px;
    padding: 0 2px;
    background: color-mix(in oklch, var(--accent) 14%, transparent);
  }

  .mention.self {
    background: color-mix(in oklch, var(--accent) 26%, transparent);
  }

  .outbound .mention {
    color: inherit;
    background: color-mix(in oklch, currentColor 22%, transparent);
  }

  /* A link in a message body: the accent on an inbound bubble. On an outbound bubble the
     accent is the bubble itself, so the link keeps the bubble's text colour. */
  .body-link {
    color: var(--accent);
    text-decoration: none;
    border-radius: 3px;
    word-break: break-all;
  }

  .body-link:hover,
  .body-link:focus-visible {
    text-decoration: underline;
  }

  .body-link:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 1px;
  }

  .outbound .body-link {
    color: inherit;
    text-decoration: underline;
  }

  .outbound .body-link:focus-visible {
    outline-color: currentColor;
  }

  .tombstone {
    margin: 0;
    font-style: italic;
    color: var(--text-muted);
  }
  .outbound .bubble .tombstone {
    color: inherit;
    opacity: 0.75;
  }
  .msg-edited {
    margin-right: 4px;
    font-style: italic;
  }
  .translation {
    display: flex;
    align-items: flex-start;
    gap: 6px;
    margin-top: 6px;
    padding-top: 6px;
    border-top: 1px solid color-mix(in srgb, currentColor 18%, transparent);
    font-size: 0.92em;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .translation.failed .translation-text {
    font-style: italic;
    opacity: 0.8;
  }
  .translation-text {
    flex: 1;
    min-width: 0;
  }
  .translation-close {
    flex: none;
    border: none;
    background: transparent;
    color: inherit;
    opacity: 0.6;
    font-size: 14px;
    line-height: 1;
    padding: 0 2px;
    cursor: pointer;
  }
  .translation-close:hover {
    opacity: 1;
  }
  .bubble p {
    font-size: 1rem;
    line-height: 1.4;
    word-wrap: break-word;
    white-space: pre-wrap;
    letter-spacing: -0.01em;
  }

  /* WhatsApp markup: the same type as a plain body, with a block per quote, list item or fence. */
  .wa-body {
    font-size: 1rem;
    line-height: 1.4;
    word-wrap: break-word;
    white-space: pre-wrap;
    letter-spacing: -0.01em;
  }

  .wa-line {
    min-height: 0;
  }

  .wa-quote {
    padding-left: 8px;
    border-left: 3px solid color-mix(in srgb, currentColor 40%, transparent);
    opacity: 0.85;
  }

  .wa-item {
    display: flex;
    gap: 6px;
    padding-left: 2px;
  }

  .wa-marker {
    flex: none;
    opacity: 0.8;
  }

  .wa-item-text {
    flex: 1 1 auto;
    min-width: 0;
  }

  .wa-code {
    margin: 2px 0;
    padding: 6px 8px;
    border-radius: 6px;
    background: color-mix(in srgb, currentColor 10%, transparent);
    font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
    font-size: 0.85em;
    line-height: 1.35;
    white-space: pre-wrap;
    overflow-x: auto;
  }

  .wa-bold {
    font-weight: 700;
  }

  .wa-italic {
    font-style: italic;
  }

  .wa-strike {
    text-decoration: line-through;
  }

  .wa-mono {
    font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
    font-size: 0.88em;
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

  /* Only admins may post here: the controls and the quote go, the reason stays. */
  .composer.locked .composer-toolbar,
  .composer.locked .reply-bar {
    display: none;
  }

  .composer-locked {
    margin: 0;
    padding: 8px 0;
    text-align: center;
    color: var(--text-muted);
  }

  .composer-locked strong {
    color: var(--accent);
    font-weight: 600;
  }

  .composer-toolbar {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 8px;
    min-width: 0;
    overflow: visible;
  }

  .composer-toolbar[inert] {
    opacity: 0.45;
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

  .mention-pop {
    list-style: none;
    margin: 0;
    padding: 4px;
  }

  .mention-pop li {
    padding: 7px 10px;
    border-radius: var(--radius-sm);
    cursor: pointer;
    font-size: 13px;
    color: var(--text);
  }

  .mention-pop li.active {
    background: var(--bg-hover);
  }

  .picker-pop.emoji-pop,
  .picker-pop.giphy-pop {
    width: min(320px, 92vw);
    max-height: none;
    overflow: visible;
    padding: 0;
  }

  .paste-pop {
    left: 0;
    width: 220px;
    padding: 6px;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .paste-title {
    margin: 4px 10px 6px;
    font-size: 12px;
    color: var(--text-muted);
  }

  .picker-action.muted {
    color: var(--text-muted);
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

  /* The location and poll forms sit under the attach button at the right edge: anchor them
     there, cap them to the viewport, and scroll the body when the window is short. */
  .picker-pop.form-pop {
    left: auto;
    right: 0;
    width: min(22rem, calc(100vw - 2rem));
    max-height: min(60vh, 26rem);
  }

  .rec-strip {
    display: flex;
    align-items: center;
    gap: 8px;
    flex: 1;
    min-width: 0;
    padding: 8px 0;
  }

  .rec-dot {
    flex: none;
    width: 9px;
    height: 9px;
    border-radius: 50%;
    background: var(--destructive, #ef4444);
    animation: rec-pulse 1.1s ease-in-out infinite;
  }

  .rec-dot.held {
    animation: none;
    opacity: 0.45;
  }

  @keyframes rec-pulse {
    0%,
    100% {
      opacity: 1;
    }
    50% {
      opacity: 0.25;
    }
  }

  .rec-clock {
    flex: none;
    font-size: 13px;
    font-weight: 600;
    font-variant-numeric: tabular-nums;
    color: var(--text);
  }

  .rec-label {
    flex: none;
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--text-muted);
  }

  .rec-meter {
    flex: 1;
    min-width: 24px;
    height: 5px;
    border-radius: 999px;
    background: var(--bg-panel);
    overflow: hidden;
  }

  .rec-meter-fill {
    display: block;
    height: 100%;
    border-radius: 999px;
    background: var(--accent);
    transition: width 60ms linear;
  }

  .rec-error {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 8px;
    padding: 7px 10px;
    border: 1px solid var(--destructive, #ef4444);
    border-radius: var(--radius-sm);
    background: color-mix(in srgb, var(--destructive, #ef4444) 12%, transparent);
    color: var(--text);
    font-size: 12px;
  }

  .rec-error span {
    flex: 1;
    min-width: 0;
  }

  .rec-error button {
    flex: none;
    width: 22px;
    height: 22px;
    border: none;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--text);
    font-size: 16px;
    line-height: 1;
    cursor: pointer;
  }

  .rec-error button:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 1px;
  }

  .mic-btn {
    width: 36px;
    height: 36px;
    flex: none;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: var(--bg-panel);
    color: var(--text);
    cursor: pointer;
    display: grid;
    place-items: center;
    animation: mic-in 140ms ease-out;
  }

  .mic-btn:hover {
    background: var(--bg-hover);
  }

  .mic-btn:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }

  .rec-stack {
    width: 36px;
    height: 36px;
    flex: none;
    display: flex;
    flex-direction: column;
    border-radius: var(--radius-md);
    overflow: hidden;
    animation: mic-in 140ms ease-out;
  }

  .rec-stack-btn {
    width: 100%;
    border: none;
    padding: 0;
    cursor: pointer;
    display: grid;
    place-items: center;
    color: var(--text-on-accent, var(--primary-foreground, #ffffff));
  }

  /* The pair fills the mic button's footprint: pause 30% on top, stop 70% below. */
  .rec-stack-btn.pause {
    flex: 0 0 30%;
    background: color-mix(in srgb, var(--accent) 62%, #000);
  }

  .rec-stack-btn.stop {
    flex: 0 0 70%;
    background: var(--destructive, #ef4444);
  }

  .rec-stack-btn:hover {
    filter: brightness(1.12);
  }

  .rec-stack-btn:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: -2px;
  }

  @keyframes mic-in {
    from {
      opacity: 0;
      transform: scale(0.82);
    }
    to {
      opacity: 1;
      transform: scale(1);
    }
  }

  .drop-overlay {
    position: absolute;
    inset: 0;
    z-index: 320;
    display: grid;
    place-items: center;
    pointer-events: none;
    background: color-mix(in srgb, var(--bg-main) 72%, transparent);
    border: 2px dashed var(--accent);
  }

  .drop-card {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    padding: 18px 24px;
    border-radius: var(--radius-lg);
    border: 1px solid var(--border);
    background: var(--bg-panel);
    color: var(--text);
    font-size: 14px;
    font-weight: 600;
    box-shadow: var(--shadow-md, 0 12px 32px rgba(0, 0, 0, 0.24));
  }

  @media (prefers-reduced-motion: reduce) {
    .rec-dot {
      animation: none;
    }

    .mic-btn,
    .rec-stack {
      animation: none;
    }
  }

  .reply-bar {
    display: flex;
    align-items: stretch;
    gap: 6px;
    margin-bottom: 8px;
  }

  .reply-bar :global(.quote) {
    flex: 1;
    min-width: 0;
    width: auto;
  }

  .reply-cancel {
    flex: none;
    align-self: center;
    width: 26px;
    height: 26px;
    border: none;
    border-radius: 50%;
    background: transparent;
    color: var(--text-secondary);
    font-size: 18px;
    line-height: 1;
    cursor: pointer;
  }

  .reply-cancel:hover {
    background: var(--bg-hover);
    color: var(--text);
  }

  .reply-cancel:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 1px;
  }

  .hidden-file {
    display: none;
  }

  .composer-inner {
    position: relative;
    display: flex;
    align-items: flex-end;
    gap: 10px;
    background: var(--bg-input);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    padding: 6px 6px 6px 16px;
    transition: border-color 0.15s ease, box-shadow 0.15s ease;
  }

  /* A take is live: the row says so even while one of its buttons holds the focus. */
  .composer-inner.recording,
  .composer-inner.recording:focus-within {
    border-color: var(--destructive, #ef4444);
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

  /* A sticker with no caption and no quote floats on the thread: no bubble box at all. */
  .bubble.sticker-bubble {
    background: transparent;
    box-shadow: none;
    padding: 0;
    border-radius: 0;
    max-width: min(176px, 68%);
  }

  .msg-row.sticker-row {
    margin-bottom: 6px;
  }

  /* The time belongs to the thread, not the sticker: faint, and only on hover. */
  .msg-row.sticker-row .msg-time,
  .msg-row.sticker-row.outbound .msg-time {
    color: var(--text-muted);
    opacity: 0;
    transition: opacity 0.12s ease;
  }

  .msg-row.sticker-row:hover .msg-time,
  .msg-row.sticker-row:focus-within .msg-time {
    opacity: 0.85;
  }

  .msg-audio {
    width: min(260px, 100%);
    height: 36px;
    margin-bottom: 4px;
  }

  .doc-card {
    display: flex;
    align-items: center;
    gap: 10px;
    min-width: 200px;
    max-width: 280px;
    padding: 8px 10px;
    margin-bottom: 4px;
    border-radius: 8px;
    background: color-mix(in srgb, currentColor 8%, transparent);
    font-size: 13px;
  }
  .doc-card.failed {
    opacity: 0.85;
  }
  .doc-icon {
    font-size: 22px;
    line-height: 1;
  }
  .doc-thumb {
    width: 40px;
    height: 40px;
    flex: none;
    border-radius: 6px;
    object-fit: cover;
  }
  .doc-meta {
    display: flex;
    flex: 1;
    min-width: 0;
    flex-direction: column;
  }
  .doc-name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .doc-size {
    font-size: 11px;
    opacity: 0.7;
  }
  .doc-action,
  .audio-play {
    display: grid;
    flex: none;
    place-items: center;
    width: 32px;
    height: 32px;
    padding: 0;
    color: inherit;
    background: color-mix(in srgb, currentColor 14%, transparent);
    border: none;
    border-radius: 50%;
    cursor: pointer;
  }
  .doc-action:disabled,
  .audio-play:disabled {
    cursor: progress;
  }
  .audio-pending {
    min-width: 200px;
  }

  /* Not-downloaded image/video/sticker: WhatsApp's own tiny preview, blurred, with a download badge. */
  .media-pending {
    position: relative;
    display: grid;
    place-items: center;
    width: 220px;
    max-width: 100%;
    height: 160px;
    padding: 0;
    overflow: hidden;
    color: #fff;
    background: color-mix(in srgb, currentColor 10%, var(--bg-panel));
    border: none;
    border-radius: 10px;
    cursor: pointer;
  }
  .media-pending.sticker {
    width: 140px;
    height: 140px;
    background: transparent;
  }
  .media-pending.sticker .pending-thumb {
    /* A sticker preview is the sticker: no blur, and it keeps its own proportions. */
    filter: none;
    transform: none;
    object-fit: contain;
  }
  .media-pending:disabled {
    cursor: progress;
  }
  .pending-thumb {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    object-fit: cover;
    /* These previews are ~100px wide; blur hides the upscaling like WhatsApp does. */
    filter: blur(5px);
    transform: scale(1.08);
  }
  .pending-icon {
    font-size: 30px;
    opacity: 0.7;
  }
  .dl-badge {
    position: relative;
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 6px 12px;
    font-size: 12px;
    color: #fff;
    background: rgba(0, 0, 0, 0.55);
    border-radius: 999px;
  }
  .dl-badge.failed {
    background: rgba(160, 30, 30, 0.75);
  }
  .dl-spin {
    width: 14px;
    height: 14px;
    border: 2px solid color-mix(in srgb, currentColor 35%, transparent);
    border-top-color: currentColor;
    border-radius: 50%;
    animation: dl-spin 0.8s linear infinite;
  }
  @keyframes dl-spin {
    to {
      transform: rotate(360deg);
    }
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

  .msg-row:global(.jump-flash) .bubble {
    animation: jump-flash 1.8s ease-out;
  }

  @keyframes jump-flash {
    0%,
    35% {
      box-shadow: 0 0 0 3px var(--accent);
    }
    100% {
      box-shadow: 0 0 0 3px transparent;
    }
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

  .thread-loading {
    margin: auto;
    padding: 40px 20px;
  }

  .thread-spinner {
    display: block;
    width: 22px;
    height: 22px;
    border: 2px solid var(--text-muted);
    border-top-color: transparent;
    border-radius: 50%;
    animation: thread-spin 0.8s linear infinite;
  }

  @keyframes thread-spin {
    to { transform: rotate(360deg); }
  }

  .thread-empty-msgs {
    margin: auto;
    max-width: 340px;
    padding: 40px 20px;
    text-align: center;
    color: var(--text-secondary);
  }

  .thread-empty-msgs p {
    margin: 0;
    font-size: 14px;
    line-height: 1.45;
  }

  .thread-empty-msgs .hint {
    margin-top: 8px;
    font-size: 13px;
    color: var(--text-muted);
  }

  .history-gap {
    margin: 0 auto 8px;
    padding: 12px 16px 0;
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

  @media (max-width: 639px) {
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
