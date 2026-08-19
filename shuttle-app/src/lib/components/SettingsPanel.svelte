<script lang="ts">
  import NetworkIcon from '$lib/components/NetworkIcon.svelte';
  import { DATETIME_FORMATS, fetchTweakcnTheme, formatTime, openExternal, saveAppConfig } from '$lib/api';
  import {
    ATTRIBUTION_SECTIONS,
    SHUTTLE_LICENSE,
    TRADEMARK_NOTICE,
  } from '$lib/attributions';
  import {
    LEGACY_THEME_PRESETS,
    parseTweakcnId,
    themeDisplayName,
    themeToCss,
  } from '$lib/tweakcn';
  import { applyAppConfig } from '$lib/theme';
  import {
    CONNECTOR_COLORS,
    type Account,
    type AppConfig,
    type Conversation,
    type ConnectorInfo,
    type ForwardRule,
    type PriorityGroup,
    type ScheduledMessage,
    type Workspace,
  } from '$lib/types';

  type SettingsTab =
    | 'appearance'
    | 'notifications'
    | 'privacy'
    | 'channels'
    | 'workspaces'
    | 'accounts'
    | 'routing'
    | 'backup'
    | 'about';

  interface Props {
    accounts: Account[];
    connectors: ConnectorInfo[];
    conversations: Conversation[];
    forwardRules: ForwardRule[];
    scheduledMessages: ScheduledMessage[];
    workspaces: Workspace[];
    priorityGroups: PriorityGroup[];
    config: AppConfig;
    onadd: () => void;
    onconfig: (cfg: AppConfig) => void;
    onaccount: (id: string, action: 'mute' | 'disable' | 'enable' | 'remove' | 'receipts' | 'workspace', extra?: string) => void;
    onworkspace: (name: string) => void;
    ondeleteworkspace: (id: string) => void;
    oncreatepriority: (name: string) => void;
    ondeletepriority: (id: string) => void;
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
    priorityGroups,
    config,
    onadd,
    onconfig,
    onaccount,
    onworkspace,
    ondeleteworkspace,
    oncreatepriority,
    ondeletepriority,
    oncreateforwardrule,
    ontoggleforwardrule,
    ondeleteforwardrule,
    ondeletescheduled,
    onexportbackup,
    onrestorebackup,
    onaccountmenu,
  }: Props = $props();

  let activeTab = $state<SettingsTab>('appearance');
  let newWs = $state('');
  let newPriority = $state('');
  let tweakcnInput = $state('');
  let themeStatus = $state<string | null>(null);
  let themeLoading = $state(false);
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

  const tabs: { id: SettingsTab; label: string }[] = [
    { id: 'appearance', label: 'Appearance' },
    { id: 'notifications', label: 'Notifications' },
    { id: 'privacy', label: 'Privacy' },
    { id: 'channels', label: 'Channels' },
    { id: 'workspaces', label: 'Workspaces & priorities' },
    { id: 'accounts', label: 'Accounts' },
    { id: 'routing', label: 'Routing' },
    { id: 'backup', label: 'Backup' },
    { id: 'about', label: 'About' },
  ];

  function patchAppearance(partial: Partial<AppConfig['appearance']>) {
    onconfig({
      ...config,
      appearance: { ...config.appearance, ...partial },
    });
  }

  async function applyTweakcnTheme() {
    const id = parseTweakcnId(tweakcnInput || config.appearance.theme_id);
    if (!id) {
      themeStatus = 'Enter a tweakcn share URL or theme id.';
      return;
    }
    themeLoading = true;
    themeStatus = null;
    try {
      const theme = await fetchTweakcnTheme(id);
      const css = themeToCss(theme);
      const saved = await saveAppConfig({
        ...config,
        appearance: {
          ...config.appearance,
          theme_id: id,
          tweakcn_css: css,
        },
      });
      applyAppConfig(saved);
      window.location.reload();
    } catch (e) {
      themeStatus = e instanceof Error ? e.message : String(e);
    } finally {
      themeLoading = false;
    }
  }

  async function applyLegacyPreset(presetId: string) {
    const saved = await saveAppConfig({
      ...config,
      appearance: {
        ...config.appearance,
        theme_id: presetId,
        tweakcn_css: null,
      },
    });
    applyAppConfig(saved);
    window.location.reload();
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

  <div class="settings-layout">
    <nav class="settings-nav" aria-label="Settings sections">
      {#each tabs as tab (tab.id)}
        <button
          type="button"
          class="nav-tab"
          class:active={activeTab === tab.id}
          onclick={() => (activeTab = tab.id)}
          aria-current={activeTab === tab.id ? 'page' : undefined}
        >
          {tab.label}
        </button>
      {/each}
    </nav>

    <div class="settings-body">
      {#if activeTab === 'appearance'}
        <section>
          <h2>Appearance</h2>
          <fieldset class="field-group">
            <legend>Color scheme</legend>
            <div class="scheme-row">
              {#each ['system', 'light', 'dark'] as scheme (scheme)}
                <button
                  type="button"
                  class="scheme-btn"
                  class:active={config.appearance.color_scheme === scheme}
                  onclick={() => patchAppearance({ color_scheme: scheme })}
                >
                  {scheme === 'system' ? 'System' : scheme === 'light' ? 'Light' : 'Dark'}
                </button>
              {/each}
            </div>
          </fieldset>
          <label>
            Chat list date and time
            <select
              value={config.appearance.datetime_format || '12h_full'}
              onchange={(e) => patchAppearance({ datetime_format: e.currentTarget.value })}
            >
              {#each DATETIME_FORMATS as fmt (fmt.id)}
                <option value={fmt.id}>{fmt.label} — {formatTime(new Date().toISOString(), fmt.id)}</option>
              {/each}
            </select>
          </label>
          <p class="hint">
            Last-message timestamp in the chat list. Preview uses the current time:
            <strong>{formatTime(new Date().toISOString(), config.appearance.datetime_format || '12h_full')}</strong>
          </p>
          <label>
            Text size
            <input
              type="range"
              min="0.85"
              max="1.35"
              step="0.05"
              value={config.appearance.font_scale ?? 1}
              oninput={(e) => patchAppearance({ font_scale: Number(e.currentTarget.value) })}
            />
          </label>
          <p class="hint">
            {#if (config.appearance.font_scale ?? 1) < 0.95}
              Smaller
            {:else if (config.appearance.font_scale ?? 1) > 1.05}
              Larger
            {:else}
              Default
            {/if}
            · {Math.round((config.appearance.font_scale ?? 1) * 100)}%
          </p>
          <p class="hint">
            Current theme: <strong>{themeDisplayName(config.appearance.theme_id)}</strong>
          </p>
          <label>
            tweakcn theme URL or id
            <input
              bind:value={tweakcnInput}
              placeholder="https://tweakcn.com/r/themes/… or theme id"
              onkeydown={(e) => {
                if (e.key === 'Enter') void applyTweakcnTheme();
              }}
            />
          </label>
          <div class="theme-actions">
            <button type="button" class="apply-theme-btn" disabled={themeLoading} onclick={() => void applyTweakcnTheme()}>
              {themeLoading ? 'Fetching theme…' : 'Apply theme'}
            </button>
          </div>
          {#if themeStatus}
            <p class="hint theme-status">{themeStatus}</p>
          {/if}
          <p class="hint">
            Paste a
            <button type="button" class="linkish" onclick={() => openLink('https://github.com/jnsahaj/tweakcn')}>tweakcn</button>
            share link or id. Shuttle fetches colours and fonts, then reloads the app.
          </p>
          <label>
            Built-in preset (legacy)
            <select
              value={LEGACY_THEME_PRESETS.includes(config.appearance.theme_id as typeof LEGACY_THEME_PRESETS[number])
                ? config.appearance.theme_id
                : ''}
              onchange={(e) => {
                const value = e.currentTarget.value;
                if (value) applyLegacyPreset(value);
              }}
            >
              <option value="">— tweakcn theme active —</option>
              {#each LEGACY_THEME_PRESETS as preset (preset)}
                <option value={preset}>{preset.charAt(0).toUpperCase() + preset.slice(1)}</option>
              {/each}
            </select>
          </label>
        </section>
      {:else if activeTab === 'notifications'}
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
              <label>
                Start
                <input
                  type="time"
                  value={config.notifications.quiet_hours_start}
                  onchange={(e) => patchNotes({ quiet_hours_start: e.currentTarget.value })}
                />
              </label>
              <label>
                End
                <input
                  type="time"
                  value={config.notifications.quiet_hours_end}
                  onchange={(e) => patchNotes({ quiet_hours_end: e.currentTarget.value })}
                />
              </label>
            </div>
          {/if}
          <p class="hint">Muted chats and accounts never notify. Read receipts stay off until you opt in per account or chat.</p>
        </section>
      {:else if activeTab === 'privacy'}
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
      {:else if activeTab === 'channels'}
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
      {:else if activeTab === 'workspaces'}
        <section>
          <h2>Workspaces</h2>
          <p class="hint">Every account starts in the Default workspace. Add more workspaces to filter chats on the main page.</p>
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
              }}>Add workspace</button
            >
          </div>
        </section>

        <section>
          <h2>Priorities</h2>
          <p class="hint">Add priority labels to tag chats. The priority filter appears on the main page after you create at least one.</p>
          {#if priorityGroups.length === 0}
            <p class="hint">No priorities yet.</p>
          {:else}
            <ul>
              {#each priorityGroups as group (group.id)}
                <li>
                  {group.name}
                  <button type="button" class="tiny" onclick={() => ondeletepriority(group.id)}>Remove</button>
                </li>
              {/each}
            </ul>
          {/if}
          <div class="row">
            <input bind:value={newPriority} placeholder="New priority" />
            <button
              type="button"
              onclick={() => {
                if (newPriority.trim()) {
                  oncreatepriority(newPriority.trim());
                  newPriority = '';
                }
              }}>Add priority</button
            >
          </div>
        </section>
      {:else if activeTab === 'accounts'}
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
      {:else if activeTab === 'routing'}
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
      {:else if activeTab === 'backup'}
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
      {:else if activeTab === 'about'}
        <section>
          <h2>About Shuttle</h2>
          <p class="about-lead">
            Shuttle is a local-first unified messaging desktop app. One inbox for WhatsApp, Telegram, Signal, Messenger,
            Instagram, Matrix, and email — on your machine, not in the cloud.
          </p>
          <p class="hint">
            Created by
            <button type="button" class="linkish" onclick={() => openLink('https://shee.se')}>Sheese Sheikh</button>
            ·
            <button type="button" class="linkish" onclick={() => openLink('https://github.com/smsheese')}>GitHub</button>
            · built with Cursor Agent
          </p>
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
      {/if}
    </div>
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
    flex-shrink: 0;
  }
  h1 {
    font-size: 22px;
    font-weight: 700;
    letter-spacing: -0.03em;
  }
  .settings-layout {
    flex: 1;
    min-height: 0;
    display: flex;
  }
  .settings-nav {
    width: 180px;
    flex-shrink: 0;
    border-right: 1px solid var(--border-subtle);
    padding: 12px 8px;
    display: flex;
    flex-direction: column;
    gap: 4px;
    overflow-y: auto;
  }
  .nav-tab {
    border: none;
    background: transparent;
    color: var(--text-muted);
    text-align: left;
    padding: 10px 12px;
    border-radius: var(--radius-sm);
    cursor: pointer;
    font: inherit;
    font-size: 13px;
  }
  .nav-tab:hover {
    background: var(--bg-hover);
    color: var(--text);
  }
  .nav-tab.active {
    background: var(--bg-active);
    color: var(--text);
    font-weight: 600;
  }
  .settings-body {
    flex: 1;
    overflow-y: auto;
    padding: 16px 20px 24px;
    min-width: 0;
  }
  section + section {
    margin-top: 28px;
  }
  h2 {
    font-size: 12px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--text-muted);
    margin-bottom: 12px;
  }
  label, input, button {
    font: inherit;
    color: inherit;
  }
  select {
    font: inherit;
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
    gap: 10px;
    cursor: pointer;
  }
  .field-group {
    border: none;
    padding: 0;
    margin: 0 0 14px;
  }
  .field-group legend {
    font-size: 13px;
    margin-bottom: 8px;
    padding: 0;
  }
  .scheme-row {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
  }
  .scheme-btn {
    border: 1px solid var(--border);
    background: var(--bg-input);
    border-radius: var(--radius-sm);
    padding: 8px 14px;
    cursor: pointer;
    font-size: 13px;
  }
  .scheme-btn.active {
    border-color: var(--accent);
    background: var(--accent-muted);
    color: var(--text);
  }
  .theme-actions {
    margin: 4px 0 8px;
  }
  .apply-theme-btn {
    border: 1px solid var(--border);
    background: var(--accent-muted);
    color: var(--text);
    border-radius: var(--radius-sm);
    padding: 8px 14px;
    cursor: pointer;
    font-size: 13px;
  }
  .apply-theme-btn:disabled {
    opacity: 0.6;
    cursor: wait;
  }
  .theme-status {
    color: #ef4444;
  }
  .channel {
    flex-direction: row;
    align-items: center;
    gap: 8px;
  }
  select, input:not([type='checkbox']):not([type='color']):not([type='range']) {
    padding: 8px;
  }
  input[type='range'] {
    width: 100%;
    padding: 8px 0;
    accent-color: var(--accent);
  }
  select {
    width: 100%;
  }
  input[type='color'] {
    width: 36px;
    height: 28px;
    border: none;
    background: transparent;
    margin-left: auto;
    cursor: pointer;
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
  .hint, .about-lead {
    font-size: 13px;
    color: var(--text-muted);
    line-height: 1.5;
  }
  .about-lead {
    color: var(--text-secondary);
    margin-bottom: 10px;
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
  .acct-actions button, .add-account-btn, .tiny, .stack > button {
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

  @media (max-width: 768px) {
    .settings-layout {
      flex-direction: column;
    }
    .settings-nav {
      width: 100%;
      flex-direction: row;
      overflow-x: auto;
      border-right: none;
      border-bottom: 1px solid var(--border-subtle);
      padding: 8px;
    }
    .nav-tab {
      white-space: nowrap;
      flex-shrink: 0;
    }
  }
</style>
