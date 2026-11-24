/** Svelte action: call `fn` once, the first time the element scrolls into view. */
export function onceInView(node: HTMLElement, fn: () => void) {
  if (typeof IntersectionObserver === 'undefined') {
    fn();
    return {};
  }
  const observer = new IntersectionObserver(
    (entries) => {
      if (entries.some((e) => e.isIntersecting)) {
        observer.disconnect();
        fn();
      }
    },
    { rootMargin: '200px' }
  );
  observer.observe(node);
  return { destroy: () => observer.disconnect() };
}
