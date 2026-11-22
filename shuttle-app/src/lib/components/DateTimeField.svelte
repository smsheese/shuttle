<script lang="ts">
  import DateTimePopover from '$lib/components/DateTimePopover.svelte';
  import { datePrefs } from '$lib/dateFormatPref.svelte';
  import { isCompleteLocalDateTime } from '$lib/localDateTime';

  /** A button showing the chosen date-time that opens the in-app picker popover. */
  interface Props {
    /** Local `YYYY-MM-DDTHH:mm`, or '' when nothing is picked. Updated only when "Set" is pressed. */
    value?: string;
    /** Earliest allowed local `YYYY-MM-DDTHH:mm` (use `toLocalDateTime(nextMinute())` for "future only"). */
    min?: string;
    disabled?: boolean;
    placeholder?: string;
    /** Show a × that empties the value. */
    clearable?: boolean;
    title?: string;
    onchange?: (value: string) => void;
  }

  let {
    value = $bindable(''),
    min,
    disabled = false,
    placeholder = 'Pick date & time',
    clearable = false,
    title = 'Pick date & time',
    onchange,
  }: Props = $props();

  let open = $state(false);
  let trigger = $state<HTMLButtonElement | null>(null);

  const label = $derived.by(() => {
    if (!isCompleteLocalDateTime(value)) return '';
    const d = new Date(value);
    const sameYear = d.getFullYear() === new Date().getFullYear();
    return d.toLocaleString(undefined, {
      weekday: 'short',
      day: 'numeric',
      month: 'short',
      ...(sameYear ? {} : { year: 'numeric' }),
      hour: 'numeric',
      minute: '2-digit',
      hour12: datePrefs.hour12,
    });
  });

  function confirm(next: string) {
    value = next;
    open = false;
    onchange?.(next);
    trigger?.focus();
  }

  function cancel() {
    open = false;
    trigger?.focus();
  }
</script>

<div class="dt-field">
  <button
    type="button"
    class="dt-trigger"
    class:empty={!label}
    bind:this={trigger}
    {disabled}
    aria-haspopup="dialog"
    aria-expanded={open}
    onclick={() => (open = !open)}
  >
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true">
      <rect x="3" y="4" width="18" height="18" rx="2" /><path d="M16 2v4M8 2v4M3 10h18" />
    </svg>
    <span class="text">{label || placeholder}</span>
  </button>
  {#if clearable && label && !disabled}
    <button type="button" class="dt-clear" aria-label="Clear date and time" onclick={() => { value = ''; onchange?.(''); }}>×</button>
  {/if}
</div>

{#if open}
  <DateTimePopover anchor={trigger} {value} {min} {title} onconfirm={confirm} oncancel={cancel} />
{/if}

<style>
  .dt-field {
    display: flex;
    align-items: center;
    gap: 4px;
    width: 100%;
    min-width: 0;
  }
  .dt-trigger {
    display: flex;
    align-items: center;
    gap: 8px;
    flex: 1;
    min-width: 0;
    box-sizing: border-box;
    padding: 8px;
    font: inherit;
    text-align: left;
    color: var(--text);
    background: var(--bg-input);
    border: 1px solid var(--border-subtle);
    border-radius: 8px;
    cursor: pointer;
  }
  .dt-trigger.empty {
    color: var(--text-muted);
  }
  .dt-trigger:disabled {
    opacity: 0.6;
    cursor: not-allowed;
  }
  .dt-trigger:focus-visible,
  .dt-clear:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 1px;
  }
  .text {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .dt-clear {
    flex: none;
    border: none;
    background: transparent;
    color: var(--text-muted);
    font-size: 16px;
    cursor: pointer;
    padding: 2px 6px;
  }
</style>
