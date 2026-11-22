import type { Conversation } from './types';

export type ChatKind = 'direct' | 'group' | 'channel';

/** What a chat knows about its own name. `Conversation` satisfies this. */
export interface NameFacts {
  title?: string | null;
  remote_id?: string | null;
  conversation_type?: string | null;
  metadata?: Record<string, unknown> | null;
}

/** Labels a connector falls back to when it has no real name, plus ids stored as names. */
const PLACEHOLDER_ANY_KIND = [
  /^(group|channel|chat)$/i,
  /^(group|channel)\s*[#-]?\s*\d+$/i,
  /^newsletter[\s_#-]*\d*$/i,
  /^(unnamed|unknown)(\s+(group|channel|chat))?$/i,
  /^\d{5,}$/,
];

/** A formatted number is a usable label for a direct chat, not for a group or a channel. */
const PLACEHOLDER_GROUP_OR_CHANNEL = [/^[+\d][\d\s().-]{4,}$/];

export function chatKind(source: NameFacts): ChatKind {
  const declared = (source.conversation_type ?? '').toLowerCase();
  if (declared === 'direct' || declared === 'group' || declared === 'channel') return declared;
  const id = (source.remote_id ?? '').toLowerCase();
  if (id.endsWith('@newsletter') || id.endsWith('@broadcast')) return 'channel';
  if (id.endsWith('@g.us')) return 'group';
  return 'direct';
}

/**
 * True when a stored label carries no name: a connector placeholder such as `Group` or
 * `Newsletter 120363041234567890`, a bare id, or nothing at all.
 */
export function isPlaceholderName(raw: string | null | undefined, kind: ChatKind = 'group'): boolean {
  const text = (raw ?? '').trim();
  if (!text) return true;
  if (text.includes('@')) return true;
  if (PLACEHOLDER_ANY_KIND.some((re) => re.test(text))) return true;
  return kind !== 'direct' && PLACEHOLDER_GROUP_OR_CHANNEL.some((re) => re.test(text));
}

function reportedNames(metadata: Record<string, unknown> | null | undefined): Record<string, string> {
  const names = metadata?.names;
  if (!names || typeof names !== 'object') return {};
  const out: Record<string, string> = {};
  for (const [key, value] of Object.entries(names as Record<string, unknown>)) {
    if (typeof value === 'string' && value.trim()) out[key] = value.trim();
  }
  return out;
}

function shortId(remoteId: string | null | undefined): string {
  const bare = (remoteId ?? '').split('@')[0].replace(/[^\d+]/g, '');
  return bare.length >= 5 ? bare : '';
}

function fallbackLabel(source: NameFacts, kind: ChatKind): string {
  if (kind === 'channel') return 'Unnamed channel';
  if (kind === 'group') return 'Unnamed group';
  const id = shortId(source.remote_id);
  return id ? `+${id}` : 'Unnamed chat';
}

/**
 * The name to draw for a chat. A real name wins over every placeholder, in this order: the
 * stored title, the names the connector reported, then `alternatives` (for example the same
 * remote id seen through another account). With no real name left the label stays neutral
 * instead of repeating a placeholder.
 */
export function displayName(source: NameFacts, alternatives: Array<string | null | undefined> = []): string {
  const kind = chatKind(source);
  const names = reportedNames(source.metadata);
  const candidates =
    kind === 'direct'
      ? [source.title, names.contact, names.verified, names.push, names.subject]
      : [source.title, names.subject, names.contact, names.verified, names.push];
  for (const candidate of [...candidates, ...alternatives]) {
    if (!isPlaceholderName(candidate, kind)) return (candidate as string).trim();
  }
  return fallbackLabel(source, kind);
}

/** `displayName` for a chat, using other rows that share its remote id as extra name sources. */
export function displayNameFor(conv: Conversation, siblings: Conversation[] = []): string {
  const alternatives = siblings
    .filter((other) => other.id !== conv.id && other.remote_id === conv.remote_id)
    .map((other) => other.title);
  return displayName(conv, alternatives);
}
