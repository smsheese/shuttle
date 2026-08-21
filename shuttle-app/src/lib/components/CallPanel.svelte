<script lang="ts">
  import { rejectCall } from '$lib/api';
  import type { CallState } from '$lib/types';

  interface Props {
    call: CallState | null;
    onclose: () => void;
  }

  let { call, onclose }: Props = $props();

  const isIncoming = $derived(call?.direction === 'inbound');
  const isRinging = $derived(call?.status === 'ringing');
  const statusLabel = $derived(
    isRinging
      ? isIncoming
        ? 'Incoming call'
        : 'Calling…'
      : call?.status ?? ''
  );

  async function decline() {
    if (!call) return;
    await rejectCall(call.account_id, call.call_id);
    onclose();
  }
</script>

{#if call}
  <div class="overlay" role="dialog" aria-modal="true" aria-label="Call">
    <div class="panel">
      {#if statusLabel}
        <div class="status">{statusLabel}</div>
      {/if}
      <div class="name">{call.remote_name ?? 'Call'}</div>
      <p class="notice">Calling isn't available in this build.</p>

      <div class="controls">
        {#if isIncoming && isRinging}
          <button type="button" class="ctrl decline" onclick={decline}>Decline</button>
        {/if}
        <button type="button" class="ctrl dismiss" onclick={onclose}>Dismiss</button>
      </div>
    </div>
  </div>
{/if}

<style>
  .overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.72);
    z-index: 250;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 16px;
  }

  .panel {
    width: min(360px, 100%);
    background: var(--bg-panel);
    border-radius: 16px;
    padding: 24px;
    display: flex;
    flex-direction: column;
    gap: 10px;
    align-items: center;
    box-shadow: 0 20px 60px rgba(0, 0, 0, 0.35);
    text-align: center;
  }

  .status {
    font-size: 12px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--text-muted);
  }

  .name {
    font-size: 22px;
    font-weight: 600;
  }

  .notice {
    margin: 0;
    font-size: 14px;
    color: var(--text-muted);
    line-height: 1.4;
  }

  .controls {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    justify-content: center;
    width: 100%;
    margin-top: 8px;
  }

  .ctrl {
    padding: 10px 18px;
    border-radius: 999px;
    border: 1px solid var(--border-subtle);
    background: var(--bg-hover);
    cursor: pointer;
    font-size: 13px;
  }

  .ctrl.decline {
    background: #ef4444;
    color: #fff;
    border-color: transparent;
  }

  .ctrl.dismiss {
    min-width: 100px;
  }
</style>
