<script lang="ts">
  import type { UpdateCheck } from '$lib/types';

  interface Props {
    update: UpdateCheck;
    onview: () => void;
    onclose: () => void;
  }
  let { update, onview, onclose }: Props = $props();

  // Release notes are Markdown; show them as plain lines without a Markdown renderer.
  const lines = $derived(
    (update.notes ?? '')
      .split('\n')
      .map((line) => line.replace(/^#{1,6}\s*/, '').replace(/\*\*(.+?)\*\*/g, '$1').replace(/`([^`]+)`/g, '$1').replace(/^[-*]\s+/, '• ').trimEnd())
      .filter((line, i, all) => line.trim() || (i > 0 && all[i - 1].trim()))
  );
</script>

<div class="notice" role="dialog" aria-label="Update available">
  <div class="head">
    <strong>Shuttle v{update.latest} is available</strong>
    <button type="button" class="close" onclick={onclose} aria-label="Dismiss">×</button>
  </div>
  <p class="sub">You have v{update.current}.</p>
  <div class="notes">
    {#if lines.length}
      {#each lines as line}<div>{line || ' '}</div>{/each}
    {:else}
      <div>No change log was published for this release.</div>
    {/if}
  </div>
  <div class="actions">
    <button type="button" class="primary" onclick={onview}>Update in Settings</button>
    <button type="button" onclick={onclose}>Later</button>
  </div>
</div>

<style>
  .notice {
    position: fixed;
    left: 76px;
    bottom: 12px;
    z-index: 60;
    width: min(360px, calc(100vw - 92px));
    max-height: 60vh;
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 12px;
    background: var(--bg-elevated, var(--bg));
    color: var(--text);
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    box-shadow: 0 8px 28px rgba(0, 0, 0, 0.3);
  }
  .head { display: flex; justify-content: space-between; align-items: center; gap: 8px; }
  .close { border: none; background: transparent; color: var(--text-muted); font-size: 18px; cursor: pointer; }
  .sub { margin: 0; color: var(--text-muted); font-size: 12px; }
  .notes { overflow-y: auto; font-size: 12.5px; line-height: 1.45; white-space: pre-wrap; }
  .actions { display: flex; gap: 8px; justify-content: flex-end; }
  .actions button { padding: 5px 12px; border-radius: var(--radius-md); border: 1px solid var(--border); background: transparent; color: var(--text); cursor: pointer; }
  .actions .primary { background: var(--accent); border-color: var(--accent); color: #fff; }
  @media (max-width: 639px) { .notice { left: 12px; bottom: 64px; width: calc(100vw - 24px); } }
</style>
