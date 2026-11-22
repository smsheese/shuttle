<script lang="ts">
  /** Small arrow before a chat-list preview: up = I sent it, down = I received it. */
  let { fromMe }: { fromMe: boolean } = $props();
</script>

<svg
  class="dir"
  viewBox="0 0 24 24"
  width="1em"
  height="1em"
  fill="none"
  stroke="currentColor"
  stroke-width="2.4"
  stroke-linecap="round"
  stroke-linejoin="round"
  role="img"
  aria-label={fromMe ? 'You sent' : 'Received'}
>
  {#if fromMe}
    <path d="M12 19V5M5.5 11.5 12 5l6.5 6.5" />
  {:else}
    <path d="M12 5v14M5.5 12.5 12 19l6.5-6.5" />
  {/if}
</svg>

<style>
  .dir {
    flex: 0 0 auto;
    width: 0.95em;
    height: 0.95em;
    color: var(--text-muted);
    opacity: 0.85;
    /* Sits on the text's x-height centre line. */
    vertical-align: -0.12em;
  }
</style>
