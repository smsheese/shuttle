<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import { SvelteSet } from 'svelte/reactivity';
  import DateTimePopover from '$lib/components/DateTimePopover.svelte';
  import NetworkIcon from '$lib/components/NetworkIcon.svelte';
  import { avatarColor, conversationAvatar, forwardMessages, getInitials, listConversations, searchConversations } from '$lib/api';
  import { convTime } from '$lib/conversationOrder';
  import { composeLocked } from '$lib/composeLock';
  import { nextMinute, toLocalDateTime } from '$lib/localDateTime';
  import { messageSnippet } from '$lib/messageSnippet';
  import { toast } from '$lib/toast.svelte';
  import type { Account, Conversation, ForwardResult, Message } from '$lib/types';

  /**
   * Forward messages of one chat to any number of chats on any account. The selection is a set
   * of conversation ids that no filter or search ever clears. After a send, the targets that
   * failed stay selected and a retry sends them only what failed.
   */
  interface Props {
    /** Messages of `sourceConversationId`, in chat order. */
    sources: Message[];
    sourceConversationId: string;
    accounts: Account[];
    onclose: () => void;
    onsent: () => void;
  }

  let { sources, sourceConversationId, accounts, onclose, onsent }: Props = $props();

  const LOAD_LIMIT = 300;

  /** ForwardSelection: chosen conversation ids, independent of the account filter and search. */
  const selection = new SvelteSet<string>();
  /** Account chips; empty means every account. */
  const accountFilter = new SvelteSet<string>();
  /** Every conversation seen so far, so a selected one stays resolvable when it is filtered out. */
  let known = $state<Record<string, Conversation>>({});
  let query = $state('');
  /** ISO time to schedule at; null sends now. */
  let sendAt = $state<string | null>(null);
  let picking = $state(false);
  let sending = $state(false);
  /** Per target, the source ids that failed last time; a retry sends only these. */
  let retry = $state<Record<string, string[]>>({});
  /** Sources the core refused for good (polls, locations...); never sent again. */
  let refused = $state<Record<string, string>>({});
  let failures = $state<ForwardResult[]>([]);
  let scheduleBtn = $state<HTMLButtonElement | null>(null);
  let searchEl = $state<HTMLInputElement | null>(null);

  const accountById = $derived(new Map(accounts.map((a) => [a.id, a])));
  // An admin-only group is no target: the core would refuse the send.
  const sendable = (c: Conversation) =>
    c.conversation_type !== 'channel' && !c.archived && !composeLocked(c) &&
    !(c.remote_id || '').toLowerCase().startsWith('status@');

  const rows = $derived.by(() => {
    const q = query.trim().toLowerCase();
    return Object.values(known)
      .filter((c) => sendable(c))
      .filter((c) => accountFilter.size === 0 || accountFilter.has(c.account_id))
      .filter((c) => !q || (c.title || c.remote_id).toLowerCase().includes(q))
      .sort((a, b) => convTime(b) - convTime(a));
  });

  const sourceIds = $derived(sources.map((m) => m.id));
  const scheduleLabel = $derived(
    sendAt ? new Date(sendAt).toLocaleString(undefined, { dateStyle: 'medium', timeStyle: 'short' }) : ''
  );

  function remember(list: Conversation[]) {
    known = { ...known, ...Object.fromEntries(list.map((c) => [c.id, c])) };
  }

  onMount(() => {
    searchEl?.focus();
    void listConversations(undefined, undefined, undefined, false, 0, LOAD_LIMIT).then(remember);
  });

  // Chats beyond the first page are found by search.
  $effect(() => {
    const q = query.trim();
    if (!q) return;
    const timer = setTimeout(() => void searchConversations(q).then((list) => untrack(() => remember(list))), 200);
    return () => clearTimeout(timer);
  });

  function toggle(set: SvelteSet<string>, id: string) {
    if (set.has(id)) set.delete(id);
    else set.add(id);
  }

  function close() {
    if (!sending) onclose();
  }

  function titleOf(conversationId: string): string {
    return known[conversationId]?.title || 'Chat';
  }

  function snippetOf(messageId: string): string {
    const msg = sources.find((m) => m.id === messageId);
    return msg ? messageSnippet(msg) || 'Message' : 'Message';
  }

  /** One call per distinct list of source ids, so a retry resends only what failed. */
  function plan(): Map<string, string[]> {
    const fresh = sourceIds.filter((id) => !refused[id]);
    const batches = new Map<string, string[]>();
    for (const id of selection) {
      const ids = retry[id] ?? fresh;
      if (ids.length === 0) continue;
      const key = ids.join('\n');
      batches.set(key, [...(batches.get(key) ?? []), id]);
    }
    return batches;
  }

  async function send() {
    if (selection.size === 0 || sending) return;
    sending = true;
    const results: ForwardResult[] = [];
    try {
      for (const [key, targetIds] of plan()) {
        const targets = targetIds
          .map((id) => known[id])
          .filter((c): c is Conversation => !!c)
          .map((c) => ({ account_id: c.account_id, conversation_id: c.id }));
        results.push(...(await forwardMessages(sourceConversationId, key.split('\n'), targets, sendAt)));
      }
    } catch (e) {
      toast.error(String(e));
      sending = false;
      return;
    }
    sending = false;

    const nextRefused = { ...refused };
    for (const r of results) if (r.status === 'not_forwardable') nextRefused[r.message_id] = r.reason;
    refused = nextRefused;
    const failed = results.filter((r) => r.status === 'failed');
    const nextRetry: Record<string, string[]> = {};
    for (const r of failed) nextRetry[r.conversation_id] = [...(nextRetry[r.conversation_id] ?? []), r.message_id];
    for (const r of results) if (!nextRetry[r.conversation_id]) selection.delete(r.conversation_id);
    retry = nextRetry;
    failures = failed;

    const delivered = new Set(results.filter((r) => r.status === 'sent' || r.status === 'scheduled').map((r) => r.conversation_id));
    const refusedNotes = [...new Set(Object.values(nextRefused))];
    if (refusedNotes.length) toast.info(`Not forwarded: ${refusedNotes.join('. ')}`);
    if (failed.length) return;
    if (delivered.size) {
      const chats = delivered.size === 1 ? titleOf([...delivered][0]) : `${delivered.size} chats`;
      toast.info(sendAt ? `Scheduled for ${chats}` : `Forwarded to ${chats}`);
    }
    onsent();
  }
