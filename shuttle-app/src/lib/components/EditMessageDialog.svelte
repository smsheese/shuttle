<script lang="ts">
  interface Props {
    original: string;
    onsave: (text: string) => void;
    oncancel: () => void;
  }

  let { original, onsave, oncancel }: Props = $props();
  let text = $state('');
  let field = $state<HTMLTextAreaElement | null>(null);
  const canSave = $derived(text.trim() !== '' && text !== original);

  $effect(() => {
    text = original;
    field?.focus();
    field?.select();
  });

  function save() {
    if (canSave) onsave(text);
  }
</script>

<div class="backdrop" onclick={oncancel} role="presentation">
  <div
    class="dialog"
    role="dialog"
    tabindex="-1"
    aria-modal="true"
    aria-labelledby="edit-title"
    onclick={(e) => e.stopPropagation()}
    onkeydown={(e) => {
      e.stopPropagation();
      if (e.key === 'Escape') oncancel();
    }}
  >
    <h2 id="edit-title">Edit message</h2>
    <textarea
      bind:this={field}
      bind:value={text}
      rows="3"
      aria-label="Message text"
      onkeydown={(e) => {
        if (e.key === 'Enter' && !e.shiftKey) {
          e.preventDefault();
          save();
        }
      }}
    ></textarea>
    <div class="actions">
      <button type="button" onclick={oncancel}>Cancel</button>
      <button type="button" class="primary" disabled={!canSave} onclick={save}>Save</button>
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
    width: min(440px, 92vw);
    padding: 16px;
    background: var(--bg-panel);
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    box-shadow: var(--shadow-md);
  }
  h2 {
    margin: 0 0 10px;
    font-size: 15px;
    color: var(--text);
  }
  textarea {
    width: 100%;
    box-sizing: border-box;
    resize: vertical;
    padding: 8px 10px;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--bg-input);
    color: var(--text);
    font: inherit;
    font-size: 14px;
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 10px;
  }
  button {
    padding: 8px 14px;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--bg-input);
    color: var(--text);
    font-size: 13px;
    cursor: pointer;
  }
  button.primary {
    background: var(--accent);
    border-color: var(--accent);
    color: var(--text-on-accent, #fff);
  }
  button:disabled {
    opacity: 0.5;
    cursor: default;
  }
</style>
