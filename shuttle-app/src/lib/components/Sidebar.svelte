<script lang="ts">
  import NetworkIcon from '$lib/components/NetworkIcon.svelte';
  import { CONNECTOR_COLORS, type Account } from '$lib/types';

  interface Props {
    accounts: Account[];
    selected: string | null;
    onselect: (id: string | null) => void;
    onadd: () => void;
    unreadTotal: number;
    mobileTab?: 'inbox' | 'settings';
    settingsActive?: boolean;
    ontabchange?: (tab: 'inbox' | 'settings') => void;
    onsettings?: () => void;
    onaccountmenu?: (account: Account, x: number, y: number) => void;
  }

  let {
    accounts,
    selected,
    onselect,
    onadd,
    unreadTotal,
    mobileTab = 'inbox',
    settingsActive = false,
    ontabchange,
    onsettings,
    onaccountmenu,
  }: Props = $props();

  let brandClicks = 0;
  let brandTimer: ReturnType<typeof setTimeout> | undefined;

  async function onBrandClick() {
    brandClicks += 1;
    clearTimeout(brandTimer);
    brandTimer = setTimeout(() => (brandClicks = 0), 2500);
    if (brandClicks >= 8) {
      brandClicks = 0;
      const { openDevtools } = await import('$lib/api');
      openDevtools();
    }
  }

</script>

