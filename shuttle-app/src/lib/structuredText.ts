import { mentionSegments, type MentionResolver, type TextSegment } from './mentions';
import type { Message } from './types';

/** A link in a body, on top of the plain text and @mention segments. */
export type BodySegment = TextSegment | { url: string };

const URL_RE = /https?:\/\/[^\s<>)"']+/g;

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
  return urlSpans(text).map((span) => span.url);
}

/** Every link in `text`, with where it starts. Trailing sentence punctuation is not the link. */
export function urlSpans(text: string): { url: string; start: number }[] {
  const out: { url: string; start: number }[] = [];
  for (const m of text.matchAll(URL_RE)) {
    const url = m[0].replace(/[.,;:!?]+$/, '');
    if (url.length > 'https://'.length) out.push({ url, start: m.index ?? 0 });
  }
  return out;
}

/**
 * A message body as segments: plain text, resolved @mentions and links, so the view renders
 * text nodes and anchors (nothing is HTML).
 */
export function bodySegments(msg: Message, text: string, resolver: MentionResolver): BodySegment[] {
  const spans = urlSpans(text);
  if (!spans.length) return mentionSegments(msg, text, resolver);
  const out: BodySegment[] = [];
  let last = 0;
  for (const span of spans) {
    if (span.start > last) out.push(...mentionSegments(msg, text.slice(last, span.start), resolver));
    out.push({ url: span.url });
    last = span.start + span.url.length;
  }
  if (last < text.length) out.push(...mentionSegments(msg, text.slice(last), resolver));
  return out;
}
