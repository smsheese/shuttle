<script lang="ts">
  import { acceptCall, hangupCall, rejectCall } from '$lib/api';
  import type { CallState } from '$lib/types';

  interface Props {
    call: CallState | null;
    onclose: () => void;
  }

  let { call, onclose }: Props = $props();

  let muted = $state(false);
  let videoOn = $state(true);
  let shareScreen = $state(false);

  const isIncoming = $derived(call?.direction === 'inbound');
  const isVideo = $derived(call?.mode === 'video');
  const statusLabel = $derived(
    call?.status === 'connected'
      ? 'Connected'
      : call?.status === 'ringing'
        ? isIncoming
          ? 'Incoming call'
          : 'Calling…'
        : call?.status ?? ''
  );

  async function accept() {
    if (!call) return;
    await acceptCall(call.account_id, call.call_id);
  }

  async function reject() {
    if (!call) return;
    await rejectCall(call.account_id, call.call_id);
    onclose();
  }

  async function hangup() {
    if (!call) return;
    await hangupCall(call.account_id, call.call_id);
    onclose();
  }
</script>

{#if call}
  <div class="overlay" role="dialog" aria-modal="true" aria-label="Call">
    <div class="panel">
      <div class="status">{statusLabel}</div>
      <div class="name">{call.remote_name ?? 'Call'}</div>
      <div class="mode">{isVideo ? 'Video call' : 'Voice call'}</div>

      {#if isVideo}
        <div class="preview">
          <div class="preview-box local">You</div>
          <div class="preview-box remote">{call.remote_name ?? 'Remote'}</div>
        </div>
      {/if}

      <div class="controls">
        {#if isIncoming && call.status === 'ringing'}
          <button type="button" class="ctrl accept" onclick={accept}>Accept</button>
          <button type="button" class="ctrl reject" onclick={reject}>Decline</button>
        {:else}
          <button type="button" class="ctrl" class:active={muted} onclick={() => (muted = !muted)}>
            {muted ? 'Unmute' : 'Mute'}
          </button>
          {#if isVideo}
            <button type="button" class="ctrl" class:active={!videoOn} onclick={() => (videoOn = !videoOn)}>
              {videoOn ? 'Video off' : 'Video on'}
            </button>
            <button type="button" class="ctrl" class:active={shareScreen} onclick={() => (shareScreen = !shareScreen)}>
              Share screen
            </button>
          {/if}
          <button type="button" class="ctrl hangup" onclick={hangup}>Hang up</button>
        {/if}
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
    width: min(420px, 100%);
    background: var(--bg-panel);
    border-radius: 16px;
    padding: 24px;
    display: flex;
    flex-direction: column;
    gap: 12px;
    align-items: center;
    box-shadow: 0 20px 60px rgba(0, 0, 0, 0.35);
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

  .mode {
    font-size: 13px;
    color: var(--text-muted);
  }

  .preview {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 8px;
    width: 100%;
  }

  .preview-box {
    aspect-ratio: 4/3;
    border-radius: 10px;
    background: var(--bg-hover);
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 12px;
    color: var(--text-muted);
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
    padding: 10px 14px;
    border-radius: 999px;
    border: 1px solid var(--border-subtle);
    background: var(--bg-hover);
    cursor: pointer;
    font-size: 13px;
  }

  .ctrl.active {
    background: var(--accent, #3b82f6);
    color: #fff;
    border-color: transparent;
  }

  .ctrl.accept {
    background: #22c55e;
    color: #fff;
    border-color: transparent;
  }

  .ctrl.reject,
  .ctrl.hangup {
    background: #ef4444;
    color: #fff;
    border-color: transparent;
  }
</style>
