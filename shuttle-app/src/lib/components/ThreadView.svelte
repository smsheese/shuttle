<script lang="ts">
  import { avatarColor, formatTime, getInitials } from '$lib/api';
  import NetworkIcon from '$lib/components/NetworkIcon.svelte';
  import { wrapSelection } from '$lib/richText';
  import { CONNECTOR_COLORS, type Account, type Conversation, type Message } from '$lib/types';

  interface Props {
    conversation: Conversation | null;
    messages: Message[];
    accounts: Account[];
    draft: string;
    ondraft: (v: string) => void;
    onsend: () => void;
    onsendlater?: () => void;
    onback?: () => void;
    showBack?: boolean;
    onmsgmenu?: (msg: Message, x: number, y: number) => void;
    ontextmenu?: (text: string, x: number, y: number) => void;
    ontogglepanel?: () => void;
    panelOpen?: boolean;
    channelColor?: string;
    connectorId?: string;
  }

  let {
    conversation,
    messages,
    accounts,
    draft,
    ondraft,
    onsend,
    onsendlater,
    onback,
    showBack = false,
    onmsgmenu,
    ontextmenu,
    ontogglepanel,
    panelOpen = false,
    channelColor,
    connectorId,
  }: Props = $props();
  let composerEl = $state<HTMLTextAreaElement | null>(null);

  const account = $derived(
    conversation ? accounts.find((a) => a.id === conversation.account_id) : null
  );

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      onsend();
    }
  }

  function formatMsgTime(iso: string): string {
    const d = new Date(iso);
    return d.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
  }

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
          {getInitials(conversation.title)}
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

      <div class="header-info">
        <h2>{conversation.title}</h2>
        {#if account}
          <span class="subtitle">{account.name}</span>
        {:else if conversation.conversation_type === 'group'}
          <span class="subtitle">Group</span>
        {/if}
      </div>
      {#if ontogglepanel}
        <button class="panel-btn" class:active={panelOpen} type="button" onclick={ontogglepanel} aria-label="Chat details">
          <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <circle cx="12" cy="12" r="10"/><path d="M12 16v-4M12 8h.01"/>
          </svg>
        </button>
      {/if}
    </header>

    <div class="messages">
      {#each messages as msg, i (msg.id)}
        {@const prev = messages[i - 1]}
        {@const pos = groupPos(messages, i)}
        {@const showGap = !prev || prev.direction !== msg.direction}
        {@const showMeta = pos === 'single' || pos === 'last'}
        <div
          class="msg-row"
          class:outbound={msg.direction === 'outbound'}
          class:gap={showGap}
          class:group-single={pos === 'single'}
          class:group-first={pos === 'first'}
          class:group-middle={pos === 'middle'}
          class:group-last={pos === 'last'}
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
            <p>{msg.body}</p>
            {#if showMeta}
              <span class="msg-time">
                {formatMsgTime(msg.timestamp)}
                {#if msg.direction === 'outbound'}
                  <span class="status" class:read={msg.status === 'read'} aria-label={msg.status === 'read' ? 'Read' : 'Sent'}>
                    {msg.status === 'read' ? '✓✓' : msg.status === 'delivered' ? '✓✓' : '✓'}
                  </span>
                {/if}
              </span>
            {/if}
          </div>
        </div>
      {/each}
    </div>

    <footer class="composer">
      <div class="format-bar">
        <button type="button" class="format-btn" onclick={() => applyMark('bold')} aria-label="Bold">B</button>
        <button type="button" class="format-btn" onclick={() => applyMark('italic')} aria-label="Italic"><em>I</em></button>
        <button type="button" class="format-btn" onclick={() => applyMark('strike')} aria-label="Strikethrough">S</button>
        <button type="button" class="format-btn code" onclick={() => applyMark('code')} aria-label="Code">{'</>'}</button>
        <span class="format-hint">
          {connectorId === 'messenger' || connectorId === 'instagram'
            ? 'Formatting will be sent as plain text on this network.'
            : connectorId === 'email'
              ? 'Formatting is kept as lightweight text markup.'
              : 'Formatting markers will be preserved for supported networks.'}
        </span>
      </div>
      <div class="composer-inner">
        <textarea
          bind:this={composerEl}
          placeholder="Type a message…"
          rows="1"
          value={draft}
          oninput={(e) => ondraft(e.currentTarget.value)}
          onkeydown={handleKeydown}
          aria-label="Message input"
        ></textarea>
        <button class="send-btn" onclick={onsend} disabled={!draft.trim()} aria-label="Send">
          <svg width="18" height="18" viewBox="0 0 24 24" fill="currentColor">
            <path d="M2.01 21L23 12 2.01 3 2 10l15 2-15 2z"/>
          </svg>
        </button>
        {#if onsendlater}
          <button class="later-btn" type="button" onclick={onsendlater} disabled={!draft.trim()}>
            Later
          </button>
        {/if}
      </div>
    </footer>
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
  }

  .thread-header {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 14px 20px;
    border-bottom: 1px solid var(--border-subtle);
    background: var(--bg-panel);
    flex-shrink: 0;
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
    flex: 1;
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
    margin-left: auto;
    border: none;
    background: transparent;
    color: var(--text-muted);
    cursor: pointer;
    padding: 8px;
    border-radius: var(--radius-sm);
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

  .msg-row {
    display: flex;
    justify-content: flex-start;
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
    padding: 9px 13px 7px;
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
    background: linear-gradient(135deg, #3b82f6 0%, #2563eb 55%, #1d4ed8 100%);
    color: white;
    box-shadow: 0 1px 3px rgba(37, 99, 235, 0.35);
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
    padding-bottom: 5px;
  }

  .msg-row.group-first:not(.group-single) .bubble,
  .msg-row.group-middle .bubble {
    padding-top: 5px;
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
    font-size: 14px;
    line-height: 1.45;
    word-wrap: break-word;
    white-space: pre-wrap;
    letter-spacing: -0.01em;
  }

  .outbound .bubble p {
    color: rgba(255, 255, 255, 0.95);
  }

  .msg-time {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 4px;
    font-size: 10px;
    color: var(--text-muted);
    margin-top: 4px;
    letter-spacing: 0.02em;
  }

  .outbound .msg-time {
    color: rgba(255, 255, 255, 0.55);
  }

  .status {
    font-size: 10px;
    opacity: 0.7;
  }

  .status.read {
    color: #7dd3fc;
    opacity: 1;
    text-shadow: 0 0 6px rgba(125, 211, 252, 0.5);
  }

  .composer {
    padding: 12px 20px 18px;
    border-top: 1px solid var(--border-subtle);
    background: var(--bg-panel);
    flex-shrink: 0;
  }

  .format-bar {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-bottom: 8px;
    flex-wrap: wrap;
  }

  .format-btn {
    border: 1px solid var(--border);
    background: var(--bg-input);
    color: var(--text);
    border-radius: var(--radius-sm);
    min-width: 30px;
    height: 28px;
    padding: 0 8px;
    cursor: pointer;
  }

  .format-btn.code {
    font-size: 11px;
  }

  .format-hint {
    font-size: 12px;
    color: var(--text-muted);
    margin-left: 4px;
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
    font-size: 14px;
    line-height: 1.45;
    max-height: 120px;
    outline: none;
    padding: 6px 0;
    min-height: 24px;
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
    color: white;
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
    transition: background 0.15s ease, transform 0.1s ease, opacity 0.15s ease;
  }

  .later-btn {
    height: 36px;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: transparent;
    color: var(--text);
    cursor: pointer;
    padding: 0 12px;
    flex-shrink: 0;
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
      min-height: 44px;
    }

    .empty-thread {
      display: none;
    }
  }
</style>
