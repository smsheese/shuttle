<script lang="ts">
  import type { Snippet } from 'svelte';
  import type { Message, PollData } from '$lib/types';
  import { pollInitials, pollShare, pollVoteTotal, pollVoterName } from '$lib/poll';

  let {
    poll,
    msg,
    trailing,
    onAccent = false,
  }: {
    poll: PollData;
    msg: Message;
    /** The time and ticks row, drawn between the options and the "View votes" action. */
    trailing?: Snippet<[Message]>;
    /** The card sits on a bubble painted with the theme's accent, so the green needs help. */
    onAccent?: boolean;
  } = $props();

  const total = $derived(pollVoteTotal(poll));
  const chipsFor = (option: PollData['options'][number]) => option.voters.slice(0, 3);

  let open = $state(false);
  let viewsBtn = $state<HTMLButtonElement | null>(null);
  let panel = $state<HTMLElement | null>(null);
  let pos = $state({ left: 0, top: 0 });

  /** Keep the panel inside the window: below the button when it fits there, else above. */
  function place() {
    const rect = viewsBtn?.getBoundingClientRect();
    const box = panel?.getBoundingClientRect();
    if (!rect || !box) return;
    const margin = 8;
    const gap = 6;
    const left = Math.min(
      Math.max(margin, rect.left),
      Math.max(margin, window.innerWidth - box.width - margin)
    );
    const below = rect.bottom + gap;
    const top =
      below + box.height + margin <= window.innerHeight
        ? below
        : Math.max(margin, rect.top - gap - box.height);
    pos = { left, top };
  }

  function toggle() {
    open = !open;
    if (open) pos = { left: -9999, top: -9999 };
  }

  $effect(() => {
    if (!open) return;
    place();
    const onDown = (e: PointerEvent) => {
      const target = e.target as Node;
      if (panel?.contains(target) || viewsBtn?.contains(target)) return;
      open = false;
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') open = false;
    };
    const onMove = () => place();
    window.addEventListener('pointerdown', onDown, true);
    window.addEventListener('keydown', onKey);
    window.addEventListener('resize', onMove);
    return () => {
      window.removeEventListener('pointerdown', onDown, true);
      window.removeEventListener('keydown', onKey);
      window.removeEventListener('resize', onMove);
    };
  });
</script>

