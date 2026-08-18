<script lang="ts">
  import { fade, fly } from 'svelte/transition';
  import { CONNECTOR_COLORS, type ConnectorInfo } from '$lib/types';

  interface Props {
    open: boolean;
    connectors: ConnectorInfo[];
    onclose: () => void;
    oncreate: (connectorId: string, name: string, credentials: Record<string, string>) => void;
    onsubmit?: (credentials: Record<string, string>) => void;
    qrData?: string | null;
    connecting?: boolean;
    errorMessage?: string | null;
    authMethod?: string | null;
    authMessage?: string | null;
    /** Full-screen solid backdrop (e.g. ?setup=1 screenshot route) */
    standalone?: boolean;
  }

  let {
    open,
    connectors,
    onclose,
    oncreate,
    onsubmit,
    qrData = null,
    connecting = false,
    standalone = false,
    errorMessage = null,
    authMethod = null,
    authMessage = null,
  }: Props = $props();
  let view: 'list' | 'setup' = $state('list');
  let selectedConnector: ConnectorInfo | null = $state(null);
  let accountName = $state('');
  let phoneNumber = $state('');
  let verificationCode = $state('');
  let username = $state('');
  let password = $state('');
  let homeserver = $state('https://matrix.org');
  let emailAddress = $state('');
  let imapHost = $state('');
  let smtpHost = $state('');
  let telegramApiId = $state('');
  let telegramApiHash = $state('');
  let qrReady = $state(false);
  let authStarted = $state(false);

  const BRAND_META: Record<string, { gradient: string; tagline: string }> = {
    whatsapp: {
      gradient: 'linear-gradient(135deg, #25D366 0%, #128C7E 100%)',
      tagline: 'Scan QR code with WhatsApp on your phone',
    },
    telegram: {
      gradient: 'linear-gradient(135deg, #2AABEE 0%, #229ED9 100%)',
      tagline: 'Log in with your phone number',
    },
    signal: {
      gradient: 'linear-gradient(135deg, #3A76F0 0%, #2E6BD0 100%)',
      tagline: 'Scan QR code with Signal on your phone',
    },
    messenger: {
      gradient: 'linear-gradient(135deg, #0084FF 0%, #0064D1 100%)',
      tagline: 'Connect your Facebook account',
    },
    instagram: {
      gradient: 'linear-gradient(135deg, #E1306C 0%, #C13584 50%, #833AB4 100%)',
      tagline: 'Log in to Instagram DMs',
    },
    email: {
      gradient: 'linear-gradient(135deg, #EA4335 0%, #C5221F 100%)',
      tagline: 'Connect IMAP and SMTP',
    },
    matrix: {
      gradient: 'linear-gradient(135deg, #0DBD8B 0%, #0A8F6A 100%)',
      tagline: 'Log in to a Matrix homeserver',
    },
    slack: {
      gradient: 'linear-gradient(135deg, #4A154B 0%, #611f69 100%)',
      tagline: 'Workspace OAuth',
    },
    discord: {
      gradient: 'linear-gradient(135deg, #5865F2 0%, #4752C4 100%)',
      tagline: 'Authorize with Discord',
    },
  };

  const QR_STEPS: Record<string, { title: string; detail: string }[]> = {
    whatsapp: [
      { title: 'Open WhatsApp', detail: 'On your phone, open the WhatsApp app' },
      { title: 'Linked Devices', detail: 'Tap Settings → Linked Devices' },
      { title: 'Scan this code', detail: 'Tap "Link a Device" and point your camera here' },
    ],
    signal: [
      { title: 'Open Signal', detail: 'Launch Signal on your linked phone' },
      { title: 'Linked Devices', detail: 'Go to Settings → Linked Devices' },
      { title: 'Scan this code', detail: 'Tap "+" and scan the QR code below' },
    ],
    default: [
      { title: 'Open the app', detail: 'On your phone, open the messaging app' },
      { title: 'Find device linking', detail: 'Navigate to Settings → Linked Devices' },
      { title: 'Scan this code', detail: 'Scan the QR code to pair this desktop' },
    ],
  };

  const PHONE_STEPS: Record<string, { title: string; detail: string }[]> = {
    telegram: [
      { title: 'Enter your number', detail: 'Use the phone number registered with Telegram' },
      { title: 'Verify via Telegram', detail: "You'll receive a code in the Telegram app" },
      { title: 'Confirm login', detail: 'Enter the code to complete pairing' },
    ],
    signal: [
      { title: 'Enter your number', detail: 'Include country code, e.g. +15551234567' },
      { title: 'Verify the SMS', detail: 'Signal sends a registration code to that number' },
      { title: 'Start messaging', detail: 'Your Signal session stays on this device' },
    ],
    default: [
      { title: 'Enter phone number', detail: 'Include your country code' },
      { title: 'Receive verification', detail: 'A code will be sent to your device' },
      { title: 'Complete setup', detail: 'Enter the code to connect your account' },
    ],
  };

  function brandMeta(id: string) {
    return (
      BRAND_META[id] ?? {
        gradient: `linear-gradient(135deg, ${CONNECTOR_COLORS[id] ?? '#888'} 0%, ${CONNECTOR_COLORS[id] ?? '#666'} 100%)`,
        tagline: 'Connect your account',
      }
    );
  }

  function startSetup(c: ConnectorInfo) {
    selectedConnector = c;
    accountName = c.name;
    authStarted = false;
    qrReady = false;
    view = 'setup';
  }

  function collectCredentials(): Record<string, string> {
    const out: Record<string, string> = {};
    const kind = selectedConnector?.auth_type;
    if (kind === 'phone') {
      const phone = phoneNumber.trim().startsWith('+') ? phoneNumber.trim() : `+${phoneNumber.trim()}`;
      if (phone.length > 1) out.phone = phone;
      if (telegramApiId.trim()) out.api_id = telegramApiId.trim();
      if (telegramApiHash.trim()) out.api_hash = telegramApiHash.trim();
    }
    if (kind === 'password') {
      if (username.trim()) {
        out.username = username.trim();
        out.email = username.trim();
      }
      if (password) out.password = password;
      if (selectedConnector?.id === 'matrix' && homeserver.trim()) {
        out.homeserver = homeserver.trim();
      }
    }
    if (kind === 'email') {
      if (emailAddress.trim()) out.email = emailAddress.trim();
      if (password) out.password = password;
      if (imapHost.trim()) out.imap_host = imapHost.trim();
      if (smtpHost.trim()) out.smtp_host = smtpHost.trim();
    }
    return out;
  }

  function beginAuth() {
    if (!selectedConnector || !accountName.trim()) return;
    oncreate(selectedConnector.id, accountName.trim(), collectCredentials());
    authStarted = true;
    qrReady = false;
  }

  function submitFollowUp() {
    const next: Record<string, string> = { ...collectCredentials() };
    if (verificationCode.trim()) next.code = verificationCode.trim();
    if (authMethod === 'password' && password) next.two_factor_password = password;
    onsubmit?.(next);
  }

  function reset() {
    view = 'list';
    selectedConnector = null;
    accountName = '';
    phoneNumber = '';
    verificationCode = '';
    username = '';
    password = '';
    homeserver = 'https://matrix.org';
    emailAddress = '';
    imapHost = '';
    smtpHost = '';
    telegramApiId = '';
    telegramApiHash = '';
    qrReady = false;
    authStarted = false;
  }

  function handleClose() {
    reset();
    onclose();
  }

  function goBack() {
    if (view === 'setup') {
      view = 'list';
      selectedConnector = null;
      authStarted = false;
      qrReady = false;
    }
  }

  function mockQrDataUri(connectorId: string): string {
    const n = 25;
    let seed = 0;
    for (let i = 0; i < connectorId.length; i++) {
      seed = (seed * 31 + connectorId.charCodeAt(i)) >>> 0;
    }
    const fillFinder = (ox: number, oy: number) => {
      let s = '';
      for (let y = 0; y < 7; y++) {
        for (let x = 0; x < 7; x++) {
          const edge = x === 0 || x === 6 || y === 0 || y === 6;
          const inner = x >= 2 && x <= 4 && y >= 2 && y <= 4;
          if (edge || inner) {
            s += `<rect x="${(ox + x) * 10}" y="${(oy + y) * 10}" width="9" height="9" fill="#111"/>`;
          }
        }
      }
      return s;
    };
    let cells = fillFinder(0, 0) + fillFinder(18, 0) + fillFinder(0, 18);
    for (let y = 0; y < n; y++) {
      for (let x = 0; x < n; x++) {
        if ((x < 8 && y < 8) || (x > 16 && y < 8) || (x < 8 && y > 16)) continue;
        seed = (seed * 1103515245 + 12345) >>> 0;
        if ((seed & 0xff) > 100) {
          cells += `<rect x="${x * 10}" y="${y * 10}" width="9" height="9" fill="#111" rx="1"/>`;
        }
      }
    }
    const svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 250 250">${cells}</svg>`;
    return `data:image/svg+xml;base64,${btoa(svg)}`;
  }

  $effect(() => {
    if (qrData) qrReady = true;
  });

  function qrSrc(data: string): string {
    if (data.startsWith('data:') || data.startsWith('http://') || data.startsWith('https://')) {
      return data;
    }
    return `data:image/png;base64,${data}`;
  }

  const displayQr = $derived(qrData ? qrSrc(qrData) : null);

  const authSteps = $derived.by(() => {
    const connector = selectedConnector;
    if (!connector) return PHONE_STEPS.default;
    return connector.auth_type === 'qr'
      ? (QR_STEPS[connector.id] ?? QR_STEPS.default)
      : (PHONE_STEPS[connector.id] ?? PHONE_STEPS.default);
  });

  $effect(() => {
    if (!open) reset();
  });
