<script lang="ts">
  import {
    avatarColor,
    checkWhatsappNumber,
    conversationAvatar,
    createGroup,
    formatTime,
    getInitials,
    listContacts,
    startConversation,
  } from '$lib/api';
  import { listPreview } from '$lib/messageMedia';
  import {
    CONTACT_ROW_LIMIT,
    contactSecondary,
    contactTitle,
    filterContacts,
    looksLikePhoneQuery,
    normalizePhoneInput,
  } from '$lib/contactPicker';
  import { formatPhone } from '$lib/senderIdentity';
  import { previewSenderPrefix } from '$lib/senderIdentity';
  import PreviewDirection from '$lib/components/PreviewDirection.svelte';
  import { compareConversations } from '$lib/conversationOrder';
  import { displayName } from '$lib/displayName';
  import NetworkIcon from '$lib/components/NetworkIcon.svelte';
  import type { Snippet } from 'svelte';
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
    /** A chat was just started from New > Contact; it may have no messages yet. */
    onstartchat?: (conv: Conversation) => void;
    onaccountselect?: (id: string | null) => void;
    oncompose?: () => void;
    onarchivedtoggle?: () => void;
    onrefresh?: () => void;
    onloadmore?: () => void;
    hasMore?: boolean;
    loadingMore?: boolean;
    oncontext?: (conv: Conversation, x: number, y: number) => void;
    channelColor?: (connectorId: string) => string;
    datetimeFormat?: string;
    focusSearchNonce?: number;
    highlightId?: string | null;
    /** Phone widths only: the Updates home (statuses, channels) rendered above the chat list. */
    updates?: Snippet;
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
    onstartchat,
    onaccountselect,
    oncompose,
    onarchivedtoggle,
    onrefresh,
    onloadmore,
    hasMore = false,
    loadingMore = false,
    oncontext,
    channelColor,
    datetimeFormat = '12h_full',
    focusSearchNonce = 0,
    highlightId = null,
    updates,
  }: Props = $props();

  let searchExpanded = $state(false);
  let toastMessage = $state<string | null>(null);
  let searchInputEl = $state<HTMLInputElement | undefined>();
  let desktopSearchInputEl = $state<HTMLInputElement | undefined>();
  let newMenuOpen = $state(false);
  let composeModal = $state<null | 'contact' | 'group'>(null);
  let contacts = $state<Contact[]>([]);
  let contactQuery = $state('');
  let groupTitle = $state('');
  let groupPicked = $state<string[]>([]);
  let composeBusy = $state(false);
  let composeError = $state<string | null>(null);
  let composeAccountId = $state<string | null>(null);
  let contactsLoading = $state(false);
  const UNKNOWN_KEY = 'shuttle.newChat.includeUnknown';
  function readIncludeUnknown(): boolean {
    try {
      return localStorage.getItem(UNKNOWN_KEY) === '1';
    } catch {
      return false;
    }
  }
  let includeUnknown = $state(readIncludeUnknown());
  function setIncludeUnknown(on: boolean) {
    includeUnknown = on;
    try {
      localStorage.setItem(UNKNOWN_KEY, on ? '1' : '0');
    } catch {
      /* per-session only */
    }
  }
  let pinnedRowEl = $state<HTMLDivElement | undefined>();
  let listEl = $state<HTMLDivElement | undefined>();
  let loadSentinel = $state<HTMLDivElement | undefined>();

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

  function isStatusRemote(remoteId: string | null | undefined): boolean {
    const id = (remoteId || '').toLowerCase();
    return id.startsWith('status@');
  }

  const sortedConversations = $derived(
    [...conversations]
      .filter((c) => !isStatusRemote(c.remote_id))
      .sort(compareConversations)
  );

  // Pinned chats live in the horizontal row and channels in the Updates home, but only in the
  // plain inbox view. Search results and the archive keep every chat in the vertical list so
  // nothing becomes unreachable.
  const groupedView = $derived(!searchQuery && !showArchived);
  const pinnedConversations = $derived(groupedView ? sortedConversations.filter((c) => c.pinned) : []);
  const listConversations = $derived(
    groupedView
      ? sortedConversations.filter((c) => !c.pinned && c.conversation_type !== 'channel')
      : sortedConversations
  );
  // Search mixes accounts. A result only earns an account label while the list is not already
  // narrowed to one account.
  const searchAcrossAccounts = $derived(!!searchQuery && !selectedAccountId);

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
  // New/Archived stay available with no network filter: New then asks which account to use.
  const accountActions = $derived(accounts.some((a) => !a.disabled));
  const composeAccounts = $derived(accounts.filter((a) => !a.disabled));
  const composeAccount = $derived(accounts.find((a) => a.id === composeAccountId));

  const matchingContacts = $derived(filterContacts(contacts, contactQuery, includeUnknown));
  const filteredContacts = $derived(matchingContacts.slice(0, CONTACT_ROW_LIMIT));
  const typedDigits = $derived(normalizePhoneInput(contactQuery));
  const queryIsPhone = $derived(looksLikePhoneQuery(contactQuery));

  function accountFor(accountId: string): Account | undefined {
    return accounts.find((a) => a.id === accountId);
  }

  function showToast(msg: string) {
    toastMessage = msg;
    setTimeout(() => {
      toastMessage = null;
    }, 2500);
  }

  function handleComposeClick() {
    newMenuOpen = !newMenuOpen;
  }

  async function loadContacts(accountId: string | null) {
    contacts = [];
    if (!accountId) return;
    contactsLoading = true;
    try {
      const rows = await listContacts(accountId);
      if (composeAccountId === accountId) contacts = rows;
    } catch {
      if (composeAccountId === accountId) contacts = [];
    } finally {
      if (composeAccountId === accountId) contactsLoading = false;
    }
  }

  async function openCompose(kind: 'contact' | 'group') {
    newMenuOpen = false;
    composeAccountId = selectedAccountId ?? composeAccounts[0]?.id ?? null;
    if (!composeAccountId) return;
    composeBusy = false;
    composeError = null;
    contactQuery = '';
    groupTitle = '';
    groupPicked = [];
    composeModal = kind;
    await loadContacts(composeAccountId);
  }

  function closeCompose() {
    composeModal = null;
    composeBusy = false;
    composeError = null;
  }

  function onComposeAccountChange(id: string) {
    composeAccountId = id;
    groupPicked = [];
    composeError = null;
    void loadContacts(id);
  }

  function toggleParticipant(remoteId: string) {
    if (groupPicked.includes(remoteId)) {
      groupPicked = groupPicked.filter((id) => id !== remoteId);
    } else {
      groupPicked = [...groupPicked, remoteId];
    }
  }

  /** Create or fetch the direct chat, then open it and close the dialog. Errors stay inline. */
  async function openChat(remoteId: string, title: string) {
    if (!composeAccountId) return;
    composeBusy = true;
    composeError = null;
    try {
      const conv = await startConversation(composeAccountId, remoteId, title);
      closeCompose();
      if (onstartchat) onstartchat(conv);
      else {
        onrefresh?.();
        onselect(conv.id);
      }
    } catch (e) {
      composeError = typeof e === 'string' ? e : e instanceof Error ? e.message : 'Could not open the chat';
      composeBusy = false;
    }
  }

  function startChatWith(contact: Contact) {
    return openChat(contact.remote_id, contactTitle(contact));
  }

  async function startChatFromQuery() {
    if (!composeAccountId || composeBusy) return;
    const digits = typedDigits;
    if (!digits) {
      composeError = 'Enter the full number with country code, e.g. +974 8899 8899';
      return;
    }
    const jid = `${digits}@s.whatsapp.net`;
    const known = contacts.find((c) => c.remote_id === jid);
    if (!known && composeAccount?.connector_id === 'whatsapp') {
      composeBusy = true;
      composeError = null;
      const onWhatsapp = await checkWhatsappNumber(composeAccountId, digits);
      if (onWhatsapp === false) {
        composeError = `${formatPhone(digits)} is not on WhatsApp.`;
        composeBusy = false;
        return;
      }
    }
    await openChat(jid, known ? contactTitle(known) : formatPhone(digits));
  }

  function onSearchKey(e: KeyboardEvent) {
    if (e.key !== 'Enter' || composeModal !== 'contact' || composeBusy) return;
    e.preventDefault();
    if (queryIsPhone && typedDigits) void startChatFromQuery();
    else if (filteredContacts.length === 1) void startChatWith(filteredContacts[0]);
  }

  function onWindowKey(e: KeyboardEvent) {
    if (e.key === 'Escape' && composeModal) {
      e.preventDefault();
      closeCompose();
    }
  }

  function focusOnMount(node: HTMLElement, enabled = true) {
    if (enabled) node.focus();
  }

  async function submitGroup() {
    if (!composeAccountId) return;
    const title = groupTitle.trim();
    if (!title || groupPicked.length === 0) {
      composeError = 'Add a name and at least one contact';
      return;
    }
    composeBusy = true;
    composeError = null;
    try {
      await createGroup(composeAccountId, title, groupPicked);
      closeCompose();
      showToast('Creating group…');
      onrefresh?.();
    } catch (e) {
      composeError = typeof e === 'string' ? e : e instanceof Error ? e.message : 'Could not create the group';
      composeBusy = false;
    }
  }

  // Any pinned chat that gets a message jumps to the left end; bring that end into view.
  let pinnedSeen = new Map<string, { at: number; unread: number }>();
  $effect(() => {
    const before = pinnedSeen;
    const after = new Map<string, { at: number; unread: number }>();
    let gotMessage = false;
    for (const conv of pinnedConversations) {
      const at = conv.last_message_at ? Date.parse(conv.last_message_at) || 0 : 0;
      const unread = conv.unread_count;
      after.set(conv.id, { at, unread });
      const was = before.get(conv.id);
      if (!was) continue;
      if (unread > was.unread || at > was.at) gotMessage = true;
    }
    pinnedSeen = after;
    if (gotMessage && pinnedRowEl && pinnedRowEl.scrollLeft > 0) {
      pinnedRowEl.scrollTo({ left: 0, behavior: 'smooth' });
    }
  });

  function onPinnedWheel(e: WheelEvent) {
    if (!pinnedRowEl) return;
    if (Math.abs(e.deltaY) <= Math.abs(e.deltaX)) return;
    e.preventDefault();
    pinnedRowEl.scrollLeft += e.deltaY;
  }

  function accountColor(account: Account | undefined): string {
    const color = account?.metadata?.accent_color;
    return typeof color === 'string' && /^#[0-9a-f]{6}$/i.test(color) ? color : CONNECTOR_COLORS[account?.connector_id ?? ''] ?? '#888888';
  }

  function displayTitle(conv: Conversation): string {
    const names = conv.metadata?.names as Record<string, string> | undefined;
    if (conv.conversation_type !== 'direct' || conv.metadata?.is_self || names?.contact || names?.verified || !names?.push || !/^[0-9]+@s\.whatsapp\.net$/.test(conv.remote_id)) return displayName(conv);
    return `+${conv.remote_id.split('@')[0]} · ${names.push}`;
  }

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

  function openSearch() {
    searchExpanded = true;
    requestAnimationFrame(() => searchInputEl?.focus());
  }

  function closeSearch() {
    searchExpanded = false;
    if (searchQuery) onsearch('');
  }

  $effect(() => {
    void focusSearchNonce;
    if (!focusSearchNonce) return;
    openSearch();
    requestAnimationFrame(() => {
      const el = desktopSearchInputEl ?? searchInputEl;
      el?.focus();
      el?.select();
    });
  });

  $effect(() => {
    if (!loadSentinel || !onloadmore || !hasMore || loadingMore || searchQuery) return;
    const root = listEl;
    const obs = new IntersectionObserver(
      (entries) => {
        if (entries.some((e) => e.isIntersecting)) onloadmore?.();
      },
      { root, rootMargin: '160px', threshold: 0 }
    );
    obs.observe(loadSentinel);
    return () => obs.disconnect();
  });
