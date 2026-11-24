<script lang="ts">
  import { messageSnippet } from '$lib/messageSnippet';
  import { MessageCache } from '$lib/messageCache';
  import {
    appendNewer,
    emptyWindow,
    mergeNewest,
    openWindow,
    prependOlder,
    reReadSpan,
    type MessageWindow,
  } from '$lib/messageWindow';
  import { setWindowTitleUnread } from '$lib/windowTitle';
  import { page } from '$app/state';
  import { onMount, tick, untrack } from 'svelte';
  import {
    addNote,
    addTodo,
    applyMessageOp,
    connectAccount,
    createAccount,
    createPriorityGroup,
    createWorkspace,
    createForwardRule,
    deleteAccount,
    deleteForwardRule,
    deletePriorityGroup,
    deleteScheduledMessage,
    deleteWorkspace,
    downloadStatusMedia,
    exportBackup,
    fetchConversationAvatar,
    fetchContactProfile,
    getAppConfig,
    getMessagePage,
    MESSAGE_PAGE_ROWS,
    listAccounts,
    listConnectors,
    countConversations,
    checkForUpdate,
    currentCall,
    listConversations,
    listForwardRules,
    listPriorityGroups,
    listScheduledMessages,
    listWorkspaces,
    markRead,
    markUnread,
    onShuttleEvent,
    openExternal,
    pinMessage,
    readMessageMedia,
    restoreBackup,
    saveAppConfig,
    scheduleMessage,
    translateText,
    searchMessages,
    senderContact,
    sendMessage,
    sendAttachment,
    starMessage,
    startCall,
    submitAuth,
    syncConversation,
    totalUnread,
    unreadByAccount,
    updateAccount,
    updateTrayUnread,
    wakeAccount,
    setActiveAccount,
    updateConversation,
    updateForwardRule,
    conversationAvatar,
  } from '$lib/api';
  import AccountSetup from '$lib/components/AccountSetup.svelte';
  import CallPanel from '$lib/components/CallPanel.svelte';
  import ChatPanel from '$lib/components/ChatPanel.svelte';
  import ContactDetails from '$lib/components/ContactDetails.svelte';
  import ContextMenu from '$lib/components/ContextMenu.svelte';
  import DeleteMessagesDialog from '$lib/components/DeleteMessagesDialog.svelte';
  import EditMessageDialog from '$lib/components/EditMessageDialog.svelte';
  import EmojiPickerPop from '$lib/components/EmojiPickerPop.svelte';
  import ForwardModal from '$lib/components/ForwardModal.svelte';
  import {
    QUICK_REACTIONS,
    isRevoked,
    offeredActions,
    toggledReaction,
    withMyReaction,
    type SelectionAction,
    type TranslationView,
  } from '$lib/messageActions';
  import { toast } from '$lib/toast.svelte';
  import ConversationList from '$lib/components/ConversationList.svelte';
  import RemindModal from '$lib/components/RemindModal.svelte';
  import QuickSwitch from '$lib/components/QuickSwitch.svelte';
  import SettingsPanel from '$lib/components/SettingsPanel.svelte';
  import Sidebar from '$lib/components/Sidebar.svelte';
  import StatusViewer from '$lib/components/StatusViewer.svelte';
  import ThreadView from '$lib/components/ThreadView.svelte';
  import { isTauri } from '$lib/mock';
  import UpdateNotice from '$lib/components/UpdateNotice.svelte';
  import UpdatesHome from '$lib/components/UpdatesHome.svelte';
  import { isModKey, isTypingTarget } from '$lib/shortcuts';
  import { normalizeRichText } from '$lib/richText';
  import { captionText, isDownloadableMedia, resolveThumbUrl } from '$lib/messageMedia';
  import { inlineSrc, orderedPosts } from '$lib/statusPreview';
  import { copyMessageMediaToClipboard, effectiveAutoDownload, mediaNameOwner, saveMessageMediaFile } from '$lib/mediaSave';
  import { applyAppConfig } from '$lib/theme';
  import { initTelemetry, markAppReady } from '$lib/telemetry';
  import { selectionContext, urlsIn } from '$lib/structuredText';
  import { compareConversations, convTime } from '$lib/conversationOrder';
  import { draftFor, loadUiState, saveDraft, saveUiPlace } from '$lib/uiState';
  import {
    CONNECTOR_COLORS,
    type Account,
    type AppConfig,
    type AutoDownloadGroup,
    type AttachmentPayload,
    type CallState,
    type ConnectorInfo,
    type Conversation,
    type UpdateCheck,
    type ForwardRule,
    type ForwardRuleDraft,
    type MenuItem,
    type Message,
    type MessageOp,
    type TranslateError,
    type PriorityGroup,
    type ReactionGroup,
    type ScheduledMessage,
    type SearchMessageHit,
    type SearchScope,
    type StatusItem,
    type Workspace,
  } from '$lib/types';

  let accounts = $state<Account[]>([]);
  let connectors = $state<ConnectorInfo[]>([]);
  const LIST_PAGE_SIZE = 30;
  const INBOX_RELOAD_DEBOUNCE_MS = 400;
  let conversations = $state<Conversation[]>([]);
  let listOffset = $state(0);
  let listTotal = $state(0);
  let listHasMore = $state(false);
  let listLoadingMore = $state(false);
  let statusFeed = $state<StatusItem[]>([]);
  let statusViewerOpen = $state(false);
  let statusViewerStartId = $state('');
  let statusViewerAccountId = $state<string | undefined>();
  const statusMediaCache = new Map<string, string>();
  // Media URLs already resolved by the status viewer; the Updates cards reuse them as thumbnails
  // (nothing is downloaded just to draw a card).
  let statusThumbs = $state<Record<string, string>>({});
  const statusMediaWaiters = new Map<string, Array<(url: string | null) => void>>();
  let messages = $state<Message[]>([]);
  /**
   * The open chat holds one contiguous run of its messages, not the whole history: 50 rows on
   * open, another page each time the reader scrolls to either end, and a page around the message
   * a search hit or a quote jumps to. `hasOlder` and `hasNewer` say which ends have more.
   */
  let hasOlder = $state(false);
  let hasNewer = $state(false);
  /** True while a page of older history or of the live end is in flight. */
  let loadingOlder = $state(false);
  let loadingNewer = $state(false);
  /** True while the open chat has nothing to show yet and its first read is in flight. */
  let messagesLoading = $state(false);
  const MESSAGE_CACHE_CHATS = 30;
  const MESSAGE_CACHE_ROWS = 100;
  /** A chat nobody opened for this long gives its cached rows back. */
  const MESSAGE_CACHE_TTL_MS = 5 * 60_000;
  /**
   * Recent messages by conversation id, so opening a chat paints at once. Every open still
   * re-reads the database and replaces the cached rows, so a stale entry is only ever shown briefly.
   */
  const messageCache = new MessageCache(MESSAGE_CACHE_CHATS, MESSAGE_CACHE_ROWS, MESSAGE_CACHE_TTL_MS);
  $effect(() => {
    const timer = setInterval(() => messageCache.sweep(), 60_000);
    return () => clearInterval(timer);
  });
  const PREFILL_CHATS_PER_ACCOUNT = 5;
  const PREFILL_MESSAGES = 15;
  /** One prefill per session: later inbox reloads open chats from the database as usual. */
  let prefillStarted = false;

  async function prefillMessageCache() {
    for (const account of accounts) {
      if (account.disabled) continue;
      let chats: Conversation[];
      try {
        chats = await listConversations(account.id, undefined, undefined, false, 0, PREFILL_CHATS_PER_ACCOUNT);
      } catch {
        continue;
      }
      for (const chat of chats) {
        if (messageCache.has(chat.id)) continue;
        try {
          const rows = await getMessagePage(chat.id, { kind: 'newest' }, PREFILL_MESSAGES);
          if (!messageCache.has(chat.id)) messageCache.set(chat.id, rows.messages);
        } catch {
          /* the chat reads from the database when opened */
        }
      }
    }
  }
  let workspaces = $state<Workspace[]>([]);
  let priorityGroups = $state<PriorityGroup[]>([]);
  let forwardRules = $state<ForwardRule[]>([]);
  let scheduledMessages = $state<ScheduledMessage[]>([]);
  let appConfig = $state<AppConfig>({
    appearance: { color_scheme: 'light', theme_id: 'cmlhfpjhw000004l4f4ax3m7z', datetime_format: '12h_full', font_scale: 1, tweakcn_css: null },
    notifications: {
      enabled: true,
      quiet_hours_enabled: false,
      quiet_hours_start: '22:00',
      quiet_hours_end: '08:00',
    },
    privacy: {
      crash_reports: false,
      usage_diagnostics: false,
    },
    sleep: {
      enabled: false,
      after_minutes: 5,
      check_minutes: 15,
    },
    channel_styles: {},
    media: { save_dir: null, ask_each_time: false, keep_days: 60 },
  });
  let selectedAccountId = $state<string | null>(null);
  const visibleStatuses = $derived(selectedAccountId ? statusFeed.filter((s) => s.account_id === selectedAccountId) : statusFeed);
  let selectedConversationId = $state<string | null>(null);
  let showArchived = $state(false);
  let selectedWorkspace = $state<string | null>(null);
  let selectedPriority = $state<string | null>(null);
  let searchQuery = $state('');
  // Channels (WhatsApp newsletters) live in the Updates home instead of the chat list.
  const channelConversations = $derived(
    searchQuery ? [] : conversations.filter((c) => c.conversation_type === 'channel' && !c.archived && !(c.remote_id || '').toLowerCase().startsWith('status@'))
  );
  const hasUpdates = $derived(visibleStatuses.length > 0 || channelConversations.length > 0);
  let searchScope = $state<SearchScope>('global');
  let searchMessageHits = $state<SearchMessageHit[]>([]);
  let activeCall = $state<CallState | null>(null);
  let draft = $state('');
  /**
   * What the previous window had open. Restored once the first list load has rows to find it in,
   * which is what makes a window rebuilt from the tray come back looking like the one that left.
   */
  let restoreConversationId: string | null = null;
  /** Effects only write the saved view after the restore, never over it with the empty defaults. */
  let hydrated = $state(false);
  /** The message the next send answers; cleared on send, on Esc and when the chat changes. */
  let replyingTo = $state<Message | null>(null);
  // A reply belongs to the chat it was started in.
  $effect(() => {
    void selectedConversationId;
    untrack(() => (replyingTo = null));
  });
  let unreadTotal = $state(0);
  let unreadCounts = $state<Record<string, number>>({});
  let editingAccount = $state<Account | null>(null);
  let editAccountName = $state('');
  let editAccountColor = $state('#888888');
  let showSetup = $state(false);
  let connecting = $state(false);
  let setupError = $state<string | null>(null);
  let sendError = $state<string | null>(null);
  let qrData = $state<string | null>(null);
  let authMethod = $state<string | null>(null);
  let authMessage = $state<string | null>(null);
  let pendingAccountId = $state<string | null>(null);
  let mobileView = $state<'list' | 'thread'>('list');
  let mobileTab = $state<'inbox' | 'settings'>('inbox');
  // Newer release found by the launch check or the 6-hourly check. The popover opens only for
  // the launch check; a later find while the app runs shows just the dot on the settings button.
  let availableUpdate = $state<UpdateCheck | null>(null);
  let updateNoticeOpen = $state(false);
  async function checkForNewRelease(atLaunch: boolean) {
    if (!isTauri()) return;
    try {
      const result = await checkForUpdate(false);
      availableUpdate = result.available ? result : null;
      if (atLaunch && result.available) updateNoticeOpen = true;
    } catch {
      // Offline or rate-limited: try again at the next interval.
    }
  }
  let panelOpen = $state(false);
  /** Contact details: of the open chat, or of a group member (their chat, or a transient one). */
  let contactView = $state<
    { of: 'chat' } | { of: 'sender'; conversation: Conversation; transient: boolean } | null
  >(null);
  let narrowWidth = $state(false);
  let isPhone = $state(false);
  let menuOpen = $state(false);
  let menuX = $state(0);
  let menuY = $state(0);
  let menuItems = $state<MenuItem[]>([]);
  let menuAction = $state<(id: string) => void>(() => {});
  /** Messages the forward dialog sends, in chat order; empty when it is closed. */
  let forwardSources = $state<Message[]>([]);
  /** Messages the delete dialog asks about. */
  let deleteTargets = $state<Message[]>([]);
  let editingMessage = $state<Message | null>(null);
  let reactPicker = $state<{ msg: Message; x: number; y: number } | null>(null);
  /** Translations shown under messages, by message id. */
  let translations = $state<Record<string, TranslationView>>({});
  let settingsFocus = $state<{ field: 'openrouter-key'; at: number } | null>(null);
  let remindOpen = $state(false);
  let remindNoteSeed = $state('');
  /** Message a new chat-panel note should quote (set by the message menu's "Add note"). */
  let noteDraftRef = $state<{ id: string; snippet: string } | null>(null);
  let threadView = $state<ReturnType<typeof ThreadView> | null>(null);
  let quickSwitchOpen = $state(false);
  let focusSearchNonce = $state(0);
  let listFocusId = $state<string | null>(null);

  const setupOnly = $derived(page.url.searchParams.get('setup') === '1');
  const settingsOpen = $derived(mobileTab === 'settings');
  const showWorkspaceFilter = $derived(workspaces.some((w) => !w.builtin));
  const showPriorityFilter = $derived(priorityGroups.length > 0);
  /**
   * A chat just started from New > Contact. It has no messages yet, so it sorts last and sits
   * outside the loaded page of the list; it still has to open.
   */
  let startedChat = $state<Conversation | null>(null);
  const selectedConversation = $derived(
    conversations.find((c) => c.id === selectedConversationId) ??
      (startedChat && startedChat.id === selectedConversationId ? startedChat : null)
  );
  const extrasTop = $derived(narrowWidth && panelOpen);

  /** Prefer stored account avatar; fall back to “(You)” / identity chat photo. */
  const accountsForRail = $derived(
    accounts.map((account) => {
      const existing = account.metadata?.avatar_data;
      if (typeof existing === 'string' && existing.length > 8) return account;
      const identity = (account.identity || '').replace(/@.*/, '');
      const selfConv = conversations.find((c) => {
        if (c.account_id !== account.id) return false;
        if (/\(you\)/i.test(c.title || '')) return true;
        if (!identity) return false;
        return (c.remote_id || '').includes(identity);
      });
      const photo = selfConv ? conversationAvatar(selfConv) : null;
      if (!photo) return account;
      return { ...account, metadata: { ...account.metadata, avatar_data: photo } };
    })
  );

  function channelColor(connectorId: string): string {
    return appConfig.channel_styles[connectorId]?.tag ?? CONNECTOR_COLORS[connectorId] ?? '#888';
  }

  function statusMediaKey(accountId: string, messageId: string): string {
    return `${accountId}::${messageId}`;
  }

  function resolveStatusMediaWaiters(key: string, url: string | null) {
    const waiters = statusMediaWaiters.get(key);
    if (!waiters?.length) return;
    statusMediaWaiters.delete(key);
    for (const resolve of waiters) resolve(url);
  }

  async function requestStatusMedia(accountId: string, messageId: string): Promise<string | null> {
    const key = statusMediaKey(accountId, messageId);
    const cached = statusMediaCache.get(key);
    if (cached) return cached;
    return new Promise((resolve) => {
      const list = statusMediaWaiters.get(key) ?? [];
      list.push(resolve);
      statusMediaWaiters.set(key, list);
      if (list.length > 1) return;
      downloadStatusMedia(accountId, messageId).catch(() => {
        resolveStatusMediaWaiters(key, null);
      });
      window.setTimeout(() => {
        if (statusMediaWaiters.has(key)) resolveStatusMediaWaiters(key, null);
      }, 25000);
    });
  }

  // WhatsApp's own tiny preview of a status post, when the connector knows it (fact, no download).
  let statusPreviews = $state<Record<string, string>>({});

  /**
   * Picture behind a status card: the first slide that has one, oldest first, so a card shows the
   * start of the status rather than its newest post.
   */
  function statusThumb(item: StatusItem): string | null {
    if (!item.account_id) return null;
    for (const post of orderedPosts(item)) {
      if (!post.id) continue;
      const key = statusMediaKey(item.account_id, post.id);
      const url = statusThumbs[key] ?? statusPreviews[key];
      if (url) return url;
    }
    return null;
  }

  /** A status preview the webview can draw: inline data, else a cached file through the core. */
  async function resolveStatusThumb(path: string | null | undefined): Promise<string | null> {
    return inlineSrc(path) ?? (await resolveThumbUrl(path));
  }

  async function loadStatusPreviews(accountId: string | undefined, items: StatusItem[]) {
    for (const item of items) {
      const acct = item.account_id ?? accountId;
      if (!acct) continue;
      for (const post of item.posts ?? []) {
        if (!post.id) continue;
        const key = statusMediaKey(acct, post.id);
        if (statusPreviews[key]) continue;
        const path = post.thumbnail_path ?? post.link_preview?.thumbnail_path;
        const url = await resolveStatusThumb(path);
        if (url) statusPreviews = { ...statusPreviews, [key]: url };
      }
    }
  }

  function openStatusViewer(item: StatusItem) {
    statusViewerStartId = item.sender_id;
    statusViewerAccountId = item.account_id;
    statusViewerOpen = true;
  }

  function connectorForAccount(accountId: string | null | undefined): string {
    return accounts.find((a) => a.id === accountId)?.connector_id ?? '';
  }

  function openMenu(x: number, y: number, items: MenuItem[], action: (id: string) => void) {
    menuX = x;
    menuY = y;
    menuItems = items;
    menuAction = action;
    menuOpen = true;
  }

  function conversationVisible(conv: Conversation): boolean {
    if (searchQuery) return false;
    if (showArchived !== Boolean(conv.archived)) return false;
    if (selectedAccountId && conv.account_id !== selectedAccountId) return false;
    if (selectedWorkspace && (conv.workspace_id ?? null) !== selectedWorkspace) return false;
    if (selectedPriority && (conv.priority_group ?? null) !== selectedPriority) return false;
    return true;
  }

  function sortConversations(list: Conversation[]): Conversation[] {
    return [...list]
      .filter((c) => !(c.remote_id || '').toLowerCase().startsWith('status@'))
      .sort(compareConversations);
  }

  function patchConversation(conv: Conversation) {
    const idx = conversations.findIndex((c) => c.id === conv.id);
    let next: Conversation[];
    if (idx >= 0) {
      next = conversations.map((c, i) => (i === idx ? conv : c));
    } else if (conversationVisible(conv)) {
      next = [conv, ...conversations];
    } else {
      return;
    }
    conversations = sortConversations(next);
    noteLocalListChange();
  }

  /** Replace the loaded message with the same id (or remote id), or append it when it is new. */
  function upsertMessage(msg: Message) {
    const idx = messages.findIndex(
      (m) => m.id === msg.id || (!!msg.remote_id && m.remote_id === msg.remote_id)
    );
    // A run that stops short of the live end has nothing to append to: the row is stored, and
    // scrolling down reads it with the page it belongs to.
    if (idx < 0 && hasNewer) return;
    // An edit or receipt for a row above the loaded page belongs up there, not under the newest.
    const newest = messages.at(-1);
    if (idx < 0 && newest && new Date(msg.timestamp).getTime() < new Date(newest.timestamp).getTime()) return;
    // An updated row from the core carries no reactions of its own: keep the ones shown.
    messages =
      idx >= 0
        ? messages.map((m, i) => (i === idx ? { ...msg, reactions: msg.reactions ?? m.reactions } : m))
        : [...messages, msg];
  }

  /** `message.reactions`: swap in the new reaction groups of the named messages (by remote id). */
  function applyReactionUpdates(payload: Record<string, unknown>) {
    if (payload.conversation_id !== selectedConversationId) return;
    const updates = payload.updates;
    if (!Array.isArray(updates)) return;
    const byRemote = new Map<string, ReactionGroup[]>();
    for (const u of updates as { remote_id?: string; reactions?: ReactionGroup[] }[]) {
      if (u && typeof u.remote_id === 'string') byRemote.set(u.remote_id, u.reactions ?? []);
    }
    if (byRemote.size === 0) return;
    messages = messages.map((m) =>
      m.remote_id && byRemote.has(m.remote_id) ? { ...m, reactions: byRemote.get(m.remote_id) } : m
    );
  }

  function applyPushedMessage(payload: Record<string, unknown>) {
    let conv = payload.conversation as Conversation | undefined;
    const msg = payload.message as Message | undefined;
    if (conv && msg?.timestamp) {
      const msgT = new Date(msg.timestamp).getTime();
      if (Number.isFinite(msgT) && msgT > convTime(conv)) {
        conv = {
          ...conv,
          last_message_at: msg.timestamp,
          last_message_preview: msg.body || conv.last_message_preview,
          last_message_from_me: msg.direction === 'outbound',
          last_message_sender_id: msg.direction === 'outbound' ? null : (msg.sender_id ?? null),
          last_message_sender_name: msg.direction === 'outbound' ? null : (msg.sender_name ?? null),
        };
      }
    }
    if (conv) patchConversation(conv);
    if (typeof payload.unread_total === 'number') unreadTotal = payload.unread_total;
    if (msg && selectedConversationId === msg.conversation_id) upsertMessage(msg);
    else if (msg) {
      const cached = messageCache.get(msg.conversation_id);
      if (cached && !cached.some((m) => m.id === msg.id)) messageCache.set(msg.conversation_id, [...cached, msg]);
    }
  }

  function listFilterArgs() {
    return {
      accountId: selectedAccountId ?? undefined,
      workspaceId: selectedWorkspace ?? undefined,
      priorityGroup: selectedPriority ?? undefined,
      archivedOnly: showArchived,
    };
  }

  /** Identifies what the list shows (account, workspace, priority, archived). */
  function listScopeKey(): string {
    const f = listFilterArgs();
    return [f.accountId ?? '', f.workspaceId ?? '', f.priorityGroup ?? '', f.archivedOnly ? 'archived' : 'inbox'].join('|');
  }

  // List loading. `listGeneration` moves on every request and every local change to the list;
  // a response only assigns if its generation is still the latest. At most one reset runs at a
  // time, and requests that arrive meanwhile share exactly one follow-up run.
  let listGeneration = 0;
  let listScope: string | null = null;
  let listRunning: Promise<void> | null = null;
  let listQueued: Promise<void> | null = null;
  let inboxReloadTimer: number | undefined;
  let unreadCountsTimer: number | undefined;
  function scheduleUnreadCounts() {
    window.clearTimeout(unreadCountsTimer);
    unreadCountsTimer = window.setTimeout(() => { void unreadByAccount().then((counts) => { unreadCounts = counts; }).catch(console.error); }, INBOX_RELOAD_DEBOUNCE_MS);
  }

  async function resetConversationList(): Promise<void> {
    if (searchQuery) return;
    const gen = listGeneration;
    const scope = listScopeKey();
    const filters = listFilterArgs();
    // Same scope: keep every row already loaded, in one call from the top. New scope: first page.
    const limit = listScope === scope ? Math.max(LIST_PAGE_SIZE, listOffset) : LIST_PAGE_SIZE;
    const [rows, total] = await Promise.all([
      listConversations(
        filters.accountId,
        filters.workspaceId,
        filters.priorityGroup,
        filters.archivedOnly,
        0,
        limit
      ),
      countConversations(
        filters.accountId,
        filters.workspaceId,
        filters.priorityGroup,
        filters.archivedOnly
      ),
    ]);
    if (gen !== listGeneration || searchQuery) return;
    listScope = scope;
    listTotal = total;
    conversations = sortConversations(rows);
    listOffset = rows.length;
    listHasMore = listOffset < listTotal;
    queueAvatarFetches(rows);
  }

  function startListReset(): Promise<void> {
    const run: Promise<void> = resetConversationList()
      .catch((e) => console.error('Conversation list reload failed', e))
      .finally(() => {
        if (listRunning === run) listRunning = null;
      });
    listRunning = run;
    return run;
  }

  /** Reload the conversation list from the top. Joins the single queued follow-up if one is running. */
  function reloadConversationList(): Promise<void> {
    listGeneration += 1;
    if (listQueued) return listQueued;
    if (listRunning) {
      listQueued = listRunning.then(() => {
        listQueued = null;
        return startListReset();
      });
      return listQueued;
    }
    return startListReset();
  }

  /**
   * A live event changed the list while a reset was reading it. That reset may hold older rows,
   * so drop its result and read again shortly after.
   */
  function noteLocalListChange() {
    if (!listRunning) return;
    listGeneration += 1;
    scheduleInboxReload();
  }

  /** Light reload for bursts of inbox events: the list and the unread total only. */
  async function reloadInbox() {
    const [total, counts] = await Promise.all([totalUnread(), unreadByAccount(), reloadConversationList()]);
    unreadTotal = total;
    unreadCounts = counts;
  }

  function scheduleInboxReload() {
    if (inboxReloadTimer !== undefined) window.clearTimeout(inboxReloadTimer);
    inboxReloadTimer = window.setTimeout(() => {
      inboxReloadTimer = undefined;
      reloadInbox().catch((e) => console.error('Inbox reload failed', e));
    }, INBOX_RELOAD_DEBOUNCE_MS);
  }

  async function loadMoreConversations() {
    if (searchQuery || listLoadingMore) return;
    listLoadingMore = true;
    try {
      for (;;) {
        // Read the offset only after a running reset has left its rows behind.
        while (listRunning) await listRunning;
        if (searchQuery || !listHasMore) return;
        const gen = listGeneration;
        const filters = listFilterArgs();
        const rows = await listConversations(
          filters.accountId,
          filters.workspaceId,
          filters.priorityGroup,
          filters.archivedOnly,
          listOffset,
          LIST_PAGE_SIZE
        );
        // The list changed while this page loaded, so its offset may be stale: read it again.
        if (gen !== listGeneration) continue;
        if (searchQuery) return;
        const seen = new Set(conversations.map((c) => c.id));
        const fresh = rows.filter((c) => !seen.has(c.id));
        conversations = sortConversations([...conversations, ...fresh]);
        listOffset += rows.length;
        listHasMore = listOffset < listTotal;
        queueAvatarFetches(rows);
        return;
      }
    } finally {
      listLoadingMore = false;
    }
  }

  async function refresh() {
    accounts = await listAccounts();
    unreadTotal = await totalUnread();
    unreadCounts = await unreadByAccount();
    workspaces = await listWorkspaces();
    priorityGroups = await listPriorityGroups();
    forwardRules = await listForwardRules();
    scheduledMessages = await listScheduledMessages();
    if (searchQuery) {
      await handleSearch(searchQuery);
    } else {
      await reloadConversationList();
      searchMessageHits = [];
    }
    const pending = restoreConversationId;
    if (pending) {
      restoreConversationId = null;
      if (conversations.some((c) => c.id === pending)) await selectConversation(pending);
    }
    if (!prefillStarted) {
      prefillStarted = true;
      void prefillMessageCache();
    }
  }

  const profileQueued = new Set<string>();
  const profileQueue: Conversation[] = [];
  let profileTimer: number | undefined;
  function queueUnknownProfiles(list: Conversation[]) {
    for (const conv of list) {
      const account = accounts.find((a) => a.id === conv.account_id);
      const names = conv.metadata?.names as Record<string, string> | undefined;
      if (account?.connector_id !== 'whatsapp' || account.status !== 'connected' || conv.conversation_type !== 'direct' || conv.metadata?.is_self || names?.contact || names?.verified || profileQueued.has(conv.id)) continue;
      profileQueued.add(conv.id);
      profileQueue.push(conv);
    }
    if (profileTimer !== undefined || !profileQueue.length) return;
    const next = () => {
      const conv = profileQueue.shift();
      if (!conv) { profileTimer = undefined; return; }
      void fetchContactProfile(conv.account_id, conv.id).catch(() => { profileQueued.delete(conv.id); });
      profileTimer = window.setTimeout(next, 1000);
    };
    profileTimer = window.setTimeout(next, 1000);
  }

  const avatarQueued = new Set<string>();
  function queueAvatarFetches(list: Conversation[]) {
    queueUnknownProfiles(list);
    for (const conv of list.slice(0, 48)) {
      if (typeof conv.metadata?.avatar_data === 'string') continue;
      if (avatarQueued.has(conv.id)) continue;
      avatarQueued.add(conv.id);
      fetchConversationAvatar(conv.account_id, conv.id).catch(() => {
        avatarQueued.delete(conv.id);
      });
    }
  }

  /** Replace the open run with a page the backend just read, flags included. */
  function applyWindow(next: MessageWindow) {
    messages = next.messages;
    hasOlder = next.hasOlder;
    hasNewer = next.hasNewer;
  }

  /** The run as the window helpers see it. */
  function currentWindow(): MessageWindow {
    return { messages, hasOlder, hasNewer };
  }

  /** Drop the run when no chat is open. */
  function clearMessages() {
    applyWindow(emptyWindow());
    loadingOlder = false;
    loadingNewer = false;
  }

  async function loadMessages(convId: string) {
    const page = await getMessagePage(convId);
    messageCache.set(convId, page.messages);
    if (selectedConversationId === convId) {
      applyWindow(openWindow(page));
      messagesLoading = false;
    }
    const conv = conversations.find((c) => c.id === convId);
    if (conv) {
      syncConversation(conv.account_id, conv.id).catch(() => {});
    }
    await markRead(convId);
    unreadTotal = await totalUnread();
    unreadCounts = await unreadByAccount();
    conversations = conversations.map((c) =>
      c.id === convId ? { ...c, unread_count: 0 } : c
    );
    noteLocalListChange();
  }

  /**
   * Re-read what the open chat holds, for an event that may have changed stored rows the reader
   * already sees. A run that reaches the newest message takes the newest page; a run stopped short
   * of the live end (a jump into history) re-reads its own span, so no page is thrown away.
   */
  async function refreshWindow(convId: string) {
    if (selectedConversationId !== convId) return;
    const held = currentWindow();
    const newest = held.messages[held.messages.length - 1];
    const deepInHistory = held.hasNewer && !!newest;
    const limit = Math.max(held.messages.length, MESSAGE_PAGE_ROWS);
    const page = await getMessagePage(
      convId,
      deepInHistory ? { kind: 'from', message_id: newest.id } : { kind: 'newest' },
      limit
    );
    if (selectedConversationId !== convId) return;
    applyWindow(deepInHistory ? reReadSpan(held, page) : mergeNewest(held, page));
    messageCache.set(convId, messages);
  }

  /** The page of history above the run, read when the reader scrolls to the top. */
  async function loadOlder() {
    const convId = selectedConversationId;
    const oldest = messages[0];
    if (!convId || !oldest || loadingOlder || !hasOlder) return;
    loadingOlder = true;
    try {
      const page = await getMessagePage(convId, { kind: 'before', message_id: oldest.id });
      if (selectedConversationId !== convId) return;
      applyWindow(prependOlder(currentWindow(), page));
    } catch (e) {
      // The row the page keyed on is gone; start the run again instead of leaving it stuck.
      console.error('older page failed', e);
      await loadMessages(convId).catch(() => {});
    } finally {
      loadingOlder = false;
    }
  }

  /** The page below a run the reader jumped into history from, read at the bottom. */
  async function loadNewer() {
    const convId = selectedConversationId;
    const newest = messages[messages.length - 1];
    if (!convId || !newest || loadingNewer || !hasNewer) return;
    loadingNewer = true;
    try {
      const page = await getMessagePage(convId, { kind: 'after', message_id: newest.id });
      if (selectedConversationId !== convId) return;
      applyWindow(appendNewer(currentWindow(), page));
    } catch (e) {
      console.error('newer page failed', e);
    } finally {
      loadingNewer = false;
    }
  }

  async function activateAccount(accountId: string | null) {
    await setActiveAccount(accountId);
    if (!accountId) return;
    const account = accounts.find((a) => a.id === accountId);
    if (!account || account.disabled || account.status !== 'sleeping') return;
    try {
      await wakeAccount(accountId);
    } catch (e) {
      console.error('wake account failed', e);
    }
  }

  async function selectConversation(id: string) {
    if (selectedConversationId && !messagesLoading) messageCache.set(selectedConversationId, messages);
    selectedConversationId = id;
    draft = draftFor(id);
    listFocusId = id;
    mobileView = 'thread';
    contactView = null;
    const cached = messageCache.get(id);
    messages = cached ?? [];
    hasOlder = false;
    hasNewer = false;
    loadingOlder = false;
    loadingNewer = false;
    messagesLoading = !cached;
    const conv = conversations.find((c) => c.id === id);
    const load = loadMessages(id).catch((e) => {
      if (selectedConversationId === id) messagesLoading = false;
      throw e;
    });
    if (conv) await activateAccount(conv.account_id);
    await load;
  }

  async function openStartedChat(conv: Conversation) {
    startedChat = conv;
    searchQuery = '';
    searchMessageHits = [];
    await selectConversation(conv.id);
    reloadConversationList().catch(() => {});
  }

  async function selectAccount(id: string | null) {
    selectedAccountId = id;
    selectedConversationId = null;
    listFocusId = null;
    showArchived = false;
    clearMessages();
    mobileView = 'list';
    mobileTab = 'inbox';
    await activateAccount(id);
    await refresh();
  }

  function selectMobileTab(tab: 'inbox' | 'settings') {
    mobileTab = tab;
    mobileView = 'list';
    selectedConversationId = null;
    clearMessages();
  }

  async function handleSearch(q: string) {
    searchQuery = q;
    if (!q) {
      searchMessageHits = [];
      await reloadConversationList();
      return;
    }
    const results = await searchMessages(
      q,
      searchScope,
      searchScope === 'account' ? selectedAccountId ?? undefined : undefined,
      searchScope === 'conversation' ? selectedConversationId ?? undefined : undefined
    );
    conversations = sortConversations(results.conversations);
    searchMessageHits = results.messages;
  }

  async function handleSearchScope(scope: SearchScope) {
    searchScope = scope;
    if (searchQuery) await handleSearch(searchQuery);
  }

  /**
   * Scroll the open thread to a message and flash it (todo and note quotes use this). A message
   * outside the loaded run is read around instead of widening the run a page at a time.
   */
  async function jumpToMessage(messageId: string) {
    const convId = selectedConversationId;
    if (!convId) return;
    const show = async () => {
      await tick();
      return threadView?.scrollToMessage(messageId) ?? false;
    };
    if (messages.some((m) => m.id === messageId) && (await show())) return;
    const page = await getMessagePage(convId, { kind: 'around', message_id: messageId });
    if (selectedConversationId !== convId) return;
    if (!page.messages.some((m) => m.id === messageId)) return; // deleted since the jump started
    applyWindow(openWindow(page));
    await show();
  }

  async function jumpToSearchHit(hit: SearchMessageHit) {
    const convId = hit.message.conversation_id;
    selectedConversationId = convId;
    mobileView = 'thread';
    searchQuery = '';
    searchMessageHits = [];
    await activateAccount(hit.account_id);
    await refresh();
    await loadMessages(convId);
    await jumpToMessage(hit.message.id);
  }

  async function handleStartCall(mode: 'audio' | 'video') {
    if (!selectedConversation) return;
    try {
      activeCall = await startCall(selectedConversation.account_id, selectedConversation.id, mode);
      contactView = null;
    } catch (e) {
      console.error(e);
    }
  }

  async function toggleArchived() {
    showArchived = !showArchived;
    selectedConversationId = null;
    clearMessages();
    mobileView = 'list';
    await refresh();
  }

  async function handleSend(expanded?: string) {
    if (!draft.trim() || !selectedConversation) return;
    const conversation = selectedConversation;
    const text = normalizeRichText((typeof expanded === 'string' ? expanded : draft).trim(), connectorForAccount(conversation.account_id));
    const replyTarget = replyingTo?.conversation_id === conversation.id ? replyingTo : null;
    draft = '';
    replyingTo = null;
    sendError = null;
    try {
      const sent = await sendMessage(conversation.account_id, conversation.id, text, replyTarget?.id);
      if (selectedConversationId === conversation.id) {
        const exists = messages.some((m) => m.id === sent.id);
        // Sending from a run that stops short of the live end returns to the newest page, where
        // the new message is, instead of leaving a hole under the rows on screen.
        if (!exists && hasNewer) applyWindow(openWindow(await getMessagePage(conversation.id)));
        else if (!exists) messages = [...messages, sent];
      }
      patchConversation({
        ...conversation,
        last_message_at: sent.timestamp,
        last_message_preview: sent.body,
        last_message_from_me: true,
        last_message_sender_id: null,
        last_message_sender_name: null,
      });
      const latestMessages = await getMessagePage(conversation.id);
      if (selectedConversationId === conversation.id) {
        applyWindow(mergeNewest(currentWindow(), latestMessages));
      }
    } catch (e) {
      sendError = e instanceof Error ? e.message : String(e);
      await loadMessages(conversation.id);
      const saved = messages.some(
        (m) =>
          m.direction === 'outbound' &&
          m.body === text &&
          (m.status === 'failed' || m.status === 'pending')
      );
      if (!saved) {
        draft = text;
        if (selectedConversationId === conversation.id) replyingTo = replyTarget;
      }
      return;
    }
  }

  async function handleSendAttachment(attachment: AttachmentPayload) {
    if (!selectedConversation) return;
    sendError = null;
    const replyTarget = replyingTo?.conversation_id === selectedConversation.id ? replyingTo : null;
    replyingTo = null;
    try {
      await sendAttachment(selectedConversation.account_id, selectedConversation.id, {
        ...attachment,
        reply_to_message_id: replyTarget?.id,
      });
    } catch (e) {
      sendError = e instanceof Error ? e.message : String(e);
      if (replyTarget && selectedConversationId === replyTarget.conversation_id) replyingTo = replyTarget;
    }
    await loadMessages(selectedConversation.id);
  }

  async function handleSendLater(sendAt: string) {
    if (!draft.trim() || !selectedConversation) return;
    await scheduleMessage({
      dest_account_id: selectedConversation.account_id,
      dest_conversation_id: selectedConversation.id,
      body: normalizeRichText(draft.trim(), connectorForAccount(selectedConversation.account_id)),
      send_at: sendAt,
    });
    draft = '';
    scheduledMessages = await listScheduledMessages();
    panelOpen = true;
  }

  async function handleCreateAccount(connectorId: string, name: string, credentials: Record<string, string>, accentColor: string) {
    connecting = true;
    setupError = null;
    qrData = null;
    authMethod = null;
    authMessage = null;
    const account = await createAccount(connectorId, name);
    await updateAccount(account.id, { accent_color: accentColor });
    pendingAccountId = account.id;
    accounts = await listAccounts();
    try {
      await connectAccount(account.id, credentials);
    } catch (e) {
      setupError = e instanceof Error ? e.message : String(e);
      connecting = false;
    }
  }

  async function handleSubmitAuth(credentials: Record<string, string>) {
    if (!pendingAccountId) return;
    connecting = true;
    setupError = null;
    try {
      await submitAuth(pendingAccountId, credentials);
    } catch (e) {
      setupError = e instanceof Error ? e.message : String(e);
      connecting = false;
    }
  }

  async function persistConfig(cfg: AppConfig) {
    appConfig = await saveAppConfig(cfg);
    applyAppConfig(appConfig);
  }

  const AUTO_DOWNLOAD_LABELS: [AutoDownloadGroup, string][] = [
    ['images', 'images'],
    ['videos', 'videos'],
    ['files', 'files'],
  ];

  /** One toggle per media group showing the effective choice, plus a reset to the account default. */
  function autoDownloadMenuItems(conv: Conversation): MenuItem[] {
    const account = accounts.find((a) => a.id === conv.account_id);
    const overrides = conv.auto_download ?? {};
    const hasOverride = AUTO_DOWNLOAD_LABELS.some(([g]) => overrides[g] != null);
    return [
      ...AUTO_DOWNLOAD_LABELS.map(([group, label]) => {
        const on = effectiveAutoDownload(group, overrides, account?.auto_download);
        return {
          id: `adl-${group}`,
          label: `Auto-download ${label}: ${on ? 'on' : 'off'}${overrides[group] != null ? ' (this chat)' : ''}`,
        };
      }),
      { id: 'adl-inherit', label: 'Auto-download: use account default', disabled: !hasOverride },
    ];
  }

  function convMenu(conv: Conversation, x: number, y: number) {
    openMenu(
      x,
      y,
      [
        { id: 'open', label: 'Open' },
        { id: 'pin', label: conv.pinned ? 'Unpin' : 'Pin' },
        { id: 'mute', label: conv.muted ? 'Unmute' : 'Mute' },
        { id: 'archive', label: conv.archived ? 'Unarchive' : 'Archive' },
        { id: 'notes', label: 'Notes' },
        { id: 'remind', label: 'Remind' },
        { id: 'unread', label: 'Mark unread' },
        { id: 'sep1', label: '', separator: true },
        ...workspaces.map((w) => ({ id: `ws:${w.id}`, label: `Workspace: ${w.name}` })),
        ...priorityGroups.map((g) => ({ id: `pg:${g.id}`, label: `Priority: ${g.name}` })),
        { id: 'clear-pg', label: 'Clear priority' },
        { id: 'sep2', label: '', separator: true },
        { id: 'notify-on', label: 'Notify for this chat' },
        { id: 'notify-off', label: 'Silence this chat' },
        { id: 'notify-inherit', label: 'Notifications: inherit' },
        { id: 'receipts-on', label: 'Send read receipts' },
        { id: 'receipts-off', label: 'Don’t send read receipts' },
        { id: 'receipts-inherit', label: 'Receipts: inherit' },
        { id: 'sep3', label: '', separator: true },
        ...autoDownloadMenuItems(conv),
      ],
      async (id) => {
        if (id === 'open') await selectConversation(conv.id);
        else if (id === 'pin') await updateConversation(conv.id, { pinned: !conv.pinned });
        else if (id === 'mute') await updateConversation(conv.id, { muted: !conv.muted });
        else if (id === 'archive') await updateConversation(conv.id, { archived: !conv.archived });
        else if (id === 'notes') {
          if (selectedConversation?.id !== conv.id) await selectConversation(conv.id);
          panelOpen = true;
        } else if (id === 'remind') {
          await selectConversation(conv.id);
          remindNoteSeed = conv.title ?? '';
          remindOpen = true;
        } else if (id === 'unread') await markUnread(conv.id);
        else if (id.startsWith('ws:')) await updateConversation(conv.id, { workspace_id: id.slice(3) });
        else if (id.startsWith('pg:')) await updateConversation(conv.id, { priority_group: id.slice(3) });
        else if (id === 'clear-pg') await updateConversation(conv.id, { clear_priority: true });
        else if (id === 'notify-on') await updateConversation(conv.id, { notify_enabled: true });
        else if (id === 'notify-off') await updateConversation(conv.id, { notify_enabled: false });
        else if (id === 'notify-inherit') await updateConversation(conv.id, { clear_notify: true });
        else if (id === 'receipts-on') await updateConversation(conv.id, { send_receipts: true });
        else if (id === 'receipts-off') await updateConversation(conv.id, { send_receipts: false });
        else if (id === 'receipts-inherit') await updateConversation(conv.id, { clear_receipts: true });
        else if (id === 'adl-inherit') await updateConversation(conv.id, { auto_download: {} });
        else if (id.startsWith('adl-')) {
          const group = id.slice(4) as AutoDownloadGroup;
          const account = accounts.find((a) => a.id === conv.account_id);
          const now = effectiveAutoDownload(group, conv.auto_download, account?.auto_download);
          await updateConversation(conv.id, {
            auto_download: { ...(conv.auto_download ?? {}), [group]: !now },
          });
        }
        await refresh();
      }
    );
  }

  function accountMenu(account: Account, x: number, y: number) {
    openMenu(
      x,
      y,
      [
        { id: 'edit', label: 'Edit account' },
        { id: 'mute', label: account.muted ? 'Unmute account' : 'Mute account' },
        { id: 'disable', label: account.disabled ? 'Enable account' : 'Disable account' },
        ...(account.status === 'sleeping' && !account.disabled
          ? [{ id: 'wake', label: 'Wake account' }]
          : []),
        { id: 'receipts', label: account.send_receipts ? 'Disable read receipts' : 'Enable read receipts' },
        { id: 'sep-ws', label: '', separator: true },
        ...workspaces.map((w) => ({ id: `ws:${w.id}`, label: `Workspace: ${w.name}` })),
        { id: 'sep', label: '', separator: true },
        { id: 'remove', label: 'Remove account', danger: true },
      ],
      async (id) => {
        if (id === 'edit') {
          editingAccount = account;
          editAccountName = account.name;
          editAccountColor = typeof account.metadata?.accent_color === 'string' ? account.metadata.accent_color : CONNECTOR_COLORS[account.connector_id] ?? '#888888';
        } else if (id === 'disable') {
          await handleAccount(account.id, account.disabled ? 'enable' : 'disable');
        } else if (id === 'wake') {
          if (account.disabled || account.status !== 'sleeping') return;
          try {
            await wakeAccount(account.id);
            await refresh();
          } catch (e) {
            console.error('wake account failed', e);
          }
        } else if (id.startsWith('ws:')) {
          await handleAccount(account.id, 'workspace', id.slice(3));
        } else {
          await handleAccount(account.id, id as 'mute' | 'remove' | 'receipts');
        }
      }
    );
  }

  async function handleAccount(
    id: string,
    action: 'mute' | 'disable' | 'enable' | 'remove' | 'receipts' | 'workspace',
    extra?: string
  ) {
    const account = accounts.find((a) => a.id === id);
    if (!account) return;
    if (action === 'remove') {
      if (!confirm(`Remove ${account.name}? Local history and the login session will be deleted.`)) return;
      await deleteAccount(id);
      if (selectedAccountId === id) selectedAccountId = null;
    } else if (action === 'mute') {
      await updateAccount(id, { muted: !account.muted });
    } else if (action === 'disable' || action === 'enable') {
      await updateAccount(id, { disabled: action === 'disable' ? true : false });
    } else if (action === 'receipts') {
      await updateAccount(id, { send_receipts: !account.send_receipts });
    } else if (action === 'workspace' && extra) {
      await updateAccount(id, { workspace_id: extra });
    }
    await refresh();
  }

  async function saveMessageMedia(msg: Message) {
    if (!selectedConversation || !isDownloadableMedia(msg)) return;
    await saveMessageMediaFile(
      selectedConversation.account_id,
      selectedConversation.id,
      msg,
      mediaNameOwner(selectedConversation, accounts)
    );
  }

  function actionsFor(conv: Conversation | null) {
    return connectors.find((c) => c.id === connectorForAccount(conv?.account_id))?.actions;
  }

  /** Apply one op to messages of the open chat; failures become one toast. True when all applied. */
  async function runMessageOp(msgs: Message[], op: MessageOp): Promise<boolean> {
    if (!selectedConversation || msgs.length === 0) return false;
    try {
      const results = await applyMessageOp(
        selectedConversation.id,
        msgs.map((m) => m.id),
        op
      );
      const failed = results.filter((r) => r.error);
      if (failed.length === 1) toast.error(failed[0].error ?? 'That did not work');
      else if (failed.length > 1) toast.error(`${failed.length} messages failed: ${failed[0].error}`);
      return failed.length === 0;
    } catch (e) {
      toast.error(String(e));
      return false;
    }
  }

  /** Shown at once; the `message.reactions` event that follows is the truth. */
  async function react(msg: Message, emoji: string) {
    const current = messages.find((m) => m.id === msg.id) ?? msg;
    const before = current.reactions;
    const next = toggledReaction(before, emoji);
    const optimistic = withMyReaction(before, next);
    messages = messages.map((m) => (m.id === msg.id ? { ...m, reactions: optimistic } : m));
    if (!(await runMessageOp([current], { kind: 'react', emoji: next }))) {
      messages = messages.map((m) =>
        m.id === msg.id && m.reactions === optimistic ? { ...m, reactions: before } : m
      );
    }
  }

  async function chooseDelete(kind: 'delete_for_me' | 'delete_for_everyone') {
    const all = deleteTargets;
    deleteTargets = [];
    threadView?.cancelSelection();
    const actions = actionsFor(selectedConversation);
    const targets = kind === 'delete_for_everyone' ? all.filter((m) => offeredActions(actions, m).deleteForEveryone) : all;
    const skipped = all.length - targets.length;
    if (skipped) toast.info(`Skipped ${skipped} ${skipped === 1 ? 'message' : 'messages'}: only your own recent messages can be deleted for everyone`);
    await runMessageOp(targets, { kind });
  }

  async function runSelectionAction(action: SelectionAction, msgs: Message[]) {
    if (action === 'forward') {
      forwardSources = msgs;
      return;
    }
    if (action === 'delete') {
      deleteTargets = msgs;
      return;
    }
    threadView?.cancelSelection();
    if (action === 'copy') {
      await navigator.clipboard.writeText(msgs.map(captionText).filter(Boolean).join('\n\n'));
      toast.copied(msgs.length === 1 ? 'Copied 1 message' : `Copied ${msgs.length} messages`);
    } else if (action === 'star') {
      const star = !msgs.every((m) => m.starred);
      const updated = new Map((await Promise.all(msgs.map((m) => starMessage(m.id, star)))).map((m) => [m.id, m]));
      messages = messages.map((m) => updated.get(m.id) ?? m);
    } else if (action === 'translate') {
      const withText = msgs.filter((m) => captionText(m));
      const skipped = msgs.length - withText.length;
      if (skipped) toast.info(`Skipped ${skipped} ${skipped === 1 ? 'message' : 'messages'} without text`);
      await translateMessages(withText);
    }
  }

  /** A group member's details: their chat on this account, else a view that stores nothing. */
  async function viewSender(msg: Message) {
    const conv = selectedConversation;
    const senderId = msg.sender_id?.trim();
    if (!conv || !senderId) return;
    const found = await senderContact(conv.account_id, senderId);
    if (found.conversation) {
      contactView = { of: 'sender', conversation: found.conversation, transient: false };
      return;
    }
    contactView = {
      of: 'sender',
      transient: true,
      conversation: {
        id: '',
        account_id: conv.account_id,
        remote_id: senderId,
        contact_id: null,
        title: found.contact?.display_name || msg.sender_name || senderId,
        conversation_type: 'direct',
        unread_count: 0,
        last_message_at: null,
        last_message_preview: null,
        pinned: false,
        archived: false,
        muted: false,
        metadata: found.contact?.avatar_url ? { avatar_url: found.contact.avatar_url } : {},
      },
    };
  }

  async function saveEdit(text: string) {
    const msg = editingMessage;
    editingMessage = null;
    if (msg) await runMessageOp([msg], { kind: 'edit', text });
  }

  function openTranslateSettings() {
    toast.info('Add a translation server to translate messages');
    settingsFocus = { field: 'openrouter-key', at: Date.now() };
    mobileTab = 'settings';
  }

  async function translateMessages(msgs: Message[]) {
    const conv = selectedConversation;
    if (!conv || msgs.length === 0) return;
    if (!appConfig.ai?.openrouter_api_key?.trim()) return openTranslateSettings();
    const target = (navigator.language || 'en').split('-')[0];
    const ids = msgs.map((m) => m.id);
    translations = { ...translations, ...Object.fromEntries(ids.map((id) => [id, { state: 'loading' as const }])) };
    try {
      const out = await translateText(conv.id, ids, target);
      const views = out.map((t): [string, TranslationView] => [
        t.message_id,
        t.text != null ? { state: 'done', text: t.text } : { state: 'error', message: t.error ?? 'Translation failed' },
      ]);
      translations = { ...translations, ...Object.fromEntries(views) };
    } catch (e) {
      translations = Object.fromEntries(Object.entries(translations).filter(([id]) => !ids.includes(id)));
      const err = e as TranslateError;
      if (err?.kind === 'not_configured') openTranslateSettings();
      else toast.error(err?.kind === 'failed' ? err.message : String(e));
    }
  }

  $effect(() => {
    if (!reactPicker) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') reactPicker = null;
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  });

  function msgMenu(msg: Message, x: number, y: number) {
    const urls = urlsIn(msg.body);
    const offered = offeredActions(actionsFor(selectedConversation), msg);
    const mine = msg.reactions?.find((g) => g.mine)?.emoji;
    const deleteItems: MenuItem[] =
      offered.deleteForMe || offered.deleteForEveryone
        ? [
            { id: 'sep-delete', label: '', separator: true },
            { id: 'delete', label: 'Delete…', danger: true },
          ]
        : [];
    const items: MenuItem[] = isRevoked(msg)
      ? deleteItems.slice(1)
      : [
          ...(offered.react
            ? [
                {
                  id: 'react-bar',
                  label: 'React',
                  row: [
                    ...QUICK_REACTIONS.map((e) => ({ id: `react:${e}`, label: e, title: `React with ${e}`, active: mine === e })),
                    { id: 'react-more', label: '+', title: 'More reactions' },
                  ],
                },
              ]
            : []),
          { id: 'copy', label: 'Copy' },
          // A message that is still sending has no WhatsApp id to answer yet.
          ...(msg.remote_id ? [{ id: 'reply', label: 'Reply' }] : []),
          ...(offered.edit ? [{ id: 'edit', label: 'Edit' }] : []),
          { id: 'forward', label: 'Forward…' },
          { id: 'select', label: 'Select' },
          ...(selectedConversation?.conversation_type === 'group' && msg.direction === 'inbound' && msg.sender_id
            ? [{ id: 'view-contact', label: 'View contact' }]
            : []),
          { id: msg.starred ? 'unstar' : 'star', label: msg.starred ? 'Unstar' : 'Star' },
          { id: msg.pinned ? 'unpin' : 'pin', label: msg.pinned ? 'Unpin' : 'Pin' },
          { id: 'todo', label: 'Add to todo list' },
          { id: 'note', label: 'Add note' },
          ...(captionText(msg) ? [{ id: 'translate', label: 'Translate' }] : []),
          ...(isDownloadableMedia(msg)
            ? [
                { id: 'copy-media', label: 'Copy media' },
                { id: 'save-media', label: 'Save media' },
              ]
            : []),
          ...urls.map((u, i) => ({ id: `url:${i}`, label: `Open ${u.slice(0, 40)}` })),
          ...deleteItems,
        ];
    openMenu(
      x,
      y,
      items,
      async (id) => {
        if (id.startsWith('react:')) await react(msg, id.slice('react:'.length));
        else if (id === 'react-more') reactPicker = { msg, x, y };
        else if (id === 'edit') editingMessage = msg;
        else if (id === 'delete') deleteTargets = [msg];
        else if (id === 'translate') await translateMessages([msg]);
        else if (id === 'copy') {
          // A caption-less media message's body is a placeholder like `[image]`, so the file
          // itself is what the user means by Copy.
          if (selectedConversation && isDownloadableMedia(msg)) {
            await copyMessageMediaToClipboard(
              selectedConversation.account_id,
              selectedConversation.id,
              msg,
              mediaNameOwner(selectedConversation, accounts)
            );
          } else {
            await navigator.clipboard.writeText(msg.body);
          }
        } else if (id === 'reply') replyingTo = msg;
        else if (id === 'forward') forwardSources = [msg];
        else if (id === 'select') threadView?.startSelection(msg);
        else if (id === 'view-contact') await viewSender(msg);
        else if (id === 'star' || id === 'unstar') {
          const updated = await starMessage(msg.id, id === 'star');
          messages = messages.map((m) => (m.id === msg.id ? updated : m));
        } else if (id === 'pin' || id === 'unpin') {
          const updated = await pinMessage(msg.id, id === 'pin');
          messages = messages.map((m) => (m.id === msg.id ? updated : m));
        } else if (id === 'todo' && selectedConversation) {
          const snippet = messageSnippet(msg);
          await addTodo(
            selectedConversation.id,
            selectedConversation.account_id,
            snippet || 'Follow up on message',
            undefined,
            { id: msg.id, snippet }
          );
          panelOpen = true;
        } else if (id === 'note' && selectedConversation) {
          noteDraftRef = { id: msg.id, snippet: messageSnippet(msg) };
          panelOpen = true;
        } else if (id === 'copy-media' && selectedConversation) {
          await copyMessageMediaToClipboard(
            selectedConversation.account_id,
            selectedConversation.id,
            msg,
            mediaNameOwner(selectedConversation, accounts)
          );
        } else if (id === 'save-media') {
          await saveMessageMedia(msg);
        } else if (id.startsWith('url:')) {
          const u = urls[Number(id.slice(4))];
          if (u) await openExternal(u);
        }
      }
    );
  }

  async function submitReminder(fireAt: string, note: string) {
    if (!selectedConversation) return;
    await addNote(selectedConversation.id, selectedConversation.account_id, note || 'Reminder', {
      schedule: { alarm_at: fireAt },
    });
    remindOpen = false;
    remindNoteSeed = '';
    panelOpen = true;
  }

  function textMenu(text: string, x: number, y: number) {
    const { selected, inner } = selectionContext();
    const value = selected || text;
    const urls = urlsIn(value);
    const items: MenuItem[] = [
      { id: 'copy', label: 'Copy' },
      ...(inner ? [{ id: 'copy-inner', label: 'Copy inner text' }] : []),
      ...urls.map((_, i) => ({ id: `url:${i}`, label: `Open URL` })),
    ];
    openMenu(x, y, items, async (id) => {
      if (id === 'copy') await navigator.clipboard.writeText(value);
      else if (id === 'copy-inner' && inner) await navigator.clipboard.writeText(inner);
      else if (id.startsWith('url:') && urls[0]) await openExternal(urls[0]);
    });
  }

  function focusedConversation(): Conversation | null {
    const id = listFocusId ?? selectedConversationId;
    if (!id) return null;
    return conversations.find((c) => c.id === id) ?? null;
  }

  function moveListFocus(delta: number) {
    if (conversations.length === 0) return;
    const currentId = listFocusId ?? selectedConversationId;
    let idx = currentId ? conversations.findIndex((c) => c.id === currentId) : -1;
    if (idx < 0) idx = delta > 0 ? -1 : conversations.length;
    const next = (idx + delta + conversations.length) % conversations.length;
    listFocusId = conversations[next]?.id ?? null;
  }

  async function toggleFocusedArchive() {
    const conv = focusedConversation();
    if (!conv) return;
    await updateConversation(conv.id, { archived: !conv.archived });
    await refresh();
  }

  async function toggleFocusedMute() {
    const conv = focusedConversation();
    if (!conv) return;
    await updateConversation(conv.id, { muted: !conv.muted });
    await refresh();
  }

  async function markFocusedUnread() {
    const conv = focusedConversation();
    if (!conv) return;
    await markUnread(conv.id);
    await refresh();
  }

  function handleGlobalKeydown(e: KeyboardEvent) {
    const typing = isTypingTarget(e.target);
    const key = e.key.toLowerCase();

    if (isModKey(e) && key === 'k') {
      e.preventDefault();
      quickSwitchOpen = true;
      return;
    }

    if (key === 'escape') {
      if (quickSwitchOpen) {
        e.preventDefault();
        quickSwitchOpen = false;
        return;
      }
      if (contactView) {
        e.preventDefault();
        contactView = null;
        return;
      }
      if (threadView?.cancelSelection()) {
        e.preventDefault();
        return;
      }
      if (panelOpen) {
        e.preventDefault();
        panelOpen = false;
        return;
      }
      return;
    }

    if (typing || quickSwitchOpen || settingsOpen || showSetup) return;

    if (key === '/') {
      e.preventDefault();
      focusSearchNonce += 1;
      return;
    }

    if (key === 'j' || e.key === 'ArrowDown') {
      e.preventDefault();
      moveListFocus(1);
      return;
    }

    if (key === 'k' || e.key === 'ArrowUp') {
      e.preventDefault();
      moveListFocus(-1);
      return;
    }

    if (e.key === 'Enter') {
      const id = listFocusId ?? selectedConversationId;
      if (id && id !== selectedConversationId) {
        e.preventDefault();
        void selectConversation(id);
      }
      return;
    }

    if (key === 'e') {
      e.preventDefault();
      void toggleFocusedArchive();
      return;
    }

    if (key === 'm') {
      e.preventDefault();
      void toggleFocusedMute();
      return;
    }

    if (key === 'u') {
      e.preventDefault();
      void markFocusedUnread();
    }
  }

  $effect(() => {
    setWindowTitleUnread(unreadTotal);
    void updateTrayUnread(unreadTotal);
  });

  const DRAFT_SAVE_DEBOUNCE_MS = 400;
  let draftSaveTimer: number | undefined;

  // The open filters, chat and half-written replies outlive the window, so the tray can rebuild
  // it without the user losing their place.
  $effect(() => {
    if (!hydrated) return;
    saveUiPlace({
      workspaceId: selectedWorkspace,
      accountId: selectedAccountId,
      conversationId: selectedConversationId,
    });
  });

  $effect(() => {
    const conversationId = selectedConversationId;
    const body = draft;
    if (!hydrated || !conversationId) return;
    window.clearTimeout(draftSaveTimer);
    draftSaveTimer = window.setTimeout(() => saveDraft(conversationId, body), DRAFT_SAVE_DEBOUNCE_MS);
  });

  $effect(() => {
    if (selectedConversationId && !listFocusId) {
      listFocusId = selectedConversationId;
    }
  });

  onMount(() => {
    const startedAt = performance.now();
    void initTelemetry();
    const measureExtras = () => {
      const screenW = window.screen?.width || window.innerWidth;
      narrowWidth = window.innerWidth < Math.min(900, screenW * 0.6);
    };
    measureExtras();
    const phoneQuery = window.matchMedia('(max-width: 639px)');
    const syncPhone = () => (isPhone = phoneQuery.matches);
    syncPhone();
    phoneQuery.addEventListener('change', syncPhone);
    window.addEventListener('resize', measureExtras);
    window.addEventListener('keydown', handleGlobalKeydown);
    listConnectors().then((c) => (connectors = c));
    void checkForNewRelease(true);
    const updateTimer = setInterval(() => void checkForNewRelease(false), 6 * 60 * 60 * 1000);
    getAppConfig().then((cfg) => {
      appConfig = cfg;
      applyAppConfig(cfg);
    });

    if (setupOnly) {
      showSetup = true;
    } else {
      const saved = loadUiState();
      selectedWorkspace = saved.workspaceId;
      selectedAccountId = saved.accountId;
      restoreConversationId = saved.conversationId;
      hydrated = true;
      void currentCall()
        .then((call) => {
          if (call) activeCall = call;
        })
        .catch(() => {});
      refresh();
    }
    void markAppReady(Math.round(performance.now() - startedAt));

    const unsub = onShuttleEvent(async (event) => {
      if (event.kind === 'auth.required') {
        const data = event.payload.qr_data;
        if (typeof data === 'string' && data.length > 0) qrData = data;
        if (typeof event.payload.method === 'string') authMethod = event.payload.method;
        authMessage = typeof event.payload.message === 'string' ? event.payload.message : null;
        connecting = true;
      }
      if (event.kind === 'account.error') {
        setupError = typeof event.payload.message === 'string' ? event.payload.message : 'Connection failed';
        connecting = false;
      }
      if (event.kind === 'media.downloaded') {
        const msg = event.payload.message as Message | undefined;
        if (msg && selectedConversationId === msg.conversation_id) {
          messages = messages.map((m) => (m.id === msg.id ? msg : m));
        }
      }
      if (event.kind === 'media.purged' || event.kind === 'media.thumbnails') {
        // Retention reset some messages' media (or previews arrived): reload the open thread.
        const accountsHit = event.payload.accounts as string[] | undefined;
        const open = selectedConversation;
        const touched =
          event.kind === 'media.thumbnails'
            ? event.payload.account_id === open?.account_id
            : !!open && (accountsHit ?? []).includes(open.account_id);
        if (open && touched) {
          refreshWindow(open.id).catch(() => {});
        }
      }
      if (event.kind === 'avatar.updated') {
        const convId = event.payload.conversation_id;
        const data = event.payload.avatar_data;
        if (typeof convId === 'string') {
          conversations = conversations.map((c) =>
            c.id === convId
              ? { ...c, metadata: { ...c.metadata, avatar_data: data } }
              : c
          );
          noteLocalListChange();
        }
      }
      if (event.kind === 'status.feed') {
        const accountId =
          typeof event.payload.account_id === 'string' ? event.payload.account_id : undefined;
        const items = event.payload.items as StatusItem[] | undefined;
        const upsert = event.payload.upsert as StatusItem | undefined;
        // A successful empty snapshot clears expired statuses for this account.
        if (Array.isArray(items)) {
          const stamped = items.map((item) => ({
            ...item,
            account_id: item.account_id ?? accountId,
          }));
          if (accountId) {
            statusFeed = [
              ...stamped,
              ...statusFeed.filter((s) => s.account_id && s.account_id !== accountId),
            ];
          } else {
            statusFeed = stamped;
          }
        }
        if (Array.isArray(items)) void loadStatusPreviews(accountId, items);
        if (upsert?.sender_id) {
          void loadStatusPreviews(accountId, [upsert]);
          const upsertAccountId = upsert.account_id ?? accountId;
          const prev = statusFeed.find((s) => s.sender_id === upsert.sender_id && s.account_id === upsertAccountId);
          const incomingId = upsert.posts?.[0]?.id;
          const nextCount = (prev?.count ?? 0) + (incomingId && prev?.posts?.some((p) => p.id === incomingId) ? 0 : 1);
          const posts = [...(prev?.posts ?? [])];
          if (upsert.posts?.[0]) {
            const p = upsert.posts[0];
            if (p.id && !posts.some((x) => x.id === p.id)) posts.push(p);
          } else if (upsert.media_type || upsert.preview) {
            posts.push({
              id: '',
              media_type: upsert.media_type || 'text',
              text: upsert.preview || '',
              timestamp: upsert.timestamp,
            });
          }
          statusFeed = [
            {
              ...upsert,
              account_id: upsert.account_id ?? accountId ?? prev?.account_id,
              count: nextCount,
              posts: posts.length ? posts : prev?.posts,
            },
            ...statusFeed.filter((s) => s.sender_id !== upsert.sender_id || s.account_id !== upsertAccountId),
          ];
        }
      }
      if (event.kind === 'status.media') {
        const accountId = event.payload.account_id;
        const messageId = event.payload.message_id;
        const path = event.payload.media_path;
        const err = event.payload.error;
        if (typeof accountId !== 'string' || typeof messageId !== 'string') {
          // ignore
        } else {
          const key = statusMediaKey(accountId, messageId);
          if (typeof err === 'string' && err) {
            resolveStatusMediaWaiters(key, null);
          } else if (typeof path === 'string' && path) {
            try {
              const url = await readMessageMedia(path);
              statusMediaCache.set(key, url);
              statusThumbs[key] = url;
              resolveStatusMediaWaiters(key, url);
            } catch {
              resolveStatusMediaWaiters(key, null);
            }
          } else {
            resolveStatusMediaWaiters(key, null);
          }
        }
      }
      if (event.kind === 'account.avatar') {
        const accountId = event.payload.account_id;
        const data = event.payload.avatar_data;
        if (typeof accountId === 'string' && typeof data === 'string' && data.length > 8) {
          accounts = accounts.map((a) =>
            a.id === accountId ? { ...a, metadata: { ...a.metadata, avatar_data: data } } : a
          );
        }
      }
      if (event.kind === 'message.received' || event.kind === 'message.sent') {
        scheduleUnreadCounts();
        applyPushedMessage(event.payload);
      }
      if (event.kind === 'message.updated') {
        const msg = event.payload.message as Message | undefined;
        const convId = event.payload.conversation_id ?? msg?.conversation_id;
        if (msg && convId === selectedConversationId) upsertMessage(msg);
      }
      if (event.kind === 'message.reactions') {
        applyReactionUpdates(event.payload);
      }
      if (event.kind === 'message.removed') {
        const messageId = event.payload.message_id;
        if (
          typeof messageId === 'string' &&
          event.payload.conversation_id === selectedConversationId &&
          messages.some((m) => m.id === messageId)
        ) {
          messages = messages.filter((m) => m.id !== messageId);
        }
      }
      if (event.kind === 'conversation.updated') {
        scheduleUnreadCounts();
        const conv = event.payload.conversation as Conversation | undefined;
        if (conv) patchConversation(conv);
        if (typeof event.payload.unread_total === 'number') unreadTotal = event.payload.unread_total;
      }
      if (event.kind === 'conversation.removed') {
        scheduleUnreadCounts();
        const convId = event.payload.conversation_id;
        const intoId = event.payload.into_conversation_id;
        if (typeof convId === 'string') {
          conversations = conversations.filter((c) => c.id !== convId);
          noteLocalListChange();
          if (listFocusId === convId) listFocusId = null;
          if (selectedConversationId === convId) {
            if (typeof intoId === 'string' && intoId) {
              await selectConversation(intoId);
            } else {
              selectedConversationId = null;
              messages = [];
              mobileView = 'list';
            }
          }
        }
      }
      if (event.kind === 'contact.profile') {
        await refresh();
      }
      if (event.kind === 'call.ringing' || event.kind === 'call.connected') {
        activeCall = event.payload as unknown as CallState;
      }
      if (event.kind === 'call.ended' || event.kind === 'call.error') {
        activeCall = null;
      }
      if (event.kind === 'chat.synced') {
        const remote = event.payload.remote_id;
        const conv = conversations.find((c) => c.id === selectedConversationId);
        if (
          selectedConversationId &&
          conv &&
          (typeof remote !== 'string' || !remote || conv.remote_id === remote)
        ) {
          await refreshWindow(selectedConversationId);
        }
      }
      if (
        event.kind === 'history.sync.completed' ||
        event.kind === 'inbox.catchup' ||
        event.kind === 'contacts.synced'
      ) {
        scheduleInboxReload();
      }
      if (event.kind === 'reminder.fired' || event.kind === 'scheduled_message.sent') {
        await refresh();
      }
      if (event.kind === 'account.connected' || event.kind === 'account.status') {
        accounts = await listAccounts();
        if (event.kind === 'account.connected' || event.payload.status === 'connected') {
          connecting = false;
          showSetup = false;
          qrData = null;
          await refresh();
        }
      }
    });

    return () => {
      phoneQuery.removeEventListener('change', syncPhone);
      window.removeEventListener('resize', measureExtras);
      window.removeEventListener('keydown', handleGlobalKeydown);
      clearInterval(updateTimer);
      window.clearTimeout(inboxReloadTimer);
      window.clearTimeout(unreadCountsTimer);
      window.clearTimeout(profileTimer);
      window.clearTimeout(draftSaveTimer);
      unsub.then((fn) => fn());
    };
  });
