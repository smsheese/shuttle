<script lang="ts">
  import {
    avatarColor,
    conversationAvatar,
    createGroup,
    formatTime,
    getInitials,
    listContacts,
    startConversation,
  } from '$lib/api';
  import { listPreview } from '$lib/messageMedia';
  import NetworkIcon from '$lib/components/NetworkIcon.svelte';
  import { CONNECTOR_COLORS, type Account, type Contact, type Conversation, type SearchMessageHit, type SearchScope } from '$lib/types';

  interface Props {
    conversations: Conversation[];
    accounts: Account[];
    selectedAccountId: string | null;
    selectedId: string | null;
    searchQuery: string;
    searchScope?: SearchScope;
    searchMessageHits?: SearchMessageHit[];
    showArchived?: boolean;
    onsearch: (q: string) => void;
    onsearchscope?: (scope: SearchScope) => void;
    onsearchhit?: (hit: SearchMessageHit) => void;
    onselect: (id: string) => void;
    onaccountselect?: (id: string | null) => void;
    oncompose?: () => void;
    onarchivedtoggle?: () => void;
    onrefresh?: () => void;
    oncontext?: (conv: Conversation, x: number, y: number) => void;
    channelColor?: (connectorId: string) => string;
    datetimeFormat?: string;
  }

  let {
    conversations,
    accounts,
    selectedAccountId,
    selectedId,
    searchQuery,
    searchScope = 'global',
    searchMessageHits = [],
    showArchived = false,
    onsearch,
    onsearchscope,
    onsearchhit,
    onselect,
    onaccountselect,
    oncompose,
    onarchivedtoggle,
    onrefresh,
    oncontext,
    channelColor,
    datetimeFormat = '12h_full',
  }: Props = $props();

  let searchExpanded = $state(false);
  let toastMessage = $state<string | null>(null);
  let searchInputEl = $state<HTMLInputElement | undefined>();
  let newMenuOpen = $state(false);
  let composeModal = $state<null | 'contact' | 'group'>(null);
  let contacts = $state<Contact[]>([]);
  let contactQuery = $state('');
  let groupTitle = $state('');
  let groupPicked = $state<string[]>([]);
  let composeBusy = $state(false);

  const CONNECTOR_LABELS: Record<string, string> = {
    whatsapp: 'WhatsApp',
    telegram: 'Telegram',
    signal: 'Signal',
    messenger: 'Messenger',
    instagram: 'Instagram',
    slack: 'Slack',
    discord: 'Discord',
  };

  const networkFilters = $derived.by(() => {
    const seen = new Set<string>();
    const filters: { connectorId: string; label: string; accountId: string }[] = [];
    for (const account of accounts) {
      if (seen.has(account.connector_id)) continue;
      seen.add(account.connector_id);
      filters.push({
        connectorId: account.connector_id,
        label: CONNECTOR_LABELS[account.connector_id] ?? account.name,
        accountId: account.id,
      });
    }
    return filters;
  });

  function isFilterActive(accountId: string): boolean {
    if (selectedAccountId === accountId) return true;
    const account = accounts.find((a) => a.id === selectedAccountId);
    const filterAccount = accounts.find((a) => a.id === accountId);
    return !!account && !!filterAccount && account.connector_id === filterAccount.connector_id;
  }

  const sortedConversations = $derived(
    [...conversations].sort((a, b) => {
      if (a.pinned !== b.pinned) return a.pinned ? -1 : 1;
      const ta = a.last_message_at ? new Date(a.last_message_at).getTime() : 0;
      const tb = b.last_message_at ? new Date(b.last_message_at).getTime() : 0;
      return tb - ta;
    })
  );

  const pinnedConversations = $derived(sortedConversations.filter((c) => c.pinned));
  const unpinnedConversations = $derived(sortedConversations.filter((c) => !c.pinned));

  const headerTitle = $derived(
    showArchived
      ? selectedAccountId
        ? `${accounts.find((a) => a.id === selectedAccountId)?.name ?? 'Chats'} · Archived`
        : 'Archived'
      : selectedAccountId
        ? (accounts.find((a) => a.id === selectedAccountId)?.name ?? 'Chats')
        : 'All Chats'
  );

  const unreadInView = $derived(conversations.reduce((s, c) => s + c.unread_count, 0));
  const accountActions = $derived(!!selectedAccountId);

  const filteredContacts = $derived.by(() => {
    const q = contactQuery.trim().toLowerCase();
    if (!q) return contacts;
    return contacts.filter(
      (c) => c.display_name.toLowerCase().includes(q) || c.remote_id.toLowerCase().includes(q)
    );
  });

  function accountFor(conv: Conversation): Account | undefined {
    return accounts.find((a) => a.id === conv.account_id);
  }

  function showToast(msg: string) {
    toastMessage = msg;
    setTimeout(() => {
      toastMessage = null;
    }, 2500);
  }

  function handleComposeClick() {
    if (!selectedAccountId) {
      showToast('Select an account first');
      oncompose?.();
      return;
    }
    newMenuOpen = !newMenuOpen;
  }

  async function openCompose(kind: 'contact' | 'group') {
    newMenuOpen = false;
    if (!selectedAccountId) return;
    composeBusy = false;
    contactQuery = '';
    groupTitle = '';
    groupPicked = [];
    composeModal = kind;
    try {
      contacts = await listContacts(selectedAccountId);
    } catch {
      contacts = [];
    }
  }

  function closeCompose() {
    composeModal = null;
    composeBusy = false;
  }

  function toggleParticipant(remoteId: string) {
    if (groupPicked.includes(remoteId)) {
      groupPicked = groupPicked.filter((id) => id !== remoteId);
    } else {
      groupPicked = [...groupPicked, remoteId];
    }
  }

  async function startChatWith(contact: Contact) {
    if (!selectedAccountId) return;
    composeBusy = true;
    try {
      const conv = await startConversation(selectedAccountId, contact.remote_id, contact.display_name);
      closeCompose();
      onrefresh?.();
      onselect(conv.id);
    } catch (e) {
      showToast(e instanceof Error ? e.message : 'Could not open chat');
      composeBusy = false;
    }
  }

  async function startChatFromQuery() {
    if (!selectedAccountId) return;
    const raw = contactQuery.trim();
    if (!raw) return;
    composeBusy = true;
    try {
      const conv = await startConversation(selectedAccountId, raw, raw);
      closeCompose();
      onrefresh?.();
      onselect(conv.id);
    } catch (e) {
      showToast(e instanceof Error ? e.message : 'Could not open chat');
      composeBusy = false;
    }
  }

  async function submitGroup() {
    if (!selectedAccountId) return;
    const title = groupTitle.trim();
    if (!title || groupPicked.length === 0) {
      showToast('Add a name and at least one contact');
      return;
    }
    composeBusy = true;
    try {
      await createGroup(selectedAccountId, title, groupPicked);
      closeCompose();
      showToast('Creating group…');
      onrefresh?.();
    } catch (e) {
      showToast(e instanceof Error ? e.message : 'Could not create group');
      composeBusy = false;
    }
  }

  function openSearch() {
    searchExpanded = true;
    requestAnimationFrame(() => searchInputEl?.focus());
  }

  function closeSearch() {
    searchExpanded = false;
    if (searchQuery) onsearch('');
  }