</script>

{#snippet brandIcon(id: string)}
  {#if id === 'whatsapp'}
    <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
      <path
        d="M17.472 14.382c-.297-.149-1.758-.867-2.03-.967-.273-.099-.471-.148-.67.15-.197.297-.767.966-.94 1.164-.173.199-.347.223-.644.075-.297-.15-1.255-.463-2.39-1.475-.883-.788-1.48-1.761-1.653-2.059-.173-.297-.018-.458.13-.606.134-.133.298-.347.446-.52.149-.174.198-.298.298-.497.099-.198.05-.371-.025-.52-.075-.149-.669-1.612-.916-2.207-.242-.579-.487-.5-.669-.51-.173-.008-.371-.01-.57-.01-.198 0-.52.074-.792.372-.272.297-1.04 1.016-1.04 2.479 0 1.462 1.065 2.875 1.213 3.074.149.198 2.096 3.2 5.077 4.487.709.306 1.262.489 1.694.625.712.227 1.36.195 1.871.118.571-.085 1.758-.719 2.006-1.413.248-.694.248-1.289.173-1.413-.074-.124-.272-.198-.57-.347m-5.421 7.403h-.004a9.87 9.87 0 01-5.031-1.378l-.361-.214-3.741.982.998-3.648-.235-.374a9.86 9.86 0 01-1.51-5.26c.001-5.45 4.436-9.884 9.888-9.884 2.64 0 5.122 1.03 6.988 2.898a9.825 9.825 0 012.893 6.994c-.003 5.45-4.435 9.884-9.885 9.884m8.413-18.297A11.815 11.815 0 0012.05 0C5.495 0 .16 5.335.157 11.892c0 2.096.547 4.142 1.588 5.945L.057 24l6.305-1.654a11.882 11.882 0 005.683 1.448h.005c6.554 0 11.89-5.335 11.893-11.893a11.821 11.821 0 00-3.48-8.413z"
      />
    </svg>
  {:else if id === 'telegram'}
    <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
      <path
        d="M11.944 0A12 12 0 0 0 0 12a12 12 0 0 0 12 12 12 12 0 0 0 12-12A12 12 0 0 0 12 0a12 12 0 0 0-.056 0zm4.962 7.224c.1-.002.321.023.465.14a.506.506 0 0 1 .171.325c.016.093.036.306.02.472-.18 1.898-.962 6.502-1.36 8.627-.168.9-.499 1.201-.82 1.23-.696.065-1.225-.46-1.9-.902-1.056-.693-1.653-1.124-2.678-1.8-1.185-.78-.417-1.21.258-1.91.177-.184 3.247-2.977 3.307-3.23.007-.032.014-.15-.056-.212s-.174-.041-.249-.024c-.106.024-1.793 1.14-5.061 3.345-.48.33-.913.49-1.302.48-.428-.008-1.252-.241-1.865-.44-.752-.245-1.349-.374-1.297-.789.027-.216.325-.437.893-.663 3.498-1.524 5.83-2.529 6.998-3.014 3.332-1.386 4.025-1.627 4.476-1.635z"
      />
    </svg>
  {:else if id === 'signal'}
    <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
      <path
        d="M12 0C5.373 0 0 5.373 0 12s5.373 12 12 12 12-5.373 12-12S18.627 0 12 0zm0 2.4c5.302 0 9.6 4.298 9.6 9.6S17.302 21.6 12 21.6 2.4 17.302 2.4 12 6.698 2.4 12 2.4zM7.2 11.4a.6.6 0 0 0-.6.6v.6a.6.6 0 0 0 .6.6h9.6a.6.6 0 0 0 .6-.6v-.6a.6.6 0 0 0-.6-.6H7.2zm0 3.6a.6.6 0 0 0-.6.6v.6a.6.6 0 0 0 .6.6h6a.6.6 0 0 0 .6-.6v-.6a.6.6 0 0 0-.6-.6h-6z"
      />
    </svg>
  {:else if id === 'messenger'}
    <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
      <path
        d="M12 0C5.373 0 0 4.974 0 11.111c0 3.498 1.744 6.614 4.469 8.654V24l4.088-2.242c1.092.3 2.246.464 3.443.464 6.627 0 12-4.974 12-11.111C24 4.974 18.627 0 12 0zm1.191 14.963l-3.055-3.259-5.963 3.259L10.732 8.1l3.13 3.259 5.889-3.259-3.56 6.863z"
      />
    </svg>
  {:else if id === 'discord'}
    <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
      <path
        d="M20.317 4.37a19.791 19.791 0 0 0-4.885-1.515.074.074 0 0 0-.079.037c-.21.375-.444.864-.608 1.25a18.27 18.27 0 0 0-5.487 0 12.64 12.64 0 0 0-.617-1.25.077.077 0 0 0-.079-.037A19.736 19.736 0 0 0 3.677 4.37a.07.07 0 0 0-.032.027C.533 9.046-.32 13.58.099 18.057a.082.082 0 0 0 .031.057 19.9 19.9 0 0 0 5.993 3.03.078.078 0 0 0 .084-.028 14.09 14.09 0 0 0 1.226-1.994.076.076 0 0 0-.041-.106 13.107 13.107 0 0 1-1.872-.892.077.077 0 0 1-.008-.128 10.2 10.2 0 0 0 .372-.292.074.074 0 0 1 .077-.01c3.928 1.793 8.18 1.793 12.062 0a.074.074 0 0 1 .078.01c.12.098.246.198.373.292a.077.077 0 0 1-.006.127 12.299 12.299 0 0 1-1.873.892.077.077 0 0 0-.041.107c.36.698.772 1.362 1.225 1.993a.076.076 0 0 0 .084.028 19.839 19.839 0 0 0 6.002-3.03.077.077 0 0 0 .032-.054c.5-5.177-.838-9.674-3.549-13.66a.061.061 0 0 0-.031-.03z"
      />
    </svg>
  {:else}
    <span class="fallback-letter">{id[0]?.toUpperCase() ?? '?'}</span>
  {/if}
{/snippet}

{#if open}
  <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
  <div
    class="overlay"
    class:standalone
    role="presentation"
    onclick={standalone ? undefined : handleClose}
    transition:fade={{ duration: 150 }}
  >
    <div
      class="modal"
      class:modal-wide={view === 'setup' && authStarted}
      role="dialog"
      aria-modal="true"
      aria-labelledby="setup-title"
      onclick={(e) => e.stopPropagation()}
      transition:fly={{ y: 8, duration: 180 }}
    >
      {#if view === 'list'}
        <div class="security-banner">
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true">
            <path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z" />
          </svg>
          <span>Your messages are stored locally on this device.</span>
        </div>

        <header>
          <h2 id="setup-title">Add an account</h2>
          <button class="close" onclick={handleClose} aria-label="Close">
            <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <path d="M18 6L6 18M6 6l12 12" />
            </svg>
          </button>
        </header>

        <div class="connector-list">
          {#each connectors as c, i (c.id)}
            {@const meta = brandMeta(c.id)}
            <div class="connector-row" class:last={i === connectors.length - 1}>
              <span class="row-icon" style="background: {meta.gradient}">
                {@render brandIcon(c.id)}
              </span>
              <span class="row-name">{c.name}</span>
              <button class="add-btn" onclick={() => startSetup(c)}>Add</button>
            </div>
          {/each}
        </div>
      {:else if selectedConnector}
        {@const meta = brandMeta(selectedConnector.id)}
        {@const isQr = selectedConnector.auth_type === 'qr'}

        <header class="setup-header">
          <div class="header-left">
            <button class="back" onclick={goBack} aria-label="Go back">
              <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <path d="M15 18l-6-6 6-6" />
              </svg>
            </button>
            <div class="header-title">
              <span class="row-icon small" style="background: {meta.gradient}">
                {@render brandIcon(selectedConnector.id)}
              </span>
              <h2 id="setup-title">{selectedConnector.name}</h2>
            </div>
          </div>
          <button class="close" onclick={handleClose} aria-label="Close">
            <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <path d="M18 6L6 18M6 6l12 12" />
            </svg>
          </button>
        </header>

        <div class="setup-body">
          <label for="account-name">Account name</label>
          <input
            id="account-name"
            bind:value={accountName}
            placeholder="e.g. Work WhatsApp"
            disabled={authStarted}
          />
          <p class="hint">{meta.tagline}</p>
          {#if authMessage}
            <p class="hint">{authMessage}</p>
          {/if}

          {#if selectedConnector.auth_type === 'phone'}
            <label for="phone">Phone number</label>
            <input
              id="phone"
              type="tel"
              bind:value={phoneNumber}
              placeholder="+15551234567"
              disabled={authStarted && authMethod === 'code'}
            />
            {#if selectedConnector.id === 'telegram'}
              <label for="api-id">Telegram API ID</label>
              <input id="api-id" bind:value={telegramApiId} placeholder="From my.telegram.org" disabled={authStarted} />
              <label for="api-hash">Telegram API hash</label>
              <input id="api-hash" bind:value={telegramApiHash} placeholder="From my.telegram.org" disabled={authStarted} />
            {/if}
          {:else if selectedConnector.auth_type === 'password'}
            <label for="username">Email or username</label>
            <input id="username" bind:value={username} autocomplete="username" disabled={authStarted && !!authMethod} />
            {#if selectedConnector.id === 'matrix'}
              <label for="homeserver">Homeserver</label>
              <input id="homeserver" bind:value={homeserver} placeholder="https://matrix.org" disabled={authStarted && !!authMethod} />
            {/if}
            <label for="password">Password</label>
            <input id="password" type="password" bind:value={password} autocomplete="current-password" />
          {:else if selectedConnector.auth_type === 'email'}
            <label for="email-address">Email address</label>
            <input id="email-address" type="email" bind:value={emailAddress} placeholder="you@example.com" />
            <label for="email-password">Password or app password</label>
            <input id="email-password" type="password" bind:value={password} />
            <label for="imap-host">IMAP host (optional)</label>
            <input id="imap-host" bind:value={imapHost} placeholder="imap.example.com" />
            <label for="smtp-host">SMTP host (optional)</label>
            <input id="smtp-host" bind:value={smtpHost} placeholder="smtp.example.com" />
          {/if}

          {#if !authStarted}
            <button class="primary connect-action" onclick={beginAuth} disabled={!accountName.trim()}>
              Connect
            </button>
          {:else}
            <div class="auth-section">
              {#if isQr}
                <div class="qr-panel">
                  <div class="qr-frame" class:ready={!!displayQr}>
                    {#if !displayQr}
                      <div class="qr-loading">
                        <div class="spinner"></div>
                        <span>Generating QR code…</span>
                      </div>
                    {:else}
                      <img src={displayQr} alt="Pairing QR code" class="qr" />
                    {/if}
                  </div>
                  <p class="qr-hint">
                    {#if displayQr}
                      Keep this window open and scan with your phone
                    {:else}
                      Keep this window open while pairing
                    {/if}
                  </p>
                </div>
              {:else if authMethod === 'code' || authMethod === 'captcha'}
                <div class="phone-form">
                  <label for="verify-code">{authMethod === 'captcha' ? 'Captcha token' : 'Verification code'}</label>
                  <input id="verify-code" bind:value={verificationCode} placeholder={authMethod === 'captcha' ? 'signalcaptchas.org token' : '12345'} />
                  <button class="primary full" onclick={submitFollowUp} disabled={connecting || !verificationCode.trim()}>
                    Continue
                  </button>
                </div>
              {:else if authMethod === 'password' && selectedConnector.auth_type === 'phone'}
                <div class="phone-form">
                  <label for="tfa">Two-step password</label>
                  <input id="tfa" type="password" bind:value={password} />
                  <button class="primary full" onclick={submitFollowUp} disabled={connecting || !password}>
                    Unlock
                  </button>
                </div>
              {:else if connecting}
                <p class="qr-hint">Connecting…</p>
              {/if}

              {#if errorMessage}
                <p class="auth-error">{errorMessage}</p>
              {/if}

              <ol class="steps">
                {#each authSteps as s, i}
                  <li class="step" class:done={connecting && i === 0}>
                    <span class="step-num">{i + 1}</span>
                    <div>
                      <strong>{s.title}</strong>
                      <p>{s.detail}</p>
                    </div>
                  </li>
                {/each}
              </ol>
            </div>
          {/if}
        </div>
      {/if}
    </div>
  </div>
{/if}

<style>
  .overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.88);
    backdrop-filter: blur(4px);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 100;
    padding: 16px;
  }

  .overlay.standalone {
    background: var(--bg-main);
    backdrop-filter: none;
  }

  .modal {
    background: var(--bg-panel);
    border: 1px solid var(--border);
    border-radius: 12px;
    width: 100%;
    max-width: 400px;
    max-height: 90vh;
    overflow: hidden;
    display: flex;
    flex-direction: column;
    box-shadow: 0 16px 48px rgba(0, 0, 0, 0.4);
  }

  .modal-wide {
    max-width: 520px;
  }

  .security-banner {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 12px 20px;
    font-size: 12px;
    color: var(--text-muted);
    background: var(--bg-input);
    border-bottom: 1px solid var(--border);
    line-height: 1.4;
  }

  .security-banner svg {
    flex-shrink: 0;
    opacity: 0.7;
    color: var(--text-muted);
  }

  header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 18px 20px 0;
  }

  .setup-header {
    align-items: flex-start;
    padding-bottom: 0;
  }

  .header-left {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }

  .header-title {
    display: flex;
    align-items: center;
    gap: 10px;
    min-width: 0;
  }

  h2 {
    font-size: 17px;
    font-weight: 600;
    letter-spacing: -0.01em;
    line-height: 1.2;
  }

  .back,
  .close {
    border: none;
    background: transparent;
    color: var(--text-muted);
    cursor: pointer;
    padding: 6px;
    border-radius: 6px;
    display: flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
    transition: background 0.15s, color 0.15s;
  }

  .back:hover,
  .close:hover {
    background: var(--bg-hover);
    color: var(--text);
  }

  .connector-list {
    padding: 8px 0 12px;
  }

  .connector-row {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 12px 20px;
    border-bottom: 1px solid var(--border);
  }

  .connector-row.last {
    border-bottom: none;
  }

  .row-icon {
    width: 36px;
    height: 36px;
    border-radius: 8px;
    display: flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
    color: white;
  }

  .row-icon.small {
    width: 28px;
    height: 28px;
    border-radius: 6px;
  }

  .row-icon :global(svg) {
    width: 20px;
    height: 20px;
  }

  .row-icon.small :global(svg) {
    width: 16px;
    height: 16px;
  }

  .fallback-letter {
    font-size: 15px;
    font-weight: 700;
  }

  .row-name {
    flex: 1;
    font-size: 15px;
    font-weight: 500;
    min-width: 0;
  }

  .add-btn {
    flex-shrink: 0;
    padding: 6px 14px;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: transparent;
    color: var(--text);
    font-size: 13px;
    font-weight: 500;
    cursor: pointer;
    transition: background 0.15s, border-color 0.15s;
  }

  .add-btn:hover {
    background: var(--bg-hover);
    border-color: var(--text-muted);
  }

  .setup-body {
    padding: 16px 20px 24px;
    overflow-y: auto;
  }

  label {
    display: block;
    font-size: 13px;
    font-weight: 500;
    color: var(--text-muted);
    margin-bottom: 6px;
  }

  input {
    width: 100%;
    padding: 10px 12px;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--bg-input);
    color: var(--text);
    font-size: 15px;
    outline: none;
    transition: border-color 0.15s;
  }

  input:focus {
    border-color: var(--accent);
  }

  input:disabled {
    opacity: 0.6;
  }

  .hint {
    font-size: 12px;
    color: var(--text-muted);
    margin-top: 6px;
    margin-bottom: 16px;
  }

  .connect-action {
    width: 100%;
    margin-top: 4px;
  }

  .primary {
    padding: 10px 16px;
    border-radius: 8px;
    font-size: 14px;
    font-weight: 600;
    cursor: pointer;
    border: none;
    background: var(--accent);
    color: white;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    transition: opacity 0.15s;
  }

  .primary:hover:not(:disabled) {
    filter: brightness(1.06);
  }

  .primary:disabled {
    opacity: 0.45;
    cursor: default;
  }

  .auth-section {
    margin-top: 20px;
    padding-top: 20px;
    border-top: 1px solid var(--border);
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 20px;
    align-items: start;
  }

  .qr-panel {
    display: flex;
    flex-direction: column;
    align-items: center;
  }

  .qr-frame {
    position: relative;
    background: white;
    border-radius: 8px;
    padding: 16px;
    border: 1px solid rgba(0, 0, 0, 0.08);
  }

  .qr {
    display: block;
    width: 160px;
    height: 160px;
    border-radius: 2px;
  }

  .qr-loading {
    width: 160px;
    height: 160px;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 10px;
    color: #666;
    font-size: 12px;
  }

  .qr-hint {
    font-size: 11px;
    color: var(--text-muted);
    margin-top: 8px;
    text-align: center;
  }

  .auth-error {
    font-size: 12px;
    color: #ff7675;
    margin-top: 10px;
    text-align: center;
  }

  .phone-form {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .phone-input-wrap {
    display: flex;
    align-items: stretch;
    border: 1px solid var(--border);
    border-radius: 8px;
    overflow: hidden;
    background: var(--bg-input);
    transition: border-color 0.15s;
  }

  .phone-input-wrap:focus-within {
    border-color: var(--accent);
  }

  .country-code {
    display: flex;
    align-items: center;
    padding: 0 12px;
    font-size: 15px;
    font-weight: 500;
    color: var(--text-muted);
    background: var(--bg-active);
    border-right: 1px solid var(--border);
  }

  .phone-input-wrap input {
    border: none;
    border-radius: 0;
  }

  .full {
    width: 100%;
    margin-top: 4px;
  }

  .steps {
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: 14px;
    padding: 0;
  }

  .step {
    display: flex;
    gap: 10px;
    align-items: flex-start;
  }

  .step-num {
    width: 24px;
    height: 24px;
    border-radius: 50%;
    background: var(--bg-input);
    border: 1px solid var(--border);
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 11px;
    font-weight: 600;
    color: var(--text-muted);
    flex-shrink: 0;
  }

  .step.done .step-num {
    background: var(--accent);
    border-color: var(--accent);
    color: white;
  }

  .step strong {
    display: block;
    font-size: 13px;
    font-weight: 600;
    margin-bottom: 2px;
  }

  .step p {
    font-size: 12px;
    color: var(--text-muted);
    line-height: 1.45;
  }

  .spinner {
    width: 24px;
    height: 24px;
    border: 2px solid rgba(108, 92, 231, 0.2);
    border-top-color: var(--accent);
    border-radius: 50%;
    animation: spin 0.7s linear infinite;
  }

  .spinner.small {
    width: 14px;
    height: 14px;
    border-width: 2px;
  }

  .spinner.inline {
    display: inline-block;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  @media (max-width: 560px) {
    .auth-section {
      grid-template-columns: 1fr;
    }

    .modal-wide {
      max-width: 400px;
    }
  }
</style>