</script>

{#snippet convRow(conv: Conversation)}
  {@const account = accountFor(conv.account_id)}
  {@const accountLabel = searchAcrossAccounts ? account?.name : undefined}
  {@const previewText = listPreview(conv.last_message_preview)}
  {@const previewLine = previewText ? previewSenderPrefix(conv) + previewText : ''}
  <button
    class="conv-item"
    class:selected={selectedId === conv.id}
    class:keyboard-focus={highlightId === conv.id && selectedId !== conv.id}
    class:unread={conv.unread_count > 0}
    class:pinned={conv.pinned}
    class:muted={conv.muted}
    class:account-stripe={!selectedAccountId && !!account}
    style="--account-color: {accountColor(account)}"
    onclick={() => onselect(conv.id)}
    oncontextmenu={(e) => {
      e.preventDefault();
      oncontext?.(conv, e.clientX, e.clientY);
    }}
  >
    <div class="avatar-wrap">
      <div class="avatar" style="background: {avatarColor(displayTitle(conv))}">
        {#if conversationAvatar(conv)}
          <img class="avatar-img" src={conversationAvatar(conv) ?? ''} alt="" />
        {:else}
          {getInitials(displayTitle(conv))}
        {/if}
      </div>
      {#if conv.pinned}
        <span class="pin-icon" title="Pinned" aria-label="Pinned"><svg width="11" height="11" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><path d="M16 9V4h1a1 1 0 0 0 0-2H7a1 1 0 0 0 0 2h1v5c0 1.66-1.34 3-3 3v2h5v6l1-1 1 1v-6h5v-2c-1.66 0-3-1.34-3-3z"/></svg></span>
      {/if}
      {#if account && !selectedAccountId}
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
          <span class="title">{displayTitle(conv)}</span>
          {#if conv.conversation_type === 'direct' && (conv.metadata?.names as Record<string, string> | undefined)?.verified}
            <span class="business-icon" title="WhatsApp business account" aria-label="WhatsApp business account">▣</span>
          {/if}
          {#if accountLabel}
            <span class="account-chip" title={accountLabel}>{accountLabel}</span>
          {/if}
        </span>
        {#if conv.unread_count > 0}
          <span class="unread-badge">{conv.unread_count > 99 ? '99+' : conv.unread_count}</span>
        {/if}
      </div>
      <div class="row preview-row">
        {#if previewText && conv.last_message_from_me != null}
          <PreviewDirection fromMe={conv.last_message_from_me} />
        {/if}
        <span class="preview" use:marquee title={previewLine}><span>{previewLine}</span></span>
        <span class="time">{formatTime(conv.last_message_at, datetimeFormat)}</span>
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

<svelte:window onkeydown={onWindowKey} />

<div class="conv-list">
  <header class="header" class:search-open={searchExpanded}>
    <!-- Mobile collapsed: single row -->
    <div class="header-mobile-bar">
      <div class="header-title-row">
        <h1>{headerTitle}</h1>
        {#if unreadInView > 0}
          <span class="header-badge">{unreadInView}</span>
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

    <!-- Mobile expanded search panel -->
    <div class="header-mobile-search">
      <label class="search search-expanded" class:has-value={searchQuery.length > 0}>
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
      </label>
      {@render filterChips('filters-mobile')}
    </div>

    <!-- Desktop header -->
    <div class="header-desktop">
      <div class="header-top">
        <div class="header-title-row">
          <h1>{headerTitle}</h1>
          {#if unreadInView > 0}
            <span class="header-badge">{unreadInView}</span>
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
      <label class="search" class:has-value={searchQuery.length > 0}>
        <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true">
          <circle cx="11" cy="11" r="8"/><path d="m21 21-4.3-4.3"/>
        </svg>
        <input
          bind:this={desktopSearchInputEl}
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
      </label>
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
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
    <div
      class="compose-overlay"
      role="dialog"
      aria-modal="true"
      aria-label={composeModal === 'group' ? 'New group' : 'New chat'}
      tabindex="-1"
      onclick={(e) => {
        if (e.target === e.currentTarget) closeCompose();
      }}
    >
      <div class="compose-card">
        <div class="compose-head">
          <h2>{composeModal === 'group' ? 'New group' : 'New chat'}</h2>
          <button type="button" class="compose-close" onclick={closeCompose} aria-label="Close">×</button>
        </div>
        {#if !selectedAccountId && composeAccounts.length > 1}
          <select
            class="compose-account"
            aria-label="Account"
            value={composeAccountId}
            onchange={(e) => onComposeAccountChange(e.currentTarget.value)}
            disabled={composeBusy}
          >
            {#each composeAccounts as account (account.id)}
              <option value={account.id}>{account.name}</option>
            {/each}
          </select>
        {/if}
        {#if composeModal === 'group'}
          <input
            class="compose-input"
            type="text"
            placeholder="Group name"
            bind:value={groupTitle}
            disabled={composeBusy}
            use:focusOnMount
          />
          <p class="compose-hint">Pick at least one contact</p>
        {/if}
        <div class="compose-search-row">
          <input
            class="compose-input compose-search"
            type="text"
            placeholder={composeModal === 'group' ? 'Search contacts' : 'Search contacts or enter a number'}
            aria-label="Search contacts"
            bind:value={contactQuery}
            oninput={() => (composeError = null)}
            onkeydown={onSearchKey}
            disabled={composeBusy}
            use:focusOnMount={composeModal === 'contact'}
          />
          <label class="compose-toggle" title="Also list people you only share a group with">
            <input
              type="checkbox"
              checked={includeUnknown}
              onchange={(e) => setIncludeUnknown(e.currentTarget.checked)}
            />
            <span>Include unknown group members</span>
          </label>
        </div>
        <div class="compose-list">
          {#each filteredContacts as contact (contact.id)}
            {@const title = contactTitle(contact)}
            {@const secondary = contactSecondary(contact)}
            {@const avatar = conversationAvatar({ metadata: { avatar_url: contact.avatar_url } })}
            <button
              type="button"
              class="compose-row"
              class:picked={composeModal === 'group' && groupPicked.includes(contact.remote_id)}
              onclick={() => (composeModal === 'group' ? toggleParticipant(contact.remote_id) : startChatWith(contact))}
              disabled={composeBusy}
            >
              <span class="compose-avatar" style="background: {avatarColor(title)}">
                {#if avatar}
                  <img class="avatar-img" src={avatar} alt="" loading="lazy" decoding="async" />
                {:else}
                  {getInitials(title).slice(0, 2)}
                {/if}
              </span>
              <span class="compose-text">
                <span class="compose-name">{title}</span>
                {#if secondary}<span class="compose-jid">{secondary}</span>{/if}
              </span>
            </button>
          {/each}
          {#if contactsLoading && filteredContacts.length === 0}
            <p class="compose-empty">Loading contacts…</p>
          {:else if filteredContacts.length === 0}
            <p class="compose-empty">
              {#if contacts.length === 0}
                No contacts yet. Sync the account, or type a phone number.
              {:else if !includeUnknown && filterContacts(contacts, contactQuery, true).length > 0}
                No saved contacts match. Turn on “Include unknown group members” to see more.
              {:else}
                No contacts match. Type a full phone number to start a new chat.
              {/if}
            </p>
          {:else if matchingContacts.length > filteredContacts.length}
            <p class="compose-empty">Showing {filteredContacts.length} of {matchingContacts.length}. Keep typing to narrow the list.</p>
          {/if}
        </div>
        {#if composeError}
          <p class="compose-error" role="alert">{composeError}</p>
        {/if}
        <div class="compose-footer">
          {#if composeModal === 'group'}
            <button type="button" class="compose-submit" onclick={submitGroup} disabled={composeBusy}>
              Create group
            </button>
          {:else if queryIsPhone}
            <button
              type="button"
              class="compose-submit"
              onclick={startChatFromQuery}
              disabled={composeBusy || !typedDigits}
            >
              {typedDigits ? `Chat with ${formatPhone(typedDigits)}` : 'Enter the full number with country code'}
            </button>
          {/if}
        </div>
      </div>
    </div>
  {/if}

  {#if pinnedConversations.length > 0}
    <div
      class="pinned-row"
      bind:this={pinnedRowEl}
      role="group"
      aria-label="Pinned conversations"
      onwheel={onPinnedWheel}
    >
      {#each pinnedConversations as conv (conv.id)}
        {@const account = accountFor(conv.account_id)}
        <button
          type="button"
          class="pin-item"
          class:selected={selectedId === conv.id}
          class:unread={conv.unread_count > 0}
          class:muted={conv.muted}
          title={displayTitle(conv)}
          onclick={() => onselect(conv.id)}
          oncontextmenu={(e) => {
            e.preventDefault();
            oncontext?.(conv, e.clientX, e.clientY);
          }}
        >
          <span class="avatar-wrap">
            <span class="pin-avatar" style="background: {avatarColor(displayTitle(conv))}">
              {#if conversationAvatar(conv)}
                <img class="avatar-img" src={conversationAvatar(conv) ?? ''} alt="" loading="lazy" />
              {:else}
                {getInitials(displayTitle(conv))}
              {/if}
            </span>
            {#if account && !selectedAccountId}
              <span
                class="network-badge"
                style="background: {channelColor?.(account.connector_id) ?? CONNECTOR_COLORS[account.connector_id] ?? '#888'}"
                title={account.name}
              >
                <NetworkIcon connectorId={account.connector_id} size={10} />
              </span>
            {/if}
            {#if conv.unread_count > 0}
              <span class="unread-badge pin-badge">{conv.unread_count > 99 ? '99+' : conv.unread_count}</span>
            {/if}
          </span>
          <span class="pin-name" use:marquee><span>{displayTitle(conv)}</span></span>
          {#if account && !selectedAccountId}
            <span class="pin-accent" style="--account-color: {accountColor(account)}" aria-hidden="true"></span>
          {/if}
        </button>
      {/each}
    </div>
  {/if}

  <div class="list" role="list" bind:this={listEl}>
    {#if updates && groupedView}
      {@render updates()}
    {/if}

    {#each listConversations as conv (conv.id)}
      {@render convRow(conv)}
    {/each}

    {#if hasMore && !searchQuery}
      <div class="list-sentinel" bind:this={loadSentinel} aria-hidden="true">
        {#if loadingMore}
          <span class="list-loading">Loading more…</span>
        {/if}
      </div>
    {/if}

    {#if searchMessageHits.length > 0}
      <div class="section-label section-label-desktop">Messages</div>
      {#each searchMessageHits as hit (hit.message.id)}
        {@const account = accountFor(hit.account_id)}
        {@const accountLabel = searchAcrossAccounts ? account?.name : undefined}
        <button
          type="button"
          class="search-hit-row"
          class:account-stripe={!!accountLabel}
          style="--account-color: {accountColor(account)}"
          onclick={() => onsearchhit?.(hit)}
        >
          <span class="search-hit-head">
            <span class="search-hit-title">{hit.conversation_title}</span>
            {#if accountLabel}
              <span class="account-chip" title={accountLabel}>{accountLabel}</span>
            {/if}
          </span>
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
    width: 100%;
    min-width: 0;
    max-width: none;
    min-height: 0;
    background: var(--bg-panel);
    border-right: 1px solid var(--border-subtle);
    display: flex;
    flex-direction: column;
    flex: 1 1 auto;
    position: relative;
    overflow: hidden;
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
    background: var(--accent);
    color: var(--text-on-accent, var(--primary-foreground, #ffffff));
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

  .search-hit-head {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    min-width: 0;
  }

  .search-hit-title {
    font-size: 12px;
    font-weight: 600;
    color: var(--text-primary);
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .account-chip {
    margin-left: auto;
    flex: 0 1 auto;
    min-width: 0;
    max-width: 45%;
    padding: 1px 5px;
    border-radius: 999px;
    background: var(--bg-hover);
    font-size: 9px;
    font-weight: 600;
    line-height: 1.4;
    color: var(--text-muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
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
  .header-mobile-search {
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

  .compose-search {
    height: 40px;
    padding: 0 12px;
    font-size: 14px;
  }

  .compose-account {
    height: 36px;
    width: 100%;
  }

  .compose-search-row {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .compose-toggle {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 12px;
    color: var(--text-muted);
    cursor: pointer;
    user-select: none;
  }

  .compose-hint,
  .compose-empty {
    margin: 0;
    font-size: 12px;
    color: var(--text-muted);
  }

  .compose-error {
    margin: 0;
    font-size: 12px;
    color: var(--destructive, #e5484d);
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
    flex-direction: row;
    align-items: center;
    gap: 10px;
    border: none;
    background: transparent;
    padding: 6px;
    border-radius: 8px;
    cursor: pointer;
    text-align: left;
    color: inherit;
    flex-shrink: 0;
  }

  .compose-row:hover,
  .compose-row.picked {
    background: var(--bg-hover);
  }

  .compose-avatar {
    width: 32px;
    height: 32px;
    border-radius: 50%;
    flex-shrink: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 12px;
    font-weight: 600;
    color: white;
    overflow: hidden;
  }

  .compose-text {
    display: flex;
    flex-direction: column;
    gap: 1px;
    min-width: 0;
  }

  .compose-name {
    font-size: 13px;
    font-weight: 600;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
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

  .conv-item.muted .unread-badge {
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
    height: 40px;
    color: var(--text-muted);
    cursor: text;
    transition: background 0.15s ease;
  }

  /* The pill carries the focus state; the input inside has no border or ring of its own. */
  .search:focus-within {
    background: color-mix(in srgb, var(--bg-input) 86%, var(--text) 14%);
  }

  .search.has-value {
    padding-right: 8px;
  }

  /* `!important`: app.css styles every text input with a border, background and focus ring
     through a long :not() selector that would otherwise outrank this rule. */
  .search input {
    flex: 1;
    align-self: stretch;
    width: 100%;
    height: 100%;
    min-width: 0;
    margin: 0;
    padding: 0;
    border: none !important;
    border-radius: 0 !important;
    background: transparent !important;
    box-shadow: none !important;
    outline: none !important;
    color: var(--text);
    font-size: 14px;
    font-weight: 400;
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
    flex: 1 1 auto;
    min-height: 0;
    overflow-y: auto;
    overflow-x: hidden;
    overscroll-behavior: contain;
    padding: 4px 0;
    display: flex;
    flex-direction: column;
    position: relative;
    z-index: 0;
    background: var(--bg-panel);
  }

  .list-sentinel {
    flex-shrink: 0;
    min-height: 1px;
    padding: 8px 16px 12px;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .list-loading {
    font-size: 12px;
    color: var(--text-muted);
  }

  .pinned-row {
    display: flex;
    flex: 0 0 auto;
    align-self: stretch;
    gap: 5px;
    overflow-x: auto;
    overflow-y: hidden;
    padding: 8px 10px 6px;
    border-bottom: 1px solid var(--border-subtle);
    background: var(--bg-panel);
    scrollbar-width: none;
    touch-action: pan-x;
    overscroll-behavior-x: contain;
    position: relative;
    z-index: 2;
    isolation: isolate;
  }

  .pinned-row::-webkit-scrollbar {
    display: none;
  }

  .pin-item {
    flex: 0 0 auto;
    width: 52px;
    border: none;
    background: transparent;
    padding: 3px 0 1px;
    border-radius: var(--radius-md);
    cursor: pointer;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 3px;
    color: var(--text);
    touch-action: manipulation;
    -webkit-tap-highlight-color: transparent;
    transition: background 0.12s ease;
  }

  .pin-item:hover {
    background: var(--bg-hover);
  }

  .pin-item:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: -2px;
  }

  .pin-avatar {
    width: 36px;
    height: 36px;
    border-radius: 50%;
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 12px;
    font-weight: 600;
    color: white;
    letter-spacing: -0.02em;
    overflow: hidden;
    transition: transform 0.1s ease, box-shadow 0.12s ease;
  }

  .pin-item:active .pin-avatar {
    transform: scale(0.94);
  }

  .pin-item.selected .pin-avatar {
    box-shadow: 0 0 0 2px var(--bg-panel), 0 0 0 4px var(--accent);
  }

  .pin-item.muted .pin-avatar {
    opacity: 0.75;
  }

  /* Sized here, not on .unread-badge alone: that rule comes later and would win the tie. */
  .pin-item .pin-badge {
    position: absolute;
    top: -2px;
    right: -4px;
    min-width: 16px;
    height: 16px;
    padding: 0 3px;
    font-size: 9px;
    border: 2px solid var(--bg-panel);
    box-sizing: content-box;
  }

  .pin-name {
    display: block;
    width: calc(100% - 6px);
    margin: 0 3px;
    overflow: hidden;
    white-space: nowrap;
    text-align: center;
    font-size: 10px;
    line-height: 1.2;
    color: var(--text-muted);
  }

  .pin-accent {
    display: block;
    width: 22px;
    height: 3px;
    margin-top: 1px;
    border-radius: 3px;
    background: var(--account-color);
  }

  .pin-item.unread .pin-name,
  .pin-item.selected .pin-name {
    color: var(--text-secondary);
    font-weight: 500;
  }

  .pin-name > span {
    display: inline-block;
    min-width: max-content;
  }

  /* Long names fade out at the edge and scroll (transform only) while hovered or focused. */
  .pin-name:global(.overflowing) {
    text-align: start;
    -webkit-mask-image: linear-gradient(90deg, #000 78%, transparent);
    mask-image: linear-gradient(90deg, #000 78%, transparent);
  }

  .pin-item:hover .pin-name:global(.overflowing),
  .pin-item:focus-visible .pin-name:global(.overflowing) {
    -webkit-mask-image: none;
    mask-image: none;
  }

  .pin-item:hover .pin-name:global(.overflowing) > span,
  .pin-item:focus-visible .pin-name:global(.overflowing) > span {
    animation: marquee var(--marquee-duration) linear infinite;
  }

  /* No hover on touch screens: let the selected chat's long name scroll instead. */
  @media (hover: none) {
    .pin-item.selected .pin-name:global(.overflowing) {
      -webkit-mask-image: none;
      mask-image: none;
    }

    .pin-item.selected .pin-name:global(.overflowing) > span {
      animation: marquee var(--marquee-duration) linear infinite;
    }
  }

  .conv-item {
    width: calc(100% - 12px);
    margin: 0 6px;
    display: flex;
    flex-shrink: 0;
    align-items: center;
    gap: 11px;
    padding: 9px 10px;
    min-height: 62px;
    /* Rows below the fold skip layout and paint until they scroll into view. */
    content-visibility: auto;
    contain-intrinsic-size: auto 64px;
    border: none;
    border-radius: var(--radius-md);
    background: transparent;
    cursor: pointer;
    text-align: left;
    color: inherit;
    transition: background 0.12s ease;
  }

  .conv-item.account-stripe,
  .search-hit-row.account-stripe { position: relative; }
  .conv-item.account-stripe::before,
  .search-hit-row.account-stripe::before { content: ""; position: absolute; left: 0; top: 6px; bottom: 6px; width: 3px; border-radius: 3px; background: var(--account-color); }

  .conv-item:hover {
    background: var(--bg-hover);
  }

  .conv-item.selected {
    background: var(--bg-active);
    box-shadow: inset 0 0 0 1px var(--border-subtle);
  }

  .conv-item.keyboard-focus {
    background: var(--bg-hover);
    box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--accent) 35%, transparent);
  }

  .conv-item.unread {
    background: color-mix(in srgb, var(--accent-muted) 22%, transparent);
  }

  .conv-item.unread.selected {
    background: color-mix(in srgb, var(--accent-muted) 18%, var(--bg-active));
    box-shadow: inset 0 0 0 1px var(--border-subtle);
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

  .pin-icon { position: absolute; top: -3px; left: -3px; z-index: 1; width: 16px; height: 16px; display: grid; place-items: center; border-radius: 50%; color: var(--text-muted); background: var(--bg-panel); }
  .business-icon { color: var(--accent); font-size: 12px; flex-shrink: 0; }

  .title {
    font-size: 1rem;
    font-weight: 500;
    color: var(--text);
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
    font-size: 9px;
    font-weight: 500;
    color: color-mix(in srgb, var(--accent) 65%, var(--text-muted));
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
    gap: 4px;
  }

  .preview-row :global(.dir) { font-size: 0.93rem; }

  .preview {
    font-size: 0.93rem;
    color: var(--text-muted);
    white-space: nowrap;
    overflow: hidden;
    flex: 1;
    min-width: 0;
    line-height: 1.35;
  }

  .preview > span { display: inline-block; min-width: max-content; }

  /* The preview scrolls only while its row is hovered or focused, so no marquee runs at rest. */
  .conv-item:hover .preview:global(.overflowing) > span,
  .conv-item:focus-visible .preview:global(.overflowing) > span {
    animation: marquee var(--marquee-duration) linear infinite;
  }

  /* No hover on touch screens: let the selected chat's long preview scroll instead. */
  @media (hover: none) {
    .conv-item.selected .preview:global(.overflowing) > span {
      animation: marquee var(--marquee-duration) linear infinite;
    }
  }
  @keyframes marquee { 0%, 20% { transform: translateX(0); } 95%, 100% { transform: translateX(calc(-1 * var(--marquee-distance))); } }

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
    flex: 0 0 auto;
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

  @media (max-width: 639px) {
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
    .preview:global(.overflowing) > span { animation: none; }
    .pin-item:hover .pin-name:global(.overflowing) > span,
    .pin-item:focus-visible .pin-name:global(.overflowing) > span,
    .pin-item.selected .pin-name:global(.overflowing) > span {
      animation: none;
    }
  }

  @media (min-width: 640px) {
    .filters-desktop {
      display: flex;
    }
  }
</style>