</script>

{#snippet convRow(conv: Conversation)}
  {@const account = accountFor(conv)}
  <button
    class="conv-item"
    class:selected={selectedId === conv.id}
    class:unread={conv.unread_count > 0}
    class:pinned={conv.pinned}
    class:muted={conv.muted}
    onclick={() => onselect(conv.id)}
    oncontextmenu={(e) => {
      e.preventDefault();
      oncontext?.(conv, e.clientX, e.clientY);
    }}
  >
    <div class="avatar-wrap">
      {#if conv.unread_count > 0}
        <span class="unread-dot" aria-hidden="true"></span>
      {/if}
      <div class="avatar" class:has-unread={conv.unread_count > 0} style="background: {avatarColor(conv.title)}">
        {#if conversationAvatar(conv)}
          <img class="avatar-img" src={conversationAvatar(conv) ?? ''} alt="" />
        {:else}
          {getInitials(conv.title)}
        {/if}
      </div>
      {#if account}
        <span
          class="network-badge"
          style="background: {channelColor?.(account.connector_id) ?? CONNECTOR_COLORS[account.connector_id] ?? '#888'}"
          title={account.name}
        >
          <NetworkIcon connectorId={account.connector_id} size={10} />
        </span>
      {/if}
    </div>
    <div class="content">
      <div class="row">
        <span class="title-row">
          {#if conv.pinned}
            <svg class="pin-icon" width="12" height="12" viewBox="0 0 24 24" fill="currentColor" aria-label="Pinned">
              <path d="M16 9V4h1a1 1 0 0 0 0-2H7a1 1 0 0 0 0 2h1v5c0 1.66-1.34 3-3 3v2h5v6l1-1 1 1v-6h5v-2c-1.66 0-3-1.34-3-3z"/>
            </svg>
          {/if}
          <span class="title">{conv.title}</span>
        </span>
        <span class="time">{formatTime(conv.last_message_at, datetimeFormat)}</span>
      </div>
      <div class="row preview-row">
        <span class="preview">{listPreview(conv.last_message_preview)}</span>
        {#if conv.unread_count > 0}
          <span class="unread-badge unread-badge-pulse">{conv.unread_count > 99 ? '99+' : conv.unread_count}</span>
        {/if}
      </div>
    </div>
  </button>
{/snippet}

{#snippet filterChips(extraClass: string)}
  {#if onaccountselect && networkFilters.length > 0}
    <div class="filters {extraClass}" role="tablist" aria-label="Filter by network">
      <button
        class="filter-chip"
        class:active={selectedAccountId === null}
        role="tab"
        aria-selected={selectedAccountId === null}
        onclick={() => onaccountselect(null)}
        type="button"
      >
        All
      </button>
      {#each networkFilters as filter (filter.connectorId)}
        <button
          class="filter-chip"
          class:active={isFilterActive(filter.accountId)}
          role="tab"
          aria-selected={isFilterActive(filter.accountId)}
          style="--chip-color: {CONNECTOR_COLORS[filter.connectorId] ?? '#888'}"
          onclick={() => onaccountselect(filter.accountId)}
          type="button"
        >
          <NetworkIcon connectorId={filter.connectorId} size={14} />
          {filter.label}
        </button>
      {/each}
    </div>
  {/if}
{/snippet}

<div class="conv-list">
  <header class="header" class:search-open={searchExpanded}>
    <!-- Mobile collapsed: single row -->
    <div class="header-mobile-bar">
      <div class="header-title-row">
        <h1>{headerTitle}</h1>
        {#if unreadInView > 0}
          <span class="header-badge header-badge-pulse">{unreadInView}</span>
        {/if}
      </div>
      <div class="header-actions">
        {#if accountActions}
          <button
            class="header-text-btn"
            class:active={showArchived}
            onclick={() => onarchivedtoggle?.()}
            type="button"
          >
            Archived
          </button>
          <div class="new-wrap">
            <button class="compose-btn" onclick={handleComposeClick} aria-label="New" type="button">
              <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true">
                <line x1="12" y1="5" x2="12" y2="19"/><line x1="5" y1="12" x2="19" y2="12"/>
              </svg>
            </button>
            {#if newMenuOpen}
              <div class="new-menu" role="menu">
                <button type="button" role="menuitem" onclick={() => openCompose('group')}>Group</button>
                <button type="button" role="menuitem" onclick={() => openCompose('contact')}>Contact</button>
              </div>
            {/if}
          </div>
        {/if}
        <button class="search-toggle" onclick={openSearch} aria-label="Search conversations" type="button">
          <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true">
            <circle cx="11" cy="11" r="8"/><path d="m21 21-4.3-4.3"/>
          </svg>
        </button>
      </div>
    </div>

    {#if pinnedConversations.length > 0}
      <div class="pinned-chips-mobile" aria-label="Pinned conversations">
        {#each pinnedConversations as conv (conv.id)}
          <button
            class="pinned-chip"
            class:selected={selectedId === conv.id}
            aria-label={conv.title}
            onclick={() => onselect(conv.id)}
            type="button"
          >
            <span class="pinned-chip-avatar" class:has-unread={conv.unread_count > 0} style="background: {avatarColor(conv.title)}">
              {#if conversationAvatar(conv)}
                <img class="avatar-img" src={conversationAvatar(conv) ?? ''} alt="" />
              {:else}
                {getInitials(conv.title)}
              {/if}
            </span>
            {#if conv.unread_count > 0}
              <span class="pinned-chip-badge pinned-chip-badge-pulse">{conv.unread_count > 99 ? '99+' : conv.unread_count}</span>
            {/if}
          </button>
        {/each}
      </div>
    {/if}

    <!-- Mobile expanded search panel -->
    <div class="header-mobile-search">
      <div class="search search-expanded" class:has-value={searchQuery.length > 0}>
        <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true">
          <circle cx="11" cy="11" r="8"/><path d="m21 21-4.3-4.3"/>
        </svg>
        <input
          bind:this={searchInputEl}
          type="text"
          placeholder="Search conversations"
          value={searchQuery}
          oninput={(e) => onsearch(e.currentTarget.value)}
          aria-label="Search conversations"
        />
        <button class="search-close" onclick={closeSearch} aria-label="Close search" type="button">
          <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/>
          </svg>
        </button>
      </div>
      {@render filterChips('filters-mobile')}
    </div>

    <!-- Desktop header -->
    <div class="header-desktop">
      <div class="header-top">
        <div class="header-title-row">
          <h1>{headerTitle}</h1>
          {#if unreadInView > 0}
            <span class="header-badge header-badge-pulse">{unreadInView}</span>
          {/if}
        </div>
        {#if accountActions}
          <div class="header-actions header-actions-desktop">
            <button
              class="header-text-btn"
              class:active={showArchived}
              onclick={() => onarchivedtoggle?.()}
              type="button"
            >
              Archived
            </button>
            <div class="new-wrap">
              <button class="header-text-btn new-btn" onclick={handleComposeClick} type="button">
                New
                <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" aria-hidden="true">
                  <path d="m6 9 6 6 6-6"/>
                </svg>
              </button>
              {#if newMenuOpen}
                <div class="new-menu" role="menu">
                  <button type="button" role="menuitem" onclick={() => openCompose('group')}>Group</button>
                  <button type="button" role="menuitem" onclick={() => openCompose('contact')}>Contact</button>
                </div>
              {/if}
            </div>
          </div>
        {/if}
      </div>
      {@render filterChips('filters-desktop')}
      <div class="search" class:has-value={searchQuery.length > 0}>
        <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true">
          <circle cx="11" cy="11" r="8"/><path d="m21 21-4.3-4.3"/>
        </svg>
        <input
          type="text"
          placeholder="Search conversations"
          value={searchQuery}
          oninput={(e) => onsearch(e.currentTarget.value)}
          aria-label="Search conversations"
        />
        {#if searchQuery}
          <button
            class="clear-search"
            onclick={() => onsearch('')}
            aria-label="Clear search"
            type="button"
          >
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/>
            </svg>
          </button>
        {/if}
      </div>
      {#if searchQuery}
        <div class="search-scope">
          <button type="button" class:active={searchScope === 'global'} onclick={() => onsearchscope?.('global')}>All</button>
          <button type="button" class:active={searchScope === 'account'} onclick={() => onsearchscope?.('account')}>This network</button>
        </div>
      {/if}
    </div>
  </header>

  {#if toastMessage}
    <div class="toast" role="status" aria-live="polite">{toastMessage}</div>
  {/if}

  {#if newMenuOpen}
    <button class="menu-dismiss" type="button" aria-label="Close menu" onclick={() => (newMenuOpen = false)}></button>
  {/if}

  {#if composeModal}
    <div class="compose-overlay" role="dialog" aria-modal="true" aria-label={composeModal === 'group' ? 'New group' : 'New contact'}>
      <div class="compose-card">
        <div class="compose-head">
          <h2>{composeModal === 'group' ? 'New group' : 'New chat'}</h2>
          <button type="button" class="compose-close" onclick={closeCompose} aria-label="Close">×</button>
        </div>
        {#if composeModal === 'group'}
          <input
            class="compose-input"
            type="text"
            placeholder="Group name"
            bind:value={groupTitle}
            disabled={composeBusy}
          />
          <p class="compose-hint">Pick at least one contact</p>
        {:else}
          <input
            class="compose-input"
            type="text"
            placeholder="Search contacts or enter a number"
            bind:value={contactQuery}
            disabled={composeBusy}
          />
        {/if}
        <div class="compose-list">
          {#each filteredContacts as contact (contact.id)}
            {#if composeModal === 'group'}
              <button
                type="button"
                class="compose-row"
                class:picked={groupPicked.includes(contact.remote_id)}
                onclick={() => toggleParticipant(contact.remote_id)}
                disabled={composeBusy}
              >
                <span class="compose-name">{contact.display_name}</span>
                <span class="compose-jid">{contact.remote_id}</span>
              </button>
            {:else}
              <button
                type="button"
                class="compose-row"
                onclick={() => startChatWith(contact)}
                disabled={composeBusy}
              >
                <span class="compose-name">{contact.display_name}</span>
                <span class="compose-jid">{contact.remote_id}</span>
              </button>
            {/if}
          {/each}
          {#if filteredContacts.length === 0}
            <p class="compose-empty">No contacts yet. Sync the account, or type a phone number.</p>
          {/if}
        </div>
        <div class="compose-footer">
          {#if composeModal === 'group'}
            <button type="button" class="compose-submit" onclick={submitGroup} disabled={composeBusy}>
              Create group
            </button>
          {:else if contactQuery.trim()}
            <button type="button" class="compose-submit" onclick={startChatFromQuery} disabled={composeBusy}>
              Chat with {contactQuery.trim()}
            </button>
          {/if}
        </div>
      </div>
    </div>
  {/if}

  <div class="list" role="list">
    {#if pinnedConversations.length > 0}
      <div class="section-label section-label-desktop">Pinned</div>
      {#each pinnedConversations as conv (conv.id)}
        <div class="conv-row-desktop">
          {@render convRow(conv)}
        </div>
      {/each}
    {/if}

    {#each unpinnedConversations as conv (conv.id)}
      {@render convRow(conv)}
    {/each}

    {#if searchMessageHits.length > 0}
      <div class="section-label section-label-desktop">Messages</div>
      {#each searchMessageHits as hit (hit.message.id)}
        <button type="button" class="search-hit-row" onclick={() => onsearchhit?.(hit)}>
          <span class="search-hit-title">{hit.conversation_title}</span>
          <span class="search-hit-body">{hit.message.body.slice(0, 80)}</span>
        </button>
      {/each}
    {/if}

    {#if conversations.length === 0 && searchMessageHits.length === 0}
      <div class="empty">
        <div class="empty-icon">
          <svg width="32" height="32" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
            <path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z"/>
          </svg>
        </div>
        <p>{showArchived ? 'No archived chats' : 'No conversations yet'}</p>
        <p class="hint">{showArchived ? 'Archived chats for this account will show up here' : 'Connect an account to get started'}</p>
      </div>
    {/if}
  </div>
</div>

<style>
  .conv-list {
    width: var(--list-width);
    min-width: 300px;
    max-width: 400px;
    background: var(--bg-panel);
    border-right: 1px solid var(--border-subtle);
    display: flex;
    flex-direction: column;
    flex-shrink: 0;
    position: relative;
  }

  .search-scope {
    display: flex;
    gap: 6px;
    padding: 0 4px 4px;
  }

  .search-scope button {
    border: 1px solid var(--border-subtle);
    background: var(--bg-hover);
    border-radius: 999px;
    padding: 4px 10px;
    font-size: 11px;
    cursor: pointer;
    color: var(--text-muted);
  }

  .search-scope button.active {
    background: var(--accent, #3b82f6);
    color: #fff;
    border-color: transparent;
  }

  .search-hit-row {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 2px;
    width: 100%;
    padding: 10px 14px;
    border: none;
    border-bottom: 1px solid var(--border-subtle);
    background: transparent;
    cursor: pointer;
    text-align: left;
  }

  .search-hit-row:hover {
    background: var(--bg-hover);
  }

  .search-hit-title {
    font-size: 12px;
    font-weight: 600;
    color: var(--text-primary);
  }

  .search-hit-body {
    font-size: 12px;
    color: var(--text-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    max-width: 100%;
  }

  .header {
    padding: 20px 16px 14px;
    border-bottom: 1px solid var(--border-subtle);
    flex-shrink: 0;
  }

  .header-mobile-bar,
  .header-mobile-search,
  .pinned-chips-mobile {
    display: none;
  }

  .header-desktop {
    display: block;
  }

  .header-top {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    margin-bottom: 14px;
  }

  .header-title-row {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }

  .header-actions {
    display: flex;
    align-items: center;
    gap: 6px;
    flex-shrink: 0;
  }

  .header-text-btn {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    height: 32px;
    padding: 0 10px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-md);
    background: var(--bg-hover);
    color: var(--text-secondary);
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
    white-space: nowrap;
  }

  .header-text-btn.active {
    background: var(--accent);
    color: white;
    border-color: transparent;
  }

  .header-text-btn.new-btn {
    background: var(--accent);
    color: white;
    border-color: transparent;
  }

  .new-wrap {
    position: relative;
  }

  .new-menu {
    position: absolute;
    top: calc(100% + 6px);
    right: 0;
    z-index: 20;
    min-width: 140px;
    background: var(--bg-panel);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-md);
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.12);
    padding: 4px;
    display: flex;
    flex-direction: column;
  }

  .new-menu button {
    text-align: left;
    border: none;
    background: transparent;
    color: var(--text-primary);
    padding: 8px 10px;
    border-radius: 6px;
    font-size: 13px;
    cursor: pointer;
  }

  .new-menu button:hover {
    background: var(--bg-hover);
  }

  .menu-dismiss {
    position: fixed;
    inset: 0;
    z-index: 15;
    border: none;
    background: transparent;
    cursor: default;
  }

  .compose-overlay {
    position: absolute;
    inset: 0;
    z-index: 30;
    background: color-mix(in srgb, var(--bg-app, #111) 45%, transparent);
    display: flex;
    align-items: flex-start;
    justify-content: center;
    padding: 48px 16px 16px;
  }

  .compose-card {
    width: 100%;
    max-width: 360px;
    background: var(--bg-panel);
    border: 1px solid var(--border-subtle);
    border-radius: 12px;
    padding: 14px;
    display: flex;
    flex-direction: column;
    gap: 10px;
    max-height: calc(100% - 24px);
  }

  .compose-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  .compose-head h2 {
    margin: 0;
    font-size: 16px;
  }

  .compose-close {
    border: none;
    background: transparent;
    font-size: 22px;
    line-height: 1;
    cursor: pointer;
    color: var(--text-muted);
  }

  .compose-input {
    width: 100%;
    height: 38px;
    border: 1px solid var(--border-subtle);
    border-radius: 8px;
    padding: 0 10px;
    background: var(--bg-app, var(--bg-panel));
    color: var(--text-primary);
  }

  .compose-hint,
  .compose-empty {
    margin: 0;
    font-size: 12px;
    color: var(--text-muted);
  }

  .compose-list {
    overflow-y: auto;
    flex: 1;
    min-height: 120px;
    display: flex;
    flex-direction: column;
  }

  .compose-row {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 2px;
    border: none;
    background: transparent;
    padding: 8px 6px;
    border-radius: 8px;
    cursor: pointer;
    text-align: left;
    color: inherit;
  }

  .compose-row:hover,
  .compose-row.picked {
    background: var(--bg-hover);
  }

  .compose-name {
    font-size: 13px;
    font-weight: 600;
  }

  .compose-jid {
    font-size: 11px;
    color: var(--text-muted);
  }

  .compose-submit {
    height: 36px;
    border: none;
    border-radius: 8px;
    background: var(--accent);
    color: white;
    font-weight: 600;
    cursor: pointer;
  }

  .avatar-img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
  }

  .compose-btn,
  .search-toggle {
    display: flex;
    align-items: center;
    justify-content: center;
    border: none;
    cursor: pointer;
    flex-shrink: 0;
    touch-action: manipulation;
    -webkit-tap-highlight-color: transparent;
    transition: background 0.15s ease, transform 0.1s ease;
  }

  .compose-btn {
    width: 48px;
    height: 48px;
    min-width: 48px;
    min-height: 48px;
    border-radius: var(--radius-full);
    background: var(--accent);
    color: white;
  }

  .compose-btn:active {
    transform: scale(0.94);
    background: var(--accent-hover);
  }

  .search-toggle {
    width: 44px;
    height: 44px;
    min-width: 44px;
    min-height: 44px;
    border-radius: var(--radius-md);
    background: transparent;
    color: var(--text-secondary);
  }

  .search-toggle:active {
    background: var(--bg-hover);
  }

  .filters {
    display: none;
    gap: 8px;
    overflow-x: auto;
    -webkit-overflow-scrolling: touch;
    scrollbar-width: none;
    padding-bottom: 2px;
  }

  .filters-desktop {
    display: none;
    margin-bottom: 12px;
  }

  .filters::-webkit-scrollbar {
    display: none;
  }

  .filter-chip {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    height: 36px;
    min-height: 36px;
    padding: 0 14px;
    border: 1px solid var(--border);
    border-radius: var(--radius-full);
    background: transparent;
    color: var(--text-secondary);
    font-size: 13px;
    font-weight: 500;
    white-space: nowrap;
    cursor: pointer;
    flex-shrink: 0;
    touch-action: manipulation;
    -webkit-tap-highlight-color: transparent;
    transition: background 0.15s ease, border-color 0.15s ease, color 0.15s ease;
  }

  .filter-chip:active {
    background: var(--bg-hover);
  }

  .filter-chip.active {
    background: color-mix(in srgb, var(--chip-color, var(--accent)) 15%, transparent);
    border-color: color-mix(in srgb, var(--chip-color, var(--accent)) 45%, transparent);
    color: var(--text);
  }

  .filter-chip:not(.active):hover {
    background: var(--bg-hover);
    border-color: var(--text-muted);
  }

  h1 {
    font-size: 22px;
    font-weight: 700;
    letter-spacing: -0.03em;
    line-height: 1.2;
  }

  .header-badge {
    min-width: 22px;
    height: 22px;
    padding: 0 7px;
    background: var(--accent-muted);
    color: var(--accent);
    font-size: 12px;
    font-weight: 600;
    border-radius: var(--radius-full);
    display: flex;
    align-items: center;
    justify-content: center;
  }

  @keyframes unread-breathe {
    0%,
    100% {
      transform: scale(1);
      opacity: 1;
    }
    50% {
      transform: scale(1.12);
      opacity: 0.88;
    }
  }

  @keyframes unread-ring-breathe {
    0%,
    100% {
      box-shadow: 0 0 0 0 color-mix(in srgb, var(--accent) 55%, transparent);
    }
    50% {
      box-shadow: 0 0 0 5px color-mix(in srgb, var(--accent) 0%, transparent);
    }
  }

  @keyframes unread-dot-glow {
    0%,
    100% {
      transform: scale(1);
      box-shadow: 0 0 0 0 color-mix(in srgb, var(--accent) 70%, transparent);
    }
    50% {
      transform: scale(1.15);
      box-shadow: 0 0 0 6px color-mix(in srgb, var(--accent) 0%, transparent);
    }
  }

  .header-badge-pulse,
  .unread-badge-pulse,
  .pinned-chip-badge-pulse {
    animation: unread-breathe 1.8s ease-in-out infinite;
  }

  .unread-dot {
    position: absolute;
    top: -2px;
    left: -2px;
    width: 12px;
    height: 12px;
    border-radius: 50%;
    background: var(--accent);
    border: 2px solid var(--bg-panel);
    z-index: 4;
    pointer-events: none;
    animation: unread-dot-glow 1.8s ease-in-out infinite;
  }

  .avatar.has-unread {
    animation: unread-ring-breathe 2.4s ease-in-out infinite;
  }

  .conv-item.muted .unread-dot,
  .conv-item.muted .unread-badge-pulse {
    opacity: 0.72;
  }

  .search {
    display: flex;
    align-items: center;
    gap: 8px;
    background: var(--bg-input);
    border: 1px solid transparent;
    border-radius: var(--radius-md);
    padding: 0 12px;
    height: 36px;
    color: var(--text-muted);
    transition: border-color 0.15s ease, background 0.15s ease, box-shadow 0.15s ease;
  }

  .search:focus-within {
    border-color: var(--accent);
    background: var(--bg-main);
    box-shadow: 0 0 0 3px var(--accent-muted);
  }

  .search.has-value {
    padding-right: 8px;
  }

  .search input {
    flex: 1;
    border: none;
    background: transparent;
    color: var(--text);
    font-size: 13px;
    font-weight: 400;
    outline: none;
    min-width: 0;
  }

  .search input::placeholder {
    color: var(--text-muted);
  }

  .clear-search,
  .search-close {
    display: flex;
    align-items: center;
    justify-content: center;
    border: none;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--text-muted);
    cursor: pointer;
    flex-shrink: 0;
    touch-action: manipulation;
    -webkit-tap-highlight-color: transparent;
  }

  .clear-search {
    width: 22px;
    height: 22px;
  }

  .clear-search:hover {
    background: var(--bg-hover);
    color: var(--text);
  }

  .search-close {
    width: 36px;
    height: 36px;
    min-width: 36px;
    min-height: 36px;
  }

  .search-close:active {
    background: var(--bg-hover);
    color: var(--text);
  }

  .toast {
    position: absolute;
    left: 50%;
    bottom: calc(16px + var(--safe-bottom, 0px));
    transform: translateX(-50%);
    z-index: 50;
    padding: 10px 18px;
    background: var(--text);
    color: var(--bg-main);
    font-size: 14px;
    font-weight: 500;
    border-radius: var(--radius-full);
    box-shadow: var(--shadow-md, 0 4px 16px rgba(0, 0, 0, 0.2));
    white-space: nowrap;
    pointer-events: none;
    animation: toast-in 0.2s ease;
  }

  @keyframes toast-in {
    from {
      opacity: 0;
      transform: translateX(-50%) translateY(8px);
    }
    to {
      opacity: 1;
      transform: translateX(-50%) translateY(0);
    }
  }

  .section-label {
    padding: 10px 16px 4px;
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--text-muted);
  }

  .conv-row-desktop {
    display: block;
  }

  .list {
    flex: 1;
    overflow-y: auto;
    overscroll-behavior: contain;
    padding: 4px 0;
  }

  .conv-item {
    width: calc(100% - 12px);
    margin: 0 6px;
    display: flex;
    gap: 11px;
    padding: 9px 10px;
    border: none;
    border-radius: var(--radius-md);
    background: transparent;
    cursor: pointer;
    text-align: left;
    color: inherit;
    transition: background 0.12s ease;
  }

  .conv-item:hover {
    background: var(--bg-hover);
  }

  .conv-item.selected {
    background: var(--bg-active);
    box-shadow: inset 0 0 0 1px var(--border-subtle);
  }

  .conv-item.unread {
    box-shadow: inset 3px 0 0 var(--accent);
  }

  .conv-item.unread.selected {
    box-shadow: inset 3px 0 0 var(--accent), inset 0 0 0 1px var(--border-subtle);
  }

  .avatar-wrap {
    position: relative;
    flex-shrink: 0;
  }

  .avatar {
    width: 44px;
    height: 44px;
    border-radius: 50%;
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 15px;
    font-weight: 600;
    color: white;
    letter-spacing: -0.02em;
    overflow: hidden;
  }

  .network-badge {
    position: absolute;
    bottom: -1px;
    right: -2px;
    width: 17px;
    height: 17px;
    border-radius: 50%;
    color: white;
    display: flex;
    align-items: center;
    justify-content: center;
    border: 2px solid var(--bg-panel);
    box-shadow: var(--shadow-sm);
  }

  .content {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    justify-content: center;
    gap: 3px;
    padding-top: 1px;
  }

  .row {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 8px;
  }

  .title-row {
    display: flex;
    align-items: center;
    gap: 4px;
    min-width: 0;
    flex: 1;
  }

  .pin-icon {
    flex-shrink: 0;
    color: var(--text-muted);
    opacity: 0.7;
  }

  .title {
    font-size: 1rem;
    font-weight: 500;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    letter-spacing: -0.01em;
  }

  .conv-item.unread .title {
    font-weight: 600;
    color: var(--text);
  }

  .time {
    font-size: 10.5px;
    font-weight: 500;
    color: var(--text-muted);
    flex-shrink: 0;
    letter-spacing: 0.01em;
    white-space: nowrap;
    font-variant-numeric: tabular-nums;
  }

  .conv-item.unread .time {
    color: var(--accent);
  }

  .preview-row {
    align-items: center;
  }

  .preview {
    font-size: 0.93rem;
    color: var(--text-muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    flex: 1;
    line-height: 1.35;
  }

  .conv-item.unread .preview {
    color: var(--text-secondary);
    font-weight: 500;
  }

  .conv-item.muted .preview {
    opacity: 0.75;
  }

  .unread-badge {
    min-width: 19px;
    height: 19px;
    padding: 0 5px;
    background: var(--accent);
    color: white;
    font-size: 10px;
    font-weight: 700;
    letter-spacing: -0.02em;
    border-radius: var(--radius-full);
    display: flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
    box-shadow: 0 0 0 0 color-mix(in srgb, var(--accent) 40%, transparent);
  }

  .empty {
    padding: 56px 24px;
    text-align: center;
    color: var(--text-muted);
  }

  .empty-icon {
    opacity: 0.25;
    margin-bottom: 12px;
    display: flex;
    justify-content: center;
  }

  .empty p {
    font-size: 14px;
    font-weight: 500;
    color: var(--text-secondary);
  }

  .hint {
    font-size: 13px;
    margin-top: 4px;
    color: var(--text-muted);
    font-weight: 400;
  }

  @media (max-width: 768px) {
    .conv-list {
      width: 100%;
      min-width: 0;
      max-width: none;
      border-right: none;
      flex: 1;
    }

    .header {
      padding: calc(12px + var(--safe-top)) 16px 12px;
    }

    .header-desktop {
      display: none;
    }

    .header-mobile-bar {
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: 8px;
    }

    .pinned-chips-mobile {
      display: flex;
      gap: 12px;
      overflow-x: auto;
      -webkit-overflow-scrolling: touch;
      scrollbar-width: none;
      margin-top: 12px;
      padding: 0 2px 2px;
    }

    .pinned-chips-mobile::-webkit-scrollbar {
      display: none;
    }

    .pinned-chip {
      position: relative;
      flex-shrink: 0;
      border: none;
      background: transparent;
      padding: 0;
      cursor: pointer;
      touch-action: manipulation;
      -webkit-tap-highlight-color: transparent;
    }

    .pinned-chip:active .pinned-chip-avatar {
      transform: scale(0.94);
    }

    .pinned-chip.selected .pinned-chip-avatar {
      box-shadow: 0 0 0 2px var(--bg-panel), 0 0 0 4px var(--accent);
    }

    .pinned-chip-avatar {
      width: 48px;
      height: 48px;
      border-radius: 50%;
      display: flex;
      align-items: center;
      justify-content: center;
      font-size: 16px;
      font-weight: 600;
      color: white;
      letter-spacing: -0.02em;
      transition: transform 0.1s ease;
    }

    .pinned-chip-avatar.has-unread {
      animation: unread-ring-breathe 2.4s ease-in-out infinite;
    }

    .pinned-chip-badge {
      position: absolute;
      top: -2px;
      right: -2px;
      min-width: 18px;
      height: 18px;
      padding: 0 4px;
      background: var(--accent);
      color: white;
      font-size: 10px;
      font-weight: 700;
      border-radius: var(--radius-full);
      display: flex;
      align-items: center;
      justify-content: center;
      border: 2px solid var(--bg-panel);
    }

    .header.search-open .pinned-chips-mobile {
      margin-top: 0;
      margin-bottom: 10px;
    }

    .pinned-chips-mobile {
      display: none;
    }

    .section-label-desktop,
    .conv-row-desktop {
      display: block;
    }

    .header-mobile-search {
      display: none;
    }

    .header.search-open .header-mobile-bar {
      display: none;
    }

    .header.search-open .header-mobile-search {
      display: block;
    }

    .header.search-open {
      padding-bottom: 10px;
    }

    .filters-mobile {
      display: flex;
      gap: 8px;
      margin-top: 10px;
    }

    .search-expanded {
      height: 44px;
      padding: 0 6px 0 14px;
    }

    .filter-chip {
      height: 44px;
      min-height: 44px;
    }

    .list {
      padding: 2px 0 8px;
    }

    .conv-item {
      width: 100%;
      margin: 0;
      padding: 12px 16px;
      min-height: 72px;
      gap: 12px;
      border-radius: 0;
      touch-action: manipulation;
      -webkit-tap-highlight-color: transparent;
    }

    .conv-item:active {
      background: var(--bg-hover);
    }

    .avatar {
      width: 48px;
      height: 48px;
      font-size: 16px;
    }

    .network-badge {
      width: 18px;
      height: 18px;
    }

    .title {
      font-size: 15px;
    }

    .preview {
      font-size: 14px;
    }

    .empty {
      padding: 48px 24px calc(24px + var(--safe-bottom));
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .header-badge-pulse,
    .unread-badge-pulse,
    .pinned-chip-badge-pulse,
    .unread-dot,
    .avatar.has-unread,
    .pinned-chip-avatar.has-unread {
      animation: none;
    }
  }

  @media (min-width: 769px) {
    .filters-desktop {
      display: flex;
    }
  }
</style>
