import { captionText, mediaFilename, mediaKindFromMessage } from '$lib/messageMedia';
import type { Message } from '$lib/types';

const SNIPPET_CHARS = 80;

/**
 * One line for a quoted message: its text, or a label for media ("📷 Photo",
 * "📄 report.pdf"), with the caption after the label.
 */
export function quoteLabel(kind: string | null | undefined, text: string, filename?: string | null): string {
  const clean = text.replace(/\s+/g, ' ').trim();
  if (kind) {
    const label =
      kind === 'image' || kind === 'photo'
        ? '📷 Photo'
        : kind === 'sticker'
          ? '📷 Sticker'
          : kind === 'video'
            ? '🎥 Video'
            : kind === 'audio' || kind === 'ptt'
              ? '🎤 Voice message'
              : kind === 'document'
                ? `📄 ${filename ?? 'Document'}`
                : kind === 'location'
                  ? '📍 Location'
                  : `[${kind}]`;
    return clean ? `${label} · ${clip(clean, SNIPPET_CHARS - label.length - 3)}` : label;
  }
  return clip(clean, SNIPPET_CHARS);
}

/** Short quote of a message for todos, notes and the reply bar: its text, or a label for media. */
export function messageSnippet(msg: Message): string {
  const kind = mediaKindFromMessage(msg);
  const text = captionText(msg);
  return quoteLabel(kind, kind ? text : text || (msg.body || '').trim(), mediaFilename(msg));
}

function clip(s: string, max: number): string {
  const chars = Array.from(s);
  return chars.length > max ? `${chars.slice(0, Math.max(1, max - 1)).join('')}…` : s;
}
