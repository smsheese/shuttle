<script lang="ts">
  import type { Account, Conversation, Workspace } from '$lib/types';

  type PickKind = 'conversation' | 'account' | 'workspace';

  interface PickItem {
    kind: PickKind;
    id: string;
    label: string;
    sublabel?: string;
  }

  interface Props {
    open: boolean;
    conversations: Conversation[];
    accounts: Account[];
    workspaces: Workspace[];
    onclose: () => void;
    onpickConversation: (id: string) => void;
    onpickAccount: (id: string) => void;
    onpickWorkspace: (id: string) => void;
  }

  let {
    open,
    conversations,
    accounts,
    workspaces,
    onclose,
    onpickConversation,
    onpickAccount,
    onpickWorkspace,
  }: Props = $props();

  let query = $state('');
  let activeIndex = $state(0);
  let inputEl = $state<HTMLInputElement | undefined>();

  function accountName(accountId: string): string {
    return accounts.find((a) => a.id === accountId)?.name ?? 'Account';
  }

  function matchesQuery(text: string, q: string): boolean {
    const needle = q.trim().toLowerCase();
    if (!needle) return true;
    const hay = text.toLowerCase();
    if (hay.includes(needle)) return true;
    let i = 0;
    for (const c of hay) {
      if (c === needle[i]) i += 1;
      if (i >= needle.length) return true;
    }
    return false;
  }

  const filteredConversations = $derived(
    conversations.filter((c) =>
      matchesQuery(c.title, query) || matchesQuery(accountName(c.account_id), query)
    )
  );

  const filteredAccounts = $derived(
    accounts.filter((a) => matchesQuery(a.name, query))
  );

  const filteredWorkspaces = $derived(
    workspaces.filter((w) => matchesQuery(w.name, query))
  );

  const items = $derived.by((): PickItem[] => {
    const list: PickItem[] = [];
    for (const conv of filteredConversations) {
      list.push({
        kind: 'conversation',
        id: conv.id,
        label: conv.title,
        sublabel: accountName(conv.account_id),
      });
    }
    for (const account of filteredAccounts) {
      list.push({
        kind: 'account',
        id: account.id,
        label: account.name,
        sublabel: account.connector_id,
      });
    }
    for (const ws of filteredWorkspaces) {
      list.push({
        kind: 'workspace',
        id: ws.id,
        label: ws.name,
        sublabel: ws.builtin ? 'Built-in' : 'Workspace',
      });
    }
    return list;
  });

  function pick(item: PickItem) {
    if (item.kind === 'conversation') onpickConversation(item.id);
    else if (item.kind === 'account') onpickAccount(item.id);
    else onpickWorkspace(item.id);
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      e.preventDefault();
      onclose();
      return;
    }
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      if (items.length === 0) return;
      activeIndex = (activeIndex + 1) % items.length;
      return;
    }
    if (e.key === 'ArrowUp') {
      e.preventDefault();
      if (items.length === 0) return;
      activeIndex = (activeIndex - 1 + items.length) % items.length;
      return;
    }
    if (e.key === 'Enter') {
      e.preventDefault();
      const item = items[activeIndex];
      if (item) pick(item);
    }
  }

  $effect(() => {
    if (open) {
      query = '';
      activeIndex = 0;
      requestAnimationFrame(() => inputEl?.focus());
    }
  });

  $effect(() => {
    void items.length;
    if (activeIndex >= items.length) activeIndex = Math.max(0, items.length - 1);
  });
</script>

{#if open}
  <div class="backdrop" role="presentation" onclick={onclose}></div>
  <div
    class="palette"
    role="dialog"
    aria-modal="true"
    aria-label="Quick switch"
    onclick={(e) => e.stopPropagation()}
    onkeydown={onKeydown}
  >
    <input
      bind:this={inputEl}
      class="query"
      type="text"
      placeholder="Jump to conversation, account, or workspace…"
      bind:value={query}
      aria-label="Quick switch search"
    />

    <div class="results" role="listbox" aria-label="Quick switch results">
      {#if items.length === 0}
        <p class="empty">No matches</p>
      {:else}
        {#if filteredConversations.length > 0}
          <p class="group-label">Conversations</p>
          {#each filteredConversations as conv, i (conv.id)}
            {@const idx = items.findIndex((it) => it.kind === 'conversation' && it.id === conv.id)}
            <button
              type="button"
              class="row"
              class:active={idx === activeIndex}
              role="option"
              aria-selected={idx === activeIndex}
              onclick={() => onpickConversation(conv.id)}
            >
              <span class="label">{conv.title}</span>
              <span class="sublabel">{accountName(conv.account_id)}</span>
            </button>
          {/each}
        {/if}

        {#if filteredAccounts.length > 0}
          <p class="group-label">Accounts</p>
          {#each filteredAccounts as account (account.id)}
            {@const idx = items.findIndex((it) => it.kind === 'account' && it.id === account.id)}
            <button
              type="button"
              class="row"
              class:active={idx === activeIndex}
              role="option"
              aria-selected={idx === activeIndex}
              onclick={() => onpickAccount(account.id)}
            >
              <span class="label">{account.name}</span>
              <span class="sublabel">{account.connector_id}</span>
            </button>
          {/each}
        {/if}

        {#if filteredWorkspaces.length > 0}
          <p class="group-label">Workspaces</p>
          {#each filteredWorkspaces as ws (ws.id)}
            {@const idx = items.findIndex((it) => it.kind === 'workspace' && it.id === ws.id)}
            <button
              type="button"
              class="row"
              class:active={idx === activeIndex}
              role="option"
              aria-selected={idx === activeIndex}
              onclick={() => onpickWorkspace(ws.id)}
            >
              <span class="label">{ws.name}</span>
              <span class="sublabel">{ws.builtin ? 'Built-in' : 'Workspace'}</span>
            </button>
          {/each}
        {/if}
      {/if}
    </div>
  </div>
{/if}

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.45);
    z-index: 80;
  }
  .palette {
    position: fixed;
    top: 12vh;
    left: 50%;
    transform: translateX(-50%);
    width: min(560px, 92vw);
    max-height: min(70vh, 520px);
    display: flex;
    flex-direction: column;
    background: var(--bg-panel);
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    box-shadow: 0 16px 48px rgba(0, 0, 0, 0.25);
    z-index: 81;
    overflow: hidden;
  }
  .query {
    width: 100%;
    border: none;
    border-bottom: 1px solid var(--border-subtle);
    background: var(--bg-input);
    color: var(--text);
    padding: 14px 16px;
    font: inherit;
    font-size: 15px;
    outline: none;
  }
  .query:focus {
    border-bottom-color: var(--accent);
  }
  .results {
    overflow-y: auto;
    padding: 8px;
  }
  .group-label {
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--text-muted);
    padding: 8px 10px 4px;
  }
  .row {
    width: 100%;
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 12px;
    border: none;
    background: transparent;
    color: var(--text);
    text-align: left;
    padding: 10px 12px;
    border-radius: var(--radius-sm);
    cursor: pointer;
    font: inherit;
  }
  .row:hover,
  .row.active {
    background: var(--bg-hover);
  }
  .row.active {
    outline: 1px solid color-mix(in srgb, var(--accent) 40%, transparent);
  }
  .label {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .sublabel {
    flex-shrink: 0;
    font-size: 12px;
    color: var(--text-muted);
  }
  .empty {
    padding: 24px 12px;
    text-align: center;
    color: var(--text-muted);
    font-size: 13px;
  }
</style>
