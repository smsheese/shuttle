<script lang="ts">
  import { tick } from 'svelte';
  import NetworkIcon from '$lib/components/NetworkIcon.svelte';
  import { clearMediaCache, DATETIME_FORMATS, fetchTweakcnTheme, formatTime, mediaCacheInfo, openExternal, restartApp, saveAppConfig, telemetryInstallationId } from '$lib/api';
  import { AUTO_DOWNLOAD_DEFAULTS } from '$lib/mediaSave';
  import { formatFileSize } from '$lib/messageMedia';
  import { isTauri } from '$lib/mock';
  import { save, open } from '@tauri-apps/plugin-dialog';
  import { shortcutModKey } from '$lib/shortcuts';
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
    type AccountPatch,
    type AppConfig,
    type AutoDownloadGroup,
    type MediaCacheInfo,
    type MediaConfig,
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
    | 'media'
    | 'routing'
    | 'backup'
    | 'keyboard'
    | 'feedback'
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
    onaccountpatch?: (id: string, patch: AccountPatch) => void;
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
    onexportbackup: (
      path: string,
      password: string,
      includeMessages: boolean,
      includeMedia: boolean
    ) => Promise<void>;
    onrestorebackup: (path: string, password: string) => Promise<void>;
    onaccountmenu?: (account: Account, x: number, y: number) => void;
    /** Open the tab that holds `field` and focus it (a new `at` asks again). */
    focus?: { field: 'openrouter-key'; at: number } | null;
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
    onaccountpatch,
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
    focus = null,
  }: Props = $props();

  let activeTab = $state<SettingsTab>('appearance');
  let newWs = $state('');
  let newPriority = $state('');
  let tweakcnInput = $state('');
  let locationQuery = $state('');
  let locationLoading = $state(false);
  let locationStatus = $state('');
  let locations = $state<{name: string; country?: string; admin1?: string; latitude: number; longitude: number}[]>([]);

  async function searchLocation() {
    if (locationQuery.trim().length < 2) { locationStatus = 'Enter at least two characters.'; return; }
    locationLoading = true; locationStatus = ''; locations = [];
    try {
      const response = await fetch(`https://geocoding-api.open-meteo.com/v1/search?name=${encodeURIComponent(locationQuery.trim())}&count=8&language=en&format=json`);
      if (!response.ok) throw new Error('Location search failed.');
      const result = await response.json();
      locations = result.results ?? [];
      if (!locations.length) locationStatus = 'No locations found.';
    } catch { locationStatus = 'Could not search locations. Check your connection.'; }
    finally { locationLoading = false; }
  }


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
  let includeMedia = $state(false);
  let restorePath = $state('');
  let restorePassword = $state('');
  let backupStatus = $state<string | null>(null);
  let restoreNeedsRestart = $state(false);

  async function browseExportPath() {
    if (!isTauri()) return;
    const picked = await save({
      defaultPath: 'shuttle-backup.age',
      filters: [{ name: 'Age backup', extensions: ['age'] }],
    });
    if (picked) backupPath = picked;
  }

  async function browseRestorePath() {
    if (!isTauri()) return;
    const picked = await open({
      filters: [{ name: 'Age backup', extensions: ['age'] }],
      multiple: false,
    });
    if (typeof picked === 'string') restorePath = picked;
  }

  const tabs: { id: SettingsTab; label: string }[] = [
    { id: 'appearance', label: 'Appearance' },
    { id: 'notifications', label: 'Notifications' },
    { id: 'privacy', label: 'Privacy' },
    { id: 'channels', label: 'Channels' },
    { id: 'workspaces', label: 'Workspaces & priorities' },
    { id: 'accounts', label: 'Accounts' },
    { id: 'media', label: 'Media & storage' },
    { id: 'routing', label: 'Routing' },
    { id: 'backup', label: 'Backup' },
    { id: 'keyboard', label: 'Keyboard' },
    { id: 'feedback', label: 'Feedback' },
    { id: 'about', label: 'About' },
  ];

  const FEEDBACK_FORM_URL = 'https://forms.tfs.qa/forms/contact-form-xj6nhk';
  let feedbackOpen = $state(false);
  let installationId = $state('');
  $effect(() => {
    if (activeTab === 'about' && !installationId) void telemetryInstallationId().then((id) => (installationId = id));
  });

  const modKey = shortcutModKey();

  function patchAppearance(partial: Partial<AppConfig['appearance']>) {
    onconfig({
      ...config,
      appearance: { ...config.appearance, ...partial },
    });
  }

  async function applyTweakcnTheme() {
    if (themeLoading) return;
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

  const sleepDefaults = { enabled: false, after_minutes: 5, check_minutes: 15 };

  const sleepCfg = $derived(config.sleep ?? sleepDefaults);

  function patchSleep(partial: Partial<AppConfig['sleep']>) {
    onconfig({
      ...config,
      sleep: { ...(config.sleep ?? sleepDefaults), ...partial },
    });
  }

  let openrouterKeyEl = $state<HTMLInputElement | null>(null);

  $effect(() => {
    if (focus?.field !== 'openrouter-key') return;
    activeTab = 'privacy';
    void tick().then(() => openrouterKeyEl?.focus());
  });

  function patchAi(partial: Partial<NonNullable<AppConfig['ai']>>) {
    onconfig({
      ...config,
      ai: { openrouter_api_key: '', model: '', ...config.ai, ...partial },
    });
  }

  function patchIntegrations(partial: Partial<NonNullable<AppConfig['integrations']>>) {
    onconfig({
      ...config,
      integrations: { giphy_api_key: '', ...config.integrations, ...partial },
    });
  }

  function patchPrivacy(partial: Partial<AppConfig['privacy']>) {
    onconfig({
      ...config,
      privacy: { ...config.privacy, ...partial },
    });
  }

  function accountSleepMode(account: Account): 'default' | 'always' | 'custom' {
    if (account.sleep_enabled === false) return 'always';
    if (account.sleep_enabled === true) return 'custom';
    return 'default';
  }

  function setAccountSleepMode(account: Account, mode: string) {
    if (!onaccountpatch) return;
    if (mode === 'default') {
      onaccountpatch(account.id, { clear_sleep_enabled: true, clear_sleep_after: true, clear_sleep_check: true });
    } else if (mode === 'always') {
      onaccountpatch(account.id, { sleep_enabled: false });
    } else {
      onaccountpatch(account.id, {
        sleep_enabled: true,
        sleep_after_minutes: account.sleep_after_minutes ?? config.sleep?.after_minutes ?? 5,
        sleep_check_minutes: account.sleep_check_minutes ?? config.sleep?.check_minutes ?? 15,
      });
    }
  }


  // ---- Media & storage -------------------------------------------------------------------
  const mediaDefaults: MediaConfig = { save_dir: null, ask_each_time: false, keep_days: 60 };
  const mediaCfg = $derived(config.media ?? mediaDefaults);
  let cacheInfo = $state<MediaCacheInfo | null>(null);
  let cacheStatus = $state('');
  let clearingCache = $state(false);
  const AUTO_DOWNLOAD_GROUPS: { group: AutoDownloadGroup; label: string }[] = [
    { group: 'images', label: 'Images' },
    { group: 'videos', label: 'Videos' },
    { group: 'files', label: 'Files' },
  ];

  function patchMedia(partial: Partial<MediaConfig>) {
    onconfig({ ...config, media: { ...mediaCfg, ...partial } });
  }

  async function refreshCacheInfo() {
    try {
      cacheInfo = await mediaCacheInfo();
    } catch {
      cacheInfo = null;
    }
  }

  async function chooseSaveFolder() {
    if (!isTauri()) return;
    const picked = await open({
      directory: true,
      multiple: false,
      defaultPath: mediaCfg.save_dir || cacheInfo?.save_dir || undefined,
      title: 'Choose where saved media goes',
    });
    if (typeof picked === 'string' && picked) {
      patchMedia({ save_dir: picked });
      setTimeout(() => void refreshCacheInfo(), 300);
    }
  }

  function resetSaveFolder() {
    patchMedia({ save_dir: null });
    setTimeout(() => void refreshCacheInfo(), 300);
  }

  function setKeepDays(raw: string) {
    const n = Math.floor(Number(raw));
    if (Number.isFinite(n) && n >= 1) patchMedia({ keep_days: n });
  }

  async function clearCacheNow() {
    if (!confirm('Delete all downloaded media from Shuttle’s cache? Saved files are not touched, and anything still on WhatsApp can be downloaded again.')) return;
    clearingCache = true;
    cacheStatus = '';
    try {
      const done = await clearMediaCache();
      cacheStatus = done.files > 0 ? `Removed ${done.files} file${done.files === 1 ? '' : 's'} (${formatFileSize(done.bytes)}).` : 'Nothing to clear.';
    } catch (e) {
      cacheStatus = e instanceof Error ? e.message : String(e);
    } finally {
      clearingCache = false;
      void refreshCacheInfo();
    }
  }

  function accountAutoDownload(account: Account, group: AutoDownloadGroup): boolean {
    return account.auto_download?.[group] ?? AUTO_DOWNLOAD_DEFAULTS[group];
  }

  function setAccountAutoDownload(account: Account, group: AutoDownloadGroup, on: boolean) {
    onaccountpatch?.(account.id, { auto_download: { ...(account.auto_download ?? {}), [group]: on } });
  }

  $effect(() => {
    if (activeTab === 'media') void refreshCacheInfo();
  });

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
              placeholder="https://tweakcn.com/themes/… or theme id"
              disabled={themeLoading}
              onkeydown={(e) => {
                if (e.key === 'Enter') void applyTweakcnTheme();
              }}
            />
          </label>
          <div class="theme-actions">
            <button type="button" class="apply-theme-btn" disabled={themeLoading} onclick={() => void applyTweakcnTheme()}>
              {themeLoading ? 'Applying theme…' : 'Apply theme'}
            </button>
          </div>
          <p class:theme-status={!!themeStatus} class="hint" role="status" aria-live="polite">
            {themeLoading ? 'Fetching and applying theme…' : themeStatus ?? ''}
          </p>
          <p class="hint">
            Paste a
            <button type="button" class="linkish" onclick={() => openLink('https://github.com/jnsahaj/tweakcn')}>tweakcn</button>
            share link or id. Shuttle fetches colours and fonts, then reloads the app.
          </p>
          <label>
            Dark mode schedule
            <select value={config.appearance.dark_schedule ?? (config.appearance.color_scheme === 'dark' ? 'always' : config.appearance.color_scheme === 'light' ? 'off' : 'system')} onchange={(e) => patchAppearance({ dark_schedule: e.currentTarget.value as NonNullable<AppConfig['appearance']['dark_schedule']> })}>
              <option value="off">Off</option><option value="always">Always dark</option><option value="system">Follow system</option><option value="time">Custom times</option><option value="sun">Sunset to sunrise</option>
            </select>
          </label>
          {#if config.appearance.dark_schedule === 'time'}
            <label>Dark from <input type="time" value={config.appearance.dark_from ?? '18:00'} onchange={(e) => patchAppearance({ dark_from: e.currentTarget.value })} /></label>
            <label>Until <input type="time" value={config.appearance.dark_to ?? '06:00'} onchange={(e) => patchAppearance({ dark_to: e.currentTarget.value })} /></label>
            <p class="hint">Times use this device’s local time.</p>
          {:else if config.appearance.dark_schedule === 'sun'}
            <label>Search for a location <input bind:value={locationQuery} placeholder="City or town" onkeydown={(e) => { if (e.key === 'Enter') { e.preventDefault(); void searchLocation(); } }} /></label>
            <button type="button" class="apply-theme-btn" disabled={locationLoading} onclick={() => void searchLocation()}>{locationLoading ? 'Searching…' : 'Find location'}</button>
            <p class="hint" role="status">{locationStatus || (config.appearance.dark_location ? `Selected: ${config.appearance.dark_location}` : 'Choose a location for sunset and sunrise.')}</p>
            {#each locations as location}
              <button type="button" class="apply-theme-btn" onclick={() => { patchAppearance({ dark_location: [location.name, location.admin1, location.country].filter(Boolean).join(', '), dark_latitude: location.latitude, dark_longitude: location.longitude }); locations = []; }}>{[location.name, location.admin1, location.country].filter(Boolean).join(', ')}</button>
            {/each}
            <p class="hint">Location search uses Open-Meteo. Sunrise and sunset are calculated on this device.</p>
          {/if}
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
          <h3 class="privacy-subhead">Account sleep</h3>
          <label class="check">
            <input
              type="checkbox"
              checked={sleepCfg.enabled}
              onchange={(e) => patchSleep({ enabled: e.currentTarget.checked })}
            />
            Sleep idle accounts
          </label>
          {#if sleepCfg.enabled}
            <label>
              Idle timeout
              <select
                value={sleepCfg.after_minutes}
                onchange={(e) => patchSleep({ after_minutes: Number(e.currentTarget.value) })}
              >
                <option value={5}>5 minutes</option>
                <option value={15}>15 minutes</option>
                <option value={30}>30 minutes</option>
              </select>
            </label>
          {/if}
          <label>
            Check while asleep
            <select
              value={sleepCfg.check_minutes}
              onchange={(e) => patchSleep({ check_minutes: Number(e.currentTarget.value) })}
            >
              <option value={0}>Never</option>
              <option value={15}>Every 15 minutes</option>
              <option value={30}>Every 30 minutes</option>
            </select>
          </label>
          <p class="hint">
            Off by default. Enable globally here, or per account under Accounts. Sleeping accounts stop their connector to save RAM.
          </p>
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
          <h3 class="privacy-subhead">AI and translation (OpenRouter)</h3>
          <p class="hint">
            Off until you add your own OpenRouter key. Translate sends only the message you pick to
            OpenRouter and the model you choose. Translations are not saved. The same key will power
            other AI features later. Get a key at openrouter.ai/keys.
          </p>
          <label>
            OpenRouter API key
            <input
              bind:this={openrouterKeyEl}
              type="password"
              autocomplete="off"
              placeholder="sk-or-..."
              value={config.ai?.openrouter_api_key ?? ''}
              onchange={(e) => patchAi({ openrouter_api_key: e.currentTarget.value.trim() })}
            />
          </label>
          <label>
            Model id
            <input
              type="text"
              autocomplete="off"
              placeholder="google/gemini-2.5-flash"
              value={config.ai?.model ?? ''}
              onchange={(e) => patchAi({ model: e.currentTarget.value.trim() })}
            />
          </label>
          <h3 class="privacy-subhead">GIFs and stickers (Giphy)</h3>
          <p class="hint">
            Search needs your own free Giphy key from developers.giphy.com. Typed searches go to
            Giphy.
          </p>
          <label>
            Giphy API key
            <input
              type="password"
              autocomplete="off"
              value={config.integrations?.giphy_api_key ?? ''}
              onchange={(e) => patchIntegrations({ giphy_api_key: e.currentTarget.value.trim() })}
            />
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
          <p class="hint">Hibernation is off by default (Notifications). Enable globally or set a custom schedule per account below.</p>
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
                  <span class="account-name">{account.name}{account.muted ? ' (muted)' : ''}{account.disabled ? ' (disabled)' : ''}{account.status === 'sleeping' ? ' (asleep)' : ''}</span>
                  {#if account.identity}
                    <span class="account-identity">{account.identity}</span>
                  {/if}
                  <label class="sleep-mode">
                    Hibernate
                    <select
                      value={accountSleepMode(account)}
                      onchange={(e) => setAccountSleepMode(account, e.currentTarget.value)}
                    >
                      <option value="default">Use default</option>
                      <option value="always">Always on</option>
                      <option value="custom">Custom</option>
                    </select>
                  </label>
                  {#if accountSleepMode(account) === 'custom'}
                    <div class="row">
                      <label>
                        After
                        <input
                          type="number"
                          min="1"
                          value={account.sleep_after_minutes ?? config.sleep?.after_minutes ?? 5}
                          onchange={(e) =>
                            onaccountpatch?.(account.id, {
                              sleep_after_minutes: Math.max(1, Number(e.currentTarget.value) || 5),
                            })}
                        />
                      </label>
                      <label>
                        Check
                        <input
                          type="number"
                          min="0"
                          value={account.sleep_check_minutes ?? config.sleep?.check_minutes ?? 15}
                          onchange={(e) =>
                            onaccountpatch?.(account.id, {
                              sleep_check_minutes: Math.max(0, Number(e.currentTarget.value) || 0),
                            })}
                        />
                      </label>
                    </div>
                  {/if}
                </div>
                <div class="acct-actions">
                  <select
                    class="workspace-select"
                    value={account.workspace_id ?? 'default'}
                    onchange={(e) => onaccount(account.id, 'workspace', e.currentTarget.value)}
                  >
                    {#each workspaces as ws (ws.id)}
                      <option value={ws.id}>{ws.name}</option>
                    {/each}
                  </select>
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
      {:else if activeTab === 'media'}
        <section>
          <h2>Media &amp; storage</h2>
          <p class="hint">
            Downloaded media lives in a private cache inside Shuttle's data folder. A file only reaches
            your save folder when you save it.
          </p>

          <h3 class="privacy-subhead">Saving</h3>
          <label>
            Save folder
            <div class="path-row">
              <input readonly value={mediaCfg.save_dir || cacheInfo?.save_dir || 'Documents/shuttle'} />
              {#if isTauri()}
                <button type="button" class="tiny" onclick={() => void chooseSaveFolder()}>Choose…</button>
              {/if}
              {#if mediaCfg.save_dir}
                <button type="button" class="tiny" onclick={resetSaveFolder}>Default</button>
              {/if}
            </div>
          </label>
          <label class="check">
            <input
              type="checkbox"
              checked={mediaCfg.ask_each_time}
              onchange={(e) => patchMedia({ ask_each_time: e.currentTarget.checked })}
            />
            Ask where to save each time
          </label>
          <p class="hint">Saving never overwrites: a name that already exists becomes “name (1)”.</p>

          <h3 class="privacy-subhead">Downloaded media</h3>
          <label class="check">
            <input
              type="checkbox"
              checked={mediaCfg.keep_days === null}
              onchange={(e) => patchMedia({ keep_days: e.currentTarget.checked ? null : 60 })}
            />
            Keep downloaded media forever
          </label>
          {#if mediaCfg.keep_days !== null}
            <label>
              Keep downloaded media for (days)
              <input
                type="number"
                min="1"
                step="1"
                value={mediaCfg.keep_days}
                onchange={(e) => setKeepDays(e.currentTarget.value)}
              />
            </label>
          {/if}
          <p class="hint">
            Media you have not opened for this long is removed from the cache; the chat keeps its preview and a
            Download button. Anything you saved is never deleted.
          </p>
          <div class="path-row">
            <span>
              Cache size:
              {#if cacheInfo}
                {formatFileSize(cacheInfo.bytes) || '0 B'} · {cacheInfo.files} file{cacheInfo.files === 1 ? '' : 's'}
              {:else}
                …
              {/if}
            </span>
            <button type="button" class="tiny" onclick={() => void refreshCacheInfo()}>Refresh</button>
            <button type="button" class="tiny danger" disabled={clearingCache || !cacheInfo?.files} onclick={() => void clearCacheNow()}>
              Clear downloaded media now
            </button>
          </div>
          {#if cacheStatus}
            <p class="hint">{cacheStatus}</p>
          {/if}

          <h3 class="privacy-subhead">Auto-download</h3>
          <p class="hint">
            What each account downloads by itself. Stickers follow Images; voice notes and audio follow Files
            but always download when you press play. Right-click a chat in the list to override this per chat.
          </p>
          {#each accounts as account (account.id)}
            <div class="auto-dl-row">
              <span class="auto-dl-name">{account.name}</span>
              {#each AUTO_DOWNLOAD_GROUPS as g (g.group)}
                <label class="check">
                  <input
                    type="checkbox"
                    checked={accountAutoDownload(account, g.group)}
                    onchange={(e) => setAccountAutoDownload(account, g.group, e.currentTarget.checked)}
                  />
                  {g.label}
                </label>
              {/each}
            </div>
          {:else}
            <p class="hint">No accounts yet.</p>
          {/each}
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
                  {#if msg.last_error}
                    <span class="account-identity">{msg.last_error}</span>
                  {/if}
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
            <div class="path-row">
              <input bind:value={backupPath} placeholder="/path/to/shuttle-backup.age" />
              {#if isTauri()}
                <button type="button" class="tiny" onclick={() => void browseExportPath()}>Browse</button>
              {/if}
            </div>
          </label>
          <label>
            Password
            <input bind:value={backupPassword} type="password" placeholder="Backup password" autocomplete="new-password" />
          </label>
          <label class="check">
            <input type="checkbox" bind:checked={includeMessages} />
            Include inbox databases
          </label>
          <label class="check">
            <input type="checkbox" bind:checked={includeMedia} />
            Include downloaded media
          </label>
          <button
            type="button"
            disabled={!backupPath.trim() || !backupPassword}
            onclick={async () => {
              restoreNeedsRestart = false;
              try {
                await onexportbackup(backupPath, backupPassword, includeMessages, includeMedia);
                backupStatus = 'Backup exported.';
              } catch (e) {
                backupStatus = e instanceof Error ? e.message : String(e);
              }
            }}>Export backup</button
          >
          <label>
            Restore path
            <div class="path-row">
              <input bind:value={restorePath} placeholder="/path/to/shuttle-backup.age" />
              {#if isTauri()}
                <button type="button" class="tiny" onclick={() => void browseRestorePath()}>Browse</button>
              {/if}
            </div>
          </label>
          <label>
            Restore password
            <input bind:value={restorePassword} type="password" placeholder="Backup password" autocomplete="current-password" />
          </label>
          <button
            type="button"
            disabled={!restorePath.trim() || !restorePassword}
            onclick={async () => {
              restoreNeedsRestart = false;
              try {
                await onrestorebackup(restorePath, restorePassword);
                backupStatus = 'Backup restored. Databases and files were merged into place.';
                restoreNeedsRestart = true;
              } catch (e) {
                backupStatus = e instanceof Error ? e.message : String(e);
              }
            }}>Restore backup</button
          >
          {#if backupStatus}
            <p class="hint">{backupStatus}</p>
          {/if}
          {#if restoreNeedsRestart}
            <button type="button" class="restart-btn" onclick={() => void restartApp()}>Restart Shuttle now</button>
          {/if}
        </section>
      {:else if activeTab === 'keyboard'}
        <section>
          <h2>Keyboard shortcuts</h2>
          <p class="hint">Shortcuts are disabled while typing in a text field, except quick switch and Escape.</p>
          <table class="shortcut-table">
            <thead>
              <tr>
                <th scope="col">Shortcut</th>
                <th scope="col">Action</th>
              </tr>
            </thead>
            <tbody>
              <tr><td><kbd>{modKey}+K</kbd></td><td>Open quick switch</td></tr>
              <tr><td><kbd>/</kbd></td><td>Focus conversation search</td></tr>
              <tr><td><kbd>j</kbd> / <kbd>↓</kbd></td><td>Next conversation</td></tr>
              <tr><td><kbd>k</kbd> / <kbd>↑</kbd></td><td>Previous conversation</td></tr>
              <tr><td><kbd>Enter</kbd></td><td>Open highlighted conversation</td></tr>
              <tr><td><kbd>e</kbd></td><td>Archive or unarchive conversation</td></tr>
              <tr><td><kbd>m</kbd></td><td>Mute or unmute conversation</td></tr>
              <tr><td><kbd>u</kbd></td><td>Mark conversation unread</td></tr>
              <tr><td><kbd>Esc</kbd></td><td>Close quick switch, contact details, or chat panel</td></tr>
            </tbody>
          </table>
        </section>
      {:else if activeTab === 'feedback'}
        <section>
          <h2>Feedback</h2>
          <p class="hint">
            Found a bug or have an idea? Tell us here. The form is hosted on forms.tfs.qa, so nothing is loaded until you
            open it, and Shuttle sends no chat data with it. Say what happened and which OS you use.
          </p>
          {#if feedbackOpen}
            <iframe
              class="feedback-frame"
              title="Send feedback"
              src={FEEDBACK_FORM_URL}
              referrerpolicy="no-referrer"
            ></iframe>
          {:else}
            <div class="feedback-actions">
              <button type="button" class="apply-theme-btn" onclick={() => (feedbackOpen = true)}>Open feedback form</button>
              <button type="button" class="linkish" onclick={() => openLink(FEEDBACK_FORM_URL)}>Open in browser instead</button>
            </div>
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
          {#if installationId}
            <p class="hint">
              Installation ID: <code>{installationId}</code>. Quote it if you ask us to delete diagnostics sent from this
              computer. Policy: <button type="button" class="linkish" onclick={() => openLink('https://github.com/smsheese/shuttle/blob/main/PRIVACY.md')}>PRIVACY.md</button>.
            </p>
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
  .settings .row > * {
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
  .sleep-mode {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 6px;
    font-size: 12px;
    color: var(--text-muted);
  }
  .sleep-mode select {
    flex: 1;
  }
  .acct-actions {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    width: 100%;
  }
  .workspace-select {
    border: 1px solid var(--border);
    background: transparent;
    border-radius: var(--radius-sm);
    padding: 6px 10px;
    font-size: 12px;
    color: var(--text-primary);
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
  .feedback-frame {
    border: none;
    width: 100%;
    height: 640px;
    border-radius: 8px;
    background: var(--bg-elevated, transparent);
  }
  .feedback-actions {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-top: 12px;
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
    color: var(--accent);
    cursor: pointer;
    font: inherit;
    text-align: left;
  }
  .linkish:hover {
    text-decoration: underline;
  }
  .auto-dl-row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px 16px;
  }
  .auto-dl-name {
    min-width: 140px;
    font-weight: 600;
  }
  .path-row {
    display: flex;
    gap: 8px;
    align-items: center;
  }
  .path-row input {
    flex: 1;
    min-width: 0;
  }
  .restart-btn {
    margin-top: 8px;
  }
  .trademark {
    margin-top: 4px;
  }
  code {
    font-size: 0.92em;
  }
  .shortcut-table {
    width: 100%;
    border-collapse: collapse;
    font-size: 13px;
  }
  .shortcut-table th,
  .shortcut-table td {
    text-align: left;
    padding: 8px 10px;
    border-bottom: 1px solid var(--border-subtle);
    vertical-align: top;
  }
  .shortcut-table th {
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--text-muted);
  }
  .shortcut-table kbd {
    display: inline-block;
    padding: 2px 6px;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--bg-input);
    font-family: inherit;
    font-size: 12px;
    line-height: 1.3;
  }

  @media (max-width: 639px) {
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