</script>

{#snippet updatesHome()}
  <UpdatesHome
    statuses={visibleStatuses}
    channels={channelConversations}
    {conversations}
    {accounts}
    showNetwork={!selectedAccountId}
    datetimeFormat={appConfig.appearance.datetime_format || '12h_full'}
    {channelColor}
    {statusThumb}
    onstatus={openStatusViewer}
    onselect={selectConversation}
    oncontext={convMenu}
    showHint={!isPhone}
  />
{/snippet}

<div class="app" class:hidden={setupOnly}>
  {#if sendError}
    <div class="send-error" role="alert">
      <span>Couldn’t send: {sendError}</span>
      <button type="button" onclick={() => (sendError = null)} aria-label="Dismiss">×</button>
    </div>
  {/if}
  {#if updateNoticeOpen && availableUpdate}
    <UpdateNotice
      update={availableUpdate}
      onview={() => {
        updateNoticeOpen = false;
        selectMobileTab('settings');
      }}
      onclose={() => (updateNoticeOpen = false)}
    />
  {/if}
  <div class="sidebar-wrap" class:hidden-mobile={mobileView === 'thread'}>
    <Sidebar
      accounts={accountsForRail}
      selected={selectedAccountId}
      onselect={selectAccount}
      onadd={() => (showSetup = true)}
      {unreadTotal}
      {unreadCounts}
      {mobileTab}
      settingsActive={settingsOpen}
      updateAvailable={!!availableUpdate}
      ontabchange={selectMobileTab}
      onsettings={() => {
        updateNoticeOpen = false;
        selectMobileTab('settings');
      }}
      onaccountmenu={accountMenu}
    />
  </div>

  <main class="main" class:mobile-thread={mobileView === 'thread'}>
    <div class="list-pane" class:hidden-mobile={mobileView === 'thread' || settingsOpen} class:hidden={settingsOpen}>
      <div class="org-filters" class:has-filters={showWorkspaceFilter || showPriorityFilter}>
        {#if showWorkspaceFilter}
          <select
            value={selectedWorkspace ?? ''}
            onchange={(e) => {
              selectedWorkspace = e.currentTarget.value || null;
              refresh();
            }}
          >
            <option value="">All workspaces</option>
            {#each workspaces as ws (ws.id)}
              <option value={ws.id}>{ws.name}</option>
            {/each}
          </select>
        {/if}
        {#if showPriorityFilter}
          <select
            value={selectedPriority ?? ''}
            onchange={(e) => {
              selectedPriority = e.currentTarget.value || null;
              refresh();
            }}
          >
            <option value="">All priorities</option>
            {#each priorityGroups as g (g.id)}
              <option value={g.id}>{g.name}</option>
            {/each}
          </select>
        {/if}
      </div>
      <ConversationList
        {conversations}
        {accounts}
        selectedAccountId={selectedAccountId}
        selectedId={selectedConversationId}
        {searchQuery}
        searchScope={searchScope}
        {searchMessageHits}
        hasMore={listHasMore}
        loadingMore={listLoadingMore}
        onloadmore={loadMoreConversations}
        onsearch={handleSearch}
        onsearchscope={handleSearchScope}
        onsearchhit={jumpToSearchHit}
        onselect={selectConversation}
        onstartchat={openStartedChat}
        onaccountselect={selectAccount}
        showArchived={showArchived}
        onarchivedtoggle={toggleArchived}
        onrefresh={refresh}
        oncompose={() => {}}
        oncontext={convMenu}
        {channelColor}
        datetimeFormat={appConfig.appearance.datetime_format || '12h_full'}
        {focusSearchNonce}
        highlightId={listFocusId}
        updates={isPhone && hasUpdates ? updatesHome : undefined}
      />
    </div>

    <div class="settings-pane" class:visible={settingsOpen} class:hidden-mobile={!settingsOpen || mobileView === 'thread'}>
      <SettingsPanel
        {accounts}
        {connectors}
        {conversations}
        {forwardRules}
        {scheduledMessages}
        {workspaces}
        {priorityGroups}
        config={appConfig}
        focus={settingsFocus}
        onadd={() => (showSetup = true)}
        onconfig={persistConfig}
        onaccount={handleAccount}
        onaccountpatch={async (id, patch) => {
          await updateAccount(id, patch);
          await refresh();
        }}
        onworkspace={async (name: string) => {
          await createWorkspace(name);
          workspaces = await listWorkspaces();
        }}
        ondeleteworkspace={async (id: string) => {
          await deleteWorkspace(id);
          workspaces = await listWorkspaces();
          await refresh();
        }}
        oncreatepriority={async (name: string) => {
          await createPriorityGroup(name);
          priorityGroups = await listPriorityGroups();
        }}
        ondeletepriority={async (id: string) => {
          await deletePriorityGroup(id);
          priorityGroups = await listPriorityGroups();
          if (selectedPriority === id) {
            selectedPriority = null;
            await refresh();
          }
        }}
        oncreateforwardrule={async (draft: ForwardRuleDraft) => {
          await createForwardRule(draft);
          forwardRules = await listForwardRules();
        }}
        ontoggleforwardrule={async (id: string, enabled: boolean) => {
          await updateForwardRule(id, { enabled });
          forwardRules = await listForwardRules();
        }}
        ondeleteforwardrule={async (id: string) => {
          await deleteForwardRule(id);
          forwardRules = await listForwardRules();
        }}
        ondeletescheduled={async (id: string) => {
          await deleteScheduledMessage(id);
          scheduledMessages = await listScheduledMessages();
        }}
        onexportbackup={async (
          path: string,
          password: string,
          includeMessages: boolean,
          includeMedia: boolean
        ) => {
          if (!path.trim() || !password) throw new Error('Export path and password are required.');
          await exportBackup(path.trim(), password, includeMessages, includeMedia);
        }}
        onrestorebackup={async (path: string, password: string) => {
          if (!path.trim() || !password) throw new Error('Restore path and password are required.');
          await restoreBackup(path.trim(), password);
        }}
        onaccountmenu={accountMenu}
      />
    </div>

    <div class="thread-pane" class:hidden-mobile={mobileView === 'list' || settingsOpen} class:hidden={settingsOpen} class:extras-top={extrasTop}>
      {#if !selectedConversation && !isPhone && hasUpdates}
        {@render updatesHome()}
      {:else}
      <ThreadView
        bind:this={threadView}
        conversation={selectedConversation}
        {messages}
        loading={messagesLoading}
        hasolder={hasOlder}
        hasnewer={hasNewer}
        loadingolder={loadingOlder}
        onloadolder={loadOlder}
        onloadnewer={loadNewer}
        {accounts}
        {draft}
        ondraft={(v) => (draft = v)}
        onsend={handleSend}
        onsendattachment={handleSendAttachment}
        onsendlater={handleSendLater}
        {replyingTo}
        oncancelreply={() => (replyingTo = null)}
        onjumptomessage={jumpToMessage}
        showBack={mobileView === 'thread'}
        onback={() => (mobileView = 'list')}
        onheaderclick={() => (contactView = { of: 'chat' })}
        onstartcall={handleStartCall}
        {connectors}
        onmsgmenu={msgMenu}
        {translations}
        onstartchat={openStartedChat}
        ondismisstranslation={(id) => (translations = Object.fromEntries(Object.entries(translations).filter(([k]) => k !== id)))}
        onselectionaction={runSelectionAction}
        ontextmenu={textMenu}
        ontogglepanel={() => (panelOpen = !panelOpen)}
        {panelOpen}
        channelColor={selectedConversation
          ? channelColor(accounts.find((a) => a.id === selectedConversation.account_id)?.connector_id ?? '')
          : undefined}
      />
      {/if}
      {#if panelOpen && selectedConversation}
        <ChatPanel
          conversation={selectedConversation}
          {scheduledMessages}
          placement={extrasTop ? 'top' : 'side'}
          {noteDraftRef}
          onnotedraftdone={() => (noteDraftRef = null)}
          onjumptomessage={jumpToMessage}
          onupdated={refresh}
        />
      {/if}
    </div>
  </main>
</div>

<ContextMenu
  open={menuOpen}
  x={menuX}
  y={menuY}
  items={menuItems}
  onclose={() => (menuOpen = false)}
  onselect={(id) => menuAction(id)}
/>

<StatusViewer
  open={statusViewerOpen}
  feed={visibleStatuses}
  startSenderId={statusViewerStartId}
  startAccountId={statusViewerAccountId}
  {conversations}
  onclose={() => (statusViewerOpen = false)}
  requestMedia={requestStatusMedia}
  resolveThumb={resolveStatusThumb}
/>

{#if forwardSources.length && selectedConversation}
  <ForwardModal
    sources={forwardSources}
    sourceConversationId={selectedConversation.id}
    {accounts}
    onclose={() => (forwardSources = [])}
    onsent={async () => {
      forwardSources = [];
      threadView?.cancelSelection();
      await refresh();
      if (selectedConversation) await loadMessages(selectedConversation.id);
    }}
  />
{/if}

{#if deleteTargets.length}
  {@const actions = actionsFor(selectedConversation)}
  <DeleteMessagesDialog
    count={deleteTargets.length}
    everyone={deleteTargets.some((m) => offeredActions(actions, m).deleteForEveryone)}
    onchoose={chooseDelete}
    oncancel={() => (deleteTargets = [])}
  />
{/if}

{#if editingMessage}
  <EditMessageDialog original={editingMessage.body} onsave={saveEdit} oncancel={() => (editingMessage = null)} />
{/if}

{#if reactPicker}
  {@const target = reactPicker}
  <div class="react-picker-backdrop" onclick={() => (reactPicker = null)} role="presentation">
    <div
      class="react-picker"
      style="left:{Math.max(8, Math.min(target.x, window.innerWidth - 316))}px;top:{Math.max(8, Math.min(target.y, window.innerHeight - 336))}px"
      role="dialog"
      tabindex="-1"
      aria-label="Choose a reaction"
      onclick={(e) => e.stopPropagation()}
      onkeydown={(e) => e.key === 'Escape' && (reactPicker = null)}
    >
      <EmojiPickerPop
        onpick={(emoji) => {
          const msg = target.msg;
          reactPicker = null;
          void react(msg, emoji);
        }}
      />
    </div>
  </div>
{/if}

<RemindModal
  open={remindOpen}
  initialNote={remindNoteSeed}
  onclose={() => {
    remindOpen = false;
    remindNoteSeed = '';
  }}
  onsubmit={submitReminder}
/>

<ContactDetails
  open={!!contactView}
  conversation={contactView?.of === 'sender' ? contactView.conversation : selectedConversation}
  transient={contactView?.of === 'sender' && contactView.transient}
  {accounts}
  {connectors}
  {workspaces}
  {priorityGroups}
  onclose={() => (contactView = null)}
  onupdated={refresh}
  onstartcall={contactView?.of === 'chat' ? handleStartCall : undefined}
/>

<CallPanel call={activeCall} onclose={() => (activeCall = null)} />

<QuickSwitch
  open={quickSwitchOpen}
  {conversations}
  {accounts}
  {workspaces}
  onclose={() => (quickSwitchOpen = false)}
  onpickConversation={async (id) => {
    quickSwitchOpen = false;
    await selectConversation(id);
  }}
  onpickAccount={async (id) => {
    quickSwitchOpen = false;
    await selectAccount(id);
  }}
  onpickWorkspace={async (id) => {
    quickSwitchOpen = false;
    selectedWorkspace = id;
    await refresh();
  }}
/>

{#if editingAccount}
  <div class="account-edit-backdrop" role="presentation">
    <form class="account-edit" onsubmit={async (e) => { e.preventDefault(); if (!editingAccount || !editAccountName.trim()) return; await updateAccount(editingAccount.id, { name: editAccountName.trim(), accent_color: editAccountColor }); editingAccount = null; await refresh(); }}>
      <h2>Edit account</h2>
      <label>Account name <input bind:value={editAccountName} required /></label>
      <label>Account accent colour <input type="color" bind:value={editAccountColor} /></label>
      <div><button type="button" onclick={() => editingAccount = null}>Cancel</button> <button type="submit">Save</button></div>
    </form>
  </div>
{/if}

<AccountSetup
  open={showSetup || setupOnly}
  standalone={setupOnly}
  {connectors}
  {connecting}
  {qrData}
  {authMethod}
  {authMessage}
  errorMessage={setupError}
  onclose={() => {
    if (!setupOnly) showSetup = false;
  }}
  oncreate={handleCreateAccount}
  onsubmit={handleSubmitAuth}
/>

<style>
  .account-edit-backdrop { position: fixed; inset: 0; z-index: 100; background: #0008; display: grid; place-items: center; }
  .account-edit { background: var(--bg-panel); color: var(--text); padding: 24px; border-radius: 12px; display: grid; gap: 14px; min-width: 280px; }
  .account-edit label { display: grid; gap: 6px; }
  .account-edit input:not([type="color"]) { padding: 8px; }
  .app {
    display: flex;
    flex: 1 1 auto;
    flex-direction: row;
    flex-wrap: nowrap;
    align-items: stretch;
    width: 100%;
    height: 100%;
    min-height: 0;
    overflow: hidden;
    background: var(--bg-main);
    position: relative;
  }
  .send-error {
    position: absolute;
    top: 10px;
    left: 50%;
    transform: translateX(-50%);
    z-index: 50;
    display: flex;
    align-items: center;
    gap: 10px;
    max-width: min(520px, calc(100vw - 24px));
    padding: 8px 10px 8px 12px;
    border-radius: var(--radius-md);
    background: color-mix(in oklch, var(--destructive, #ef4444) 16%, var(--bg-panel));
    border: 1px solid color-mix(in oklch, var(--destructive, #ef4444) 45%, var(--border));
    color: var(--text);
    box-shadow: var(--shadow-md);
    font-size: 13px;
  }
  .send-error button {
    border: none;
    background: transparent;
    color: var(--text-muted);
    font-size: 18px;
    line-height: 1;
    cursor: pointer;
    padding: 0 4px;
  }
  .app.hidden {
    visibility: hidden;
    pointer-events: none;
    position: absolute;
    width: 0;
    height: 0;
    overflow: hidden;
  }
  .main {
    flex: 1 1 0;
    display: flex;
    flex-direction: row;
    align-items: stretch;
    min-width: 0;
    min-height: 0;
    overflow: hidden;
  }
  .list-pane {
    display: flex;
    flex-direction: column;
    flex: 0 0 var(--list-width);
    width: var(--list-width);
    min-width: 0;
    min-height: 0;
    overflow: hidden;
  }
  .list-pane :global(.conv-list) {
    flex: 1 1 auto;
    width: 100%;
    min-width: 0;
    max-width: none;
    min-height: 0;
    height: auto;
  }
  .thread-pane, .settings-pane {
    display: flex;
    flex-direction: column;
    flex: 1 1 0;
    min-width: 0;
    min-height: 0;
    overflow: hidden;
  }
  .thread-pane { flex-direction: row; }
  .thread-pane :global(.thread) {
    flex: 1;
    min-width: 0;
    min-height: 0;
  }
  .sidebar-wrap :global(.sidebar) {
    width: 100%;
    min-width: 0;
    height: 100%;
    max-height: 100%;
  }
  .thread-pane.extras-top { flex-direction: column; }
  .thread-pane.extras-top :global(.thread) {
    flex: 1;
    min-height: 0;
  }
  .thread-pane.extras-top :global(.panel) {
    order: -1;
  }
  .list-pane.hidden, .thread-pane.hidden { display: none; }
  .settings-pane {
    display: none;
    flex: 1;
    background: var(--bg-panel);
    border-right: 1px solid var(--border-subtle);
  }
  .settings-pane.visible { display: flex; }
  .org-filters {
    display: none;
    gap: 8px;
    padding: 10px 12px 0;
    background: var(--bg-panel);
  }
  .org-filters.has-filters {
    display: flex;
  }
  .org-filters select {
    flex: 1;
    padding: 6px 28px 6px 8px;
    font: inherit;
  }
  .sidebar-wrap {
    display: flex;
    flex: 0 0 var(--rail-width);
    width: var(--rail-width);
    min-width: 0;
    min-height: 0;
    overflow: hidden;
  }
  .react-picker-backdrop {
    position: fixed;
    inset: 0;
    z-index: 82;
  }
  .react-picker {
    position: fixed;
    background: var(--bg-panel);
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    box-shadow: var(--shadow-md);
    overflow: hidden;
  }
  @media (min-width: 640px) {
    .app {
      display: grid;
      grid-template-columns: var(--rail-width) minmax(0, 1fr);
      grid-template-rows: minmax(0, 1fr);
      height: 100%;
      min-height: 0;
    }
    .sidebar-wrap {
      grid-column: 1;
      grid-row: 1;
      order: 0;
      flex: none;
      width: var(--rail-width);
      height: 100%;
      min-height: 0;
      max-height: 100%;
    }
    .main {
      grid-column: 2;
      grid-row: 1;
      display: grid;
      grid-template-columns: var(--list-width) minmax(0, 1fr);
      grid-template-rows: minmax(0, 1fr);
      flex: none;
      width: auto;
      height: 100%;
      min-width: 0;
      min-height: 0;
    }
    .list-pane {
      grid-column: 1;
      grid-row: 1;
      flex: none;
      width: auto;
      height: 100%;
      min-height: 0;
    }
    .thread-pane {
      grid-column: 2;
      grid-row: 1;
      flex: none;
      width: auto;
      height: 100%;
      min-height: 0;
      flex-direction: row;
    }
    .settings-pane.visible {
      grid-column: 1 / -1;
      grid-row: 1;
      height: 100%;
      min-height: 0;
    }
  }

  @media (max-width: 639px) {
    .app { flex-direction: column; }
    .sidebar-wrap {
      order: 2;
      flex: 0 0 auto;
      width: 100%;
    }
    .main {
      order: 1;
      flex: 1 1 0;
      min-height: 0;
      width: 100%;
    }
    .list-pane, .thread-pane, .settings-pane {
      flex: 1 1 0;
      width: 100%;
      min-height: 0;
    }
    .thread-pane { flex-direction: column; }
    .settings-pane { max-width: none; border-right: none; }
    .hidden-mobile { display: none !important; }
    .sidebar-wrap.hidden-mobile { display: none; }
  }
</style>
