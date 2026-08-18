<script lang="ts">
  import NetworkIcon from '$lib/components/NetworkIcon.svelte';
  import { openExternal } from '$lib/api';
  import {
    ATTRIBUTION_SECTIONS,
    SHUTTLE_LICENSE,
    TRADEMARK_NOTICE,
  } from '$lib/attributions';
  import {
    CONNECTOR_COLORS,
    THEME_PRESETS,
    type Account,
    type AppConfig,
    type Conversation,
    type ConnectorInfo,
    type ForwardRule,
    type ScheduledMessage,
    type Workspace,
  } from '$lib/types';

  interface Props {
    accounts: Account[];
    connectors: ConnectorInfo[];
    conversations: Conversation[];
    forwardRules: ForwardRule[];
    scheduledMessages: ScheduledMessage[];
    workspaces: Workspace[];
    config: AppConfig;
    onadd: () => void;
    onconfig: (cfg: AppConfig) => void;
    onaccount: (id: string, action: 'mute' | 'disable' | 'enable' | 'remove' | 'receipts' | 'workspace', extra?: string) => void;
    onworkspace: (name: string) => void;
    ondeleteworkspace: (id: string) => void;
    oncreateforwardrule: (draft: {
      source_account_id?: string | null;
      source_conversation_id?: string | null;
      dest_account_id: string;
      dest_conversation_id: string;
      keyword?: string | null;
      prefix?: string | null;
      suffix?: string | null;
      delay_seconds?: number;
    }) => void;
    ontoggleforwardrule: (id: string, enabled: boolean) => void;
    ondeleteforwardrule: (id: string) => void;
    ondeletescheduled: (id: string) => void;
    onexportbackup: (path: string, password: string, includeMessages: boolean) => Promise<void>;
    onrestorebackup: (path: string, password: string) => Promise<void>;
    onaccountmenu?: (account: Account, x: number, y: number) => void;
  }

  let {
    accounts,
    connectors,
    conversations,
    forwardRules,
    scheduledMessages,
    workspaces,
    config,
    onadd,
    onconfig,
    onaccount,
    onworkspace,
    ondeleteworkspace,
    oncreateforwardrule,
    ontoggleforwardrule,
    ondeleteforwardrule,
    ondeletescheduled,
    onexportbackup,
    onrestorebackup,
    onaccountmenu,
  }: Props = $props();

  let newWs = $state('');
  let tweakcn = $state(config.appearance.tweakcn_css ?? '');
  let ruleSourceConversationId = $state('');
  let ruleDestConversationId = $state('');
  let ruleKeyword = $state('');
  let rulePrefix = $state('Forwarded from Shuttle');
  let ruleSuffix = $state('');
  let ruleDelaySeconds = $state('0');
  let backupPath = $state('');
  let backupPassword = $state('');
  let includeMessages = $state(true);
  let restorePath = $state('');
  let restorePassword = $state('');
  let backupStatus = $state<string | null>(null);

  $effect(() => {
    tweakcn = config.appearance.tweakcn_css ?? '';
  });

  function patchAppearance(partial: Partial<AppConfig['appearance']>) {
    onconfig({
      ...config,
      appearance: { ...config.appearance, ...partial },
    });
  }

  function patchNotes(partial: Partial<AppConfig['notifications']>) {
    onconfig({
      ...config,
      notifications: { ...config.notifications, ...partial },
    });
  }

  function patchPrivacy(partial: Partial<AppConfig['privacy']>) {
    onconfig({
      ...config,
      privacy: { ...config.privacy, ...partial },
    });
  }

  function setChannelTag(id: string, tag: string) {
    onconfig({
      ...config,
      channel_styles: {
        ...config.channel_styles,
        [id]: { ...config.channel_styles[id], tag },
      },
    });
  }

  function conversationLabel(conversationId: string): string {
    const conv = conversations.find((c) => c.id === conversationId);
    if (!conv) return conversationId;
    const account = accounts.find((a) => a.id === conv.account_id);
    return account ? `${conv.title} (${account.name})` : conv.title;
  }

  async function openLink(url: string | undefined) {
    if (url) await openExternal(url);
  }
