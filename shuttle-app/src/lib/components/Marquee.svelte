<script lang="ts">
  let { text, class: className = '' }: { text: string; class?: string } = $props();

  let box = $state<HTMLElement | null>(null);
  let inner = $state<HTMLElement | null>(null);
  let distance = $state(0);

  function measure() {
    if (!box || !inner) return;
    distance = Math.max(0, Math.ceil(inner.scrollWidth - box.clientWidth));
  }

  $effect(() => {
    void text;
    if (!box) return;
    queueMicrotask(measure);
    if (typeof ResizeObserver === 'undefined') return;
    const ro = new ResizeObserver(measure);
    ro.observe(box);
    return () => ro.disconnect();
  });
</script>

<span class="marquee {className}" bind:this={box} title={distance > 0 ? text : undefined}>
  <span
    class="inner"
    class:scrolls={distance > 0}
    bind:this={inner}
    style:--d="{distance}px"
    style:--t="{Math.max(3, distance / 30)}s"
  >{text}</span>
</span>

<style>
  .marquee {
    display: block;
    overflow: hidden;
    white-space: nowrap;
    min-width: 0;
  }
  .inner {
    display: inline-block;
  }
  .inner.scrolls {
    animation: marquee var(--t) linear 1.2s infinite alternate;
  }
  @keyframes marquee {
    from {
      transform: translateX(0);
    }
    to {
      transform: translateX(calc(-1 * var(--d)));
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .marquee {
      text-overflow: ellipsis;
    }
    .inner.scrolls {
      animation: none;
    }
  }
</style>
