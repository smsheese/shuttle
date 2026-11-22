/**
 * WhatsApp's own message markup, parsed into plain runs and blocks. The view renders those as
 * text nodes, so no markup ever reaches the DOM as HTML.
 *
 * Supported: `*bold*`, `_italic_`, `~strike~`, `` `mono` ``, ``` fenced mono blocks, `> quote`,
 * `- ` / `* ` bullets and `1. ` ordered items.
 */

export type MarkupStyle = 'bold' | 'italic' | 'strike' | 'mono';

export interface MarkupRun {
  text: string;
  styles: MarkupStyle[];
}

export type MarkupBlock =
  | { kind: 'text'; runs: MarkupRun[] }
  | { kind: 'quote'; runs: MarkupRun[] }
  | { kind: 'bullet'; runs: MarkupRun[] }
  | { kind: 'ordered'; marker: string; runs: MarkupRun[] }
  | { kind: 'code'; text: string };

const STYLES: { [marker: string]: MarkupStyle } = {
  '*': 'bold',
  _: 'italic',
  '~': 'strike',
  '`': 'mono',
};

const FENCE = '```';
const WORD = /[\p{L}\p{N}]/u;
const BULLET = /^[-*]\s+(?=\S)/;
const ORDERED = /^(\d{1,3})[.)]\s+(?=\S)/;
const QUOTE = /^>\s+/;

function styleOf(marker: string): MarkupStyle | null {
  return Object.hasOwn(STYLES, marker) ? STYLES[marker] : null;
}

/**
 * A marker opens a run only at a word boundary (so `snake_case` and `2*3*4` stay literal) and
 * only when text follows it. `***` is not a marker run.
 */
function opens(text: string, at: number): boolean {
  const marker = text[at];
  const next = text[at + 1];
  if (next === undefined || next === marker || /\s/.test(next)) return false;
  const prev = text[at - 1];
  return prev === undefined || !WORD.test(prev);
}

/** The index of the marker that closes the run opened at `from`, or -1 for a literal marker. */
function closing(text: string, from: number): number {
  const marker = text[from];
  for (let at = from + 1; at < text.length; at += 1) {
    if (text[at] !== marker) continue;
    if (text[at - 1] === marker) continue;
    if (at === from + 1 || /\s/.test(text[at - 1])) continue;
    return at;
  }
  return -1;
}

/** Inline runs of one line of text. A mono run holds literal text: markup inside it is not parsed. */
export function markupRuns(text: string, styles: MarkupStyle[] = []): MarkupRun[] {
  const runs: MarkupRun[] = [];
  let literal = '';
  let at = 0;

  const flush = () => {
    if (!literal) return;
    runs.push({ text: literal, styles });
    literal = '';
  };

  while (at < text.length) {
    if (text.startsWith(FENCE, at)) {
      const close = text.indexOf(FENCE, at + FENCE.length);
      if (close > at + FENCE.length) {
        flush();
        runs.push({ text: text.slice(at + FENCE.length, close), styles: [...styles, 'mono'] });
        at = close + FENCE.length;
        continue;
      }
    }
    const marker = text[at];
    const style = styleOf(marker);
    if (!style || !opens(text, at)) {
      literal += marker;
      at += 1;
      continue;
    }
    const close = closing(text, at);
    if (close < 0) {
      literal += marker;
      at += 1;
      continue;
    }
    flush();
    const inner = text.slice(at + 1, close);
    if (style === 'mono') runs.push({ text: inner, styles: [...styles, style] });
    else runs.push(...markupRuns(inner, [...styles, style]));
    at = close + 1;
  }
  flush();
  return runs;
}

/**
 * A message body as blocks. Plain lines stay in one text block, so a message with no markup
 * renders exactly as it did before (newlines preserved by the view).
 */
export function whatsappBlocks(text: string): MarkupBlock[] {
  const blocks: MarkupBlock[] = [];
  const lines = text.replace(/\r\n?/g, '\n').split('\n');
  let plain: string[] = [];
  let fence: string[] | null = null;

  const flush = () => {
    if (!plain.length) return;
    const joined = plain.join('\n');
    plain = [];
    if (joined.trim()) blocks.push({ kind: 'text', runs: markupRuns(joined) });
  };

  for (const line of lines) {
    const trimmed = line.trim();
    if (fence) {
      if (trimmed.startsWith(FENCE)) {
        blocks.push({ kind: 'code', text: fence.join('\n') });
        fence = null;
      } else {
        fence.push(line);
      }
      continue;
    }
    // A line opens a block only when it has no second fence on it: ```npm test``` is inline.
    if (trimmed.startsWith(FENCE) && !trimmed.slice(FENCE.length).trim().includes(FENCE)) {
      flush();
      fence = [];
      continue;
    }
    const ordered = ORDERED.exec(line);
    if (ordered) {
      flush();
      blocks.push({ kind: 'ordered', marker: `${ordered[1]}.`, runs: markupRuns(line.slice(ordered[0].length)) });
      continue;
    }
    if (BULLET.test(line)) {
      flush();
      blocks.push({ kind: 'bullet', runs: markupRuns(line.replace(BULLET, '')) });
      continue;
    }
    if (QUOTE.test(line)) {
      flush();
      blocks.push({ kind: 'quote', runs: markupRuns(line.replace(QUOTE, '')) });
      continue;
    }
    plain.push(line);
  }
  if (fence) blocks.push({ kind: 'code', text: fence.join('\n') });
  flush();
  return blocks;
}