<nav class="sidebar" aria-label="Main navigation">
  <!-- Desktop: network rail -->
  <div class="desktop-rail">
    <button
      type="button"
      class="brand"
      title="Shuttle"
      aria-label="Shuttle"
      onclick={onBrandClick}
    >
      <img class="brand-logo" src="/shuttle-logo.svg" width="22" height="22" alt="" />
    </button>

    <button
      class="nav-item all-chats"
      class:active={selected === null}
      onclick={() => onselect(null)}
      title="All Chats"
      aria-label="All Chats"
      aria-current={selected === null ? 'page' : undefined}
    >
      <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" aria-hidden="true">
        <path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z"/>
      </svg>
      {#if unreadTotal > 0}
        <span class="badge badge-pulse">{unreadTotal > 99 ? '99+' : unreadTotal}</span>
      {/if}
    </button>

    <div class="divider" role="separator"></div>

    <div class="networks">
      {#each accounts as account (account.id)}
        <button
          class="nav-item network"
          class:active={selected === account.id}
          class:disabled={account.disabled}
          style="--network-color: {CONNECTOR_COLORS[account.connector_id] ?? '#888'}"
          onclick={() => onselect(account.id)}
          oncontextmenu={(e) => {
            e.preventDefault();
            onaccountmenu?.(account, e.clientX, e.clientY);
          }}
          title={account.name}
          aria-label={account.name}
          aria-current={selected === account.id ? 'page' : undefined}
        >
          <span class="network-icon-wrap">
            <NetworkIcon connectorId={account.connector_id} size={16} />
          </span>
          {#if account.status !== 'connected'}
            <span class="status-dot" class:connecting={account.status === 'connecting'}></span>
          {/if}
        </button>
      {/each}
    </div>

    <div class="rail-footer">
      <button class="nav-item add" onclick={onadd} title="Add account" aria-label="Add account">
        <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true">
          <line x1="12" y1="5" x2="12" y2="19"/><line x1="5" y1="12" x2="19" y2="12"/>
        </svg>
      </button>
      <button
        class="nav-item settings"
        class:active={settingsActive}
        onclick={() => onsettings?.()}
        title="Settings"
        aria-label="Settings"
        aria-current={settingsActive ? 'page' : undefined}
      >
        <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" aria-hidden="true">
          <circle cx="12" cy="12" r="3"/>
          <path d="M12 1v2M12 21v2M4.22 4.22l1.42 1.42M18.36 18.36l1.42 1.42M1 12h2M21 12h2M4.22 19.78l1.42-1.42M18.36 5.64l1.42-1.42"/>
        </svg>
      </button>
    </div>
  </div>

  <!-- Mobile: Inbox | Settings tab bar -->
  <div class="mobile-tabs">
    <button
      class="tab-item"
      class:active={mobileTab === 'inbox'}
      onclick={() => ontabchange?.('inbox')}
      aria-label="Inbox"
      aria-current={mobileTab === 'inbox' ? 'page' : undefined}
    >
      <svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" aria-hidden="true">
        <path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z"/>
      </svg>
      <span class="tab-label">Inbox</span>
    </button>
    <button
      class="tab-item"
      class:active={mobileTab === 'settings'}
      onclick={() => ontabchange?.('settings')}
      aria-label="Settings"
      aria-current={mobileTab === 'settings' ? 'page' : undefined}
    >
      <svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" aria-hidden="true">
        <circle cx="12" cy="12" r="3"/>
        <path d="M12 1v2M12 21v2M4.22 4.22l1.42 1.42M18.36 18.36l1.42 1.42M1 12h2M21 12h2M4.22 19.78l1.42-1.42M18.36 5.64l1.42-1.42"/>
      </svg>
      <span class="tab-label">Settings</span>
    </button>
  </div>
</nav>

<style>
  .sidebar {
    width: 100%;
    min-width: var(--rail-width);
    height: 100%;
    background: var(--bg-sidebar);
    border-right: 1px solid var(--border-subtle);
    display: flex;
    flex-direction: column;
    align-items: center;
    padding: 14px 0 16px;
    gap: 6px;
  }

  .desktop-rail {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    width: 100%;
    flex: 1;
    min-height: 0;
  }

  .mobile-tabs {
    display: none;
  }

  .brand {
    width: 36px;
    height: 36px;
    display: flex;
    align-items: center;
    justify-content: center;
    margin-bottom: 8px;
    border: none;
    padding: 0;
    border-radius: var(--radius-md);
    background: var(--accent-muted);
    cursor: pointer;
    color: inherit;
  }

  .brand-logo {
    display: block;
    width: 22px;
    height: 22px;
    object-fit: contain;
  }

  .networks {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 4px;
    flex: 1;
    overflow-y: auto;
    width: 100%;
    padding: 0 8px;
  }

  .nav-item {
    position: relative;
    width: 44px;
    height: 44px;
    border: none;
    border-radius: var(--radius-md);
    background: transparent;
    color: var(--text-muted);
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
    transition: background 0.15s ease, color 0.15s ease, transform 0.1s ease;
    flex-shrink: 0;
  }

  .nav-item:hover {
    background: var(--bg-hover);
    color: var(--text-secondary);
  }

  .nav-item:active {
    transform: scale(0.96);
  }

  .nav-item.active {
    background: var(--bg-active);
    color: var(--text);
  }

  .nav-item.active::before {
    content: '';
    position: absolute;
    left: -8px;
    top: 50%;
    transform: translateY(-50%);
    width: 3px;
    height: 22px;
    border-radius: 0 2px 2px 0;
    background: var(--accent);
  }

  .network.active::before {
    background: var(--network-color);
  }

  .network-icon-wrap {
    width: 28px;
    height: 28px;
    border-radius: var(--radius-sm);
    display: flex;
    align-items: center;
    justify-content: center;
    background: color-mix(in srgb, var(--network-color) 18%, transparent);
    color: var(--network-color);
    transition: background 0.15s ease;
  }

  .network.active .network-icon-wrap {
    background: color-mix(in srgb, var(--network-color) 28%, transparent);
    box-shadow: 0 0 0 1px color-mix(in srgb, var(--network-color) 40%, transparent);
  }

  .network:hover .network-icon-wrap {
    background: color-mix(in srgb, var(--network-color) 24%, transparent);
  }

  .divider {
    width: 24px;
    height: 1px;
    background: var(--border);
    margin: 4px 0 6px;
    flex-shrink: 0;
  }

  .badge {
    position: absolute;
    top: 2px;
    right: 2px;
    min-width: 17px;
    height: 17px;
    padding: 0 4px;
    background: var(--accent);
    color: white;
    font-size: 10px;
    font-weight: 700;
    letter-spacing: -0.02em;
    border-radius: var(--radius-full);
    display: flex;
    align-items: center;
    justify-content: center;
    box-shadow: 0 0 0 2px var(--bg-sidebar);
  }

  .status-dot {
    position: absolute;
    bottom: 4px;
    right: 4px;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--warning);
    border: 2px solid var(--bg-sidebar);
  }

  .status-dot.connecting {
    background: var(--accent);
    animation: pulse 1.5s infinite;
  }

  .badge-pulse {
    animation: badge-breathe 2.4s ease-in-out infinite;
  }

  @keyframes pulse {
    0%, 100% { opacity: 1; }
    50% { opacity: 0.35; }
  }

  @keyframes badge-breathe {
    0%, 100% {
      transform: scale(1);
      box-shadow: 0 0 0 2px var(--bg-sidebar), 0 0 0 0 color-mix(in srgb, var(--accent) 50%, transparent);
    }
    50% {
      transform: scale(1.1);
      box-shadow: 0 0 0 2px var(--bg-sidebar), 0 0 0 5px color-mix(in srgb, var(--accent) 0%, transparent);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .badge-pulse {
      animation: none;
    }
  }

  .rail-footer {
    margin-top: auto;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    width: 100%;
    padding-top: 6px;
  }

  .add {
    opacity: 0.55;
    border: 1px dashed var(--border);
    width: 40px;
    height: 40px;
  }

  .settings {
    width: 44px;
    height: 44px;
    opacity: 0.72;
  }

  .settings.active {
    opacity: 1;
    background: var(--bg-active);
    color: var(--text);
  }

  .settings.active::before {
    content: '';
    position: absolute;
    left: -8px;
    top: 50%;
    transform: translateY(-50%);
    width: 3px;
    height: 22px;
    border-radius: 0 2px 2px 0;
    background: var(--accent);
  }

  .add:hover,
  .settings:hover {
    opacity: 1;
    border-color: var(--text-muted);
    color: var(--text);
  }

  .network.disabled {
    opacity: 0.45;
  }

  @media (max-width: 768px) {
    .sidebar {
      position: relative;
      width: 100%;
      min-width: unset;
      height: calc(var(--mobile-nav-height) + var(--safe-bottom));
      flex-direction: row;
      align-items: stretch;
      justify-content: center;
      padding: 0 0 var(--safe-bottom);
      gap: 0;
      border-right: none;
      border-top: 1px solid var(--border-subtle);
      flex-shrink: 0;
    }

    .desktop-rail {
      display: none;
    }

    .mobile-tabs {
      display: flex;
      width: 100%;
      align-items: stretch;
      justify-content: space-around;
    }

    .tab-item {
      flex: 1;
      display: flex;
      flex-direction: column;
      align-items: center;
      justify-content: center;
      gap: 3px;
      min-height: 48px;
      min-width: 48px;
      padding: 6px 12px;
      border: none;
      background: transparent;
      color: var(--text-muted);
      cursor: pointer;
      touch-action: manipulation;
      -webkit-tap-highlight-color: transparent;
      transition: color 0.15s ease;
    }

    .tab-item:active {
      background: var(--bg-hover);
    }

    .tab-item.active {
      color: var(--accent);
    }

    .tab-label {
      font-size: 11px;
      font-weight: 600;
      letter-spacing: 0.01em;
      line-height: 1;
    }
  }
</style>