<div class="poll-card" class:multiple={poll.multiple} class:on-accent={onAccent}>
  <p class="poll-question">{poll.question}</p>
  <p class="poll-mode">
    <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
      <circle cx="12" cy="12" r="9" />
      <path d="m8.5 12.2 2.4 2.4 4.6-4.8" />
    </svg>
    {poll.multiple ? 'Select multiple' : 'Select one'}
  </p>

  <ul class="poll-options">
    {#each poll.options as option (option.id)}
      {@const share = pollShare(option, total)}
      <li class="poll-option" class:mine={option.mine}>
        <span class="poll-mark" class:checked={option.mine} aria-hidden="true">
          {#if option.mine}
            <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3.2" stroke-linecap="round" stroke-linejoin="round">
              <path d="m5 12.5 4.6 4.6L19 7" />
            </svg>
          {/if}
        </span>
        <span class="poll-label">
          <span class="poll-text">{option.text}</span>
          <span class="poll-count">
            {#if option.votes > 0}
              <span class="poll-chips">
                {#each chipsFor(option) as voter (voter.id)}
                  <span class="poll-chip" class:mine={option.mine} title={pollVoterName(voter, false)}>{pollInitials(voter)}</span>
                {/each}
                {#if option.voters.length > 3}<span class="poll-more">+{option.voters.length - 3}</span>{/if}
              </span>
            {/if}
            <span class="poll-number">{option.votes}</span>
          </span>
        </span>
        <span class="poll-bar">
          <span class="poll-fill" style="width: {(share * 100).toFixed(1)}%"></span>
        </span>
      </li>
    {/each}
  </ul>

  {#if trailing}{@render trailing(msg)}{/if}

  <button
    type="button"
    class="poll-views"
    class:open
    bind:this={viewsBtn}
    onclick={toggle}
    aria-expanded={open}
    aria-label="View votes"
  >
    View votes
  </button>
</div>

{#if open}
  <div
    class="poll-panel"
    bind:this={panel}
    style="left: {pos.left}px; top: {pos.top}px"
    role="dialog"
    aria-label="Poll votes"
  >
    <p class="poll-panel-title">{poll.question}</p>
    <p class="poll-panel-sub">{total} {total === 1 ? 'vote' : 'votes'}</p>
    {#each poll.options as option (option.id)}
      <div class="poll-panel-row">
        <p class="poll-panel-option">
          <span class="poll-panel-text">{option.text}</span>
          <span class="poll-number">{option.votes}</span>
        </p>
        {#if option.voters.length}
          <p class="poll-panel-voters">
            {#each option.voters as voter, i (voter.id)}<span class="poll-panel-voter">{pollVoterName(voter, option.mine)}</span>{#if i < option.voters.length - 1}<span class="poll-comma">,</span>{/if}{/each}
          </p>
        {:else}
          <p class="poll-panel-voters empty">No votes yet</p>
        {/if}
      </div>
    {/each}
  </div>
{/if}

<style>
  .poll-card {
    --poll-accent: var(--success, #25d366);
    /* The "View votes" label sits on the bubble itself, so it follows the bubble's text
       colour there instead of a tint that would wash out on a saturated bubble. */
    --poll-action: var(--success, #25d366);
    --poll-track: color-mix(in oklch, currentColor 20%, transparent);
    --poll-ring: color-mix(in oklch, currentColor 45%, transparent);
    --poll-divider: color-mix(in oklch, currentColor 18%, transparent);
    display: flex;
    flex-direction: column;
    min-width: min(240px, 62vw);
    max-width: 420px;
    padding-bottom: 2px;
  }

  /* On an accent-coloured bubble the theme green can sit on its own colour, so pull the
     rings and bars toward the bubble's text colour. Green still reads, light and dark. */
  .poll-card.on-accent {
    --poll-accent: color-mix(in oklch, var(--success, #25d366) 55%, currentColor);
    --poll-action: currentColor;
  }

  .poll-question {
    margin: 2px 0 0;
    font-size: 15px;
    font-weight: 600;
    line-height: 1.35;
    overflow-wrap: anywhere;
  }

  .poll-mode {
    display: flex;
    align-items: center;
    gap: 5px;
    margin: 5px 0 2px;
    font-size: 12px;
    opacity: 0.78;
  }

  .poll-options {
    list-style: none;
    margin: 10px 0 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 14px;
  }

  .poll-option {
    display: grid;
    grid-template-columns: 18px minmax(0, 1fr);
    column-gap: 9px;
    row-gap: 6px;
    align-items: start;
  }

  .poll-mark {
    width: 18px;
    height: 18px;
    margin-top: 1px;
    border-radius: 50%;
    border: 1.6px solid var(--poll-ring);
    display: grid;
    place-items: center;
    color: #fff;
    box-sizing: border-box;
  }

  .multiple .poll-mark {
    border-radius: 5px;
  }

  .poll-mark.checked {
    background: var(--poll-accent);
    border-color: var(--poll-accent);
  }

  .poll-label {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    min-height: 20px;
    font-size: 14px;
    line-height: 1.35;
  }

  .poll-text {
    overflow-wrap: anywhere;
  }

  .poll-count {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    flex: none;
    font-variant-numeric: tabular-nums;
  }

  .poll-chips {
    display: inline-flex;
    align-items: center;
    gap: 3px;
  }

  .poll-chip {
    width: 17px;
    height: 17px;
    border-radius: 50%;
    display: grid;
    place-items: center;
    font-size: 7px;
    line-height: 1;
    font-weight: 700;
    letter-spacing: -0.2px;
    /* Voter photos are not part of a poll: the chip carries initials on a muted disc. */
    background: color-mix(in oklch, currentColor 18%, transparent);
    color: inherit;
    border: 1px solid var(--poll-track);
  }

  .poll-more {
    font-size: 11px;
    opacity: 0.75;
  }

  .poll-number {
    font-size: 12px;
    opacity: 0.85;
  }

  .poll-bar {
    grid-column: 1 / -1;
    height: 4px;
    border-radius: 999px;
    background: var(--poll-track);
    overflow: hidden;
  }

  .poll-fill {
    display: block;
    height: 100%;
    border-radius: inherit;
    background: var(--poll-accent);
    transition: width 140ms ease-out;
  }

  .poll-views {
    margin-top: 8px;
    padding: 7px 8px 3px;
    border: 0;
    border-top: 1px solid var(--poll-divider);
    background: none;
    color: var(--poll-action);
    font: inherit;
    font-size: 13px;
    font-weight: 600;
    cursor: pointer;
    border-radius: var(--radius-sm, 6px);
  }

  .poll-views:hover,
  .poll-views.open {
    background: color-mix(in oklch, var(--poll-action) 14%, transparent);
  }

  .poll-panel {
    position: fixed;
    z-index: 60;
    width: min(300px, calc(100vw - 16px));
    max-height: min(50vh, 420px);
    overflow-y: auto;
    padding: 12px 14px;
    border-radius: var(--radius-md, 10px);
    border: 1px solid var(--border);
    background: var(--bg-panel, #fff);
    color: var(--text, #111);
    box-shadow: var(--shadow-md, 0 6px 20px rgba(0, 0, 0, 0.25));
  }

  .poll-panel-title {
    margin: 0;
    font-size: 13px;
    font-weight: 600;
    overflow-wrap: anywhere;
  }

  .poll-panel-sub {
    margin: 2px 0 10px;
    font-size: 12px;
    color: var(--text-muted, #71717a);
  }

  .poll-panel-row {
    padding: 8px 0;
    border-top: 1px solid var(--border-subtle, #ececee);
  }

  .poll-panel-option {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 8px;
    margin: 0;
    font-size: 13px;
    font-weight: 600;
  }

  .poll-panel-text {
    overflow-wrap: anywhere;
  }

  .poll-panel-voters {
    margin: 4px 0 0;
    font-size: 12px;
    line-height: 1.45;
    color: var(--text-secondary, #3f3f46);
  }

  .poll-panel-voters.empty {
    color: var(--text-muted, #71717a);
  }

  .poll-comma {
    margin-right: 4px;
  }
</style>