</script>

<div class="backdrop" onclick={close} role="presentation">
  <div
    class="dialog"
    role="dialog"
    tabindex="-1"
    aria-modal="true"
    aria-labelledby="forward-title"
    onclick={(e) => e.stopPropagation()}
    onkeydown={(e) => {
      e.stopPropagation();
      if (e.key === 'Escape') close();
    }}
  >
    <header class="head">
      <h2 id="forward-title">Forward to</h2>
      <div class="head-actions">
        <button
          type="button"
          class="icon-btn"
          class:active={!!sendAt || picking}
          bind:this={scheduleBtn}
          disabled={sending}
          onclick={() => (picking = !picking)}
          aria-label={sendAt ? `Scheduled for ${scheduleLabel}. Change time` : 'Schedule'}
          title={sendAt ? `Sends ${scheduleLabel}` : 'Schedule'}
        >
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true">
            <circle cx="12" cy="12" r="9" />
            <path d="M12 7v5l3 2" />
          </svg>
        </button>
        <button type="button" class="send-btn" disabled={selection.size === 0 || sending} onclick={send}>
          {sending ? 'Sending…' : sendAt ? 'Schedule' : 'Send'}{selection.size ? ` (${selection.size})` : ''}
        </button>
      </div>
    </header>

    {#if picking}
      <DateTimePopover
        anchor={scheduleBtn}
        value={sendAt ? toLocalDateTime(new Date(sendAt)) : ''}
        min={toLocalDateTime(nextMinute())}
        title="Send at"
        confirmLabel="Set"
        onconfirm={(local) => {
          sendAt = new Date(local).toISOString();
          picking = false;
        }}
        oncancel={() => (picking = false)}
      />
    {/if}

    {#if accounts.length > 1}
      <div class="chips" role="group" aria-label="Filter by account">
        {#each accounts as account (account.id)}
          <button
            type="button"
            class="chip"
            aria-pressed={accountFilter.has(account.id)}
            onclick={() => toggle(accountFilter, account.id)}
          >
            <NetworkIcon connectorId={account.connector_id} size={12} />
            {account.name}
          </button>
        {/each}
      </div>
    {/if}

    {#if sendAt}
      <div class="schedule-note">
        Sends {scheduleLabel}
        <button type="button" class="link" disabled={sending} onclick={() => (sendAt = null)}>Send now instead</button>
      </div>
    {/if}

    <input
      bind:this={searchEl}
      class="search"
      type="search"
      placeholder="Search chats"
      aria-label="Search chats"
      bind:value={query}
    />

    {#if failures.length}
      <div class="failures" role="alert">
        <strong>Not sent. The chats below stay selected, press Send to retry.</strong>
        <ul>
          {#each failures as f (f.conversation_id + f.message_id)}
            <li>{titleOf(f.conversation_id)}: {snippetOf(f.message_id)}. {f.status === 'failed' ? f.error : ''}</li>
          {/each}
        </ul>
      </div>
    {/if}

    <ul class="list" aria-label="Chats">
      {#each rows as conv (conv.id)}
        {@const account = accountById.get(conv.account_id)}
        {@const avatar = conversationAvatar(conv)}
        {@const checked = selection.has(conv.id)}
        <li>
          <label class="row" class:checked>
            <input type="checkbox" {checked} disabled={sending} onchange={() => toggle(selection, conv.id)} />
            <span class="avatar" style="background: {avatarColor(conv.title || conv.remote_id)}">
              {#if avatar}
                <img src={avatar} alt="" loading="lazy" decoding="async" />
              {:else}
                {getInitials(conv.title || conv.remote_id)}
              {/if}
            </span>
            <span class="title">{conv.title || conv.remote_id}</span>
            <span class="meta">
              {#if conv.conversation_type === 'group'}
                <svg class="group-icon" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-label="Group" role="img">
                  <circle cx="9" cy="8" r="3.2" /><path d="M3 19c0-3.3 2.7-5.5 6-5.5s6 2.2 6 5.5" /><circle cx="17" cy="9" r="2.6" /><path d="M16 13.6c2.9.2 5 2.1 5 5" />
                </svg>
              {/if}
              {#if account}
                <span class="account">
                  <NetworkIcon connectorId={account.connector_id} size={12} />
                  {account.name}
                </span>
              {/if}
            </span>
            <span class="check" aria-hidden="true">{checked ? '✓' : ''}</span>
          </label>
        </li>
      {:else}
        <li class="empty">{query.trim() ? 'No chats match' : 'Loading chats…'}</li>
      {/each}
    </ul>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 80;
    display: flex;
    align-items: center;
    justify-content: center;
    background: rgba(0, 0, 0, 0.45);
  }
  .dialog {
    display: flex;
    flex-direction: column;
    gap: 10px;
    width: min(460px, calc(100vw - 32px));
    max-height: min(640px, 86vh);
    padding: 16px;
    background: var(--bg-panel);
    color: var(--text);
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    box-shadow: var(--shadow-md);
  }
  .head {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  h2 {
    flex: 1;
    margin: 0;
    font-size: 15px;
  }
  .head-actions {
    display: flex;
    gap: 6px;
  }
  button {
    font: inherit;
    font-size: 13px;
    color: var(--text);
    cursor: pointer;
  }
  button:disabled {
    cursor: default;
    opacity: 0.55;
  }
  .icon-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 32px;
    height: 32px;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--bg-input);
  }
  .icon-btn.active {
    border-color: var(--accent);
    color: var(--accent);
  }
  .send-btn {
    padding: 0 14px;
    height: 32px;
    border: none;
    border-radius: var(--radius-sm);
    background: var(--accent);
    color: var(--text-on-accent, #fff);
    font-weight: 600;
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  .chip {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    padding: 4px 10px;
    border: 1px solid var(--border);
    border-radius: var(--radius-full);
    background: transparent;
    font-size: 12px;
    color: var(--text-muted);
  }
  .chip[aria-pressed='true'] {
    border-color: var(--accent);
    background: var(--accent-muted, var(--bg-active));
    color: var(--text);
  }
  .schedule-note {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 12px;
    color: var(--text-muted);
  }
  .link {
    padding: 0;
    border: none;
    background: none;
    color: var(--accent);
    font-size: 12px;
  }
  .search {
    padding: 8px 10px;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--bg-input);
    color: var(--text);
    font: inherit;
    font-size: 13px;
  }
  .failures {
    padding: 8px 10px;
    border: 1px solid var(--destructive, #ef4444);
    border-radius: var(--radius-sm);
    font-size: 12px;
  }
  .failures ul {
    margin: 4px 0 0;
    padding-left: 16px;
    color: var(--text-muted);
  }
  .list {
    flex: 1;
    min-height: 120px;
    margin: 0;
    padding: 0;
    overflow-y: auto;
    list-style: none;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 7px 8px;
    border-radius: var(--radius-sm);
    cursor: pointer;
  }
  .row:hover {
    background: var(--bg-hover);
  }
  .row.checked {
    background: var(--accent-muted, var(--bg-active));
  }
  .row:focus-within {
    outline: 2px solid var(--accent);
    outline-offset: -2px;
  }
  .row input {
    position: absolute;
    opacity: 0;
    width: 1px;
    height: 1px;
  }
  .avatar {
    display: inline-flex;
    flex: 0 0 auto;
    align-items: center;
    justify-content: center;
    width: 34px;
    height: 34px;
    overflow: hidden;
    border-radius: 50%;
    color: #fff;
    font-size: 12px;
    font-weight: 600;
  }
  .avatar img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }
  .title {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    font-size: 13px;
    white-space: nowrap;
    text-overflow: ellipsis;
  }
  .meta {
    display: inline-flex;
    flex: 0 1 auto;
    align-items: center;
    gap: 6px;
    min-width: 0;
    color: var(--text-muted);
    font-size: 11px;
  }
  .account {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    max-width: 140px;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }
  .group-icon {
    flex: 0 0 auto;
  }
  .check {
    display: inline-flex;
    flex: 0 0 auto;
    align-items: center;
    justify-content: center;
    width: 18px;
    height: 18px;
    border: 1.5px solid var(--border);
    border-radius: 50%;
    font-size: 11px;
    font-weight: 700;
  }
  .row.checked .check {
    border-color: var(--accent);
    background: var(--accent);
    color: var(--text-on-accent, #fff);
  }
  .empty {
    padding: 12px 8px;
    color: var(--text-muted);
    font-size: 13px;
  }
</style>
