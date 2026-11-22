<script lang="ts">
  import { untrack } from 'svelte';
  import { datePrefs } from '$lib/dateFormatPref.svelte';
  import { isCompleteLocalDateTime, nextMinute, toLocalDateTime } from '$lib/localDateTime';

  /**
   * In-app date + time picker popover (month calendar, hour/minute, quick presets).
   * Closes without applying on an outside click or Escape; Enter or "Set" applies it.
   * Rendered into <body> so no overflow/transform ancestor can clip or offset it.
   */
  interface Props {
    /** Element the popover hangs from; clicks on it are left to its own handler. */
    anchor: HTMLElement | null;
    /** Initial value, local `YYYY-MM-DDTHH:mm`; empty starts at one hour from now. */
    value?: string;
    /** Earliest allowed local `YYYY-MM-DDTHH:mm`; earlier days and times are refused. */
    min?: string;
    title?: string;
    confirmLabel?: string;
    onconfirm: (value: string) => void;
    oncancel: () => void;
  }

  let { anchor, value = '', min, title = 'Pick date & time', confirmLabel = 'Set', onconfirm, oncancel }: Props = $props();

  const WIDTH = 296;
  const WEEKDAYS = ['Mo', 'Tu', 'We', 'Th', 'Fr', 'Sa', 'Su'];

  function startOf(v: string): Date {
    if (isCompleteLocalDateTime(v)) return new Date(v);
    const d = new Date(Date.now() + 3600_000);
    d.setSeconds(0, 0);
    if (min && isCompleteLocalDateTime(min) && d < new Date(min)) return new Date(min);
    return d;
  }

  const initial = untrack(() => startOf(value));
  let year = $state(initial.getFullYear());
  let month = $state(initial.getMonth());
  let day = $state(initial.getDate());
  let selYear = $state(initial.getFullYear());
  let selMonth = $state(initial.getMonth());
  let hour = $state(initial.getHours());
  let minute = $state(initial.getMinutes());
  let root = $state<HTMLDivElement | null>(null);
  let pos = $state({ top: 0, left: 0 });
  let height = $state(0);

  const minDate = $derived(min && isCompleteLocalDateTime(min) ? new Date(min) : null);
  const candidate = $derived(new Date(selYear, selMonth, day, hour, minute));
  const candidateValue = $derived(toLocalDateTime(candidate));
  const tooEarly = $derived(!!minDate && candidate.getTime() < minDate.getTime());

  const monthLabel = $derived(new Date(year, month, 1).toLocaleDateString(undefined, { month: 'long', year: 'numeric' }));
  const cells = $derived.by(() => {
    const first = new Date(year, month, 1);
    const lead = (first.getDay() + 6) % 7; // Monday first
    const out: { d: number; m: number; y: number; outside: boolean }[] = [];
    for (let i = 0; i < 42; i++) {
      const dt = new Date(year, month, 1 - lead + i);
      out.push({ d: dt.getDate(), m: dt.getMonth(), y: dt.getFullYear(), outside: dt.getMonth() !== month });
    }
    return out;
  });

  const todayKey = $derived.by(() => {
    const t = new Date();
    return `${t.getFullYear()}-${t.getMonth()}-${t.getDate()}`;
  });

  function dayDisabled(c: { d: number; m: number; y: number }): boolean {
    if (!minDate) return false;
    return new Date(c.y, c.m, c.d, 23, 59, 59).getTime() < minDate.getTime();
  }

  function pickDay(c: { d: number; m: number; y: number }) {
    if (dayDisabled(c)) return;
    selYear = c.y;
    selMonth = c.m;
    day = c.d;
    year = c.y;
    month = c.m;
  }

  function shiftMonth(delta: number) {
    const d = new Date(year, month + delta, 1);
    year = d.getFullYear();
    month = d.getMonth();
  }

  function setFrom(d: Date) {
    selYear = year = d.getFullYear();
    selMonth = month = d.getMonth();
    day = d.getDate();
    hour = d.getHours();
    minute = d.getMinutes();
  }

  const presets = $derived.by(() => {
    const now = new Date();
    const inHour = nextMinute(new Date(now.getTime() + 3600_000 - 60_000));
    const tonight = new Date(now.getFullYear(), now.getMonth(), now.getDate(), 20, 0);
    const tomorrow = new Date(now.getFullYear(), now.getMonth(), now.getDate() + 1, 9, 0);
    const list = [
      { label: 'In 1 hour', at: inHour },
      { label: 'Tonight 8 PM', at: tonight },
      { label: 'Tomorrow 9 AM', at: tomorrow },
    ];
    return list.filter((p) => p.at.getTime() > now.getTime() && (!minDate || p.at.getTime() >= minDate.getTime()));
  });

  function presetLabel(label: string): string {
    if (datePrefs.hour12) return label;
    return label.replace('8 PM', '20:00').replace('9 AM', '09:00');
  }

  // --- time fields ---
  const hour12 = $derived(datePrefs.hour12);
  const pm = $derived(hour >= 12);
  const hourShown = $derived(hour12 ? hour % 12 || 12 : hour);

  function setHourShown(n: number) {
    if (!Number.isFinite(n)) return;
    if (hour12) {
      const h12 = ((Math.round(n) - 1 + 12) % 12) + 1;
      hour = (h12 % 12) + (pm ? 12 : 0);
    } else {
      hour = ((Math.round(n) % 24) + 24) % 24;
    }
  }

  function setMinute(n: number) {
    if (!Number.isFinite(n)) return;
    minute = ((Math.round(n) % 60) + 60) % 60;
  }

  function setPm(next: boolean) {
    if (next !== pm) hour = (hour + 12) % 24;
  }

  function stepKey(e: KeyboardEvent, apply: (delta: number) => void) {
    if (e.key === 'ArrowUp' || e.key === 'ArrowDown') {
      e.preventDefault();
      apply(e.key === 'ArrowUp' ? 1 : -1);
    }
  }

  function fieldChange(e: Event, apply: (n: number) => void) {
    const input = e.currentTarget as HTMLInputElement;
    const n = parseInt(input.value, 10);
    if (Number.isFinite(n)) apply(n);
    input.value = input === hourInput ? String(hourShown).padStart(2, '0') : String(minute).padStart(2, '0');
  }

  let hourInput = $state<HTMLInputElement | null>(null);

  function confirm() {
    if (tooEarly) return;
    onconfirm(candidateValue);
  }

  // --- placement ---
  function place() {
    const vw = window.innerWidth;
    const vh = window.innerHeight;
    const rect = anchor?.getBoundingClientRect();
    const h = height || 380;
    if (!rect) {
      pos = { top: Math.max(8, (vh - h) / 2), left: Math.max(8, (vw - WIDTH) / 2) };
      return;
    }
    const left = Math.min(Math.max(8, rect.left), Math.max(8, vw - WIDTH - 8));
    let top = rect.bottom + 6;
    if (top + h > vh - 8) top = rect.top - h - 6 >= 8 ? rect.top - h - 6 : Math.max(8, vh - h - 8);
    pos = { top, left };
  }

  $effect(() => {
    void height;
    place();
  });

  $effect(() => {
    function onPointerDown(e: PointerEvent) {
      const t = e.target as Node | null;
      if (!t) return;
      if (root?.contains(t) || anchor?.contains(t)) return;
      // The click that follows this press only dismisses the popover; it must not also
      // activate whatever lies underneath (a modal backdrop, a button).
      const swallow = (ev: Event) => {
        ev.stopPropagation();
        ev.preventDefault();
      };
      window.addEventListener('click', swallow, { capture: true, once: true });
      setTimeout(() => window.removeEventListener('click', swallow, true), 400);
      oncancel();
    }
    function onKey(e: KeyboardEvent) {
      if (e.key === 'Escape') {
        e.preventDefault();
        e.stopPropagation();
        oncancel();
      } else if (e.key === 'Enter' && !e.isComposing) {
        const t = e.target as HTMLElement | null;
        if (t?.closest('[data-dt-cancel]')) return; // let Enter press Cancel
        // A half-typed hour/minute commits on blur; commit it now so Enter applies what is shown.
        if (t instanceof HTMLInputElement && root?.contains(t)) t.dispatchEvent(new Event('change'));
        e.preventDefault();
        e.stopPropagation();
        confirm();
      }
    }
    window.addEventListener('pointerdown', onPointerDown, true);
    window.addEventListener('keydown', onKey, true);
    window.addEventListener('resize', place);
    window.addEventListener('scroll', place, true);
    root?.focus({ preventScroll: true });
    return () => {
      window.removeEventListener('pointerdown', onPointerDown, true);
      window.removeEventListener('keydown', onKey, true);
      window.removeEventListener('resize', place);
      window.removeEventListener('scroll', place, true);
    };
  });

  function portal(node: HTMLElement) {
    document.body.appendChild(node);
    return { destroy: () => node.remove() };
  }
