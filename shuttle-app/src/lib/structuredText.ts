export function innerSpan(text: string, offset: number): { inner: string; wrapper: string } | null {
  const pairs: [string, string][] = [
    ['```', '```'],
    ['`', '`'],
    ['"', '"'],
    ["'", "'"],
    ['“', '”'],
    ['(', ')'],
    ['[', ']'],
    ['{', '}'],
  ];
  let best: { inner: string; wrapper: string; size: number } | null = null;
  for (const [open, close] of pairs) {
    const left = text.lastIndexOf(open, Math.max(0, offset - 1));
    if (left < 0) continue;
    const start = left + open.length;
    const right = text.indexOf(close, Math.max(start, offset));
    if (right < 0 || right < offset) continue;
    const inner = text.slice(start, right);
    if (!inner) continue;
    const size = right - left;
    if (!best || size < best.size) {
      best = { inner, wrapper: `${open}…${close}`, size };
    }
  }
  return best ? { inner: best.inner, wrapper: best.wrapper } : null;
}

export function selectionContext(): { selected: string; inner: string | null } {
  const sel = typeof window !== 'undefined' ? window.getSelection()?.toString() ?? '' : '';
  const selected = sel.trim();
  if (!selected) return { selected: '', inner: null };
  const span = innerSpan(selected, Math.floor(selected.length / 2));
  return { selected, inner: span && span.inner !== selected ? span.inner : null };
}

export function urlsIn(text: string): string[] {
  return text.match(/https?:\/\/[^\s<>)"']+/g) ?? [];
}
