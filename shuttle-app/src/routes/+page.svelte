<script lang="ts">
  import { page } from '$app/state';
  import { onMount } from 'svelte';
  import {
    addTodo,
    connectAccount,
    createAccount,
    createPriorityGroup,
    createReminder,
    createWorkspace,
    createForwardRule,
    deleteAccount,
    deleteForwardRule,
    deletePriorityGroup,
    deleteScheduledMessage,
    deleteWorkspace,
    exportBackup,
    fetchConversationAvatar,
    fetchContactProfile,
    forwardMessage,
    getAppConfig,
    getMessages,
    listAccounts,
    listConnectors,
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
    restoreBackup,
    saveAppConfig,
    scheduleMessage,
    searchMessages,
    sendMessage,
    sendAttachment,
    starMessage,
    startCall,
    submitAuth,
    syncConversation,
    totalUnread,
    updateAccount,
    updateConversation,
    updateForwardRule,
  } from '$lib/api';
  import AccountSetup from '$lib/components/AccountSetup.svelte';
  import CallPanel from '$lib/components/CallPanel.svelte';
  import ChatPanel from '$lib/components/ChatPanel.svelte';
  import ContactDetails from '$lib/components/ContactDetails.svelte';
  import ContextMenu from '$lib/components/ContextMenu.svelte';
  import ConversationList from '$lib/components/ConversationList.svelte';
  import RemindModal from '$lib/components/RemindModal.svelte';
  import SettingsPanel from '$lib/components/SettingsPanel.svelte';
  import Sidebar from '$lib/components/Sidebar.svelte';
  import ThreadView from '$lib/components/ThreadView.svelte';
  import { normalizeRichText } from '$lib/richText';
  import { applyAppConfig } from '$lib/theme';
  import { initTelemetry, markAppReady } from '$lib/telemetry';
  import { selectionContext, urlsIn } from '$lib/structuredText';
  import {
    CONNECTOR_COLORS,
    type Account,
    type AppConfig,
    type MediaRetentionConfig,
    type AttachmentPayload,
    type CallState,
    type ConnectorInfo,
    type Conversation,
    type ForwardRule,
    type ForwardRuleDraft,
    type MenuItem,
    type Message,
    type PriorityGroup,
    type ScheduledMessage,
    type ScheduleMessageDraft,
    type SearchMessageHit,
    type SearchScope,
    type Workspace,
  } from '$lib/types';

  let accounts = $state<Account[]>([]);
  let connectors = $state<ConnectorInfo[]>([]);
  let conversations = $state<Conversation[]>([]);
  let messages = $state<Message[]>([]);
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
    channel_styles: {},
    media_retention: {},
  });
  let selectedAccountId = $state<string | null>(null);
  let selectedConversationId = $state<string | null>(null);
  let showArchived = $state(false);
  let selectedWorkspace = $state<string | null>(null);
  let selectedPriority = $state<string | null>(null);
  let searchQuery = $state('');
  let searchScope = $state<SearchScope>('global');
  let searchMessageHits = $state<SearchMessageHit[]>([]);
  let activeCall = $state<CallState | null>(null);
  let draft = $state('');
  let unreadTotal = $state(0);
  let showSetup = $state(false);
  let connecting = $state(false);
  let setupError = $state<string | null>(null);
  let qrData = $state<string | null>(null);
  let authMethod = $state<string | null>(null);
  let authMessage = $state<string | null>(null);
  let pendingAccountId = $state<string | null>(null);
  let mobileView = $state<'list' | 'thread'>('list');
  let mobileTab = $state<'inbox' | 'settings'>('inbox');
  let panelOpen = $state(false);
  let contactOpen = $state(false);
  let narrowWidth = $state(false);
  let menuOpen = $state(false);
  let menuX = $state(0);
  let menuY = $state(0);
  let menuItems = $state<MenuItem[]>([]);
  let menuAction = $state<(id: string) => void>(() => {});
  let forwardOpen = $state(false);
  let forwardText = $state('');
  let forwardSendAt = $state('');
  let remindOpen = $state(false);
  let remindNoteSeed = $state('');

  const setupOnly = $derived(page.url.searchParams.get('setup') === '1');
  const settingsOpen = $derived(mobileTab === 'settings');
  const showWorkspaceFilter = $derived(workspaces.some((w) => !w.builtin));
  const showPriorityFilter = $derived(priorityGroups.length > 0);
  const selectedConversation = $derived(
    conversations.find((c) => c.id === selectedConversationId) ?? null
  );
  const extrasTop = $derived(narrowWidth && panelOpen);

  function channelColor(connectorId: string): string {
    return appConfig.channel_styles[connectorId]?.tag ?? CONNECTOR_COLORS[connectorId] ?? '#888';
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

  function convTime(conv: Conversation): number {
    if (!conv.last_message_at) return 0;
    const t = new Date(conv.last_message_at).getTime();
    return Number.isFinite(t) ? t : 0;
  }

  function listRank(conv: Conversation): number {
    const rank = conv.metadata?.list_rank;
    return typeof rank === 'number' ? rank : 999999;
  }

  function sortConversations(list: Conversation[]): Conversation[] {
    return [...list].sort((a, b) => {
      if (a.pinned !== b.pinned) return a.pinned ? -1 : 1;
      const byTime = convTime(b) - convTime(a);
      if (byTime !== 0) return byTime;
      return listRank(a) - listRank(b);
    });
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
        };
      }
    }
    if (conv) patchConversation(conv);
    if (typeof payload.unread_total === 'number') unreadTotal = payload.unread_total;
    if (msg && selectedConversationId === msg.conversation_id) {
      const exists = messages.some(
        (m) => m.id === msg.id || (msg.remote_id && m.remote_id === msg.remote_id)
      );
      if (!exists) messages = [...messages, msg];
    }
  }

  async function refresh() {
    accounts = await listAccounts();
    unreadTotal = await totalUnread();
    workspaces = await listWorkspaces();
    priorityGroups = await listPriorityGroups();
    forwardRules = await listForwardRules();
    scheduledMessages = await listScheduledMessages();
    if (searchQuery) {
      await handleSearch(searchQuery);
    } else {
      conversations = sortConversations(
        await listConversations(
          selectedAccountId ?? undefined,
          selectedWorkspace ?? undefined,
          selectedPriority ?? undefined,
          showArchived
        )
      );
      searchMessageHits = [];
    }
    queueAvatarFetches(conversations);
  }

  const avatarQueued = new Set<string>();
  function queueAvatarFetches(list: Conversation[]) {
    for (const conv of list.slice(0, 48)) {
      if (typeof conv.metadata?.avatar_data === 'string') continue;
      if (avatarQueued.has(conv.id)) continue;
      avatarQueued.add(conv.id);
      fetchConversationAvatar(conv.account_id, conv.id).catch(() => {
        avatarQueued.delete(conv.id);
      });
    }
  }

  async function loadMessages(convId: string) {
    messages = await getMessages(convId);
    const conv = conversations.find((c) => c.id === convId);
    if (conv) {
      syncConversation(conv.account_id, conv.id).catch(() => {});
    }
    await markRead(convId);
    unreadTotal = await totalUnread();
    conversations = await listConversations(
      selectedAccountId ?? undefined,
      selectedWorkspace ?? undefined,
      selectedPriority ?? undefined,
      showArchived
    );
  }

  async function selectConversation(id: string) {
    selectedConversationId = id;
    mobileView = 'thread';
    contactOpen = false;
    await loadMessages(id);
  }

  async function selectAccount(id: string | null) {
    selectedAccountId = id;
    selectedConversationId = null;
    showArchived = false;
    messages = [];
    mobileView = 'list';
    mobileTab = 'inbox';
    await refresh();
  }

  function selectMobileTab(tab: 'inbox' | 'settings') {
    mobileTab = tab;
    mobileView = 'list';
    selectedConversationId = null;
    messages = [];
  }

  async function handleSearch(q: string) {
    searchQuery = q;
    if (!q) {
      searchMessageHits = [];
      conversations = sortConversations(
        await listConversations(
          selectedAccountId ?? undefined,
          selectedWorkspace ?? undefined,
          selectedPriority ?? undefined,
          showArchived
        )
      );
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

  async function jumpToSearchHit(hit: SearchMessageHit) {
    selectedConversationId = hit.message.conversation_id;
    mobileView = 'thread';
    messages = await getMessages(hit.message.conversation_id);
    searchQuery = '';
    searchMessageHits = [];
    await refresh();
  }

  async function handleStartCall(mode: 'audio' | 'video') {
    if (!selectedConversation) return;
    try {
      activeCall = await startCall(selectedConversation.account_id, selectedConversation.id, mode);
      contactOpen = false;
    } catch (e) {
      console.error(e);
    }
  }

  async function toggleArchived() {
    showArchived = !showArchived;
    selectedConversationId = null;
    messages = [];
    mobileView = 'list';
    await refresh();
  }

  async function handleSend() {
    if (!draft.trim() || !selectedConversation) return;
    const text = normalizeRichText(draft.trim(), connectorForAccount(selectedConversation.account_id));
    draft = '';
    await sendMessage(selectedConversation.account_id, selectedConversation.id, text);
    await loadMessages(selectedConversation.id);
  }

  async function handleSendAttachment(attachment: AttachmentPayload) {
    if (!selectedConversation) return;
    draft = '';
    await sendAttachment(selectedConversation.account_id, selectedConversation.id, attachment);
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

  async function handleCreateAccount(connectorId: string, name: string, credentials: Record<string, string>) {
    connecting = true;
    setupError = null;
    qrData = null;
    authMethod = null;
    authMessage = null;
    const account = await createAccount(connectorId, name);
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

  function convMenu(conv: Conversation, x: number, y: number) {
    openMenu(
      x,
      y,
      [
        { id: 'open', label: 'Open' },
        { id: 'pin', label: conv.pinned ? 'Unpin' : 'Pin' },
        { id: 'mute', label: conv.muted ? 'Unmute' : 'Mute' },
        { id: 'archive', label: conv.archived ? 'Unarchive' : 'Archive' },
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
      ],
      async (id) => {
        if (id === 'open') await selectConversation(conv.id);
        else if (id === 'pin') await updateConversation(conv.id, { pinned: !conv.pinned });
        else if (id === 'mute') await updateConversation(conv.id, { muted: !conv.muted });
        else if (id === 'archive') await updateConversation(conv.id, { archived: !conv.archived });
        else if (id === 'unread') await markUnread(conv.id);
        else if (id.startsWith('ws:')) await updateConversation(conv.id, { workspace_id: id.slice(3) });
        else if (id.startsWith('pg:')) await updateConversation(conv.id, { priority_group: id.slice(3) });
        else if (id === 'clear-pg') await updateConversation(conv.id, { clear_priority: true });
        else if (id === 'notify-on') await updateConversation(conv.id, { notify_enabled: true });
        else if (id === 'notify-off') await updateConversation(conv.id, { notify_enabled: false });
        else if (id === 'notify-inherit') await updateConversation(conv.id, { clear_notify: true });
        else if (id === 'receipts-on') await updateConversation(conv.id, { send_receipts: true });
        else if (id === 'receipts-off') await updateConversation(conv.id, { send_receipts: false });
        else if (id === 'receipts-inherit') await updateConversation(conv.id, { clear_receipts: true });
        await refresh();
      }
    );
  }

  function accountMenu(account: Account, x: number, y: number) {
    openMenu(
      x,
      y,
      [
        { id: 'mute', label: account.muted ? 'Unmute account' : 'Mute account' },
        { id: 'disable', label: account.disabled ? 'Enable account' : 'Disable account' },
        { id: 'receipts', label: account.send_receipts ? 'Disable read receipts' : 'Enable read receipts' },
        { id: 'sep', label: '', separator: true },
        { id: 'remove', label: 'Remove account', danger: true },
      ],
      async (id) => {
        if (id === 'disable') {
          await handleAccount(account.id, account.disabled ? 'enable' : 'disable');
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

  function msgMenu(msg: Message, x: number, y: number) {
    const urls = urlsIn(msg.body);
    openMenu(
      x,
      y,
      [
        { id: 'copy', label: 'Copy' },
        { id: 'reply', label: 'Reply' },
        { id: 'forward', label: 'Forward…' },
        { id: msg.starred ? 'unstar' : 'star', label: msg.starred ? 'Unstar' : 'Star' },
        { id: msg.pinned ? 'unpin' : 'pin', label: msg.pinned ? 'Unpin' : 'Pin' },
        { id: 'todo', label: 'Add to todo list' },
        { id: 'remind', label: 'Remind me about this chat' },
        ...urls.map((u, i) => ({ id: `url:${i}`, label: `Open ${u.slice(0, 40)}` })),
      ],
      async (id) => {
        if (id === 'copy') await navigator.clipboard.writeText(msg.body);
        else if (id === 'reply') draft = `> ${msg.body.replace(/\n/g, '\n> ')}\n\n${draft}`;
        else if (id === 'forward') {
          forwardText = `Forwarded:\n${msg.body}`;
          forwardSendAt = '';
          forwardOpen = true;
        } else if (id === 'star' || id === 'unstar') {
          const updated = await starMessage(msg.id, id === 'star');
          messages = messages.map((m) => (m.id === msg.id ? updated : m));
        } else if (id === 'pin' || id === 'unpin') {
          const updated = await pinMessage(msg.id, id === 'pin');
          messages = messages.map((m) => (m.id === msg.id ? updated : m));
        } else if (id === 'todo' && selectedConversation) {
          const body = msg.body.trim() || 'Follow up on message';
          await addTodo(selectedConversation.id, selectedConversation.account_id, body);
          panelOpen = true;
        } else if (id === 'remind' && selectedConversation) {
          remindNoteSeed = msg.body.trim();
          remindOpen = true;
        } else if (id.startsWith('url:')) {
          const u = urls[Number(id.slice(4))];
          if (u) await openExternal(u);
        }
      }
    );
  }

  async function submitReminder(fireAt: string, note: string) {
    if (!selectedConversation) return;
    await createReminder(
      selectedConversation.id,
      selectedConversation.account_id,
      fireAt,
      'nudge',
      note || undefined
    );
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
      ...urls.map((u, i) => ({ id: `url:${i}`, label: `Open URL` })),
    ];
    openMenu(x, y, items, async (id) => {
      if (id === 'copy') await navigator.clipboard.writeText(value);
      else if (id === 'copy-inner' && inner) await navigator.clipboard.writeText(inner);
      else if (id.startsWith('url:') && urls[0]) await openExternal(urls[0]);
    });
  }

  onMount(() => {
    const startedAt = performance.now();
    void initTelemetry();
    const measureExtras = () => {
      const screenW = window.screen?.width || window.innerWidth;
      narrowWidth = window.innerWidth < Math.min(900, screenW * 0.6);
    };
    measureExtras();
    window.addEventListener('resize', measureExtras);
    listConnectors().then((c) => (connectors = c));
    getAppConfig().then((cfg) => {
      appConfig = cfg;
      applyAppConfig(cfg);
    });

    if (setupOnly) {
      showSetup = true;
    } else {
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
      if (event.kind === 'avatar.updated') {
        const convId = event.payload.conversation_id;
        const data = event.payload.avatar_data;
        if (typeof convId === 'string') {
          conversations = conversations.map((c) =>
            c.id === convId
              ? { ...c, metadata: { ...c.metadata, avatar_data: data } }
              : c
          );
        }
      }
      if (event.kind === 'message.received' || event.kind === 'message.sent') {
        applyPushedMessage(event.payload);
      }
      if (event.kind === 'conversation.updated') {
        const conv = event.payload.conversation as Conversation | undefined;
        if (conv) patchConversation(conv);
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
      if (
        event.kind === 'history.sync.completed' ||
        event.kind === 'inbox.catchup' ||
        event.kind === 'reminder.fired' ||
        event.kind === 'scheduled_message.sent'
      ) {
        await refresh();
        if (selectedConversationId) messages = await getMessages(selectedConversationId);
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
      window.removeEventListener('resize', measureExtras);
      unsub.then((fn) => fn());
    };
  });
</script>

<div class="app" class:hidden={setupOnly}>
  <div class="sidebar-wrap" class:hidden-mobile={mobileView === 'thread'}>
    <Sidebar
      {accounts}
      selected={selectedAccountId}
      onselect={selectAccount}
      onadd={() => (showSetup = true)}
      {unreadTotal}
      {mobileTab}
      settingsActive={settingsOpen}
      ontabchange={selectMobileTab}
      onsettings={() => selectMobileTab('settings')}
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
        onsearch={handleSearch}
        onsearchscope={handleSearchScope}
        onsearchhit={jumpToSearchHit}
        onselect={selectConversation}
        onaccountselect={selectAccount}
        showArchived={showArchived}
        onarchivedtoggle={toggleArchived}
        onrefresh={refresh}
        oncompose={() => {}}
        oncontext={convMenu}
        {channelColor}
        datetimeFormat={appConfig.appearance.datetime_format || '12h_full'}
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
        onadd={() => (showSetup = true)}
        onconfig={persistConfig}
        onaccount={handleAccount}
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
        onexportbackup={async (path: string, password: string, includeMessages: boolean) => {
          if (!path.trim() || !password) return;
          await exportBackup(path.trim(), password, includeMessages);
        }}
        onrestorebackup={async (path: string, password: string) => {
          if (!path.trim() || !password) return;
          await restoreBackup(path.trim(), password);
        }}
        onaccountmenu={accountMenu}
      />
    </div>

    <div class="thread-pane" class:hidden-mobile={mobileView === 'list' || settingsOpen} class:hidden={settingsOpen} class:extras-top={extrasTop}>
      <ThreadView
        conversation={selectedConversation}
        {messages}
        {accounts}
        {draft}
        ondraft={(v) => (draft = v)}
        onsend={handleSend}
        onsendattachment={handleSendAttachment}
        onsendlater={handleSendLater}
        showBack={mobileView === 'thread'}
        onback={() => (mobileView = 'list')}
        onheaderclick={() => (contactOpen = true)}
        onstartcall={handleStartCall}
        {connectors}
        onmsgmenu={msgMenu}
        ontextmenu={textMenu}
        ontogglepanel={() => (panelOpen = !panelOpen)}
        {panelOpen}
        channelColor={selectedConversation
          ? channelColor(accounts.find((a) => a.id === selectedConversation.account_id)?.connector_id ?? '')
          : undefined}
      />
      {#if panelOpen && selectedConversation}
        <ChatPanel
          conversation={selectedConversation}
          {scheduledMessages}
          placement={extrasTop ? 'top' : 'side'}
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

{#if forwardOpen}
  <div class="modal-backdrop" onclick={() => (forwardOpen = false)} role="presentation">
    <div
      class="modal"
      role="dialog"
      tabindex="-1"
      aria-modal="true"
      aria-labelledby="forward-title"
      onclick={(e) => e.stopPropagation()}
      onkeydown={(e) => e.stopPropagation()}
    >
      <h2 id="forward-title">Forward to</h2>
      <label class="modal-field">
        Send at
        <input type="datetime-local" bind:value={forwardSendAt} />
        <small>Leave empty to send now.</small>
      </label>
      <ul>
        {#each conversations as conv (conv.id)}
          <li>
            <button
              type="button"
              onclick={async () => {
                if (forwardSendAt) {
                  await scheduleMessage({
                    dest_account_id: conv.account_id,
                    dest_conversation_id: conv.id,
                    body: normalizeRichText(forwardText, connectorForAccount(conv.account_id)),
                    send_at: new Date(forwardSendAt).toISOString(),
                  });
                } else {
                  await forwardMessage(
                    conv.account_id,
                    conv.id,
                    normalizeRichText(forwardText, connectorForAccount(conv.account_id))
                  );
                }
                forwardOpen = false;
                forwardSendAt = '';
                await refresh();
              }}>{conv.title}</button
            >
          </li>
        {/each}
      </ul>
      <button type="button" class="cancel" onclick={() => (forwardOpen = false)}>Cancel</button>
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
  open={contactOpen}
  conversation={selectedConversation}
  {accounts}
  {connectors}
  {workspaces}
  {priorityGroups}
  globalMediaRetention={appConfig.media_retention}
  onclose={() => (contactOpen = false)}
  onupdated={refresh}
  onstartcall={handleStartCall}
  onsaveglobalmediaretention={(cfg: MediaRetentionConfig) => persistConfig({ ...appConfig, media_retention: cfg })}
/>

<CallPanel call={activeCall} onclose={() => (activeCall = null)} />

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
  .app {
    display: flex;
    flex-direction: row;
    flex-wrap: nowrap;
    height: 100vh;
    height: 100dvh;
    width: 100vw;
    overflow: hidden;
    background: var(--bg-main);
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
    min-width: 0;
    min-height: 0;
    overflow: hidden;
  }
  .list-pane {
    display: flex;
    flex-direction: column;
    flex: 0 0 var(--list-width);
    width: var(--list-width);
    min-height: 0;
    overflow: hidden;
  }
  .list-pane :global(.conv-list) {
    flex: 1;
    width: 100%;
    min-width: 0;
    max-width: none;
    min-height: 0;
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
    min-height: 0;
    align-self: stretch;
    overflow: hidden;
  }
  .modal-backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.45);
    z-index: 70;
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .modal {
    background: var(--bg-panel);
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    padding: 16px;
    width: min(420px, 92vw);
    max-height: 70vh;
    overflow: auto;
  }
  .modal ul { list-style: none; display: flex; flex-direction: column; gap: 6px; margin: 12px 0; }
  .modal-field {
    display: flex;
    flex-direction: column;
    gap: 6px;
    font-size: 13px;
    color: var(--text);
  }
  .modal-field input {
    width: 100%;
    background: var(--bg-input);
    color: var(--text);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    padding: 8px;
    font: inherit;
  }
  .modal-field small {
    color: var(--text-muted);
  }
  .modal button {
    width: 100%;
    text-align: left;
    border: 1px solid var(--border);
    background: var(--bg-input);
    color: var(--text);
    padding: 10px;
    border-radius: var(--radius-sm);
    cursor: pointer;
  }
  .modal .cancel { text-align: center; margin-top: 8px; }
  @media (max-width: 768px) {
    .app { flex-direction: column; }
    .sidebar-wrap {
      order: 2;
      flex: 0 0 auto;
      width: 100%;
      height: auto;
    }
    .main {
      order: 1;
      flex: 1 1 0;
      min-height: 0;
      width: 100%;
      height: auto;
    }
    .list-pane, .thread-pane, .settings-pane {
      flex: 1 1 0;
      width: 100%;
      height: auto;
      min-height: 0;
    }
    .thread-pane { flex-direction: column; }
    .settings-pane { max-width: none; border-right: none; }
    .hidden-mobile { display: none !important; }
    .sidebar-wrap.hidden-mobile { display: none; }
  }
</style>
