<script lang="ts">
  interface Props {
    /** How many messages the choice applies to. */
    count: number;
    /** Offer "Delete for everyone" (own messages, network allows it, still inside its window). */
    everyone: boolean;
    onchoose: (kind: 'delete_for_me' | 'delete_for_everyone') => void;
    oncancel: () => void;
  }

  let { count, everyone, onchoose, oncancel }: Props = $props();
  let first = $state<HTMLButtonElement | null>(null);

  $effect(() => {
    first?.focus();
  });
</script>

<div class="backdrop" onclick={oncancel} role="presentation">
  <div
    class="dialog"
    role="dialog"
    tabindex="-1"
    aria-modal="true"
    aria-labelledby="delete-title"
    onclick={(e) => e.stopPropagation()}
    onkeydown={(e) => {
      e.stopPropagation();
      if (e.key === 'Escape') oncancel();
    }}
  >
    <h2 id="delete-title">{count === 1 ? 'Delete message?' : `Delete ${count} messages?`}</h2>
    {#if !everyone}
      <p class="hint">It is removed from this account. Other people still see it.</p>
    {/if}
    <div class="actions">
      {#if everyone}
        <button type="button" class="danger" bind:this={first} onclick={() => onchoose('delete_for_everyone')}>
          Delete for everyone
        </button>
        <button type="button" class="danger" onclick={() => onchoose('delete_for_me')}>Delete for me</button>
      {:else}
        <button type="button" class="danger" bind:this={first} onclick={() => onchoose('delete_for_me')}>
          Delete for me
        </button>
      {/if}
      <button type="button" onclick={oncancel}>Cancel</button>
    </div>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 85;
    display: flex;
    align-items: center;
    justify-content: center;
    background: rgba(0, 0, 0, 0.45);
  }
  .dialog {
    width: min(360px, 92vw);
    padding: 16px;
    background: var(--bg-panel);
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    box-shadow: var(--shadow-md);
  }
  h2 {
    margin: 0 0 8px;
    font-size: 15px;
    color: var(--text);
  }
  .hint {
    margin: 0 0 12px;
    font-size: 13px;
    color: var(--text-muted);
  }
  .actions {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  button {
    padding: 9px 10px;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--bg-input);
    color: var(--text);
    font-size: 13px;
    cursor: pointer;
  }
  button:hover {
    background: var(--bg-hover);
  }
  button.danger {
    color: #ef4444;
  }
</style>
