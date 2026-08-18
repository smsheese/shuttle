<script lang="ts">
  import { page } from '$app/state';
  import { onMount } from 'svelte';
  import {
    connectAccount,
    createAccount,
    createWorkspace,
    createForwardRule,
    deleteAccount,
    deleteForwardRule,
    deleteScheduledMessage,
    deleteWorkspace,
    exportBackup,
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
    restoreBackup,
    saveAppConfig,
    scheduleMessage,
    searchConversations,
    sendMessage,
    submitAuth,
    totalUnread,
    updateAccount,
    updateConversation,
    updateForwardRule,
  } from '$lib/api';
  import AccountSetup from '$lib/components/AccountSetup.svelte';
  import ChatPanel from '$lib/components/ChatPanel.svelte';
  import ContextMenu from '$lib/components/ContextMenu.svelte';
  import ConversationList from '$lib/components/ConversationList.svelte';
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
    type ConnectorInfo,
    type Conversation,
    type ForwardRule,
    type ForwardRuleDraft,
    type MenuItem,
    type Message,
    type PriorityGroup,
    type ScheduledMessage,
    type ScheduleMessageDraft,
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
    appearance: { color_scheme: 'system', theme_id: 'shuttle', tweakcn_css: null },
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
  });
  let selectedAccountId = $state<string | null>(null);
  let selectedConversationId = $state<string | null>(null);
  let selectedWorkspace = $state<string | null>(null);
  let selectedPriority = $state<string | null>(null);
  let searchQuery = $state('');
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
  let menuOpen = $state(false);
  let menuX = $state(0);
  let menuY = $state(0);
  let menuItems = $state<MenuItem[]>([]);
  let menuAction = $state<(id: string) => void>(() => {});
  let forwardOpen = $state(false);
  let forwardText = $state('');
  let forwardSendAt = $state('');

  const setupOnly = $derived(page.url.searchParams.get('setup') === '1');
  const settingsOpen = $derived(mobileTab === 'settings');
  const selectedConversation = $derived(
    conversations.find((c) => c.id === selectedConversationId) ?? null
  );

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

  async function refresh() {
    accounts = await listAccounts();
    unreadTotal = await totalUnread();
    workspaces = await listWorkspaces();
    priorityGroups = await listPriorityGroups();
    forwardRules = await listForwardRules();
    scheduledMessages = await listScheduledMessages();
    conversations = searchQuery
      ? await searchConversations(searchQuery)
      : await listConversations(
          selectedAccountId ?? undefined,
          selectedWorkspace ?? undefined,
          selectedPriority ?? undefined
        );
  }

  async function loadMessages(convId: string) {
    messages = await getMessages(convId);
    await markRead(convId);
    unreadTotal = await totalUnread();
    conversations = await listConversations(
      selectedAccountId ?? undefined,
      selectedWorkspace ?? undefined,
      selectedPriority ?? undefined
    );
  }

  async function selectConversation(id: string) {
    selectedConversationId = id;
    mobileView = 'thread';
    await loadMessages(id);
  }

  async function selectAccount(id: string | null) {
    selectedAccountId = id;
    selectedConversationId = null;
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
    conversations = q
      ? await searchConversations(q)
      : await listConversations(
          selectedAccountId ?? undefined,
          selectedWorkspace ?? undefined,
          selectedPriority ?? undefined
        );
  }

  async function handleSend() {
    if (!draft.trim() || !selectedConversation) return;
    const text = normalizeRichText(draft.trim(), connectorForAccount(selectedConversation.account_id));
    draft = '';
    await sendMessage(selectedConversation.account_id, selectedConversation.id, text);
    await loadMessages(selectedConversation.id);
  }

  async function handleSendLater() {
    if (!draft.trim() || !selectedConversation) return;
    const initial = new Date(Date.now() + 3600_000).toISOString().slice(0, 16);
    const value = prompt('Send at (local datetime, YYYY-MM-DDTHH:MM)', initial);
    if (!value) return;
    await scheduleMessage({
      dest_account_id: selectedConversation.account_id,
      dest_conversation_id: selectedConversation.id,
      body: normalizeRichText(draft.trim(), connectorForAccount(selectedConversation.account_id)),
      send_at: new Date(value).toISOString(),
    });
    draft = '';
    scheduledMessages = await listScheduledMessages();
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
        { id: 'schedule-follow-up', label: 'Schedule follow-up' },
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
        } else if (id === 'schedule-follow-up' && selectedConversation) {
          forwardText = `Follow up:\n${msg.body}`;
          forwardSendAt = new Date(Date.now() + 3600_000).toISOString().slice(0, 16);
          forwardOpen = true;
        } else if (id === 'remind' && selectedConversation) {
          panelOpen = true;
        } else if (id.startsWith('url:')) {
          const u = urls[Number(id.slice(4))];
          if (u) await openExternal(u);
        }
      }
    );
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
      if (
        event.kind === 'message.received' ||
        event.kind === 'message.sent' ||
        event.kind === 'conversation.updated' ||
        event.kind === 'history.sync.started' ||
        event.kind === 'history.sync.completed' ||
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
      ontabchange={selectMobileTab}
      onsettings={() => selectMobileTab('settings')}
      onaccountmenu={accountMenu}
    />
  </div>

  <main class="main" class:mobile-thread={mobileView === 'thread'}>
    <div class="list-pane" class:hidden-mobile={mobileView === 'thread' || settingsOpen} class:hidden={settingsOpen}>
      <div class="org-filters">
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
      </div>
      <ConversationList
        {conversations}
        {accounts}
        selectedAccountId={selectedAccountId}
        selectedId={selectedConversationId}
        {searchQuery}
        onsearch={handleSearch}
        onselect={selectConversation}
        onaccountselect={selectAccount}
        oncompose={() => {}}
        oncontext={convMenu}
        {channelColor}
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

    <div class="thread-pane" class:hidden-mobile={mobileView === 'list' || settingsOpen} class:hidden={settingsOpen}>
      <ThreadView
        conversation={selectedConversation}
        {messages}
        {accounts}
        {draft}
        ondraft={(v) => (draft = v)}
        onsend={handleSend}
        onsendlater={handleSendLater}
        showBack={mobileView === 'thread'}
        onback={() => (mobileView = 'list')}
        onmsgmenu={msgMenu}
        ontextmenu={textMenu}
        ontogglepanel={() => (panelOpen = !panelOpen)}
        {panelOpen}
        channelColor={selectedConversation
          ? channelColor(accounts.find((a) => a.id === selectedConversation.account_id)?.connector_id ?? '')
          : undefined}
        connectorId={selectedConversation ? connectorForAccount(selectedConversation.account_id) : undefined}
      />
      {#if panelOpen && selectedConversation}
        <ChatPanel
          conversation={selectedConversation}
          {workspaces}
          {priorityGroups}
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
    <div class="modal" role="dialog" onclick={(e) => e.stopPropagation()}>
      <h2>Forward to</h2>
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
    flex: 1;
    display: flex;
    min-width: 0;
  }
  .list-pane, .thread-pane, .settings-pane {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .thread-pane { flex: 1; flex-direction: row; }
  .list-pane.hidden, .thread-pane.hidden { display: none; }
  .settings-pane {
    display: none;
    flex: 1;
    background: var(--bg-panel);
    border-right: 1px solid var(--border-subtle);
  }
  .settings-pane.visible { display: flex; }
  .org-filters {
    display: flex;
    gap: 8px;
    padding: 10px 12px 0;
    background: var(--bg-panel);
  }
  .org-filters select {
    flex: 1;
    background: var(--bg-input);
    color: var(--text);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    padding: 6px 8px;
    font: inherit;
  }
  .sidebar-wrap { display: contents; }
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
    .main { flex: 1; min-height: 0; width: 100%; }
    .list-pane, .thread-pane, .settings-pane {
      flex: 1;
      width: 100%;
      min-height: 0;
    }
    .thread-pane { flex-direction: column; }
    .settings-pane { max-width: none; border-right: none; }
    .hidden-mobile { display: none !important; }
    .sidebar-wrap.hidden-mobile { display: none; }
  }
</style>
