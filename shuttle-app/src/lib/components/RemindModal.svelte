<script lang="ts">
  interface Props {
    open: boolean;
    initialNote?: string;
    onclose: () => void;
    onsubmit: (fireAt: string, note: string) => void | Promise<void>;
  }

  let { open, initialNote = '', onclose, onsubmit }: Props = $props();
  let fireAt = $state('');
  let note = $state('');
  let busy = $state(false);

  $effect(() => {
    if (open) {
      fireAt = new Date(Date.now() + 3600_000).toISOString().slice(0, 16);
      note = initialNote;
    }
  });

  async function submit() {
    if (!fireAt || busy) return;
    busy = true;
    try {
      await onsubmit(new Date(fireAt).toISOString(), note.trim());
      onclose();
    } finally {
      busy = false;
    }
  }
</script>

{#if open}
  <div class="backdrop" role="presentation" onclick={onclose}></div>
  <div class="modal" role="dialog" aria-modal="true" aria-label="Remind me">
    <h2>Remind me</h2>
    <label class="field">
      When
      <input type="datetime-local" bind:value={fireAt} disabled={busy} />
    </label>
    <label class="field">
      Note
      <textarea bind:value={note} rows="3" placeholder="Optional reminder note" disabled={busy}></textarea>
    </label>
    <div class="actions">
      <button type="button" class="cancel" onclick={onclose} disabled={busy}>Cancel</button>
      <button type="button" class="submit" onclick={submit} disabled={busy || !fireAt}>Set reminder</button>
    </div>
  </div>
{/if}

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.45);
    z-index: 200;
  }

  .modal {
    position: fixed;
    left: 50%;
    top: 50%;
    transform: translate(-50%, -50%);
    z-index: 201;
    width: min(420px, calc(100vw - 32px));
    background: var(--bg-panel);
    border: 1px solid var(--border-subtle);
    border-radius: 12px;
    padding: 18px;
    display: flex;
    flex-direction: column;
    gap: 12px;
    box-shadow: 0 16px 48px rgba(0, 0, 0, 0.2);
  }

  h2 {
    margin: 0;
    font-size: 18px;
  }

  .field {
    display: flex;
    flex-direction: column;
    gap: 6px;
    font-size: 12px;
    color: var(--text-muted);
  }

  input,
  textarea {
    font: inherit;
    color: var(--text-primary);
    background: var(--bg-input, var(--bg-panel));
    border: 1px solid var(--border-subtle);
    border-radius: 8px;
    padding: 8px 10px;
  }

  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 4px;
  }

  .cancel,
  .submit {
    border: none;
    border-radius: 8px;
    padding: 8px 14px;
    font: inherit;
    cursor: pointer;
  }

  .cancel {
    background: var(--bg-hover);
    color: var(--text-secondary);
  }

  .submit {
    background: var(--accent);
    color: white;
    font-weight: 600;
  }
</style>