</script>

<div
  class="dt-popover"
  role="dialog"
  aria-label={title}
  tabindex="-1"
  bind:this={root}
  bind:clientHeight={height}
  use:portal
  style:top="{pos.top}px"
  style:left="{pos.left}px"
  style:width="{WIDTH}px"
>
  <div class="dt-head">
    <button type="button" class="nav" aria-label="Previous month" onclick={() => shiftMonth(-1)}>‹</button>
    <span class="month">{monthLabel}</span>
    <button type="button" class="nav" aria-label="Next month" onclick={() => shiftMonth(1)}>›</button>
  </div>

  <div class="grid weekdays" aria-hidden="true">
    {#each WEEKDAYS as w (w)}<span>{w}</span>{/each}
  </div>
  <div class="grid days" role="grid">
    {#each cells as c (`${c.y}-${c.m}-${c.d}`)}
      {@const key = `${c.y}-${c.m}-${c.d}`}
      <button
        type="button"
        class="day"
        class:outside={c.outside}
        class:today={key === todayKey}
        class:selected={c.y === selYear && c.m === selMonth && c.d === day}
        disabled={dayDisabled(c)}
        aria-label={new Date(c.y, c.m, c.d).toLocaleDateString(undefined, { dateStyle: 'full' })}
        aria-pressed={c.y === selYear && c.m === selMonth && c.d === day}
        onclick={() => pickDay(c)}
      >{c.d}</button>
    {/each}
  </div>

  <div class="time-row">
    <span class="time-label">Time</span>
    <input
      class="time-input"
      bind:this={hourInput}
      inputmode="numeric"
      maxlength="2"
      aria-label="Hour"
      value={String(hourShown).padStart(2, '0')}
      onfocus={(e) => e.currentTarget.select()}
      onchange={(e) => fieldChange(e, setHourShown)}
      onkeydown={(e) => stepKey(e, (d) => setHourShown(hourShown + d))}
    />
    <span class="colon">:</span>
    <input
      class="time-input"
      inputmode="numeric"
      maxlength="2"
      aria-label="Minute"
      value={String(minute).padStart(2, '0')}
      onfocus={(e) => e.currentTarget.select()}
      onchange={(e) => fieldChange(e, setMinute)}
      onkeydown={(e) => stepKey(e, (d) => setMinute(minute + d))}
    />
    {#if hour12}
      <div class="ampm" role="group" aria-label="AM or PM">
        <button type="button" class:on={!pm} onclick={() => setPm(false)}>AM</button>
        <button type="button" class:on={pm} onclick={() => setPm(true)}>PM</button>
      </div>
    {/if}
  </div>

  {#if presets.length > 0}
    <div class="presets">
      {#each presets as p (p.label)}
        <button type="button" class="chip" onclick={() => setFrom(p.at)}>{presetLabel(p.label)}</button>
      {/each}
    </div>
  {/if}

  {#if tooEarly}
    <p class="warn" role="alert">That time has already passed.</p>
  {/if}

  <div class="actions">
    <button type="button" class="cancel" data-dt-cancel onclick={oncancel}>Cancel</button>
    <button type="button" class="set" disabled={tooEarly} onclick={confirm}>{confirmLabel}</button>
  </div>
</div>

<style>
  .dt-popover {
    position: fixed;
    z-index: 400;
    box-sizing: border-box;
    padding: 12px;
    display: flex;
    flex-direction: column;
    gap: 10px;
    background: var(--bg-panel);
    color: var(--text);
    border: 1px solid var(--border-subtle);
    border-radius: 12px;
    box-shadow: 0 16px 48px rgba(0, 0, 0, 0.25);
    outline: none;
    font-size: 13px;
  }
  button {
    font: inherit;
    color: inherit;
    cursor: pointer;
  }
  .dt-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }
  .month {
    font-weight: 600;
    text-transform: capitalize;
  }
  .nav {
    width: 28px;
    height: 28px;
    border: none;
    border-radius: 8px;
    background: transparent;
    font-size: 18px;
    line-height: 1;
  }
  .nav:hover,
  .day:hover:not(:disabled):not(.selected) {
    background: var(--bg-hover);
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(7, 1fr);
    gap: 2px;
    text-align: center;
  }
  .weekdays span {
    font-size: 11px;
    color: var(--text-muted);
    padding: 2px 0;
  }
  .day {
    aspect-ratio: 1;
    border: none;
    border-radius: 8px;
    background: transparent;
    padding: 0;
  }
  .day.outside {
    color: var(--text-muted);
    opacity: 0.55;
  }
  .day.today {
    box-shadow: inset 0 0 0 1px var(--accent);
  }
  .day.selected {
    background: var(--accent);
    color: white;
    font-weight: 600;
    opacity: 1;
  }
  .day:disabled {
    opacity: 0.3;
    cursor: not-allowed;
  }
  .time-row {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .time-label {
    flex: 1;
    color: var(--text-muted);
  }
  .time-input {
    box-sizing: border-box;
    width: 44px;
    padding: 6px 4px;
    text-align: center;
    font: inherit;
    color: var(--text);
    background: var(--bg-input);
    border: 1px solid var(--border-subtle);
    border-radius: 8px;
  }
  .time-input:focus-visible,
  .dt-popover button:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 1px;
  }
  .colon {
    color: var(--text-muted);
  }
  .ampm {
    display: flex;
    border: 1px solid var(--border-subtle);
    border-radius: 8px;
    overflow: hidden;
  }
  .ampm button {
    border: none;
    background: transparent;
    padding: 6px 8px;
    font-size: 12px;
  }
  .ampm button.on {
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
    border: 1px solid var(--border-subtle);
    border-radius: 999px;
    background: var(--bg-hover);
    padding: 3px 10px;
    font-size: 12px;
  }
  .chip:hover {
    border-color: var(--accent);
    color: var(--accent);
  }
  .warn {
    margin: 0;
    font-size: 12px;
    color: var(--danger, #d33);
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }
  .cancel,
  .set {
    border: none;
    border-radius: 8px;
    padding: 7px 14px;
  }
  .cancel {
    background: var(--bg-hover);
    color: var(--text-secondary, var(--text-muted));
  }
  .set {
    background: var(--accent);
    color: white;
    font-weight: 600;
  }
  .set:disabled {
    opacity: 0.45;
    cursor: not-allowed;
  }
</style>