</script>

<div class="settings">
  <header class="settings-header">
    <h1>Settings</h1>
  </header>
  <div class="settings-body">
    <section>
      <h2>Appearance</h2>
      <label>
        Color scheme
        <select
          value={config.appearance.color_scheme}
          onchange={(e) => patchAppearance({ color_scheme: e.currentTarget.value })}
        >
          <option value="system">System</option>
          <option value="dark">Dark</option>
          <option value="light">Light</option>
        </select>
      </label>
      <label>
        Theme
        <select
          value={config.appearance.theme_id}
          onchange={(e) => patchAppearance({ theme_id: e.currentTarget.value, tweakcn_css: null })}
        >
          {#each THEME_PRESETS as p}
            <option value={p.id}>{p.label}</option>
          {/each}
          <option value="custom">Custom (tweakcn)</option>
        </select>
      </label>
      <label>
        Paste a tweakcn theme
        <textarea
          rows="5"
          placeholder="Paste tweakcn root CSS"
          bind:value={tweakcn}
          onblur={() => patchAppearance({ tweakcn_css: tweakcn.trim() || null, theme_id: tweakcn.trim() ? 'custom' : config.appearance.theme_id })}
        ></textarea>
      </label>
    </section>

    <section>
      <h2>Notifications</h2>
      <label class="check">
        <input
          type="checkbox"
          checked={config.notifications.enabled}
          onchange={(e) => patchNotes({ enabled: e.currentTarget.checked })}
        />
        Desktop notifications
      </label>
      <label class="check">
        <input
          type="checkbox"
          checked={config.notifications.quiet_hours_enabled}
          onchange={(e) => patchNotes({ quiet_hours_enabled: e.currentTarget.checked })}
        />
        Quiet hours
      </label>
      {#if config.notifications.quiet_hours_enabled}
        <div class="row">
          <input
            type="time"
            value={config.notifications.quiet_hours_start}
            onchange={(e) => patchNotes({ quiet_hours_start: e.currentTarget.value })}
          />
          <input
            type="time"
            value={config.notifications.quiet_hours_end}
            onchange={(e) => patchNotes({ quiet_hours_end: e.currentTarget.value })}
          />
        </div>
      {/if}
      <p class="hint">Muted chats and accounts never notify. Read receipts stay off until you opt in per account or chat.</p>
    </section>

    <section>
      <h2>Privacy</h2>
      <p class="hint">Anonymous diagnostics are off by default. No message content, contacts, or account identifiers are ever sent.</p>
      <h3 class="privacy-subhead">Anonymous diagnostics</h3>
      <label class="check">
        <input
          type="checkbox"
          checked={config.privacy?.crash_reports ?? false}
          onchange={(e) => patchPrivacy({ crash_reports: e.currentTarget.checked })}
        />
        Send anonymous crash reports
      </label>
      <label class="check">
        <input
          type="checkbox"
          checked={config.privacy?.usage_diagnostics ?? false}
          onchange={(e) => patchPrivacy({ usage_diagnostics: e.currentTarget.checked })}
        />
        Send anonymous usage and performance diagnostics
      </label>
    </section>

    <section>
      <h2>Channel colours</h2>
      {#each connectors as c (c.id)}
        <label class="channel">
          <NetworkIcon connectorId={c.id} size={14} />
          {c.name}
          <input
            type="color"
            value={config.channel_styles[c.id]?.tag ?? CONNECTOR_COLORS[c.id] ?? '#888888'}
            onchange={(e) => setChannelTag(c.id, e.currentTarget.value)}
          />
        </label>
      {/each}
    </section>

    <section>
      <h2>Workspaces</h2>
      <ul>
        {#each workspaces as ws (ws.id)}
          <li>
            {ws.name}
            {#if !ws.builtin}
              <button type="button" class="tiny" onclick={() => ondeleteworkspace(ws.id)}>Remove</button>
            {/if}
          </li>
        {/each}
      </ul>
      <div class="row">
        <input bind:value={newWs} placeholder="New workspace" />
        <button
          type="button"
          onclick={() => {
            if (newWs.trim()) {
              onworkspace(newWs.trim());
              newWs = '';
            }
          }}>Add</button
        >
      </div>
    </section>

    <section>
      <h2>Accounts</h2>
      <ul class="account-list">
        {#each accounts as account (account.id)}
          <li
            class="account-item"
            class:disabled={account.disabled}
            oncontextmenu={(e) => {
              e.preventDefault();
              onaccountmenu?.(account, e.clientX, e.clientY);
            }}
          >
            <span class="account-icon" style="background: {config.channel_styles[account.connector_id]?.tag ?? CONNECTOR_COLORS[account.connector_id] ?? '#888'}">
              <NetworkIcon connectorId={account.connector_id} size={18} />
            </span>
            <div class="account-info">
              <span class="account-name">{account.name}{account.muted ? ' (muted)' : ''}{account.disabled ? ' (disabled)' : ''}</span>
              {#if account.identity}
                <span class="account-identity">{account.identity}</span>
              {/if}
            </div>
            <div class="acct-actions">
              <button type="button" onclick={() => onaccount(account.id, 'mute')}>{account.muted ? 'Unmute' : 'Mute'}</button>
              <button type="button" onclick={() => onaccount(account.id, account.disabled ? 'enable' : 'disable')}>
                {account.disabled ? 'Enable' : 'Disable'}
              </button>
              <button type="button" onclick={() => onaccount(account.id, 'receipts')}>
                Receipts {account.send_receipts ? 'on' : 'off'}
              </button>
              <button type="button" class="danger" onclick={() => onaccount(account.id, 'remove')}>Remove</button>
            </div>
          </li>
        {/each}
      </ul>
      <button class="add-account-btn" onclick={onadd} type="button">Add account</button>
    </section>

    <section>
      <h2>Forwarding rules</h2>
      <div class="stack">
        <label>
          Source conversation
          <select bind:value={ruleSourceConversationId}>
            <option value="">Any conversation</option>
            {#each conversations as conv (conv.id)}
              <option value={conv.id}>{conversationLabel(conv.id)}</option>
            {/each}
          </select>
        </label>
        <label>
          Destination conversation
          <select bind:value={ruleDestConversationId}>
            <option value="">Select destination</option>
            {#each conversations as conv (conv.id)}
              <option value={conv.id}>{conversationLabel(conv.id)}</option>
            {/each}
          </select>
        </label>
        <label>
          Keyword filter
          <input bind:value={ruleKeyword} placeholder="Optional keyword" />
        </label>
        <div class="row">
          <label>
            Prefix
            <input bind:value={rulePrefix} placeholder="Optional prefix" />
          </label>
          <label>
            Delay seconds
            <input bind:value={ruleDelaySeconds} type="number" min="0" />
          </label>
        </div>
        <label>
          Suffix
          <input bind:value={ruleSuffix} placeholder="Optional suffix" />
        </label>
        <button
          type="button"
          onclick={() => {
            const dest = conversations.find((c) => c.id === ruleDestConversationId);
            if (!dest) return;
            oncreateforwardrule({
              source_conversation_id: ruleSourceConversationId || null,
              source_account_id:
                conversations.find((c) => c.id === ruleSourceConversationId)?.account_id ?? null,
              dest_account_id: dest.account_id,
              dest_conversation_id: dest.id,
              keyword: ruleKeyword.trim() || null,
              prefix: rulePrefix.trim() || null,
              suffix: ruleSuffix.trim() || null,
              delay_seconds: Number(ruleDelaySeconds || '0'),
            });
            ruleKeyword = '';
            ruleSuffix = '';
          }}>Add forwarding rule</button
        >
      </div>
      <ul>
        {#each forwardRules as rule (rule.id)}
          <li class="rule-item">
            <div class="rule-copy">
              <strong>{rule.source_conversation_id ? conversationLabel(rule.source_conversation_id) : 'Any chat'}</strong>
              <span>→ {conversationLabel(rule.dest_conversation_id)}</span>
              {#if rule.keyword}
                <span class="account-identity">keyword: {rule.keyword}</span>
              {/if}
              {#if rule.delay_seconds}
                <span class="account-identity">delay: {rule.delay_seconds}s</span>
              {/if}
            </div>
            <div class="acct-actions">
              <button type="button" onclick={() => ontoggleforwardrule(rule.id, !rule.enabled)}>
                {rule.enabled ? 'Disable' : 'Enable'}
              </button>
              <button type="button" class="danger" onclick={() => ondeleteforwardrule(rule.id)}>Delete</button>
            </div>
          </li>
        {/each}
      </ul>
    </section>

    <section>
      <h2>Scheduled messages</h2>
      <ul>
        {#each scheduledMessages as msg (msg.id)}
          <li class="rule-item">
            <div class="rule-copy">
              <strong>{conversationLabel(msg.dest_conversation_id)}</strong>
              <span>{new Date(msg.send_at).toLocaleString()}</span>
              <span class="account-identity">{msg.body}</span>
            </div>
            <div class="acct-actions">
              <button type="button" class="danger" onclick={() => ondeletescheduled(msg.id)}>Delete</button>
            </div>
          </li>
        {/each}
      </ul>
    </section>

    <section>
      <h2>Backup</h2>
      <label>
        Export path
        <input bind:value={backupPath} placeholder="/path/to/shuttle-backup.age" />
      </label>
      <label>
        Password
        <input bind:value={backupPassword} type="password" placeholder="Backup password" />
      </label>
      <label class="check">
        <input type="checkbox" bind:checked={includeMessages} />
        Include inbox databases
      </label>
      <button
        type="button"
        onclick={async () => {
          await onexportbackup(backupPath, backupPassword, includeMessages);
          backupStatus = 'Backup exported.';
        }}>Export backup</button
      >
      <label>
        Restore path
        <input bind:value={restorePath} placeholder="/path/to/shuttle-backup.age" />
      </label>
      <label>
        Restore password
        <input bind:value={restorePassword} type="password" placeholder="Backup password" />
      </label>
      <button
        type="button"
        onclick={async () => {
          await onrestorebackup(restorePath, restorePassword);
          backupStatus = 'Backup restored. Restart Shuttle to reload databases safely.';
        }}>Restore backup</button
      >
      {#if backupStatus}
        <p class="hint">{backupStatus}</p>
      {/if}
    </section>

    <section>
      <h2>Attributions</h2>
      <div class="attribution-block">
        <h3>{SHUTTLE_LICENSE.name}</h3>
        <p class="hint">{SHUTTLE_LICENSE.summary}</p>
        <p class="attribution-meta">
          <button type="button" class="linkish" onclick={() => openLink(SHUTTLE_LICENSE.url)}>
            {SHUTTLE_LICENSE.license}
          </button>
          ·
          <button type="button" class="linkish" onclick={() => openLink(SHUTTLE_LICENSE.sourceUrl)}>
            Source
          </button>
        </p>
      </div>

      {#each ATTRIBUTION_SECTIONS as section (section.title)}
        <div class="attribution-block">
          <h3>{section.title}</h3>
          <ul class="attribution-list">
            {#each section.entries as entry (entry.name)}
              <li class="attribution-item">
                <div class="attribution-head">
                  {#if entry.url}
                    <button type="button" class="linkish name" onclick={() => openLink(entry.url)}>
                      {entry.name}
                    </button>
                  {:else}
                    <span class="name">{entry.name}</span>
                  {/if}
                  <span class="attribution-license">{entry.license}</span>
                </div>
                <span class="account-identity">{entry.role}</span>
                {#if entry.notes}
                  <span class="account-identity">{entry.notes}</span>
                {/if}
              </li>
            {/each}
          </ul>
        </div>
      {/each}

      <p class="hint trademark">{TRADEMARK_NOTICE}</p>
      <p class="hint">Full license texts ship with release builds under <code>licenses/</code> in the app bundle.</p>
    </section>
  </div>
</div>

<style>
  .settings {
    display: flex;
    flex-direction: column;
    height: 100%;
    background: var(--bg-panel);
    min-width: 0;
    flex: 1;
  }
  .settings-header {
    padding: 20px 16px 14px;
    border-bottom: 1px solid var(--border-subtle);
  }
  h1 {
    font-size: 22px;
    font-weight: 700;
    letter-spacing: -0.03em;
  }
  .settings-body {
    flex: 1;
    overflow-y: auto;
    padding: 16px;
    display: flex;
    flex-direction: column;
    gap: 24px;
  }
  h2 {
    font-size: 12px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--text-muted);
    margin-bottom: 12px;
  }
  label, select, textarea, input, button {
    font: inherit;
    color: inherit;
  }
  label {
    display: flex;
    flex-direction: column;
    gap: 6px;
    font-size: 13px;
    margin-bottom: 10px;
  }
  .check {
    flex-direction: row;
    align-items: center;
  }
  .channel {
    flex-direction: row;
    align-items: center;
    gap: 8px;
  }
  select, textarea, input:not([type='checkbox']):not([type='color']) {
    background: var(--bg-input);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    padding: 8px;
  }
  input[type='color'] {
    width: 36px;
    height: 28px;
    border: none;
    background: transparent;
    margin-left: auto;
  }
  .row {
    display: flex;
    gap: 8px;
  }
  .stack {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .row > * {
    flex: 1;
  }
  .hint {
    font-size: 12px;
    color: var(--text-muted);
  }
  .account-list, ul {
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .account-item {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 12px;
    padding: 12px;
    background: var(--bg-input);
    border-radius: var(--radius-md);
  }
  .rule-item {
    display: flex;
    justify-content: space-between;
    gap: 12px;
    align-items: flex-start;
    padding: 12px;
    background: var(--bg-input);
    border-radius: var(--radius-md);
  }
  .rule-copy {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }
  .account-item.disabled {
    opacity: 0.65;
  }
  .account-icon {
    width: 40px;
    height: 40px;
    border-radius: var(--radius-md);
    display: flex;
    align-items: center;
    justify-content: center;
    color: white;
  }
  .account-info {
    flex: 1;
    min-width: 0;
  }
  .account-name {
    font-weight: 600;
    font-size: 14px;
  }
  .account-identity {
    display: block;
    font-size: 12px;
    color: var(--text-muted);
  }
  .acct-actions {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    width: 100%;
  }
  .acct-actions button, .add-account-btn, .tiny {
    border: 1px solid var(--border);
    background: transparent;
    border-radius: var(--radius-sm);
    padding: 6px 10px;
    cursor: pointer;
    font-size: 12px;
  }
  .danger {
    color: #ef4444;
  }
  .add-account-btn {
    width: 100%;
    min-height: 44px;
    border-style: dashed;
  }
  .attribution-block {
    display: flex;
    flex-direction: column;
    gap: 8px;
    margin-bottom: 16px;
  }
  .attribution-block h3 {
    font-size: 14px;
    font-weight: 600;
    color: var(--text-primary, inherit);
    text-transform: none;
    letter-spacing: normal;
    margin: 0;
  }
  .privacy-subhead {
    font-size: 13px;
    font-weight: 600;
    margin: 4px 0 8px;
    color: inherit;
    text-transform: none;
    letter-spacing: normal;
  }
  .attribution-list {
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 0;
    margin: 0;
  }
  .attribution-item {
    padding: 10px 12px;
    background: var(--bg-input);
    border-radius: var(--radius-md);
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .attribution-head {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    gap: 8px;
    flex-wrap: wrap;
  }
  .attribution-head .name {
    font-weight: 600;
    font-size: 13px;
  }
  .attribution-license {
    font-size: 11px;
    color: var(--text-muted);
    white-space: nowrap;
  }
  .attribution-meta {
    font-size: 12px;
    color: var(--text-muted);
  }
  .linkish {
    border: none;
    background: none;
    padding: 0;
    color: var(--accent, #3b82f6);
    cursor: pointer;
    font: inherit;
    text-align: left;
  }
  .linkish:hover {
    text-decoration: underline;
  }
  .trademark {
    margin-top: 4px;
  }
  code {
    font-size: 0.92em;
  }
</style>
