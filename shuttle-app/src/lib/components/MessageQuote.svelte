<script lang="ts">
  /**
   * Small quoted block for another message: the one a todo or note came from, the one a reply
   * answers (inside a bubble, `in` / `out`), or the one the composer is about to answer.
   */
  interface Props {
    snippet: string;
    /** Author of the quoted message ("You" for the account's own). */
    sender?: string | null;
    /** Preview image of the quoted message (an image, sticker or video thumbnail). */
    thumb?: string | null;
    /** `note`: todos and notes. `in` / `out`: inside an inbound / outbound bubble. */
    variant?: 'note' | 'in' | 'out' | 'composer';
    /** Jump to the quoted message in the thread. Without it the block is plain text. */
    onclick?: () => void;
    /** Shown on hover when the block is a link. */
    title?: string;
  }
  let { snippet, sender = null, thumb = null, variant = 'note', onclick, title = 'Show in chat' }: Props = $props();
</script>

{#snippet content()}
  <span class="body">
    {#if sender}<span class="who">{sender}</span>{/if}
    <span class="text">{snippet}</span>
  </span>
  {#if thumb}<img class="thumb" src={thumb} alt="" draggable="false" />{/if}
{/snippet}

{#if onclick}
  <button type="button" class="quote {variant}" {title} {onclick}>{@render content()}</button>
{:else}
  <div class="quote {variant} static">{@render content()}</div>
{/if}

<style>
  .quote {
    display: flex;
    align-items: stretch;
    gap: 8px;
    width: 100%;
    box-sizing: border-box;
    margin: 0;
    padding: 4px 8px;
    text-align: left;
    font: inherit;
    font-size: 12px;
    color: var(--text-muted);
    background: var(--bg-hover);
    border: none;
    border-left: 3px solid var(--accent);
    border-radius: var(--radius-sm, 6px);
    cursor: pointer;
  }
  .quote.static {
    cursor: default;
  }
  button.quote:hover {
    color: var(--text);
  }
  button.quote:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 1px;
  }
  .body {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 1px;
  }
  .who {
    font-size: 11px;
    font-weight: 600;
    color: var(--accent);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .text {
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
    word-break: break-word;
  }
  .thumb {
    flex: none;
    width: 38px;
    height: 38px;
    align-self: center;
    object-fit: cover;
    border-radius: 4px;
    margin: -1px -4px -1px 0;
  }

  /* Inside a bubble: a tint of the bubble's own text colour, so it reads on any theme. */
  .quote.in,
  .quote.out {
    margin-bottom: 4px;
    padding: 5px 8px;
    border-radius: 8px;
    color: inherit;
    background: color-mix(in srgb, var(--text) 8%, transparent);
    border-left-color: var(--accent);
  }
  .quote.in .text {
    color: var(--text-secondary, var(--text-muted));
  }
  button.quote.in:hover {
    background: color-mix(in srgb, var(--text) 13%, transparent);
    color: inherit;
  }
  .quote.out {
    --on: var(--text-on-accent, var(--primary-foreground, #ffffff));
    background: color-mix(in srgb, var(--on) 16%, transparent);
    border-left-color: color-mix(in srgb, var(--on) 80%, transparent);
  }
  .quote.out .who {
    color: var(--on);
  }
  .quote.out .text {
    color: color-mix(in srgb, var(--on) 88%, transparent);
  }
  button.quote.out:hover {
    background: color-mix(in srgb, var(--on) 24%, transparent);
    color: inherit;
  }

  /* The bar above the composer: the quote fills it, the cancel button sits beside it. */
  .quote.composer {
    background: var(--bg-input);
    padding: 6px 10px;
    border-radius: var(--radius-sm, 6px);
  }
  .quote.composer .text {
    -webkit-line-clamp: 1;
    line-clamp: 1;
  }
</style>
