<script lang="ts">
  import DateTimeField from '$lib/components/DateTimeField.svelte';
  import { nextMinute, toLocalDateTime } from '$lib/localDateTime';
  import { TIMER_PRESETS, TIMER_UNITS, type NoteScheduleMode } from '$lib/noteSchedule';

  /** Choose "no reminder", a countdown timer, or an alarm for a note. */
  interface Props {
    mode?: NoteScheduleMode;
    /** Countdown length in seconds when `mode` is `timer`. */
    timerSecs?: number;
    /** Local `YYYY-MM-DDTHH:mm` when `mode` is `alarm`. */
    alarmAt?: string;
    disabled?: boolean;
  }

  let {
    mode = $bindable('none'),
    timerSecs = $bindable(600),
    alarmAt = $bindable(''),
    disabled = false,
  }: Props = $props();

  let customValue = $state('');
  let customUnit = $state<keyof typeof TIMER_UNITS>('m');

  function applyCustom() {
    const n = parseFloat(customValue);
    timerSecs = Number.isFinite(n) && n > 0 ? Math.round(n * TIMER_UNITS[customUnit]) : 0;
  }

  const MODES: { id: NoteScheduleMode; label: string }[] = [
    { id: 'none', label: 'No reminder' },
    { id: 'timer', label: 'Timer' },
    { id: 'alarm', label: 'Alarm' },
  ];
</script>

<div class="sched">
  <div class="modes" role="group" aria-label="Reminder type">
    {#each MODES as m (m.id)}
      <button type="button" class:on={mode === m.id} {disabled} onclick={() => (mode = m.id)}>{m.label}</button>
    {/each}
  </div>

  {#if mode === 'timer'}
    <div class="presets">
      {#each TIMER_PRESETS as p (p.secs)}
        <button
          type="button"
          class="chip"
          class:on={customValue === '' && timerSecs === p.secs}
          {disabled}
          onclick={() => {
            customValue = '';
            timerSecs = p.secs;
          }}>{p.label}</button
        >
      {/each}
    </div>
    <div class="custom">
      <input
        inputmode="decimal"
        placeholder="Custom"
        aria-label="Custom timer length"
        bind:value={customValue}
        {disabled}
        oninput={applyCustom}
      />
      <select bind:value={customUnit} {disabled} aria-label="Timer unit" onchange={applyCustom}>
        <option value="m">min</option>
        <option value="h">hours</option>
        <option value="d">days</option>
      </select>
    </div>
  {:else if mode === 'alarm'}
    <DateTimeField
      bind:value={alarmAt}
      min={toLocalDateTime(nextMinute())}
      {disabled}
      placeholder="Pick alarm time"
      title="Set alarm"
    />
  {/if}
</div>

<style>
  .sched {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .modes {
    display: flex;
    border: 1px solid var(--border-subtle);
    border-radius: 8px;
    overflow: hidden;
  }
  .modes button {
    flex: 1;
    border: none;
    border-radius: 0;
    background: transparent;
    color: var(--text-muted);
    padding: 5px 4px;
    font-size: 12px;
    font-weight: 500;
  }
  .modes button.on {
    background: var(--accent);
    color: white;
    font-weight: 600;
  }
  .presets {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  .chip {
    width: auto;
    border: 1px solid var(--border-subtle);
    border-radius: 999px;
    background: var(--bg-hover);
    color: var(--text);
    padding: 3px 10px;
    font-size: 12px;
    font-weight: 500;
  }
  .chip.on {
    border-color: var(--accent);
    color: var(--accent);
    font-weight: 600;
  }
  .custom {
    display: flex;
    gap: 6px;
  }
  .custom input,
  .custom select {
    box-sizing: border-box;
    padding: 6px 8px;
    font: inherit;
    color: var(--text);
    background: var(--bg-input);
    border: 1px solid var(--border-subtle);
    border-radius: 8px;
  }
  .custom input {
    flex: 1;
    min-width: 0;
  }
  button {
    font: inherit;
    cursor: pointer;
  }
  button:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: -2px;
  }
</style>
