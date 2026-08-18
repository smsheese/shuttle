export type RichTextTarget = 'whatsapp' | 'telegram' | 'signal' | 'email' | 'messenger' | 'instagram' | string;

const MARKS = {
  bold: ['**', '**'],
  italic: ['_', '_'],
  strike: ['~', '~'],
  code: ['`', '`'],
} as const;

export function wrapSelection(
  text: string,
  start: number,
  end: number,
  mark: keyof typeof MARKS
): { text: string; start: number; end: number } {
  const [open, close] = MARKS[mark];
  const from = Math.min(start, end);
  const to = Math.max(start, end);
  const selected = text.slice(from, to) || 'text';
  const next = `${text.slice(0, from)}${open}${selected}${close}${text.slice(to)}`;
  return {
    text: next,
    start: from + open.length,
    end: from + open.length + selected.length,
  };
}

export function normalizeRichText(text: string, target: RichTextTarget): string {
  if (target === 'messenger' || target === 'instagram') {
    return stripMarkup(text);
  }
  if (target === 'email') {
    return text;
  }
  if (target === 'whatsapp' || target === 'telegram' || target === 'signal') {
    return text;
  }
  return stripMarkup(text);
}

export function stripMarkup(text: string): string {
  return text
    .replace(/\*\*(.*?)\*\*/g, '$1')
    .replace(/_(.*?)_/g, '$1')
    .replace(/~(.*?)~/g, '$1')
    .replace(/`(.*?)`/g, '$1');
}
