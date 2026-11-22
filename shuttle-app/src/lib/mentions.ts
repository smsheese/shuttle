/**
 * @mentions in group messages. WhatsApp writes a mention as `@<digits>` (a phone number or a
 * LID) in the text; the connector reports who each token is as `metadata.mentions`
 * (`{ token, jid, self? }`). Names are resolved here from contacts and message senders.
 * Output is plain segments, so the view renders them as text nodes (nothing is HTML).
 */
import type { Contact, Message } from '$lib/types';
import { looksLikeJid } from '$lib/messageMedia';
import { formatPhone } from '$lib/senderIdentity';

export interface MentionFact {
  token: string;
  jid: string;
  self?: boolean;
}

export type TextSegment = { text: string } | { mention: string; self: boolean };

export interface MentionResolver {
  /** Names by the digits of a person's JID (phone and LID alike). */
  names: Map<string, string>;
  /** Digits of this account's own number. */
  selfUser: string;
}

const TOKEN = /\d{5,16}/;
const PHONEISH = /^\+?[\d\s().-]{6,}$/;

/** Digits of a JID: no domain, no `:device` or `.agent` suffix. */
export function jidUser(jid: string | null | undefined): string {
  const local = String(jid ?? '').trim().split('@', 1)[0];
  return local.split(':', 1)[0].split('.', 1)[0].replace(/^\+/, '');
}

function usableName(name: string | null | undefined): string {
  const n = (name ?? '').trim();
  return n && !looksLikeJid(n) && !PHONEISH.test(n) ? n : '';
}

/** Saved contact names win over push names taken from messages. */
export function buildMentionResolver(
  contacts: Contact[],
  messages: Message[],
  identity: string | null | undefined
): MentionResolver {
  const names = new Map<string, string>();
  for (const msg of messages) {
    if (msg.direction !== 'inbound') continue;
    const user = jidUser(msg.sender_id);
    const name = usableName(msg.sender_name);
    if (user && name) names.set(user, name);
  }
  for (const c of contacts) {
    const user = jidUser(c.remote_id);
    const name = usableName(c.display_name);
    if (user && name) names.set(user, name);
  }
  return { names, selfUser: jidUser(identity) };
}

export function mentionFacts(msg: Message): Map<string, MentionFact> {
  const out = new Map<string, MentionFact>();
  const raw = msg.metadata?.mentions;
  if (!Array.isArray(raw)) return out;
  for (const item of raw) {
    if (item && typeof item === 'object' && typeof (item as MentionFact).token === 'string') {
      out.set((item as MentionFact).token, item as MentionFact);
    }
  }
  return out;
}

function mentionName(
  digits: string,
  fact: MentionFact | undefined,
  resolver: MentionResolver
): { label: string; self: boolean } | null {
  const factUser = jidUser(fact?.jid);
  if (fact?.self || (resolver.selfUser && (digits === resolver.selfUser || factUser === resolver.selfUser))) {
    return { label: 'You', self: true };
  }
  const known = resolver.names.get(factUser) ?? resolver.names.get(digits);
  if (known) return { label: known, self: false };
  // No name: a phone number reads as one; a LID with no known phone is left as written.
  const phone = factUser && fact?.jid.endsWith('@s.whatsapp.net') ? factUser : digits.length <= 13 ? digits : '';
  return phone ? { label: formatPhone(phone), self: false } : null;
}

/** Split message text into plain text and resolved mentions; unchanged text otherwise. */
export function mentionSegments(msg: Message, text: string, resolver: MentionResolver): TextSegment[] {
  if (!text.includes('@')) return [{ text }];
  const facts = mentionFacts(msg);
  const re = new RegExp(`(^|[^\\w@])@(${TOKEN.source})(?!\\d)`, 'g');
  const out: TextSegment[] = [];
  let last = 0;
  let m: RegExpExecArray | null;
  while ((m = re.exec(text))) {
    const start = m.index + m[1].length;
    const resolved = mentionName(m[2], facts.get(m[2]), resolver);
    if (!resolved) continue;
    if (start > last) out.push({ text: text.slice(last, start) });
    out.push({ mention: resolved.label, self: resolved.self });
    last = start + 1 + m[2].length;
  }
  if (last === 0) return [{ text }];
  if (last < text.length) out.push({ text: text.slice(last) });
  return out;
}

// ---- Composing a mention ---------------------------------------------------------------------

export interface MentionCandidate {
  name: string;
  /** Digits of the person's JID, as WhatsApp writes the token (`@<digits>`). */
  user: string;
}

/** A person of the group as the connector lists them. */
export interface GroupMember {
  user: string;
  ids: string[];
}

/** The group's members (not this account) with the best name known for each. */
export function mentionCandidates(members: GroupMember[], resolver: MentionResolver): MentionCandidate[] {
  const out: MentionCandidate[] = [];
  for (const m of members) {
    if (!m.user || m.ids.includes(resolver.selfUser)) continue;
    const known = m.ids.map((id) => resolver.names.get(id)).find(Boolean);
    const name = known ?? (m.user.length <= 13 ? formatPhone(m.user) : '');
    if (name) out.push({ name, user: m.user });
  }
  return out.sort((a, b) => a.name.localeCompare(b.name));
}

/** The `@query` being typed right before the caret, or null when the caret is not in one. */
export function activeMention(text: string, caret: number): { start: number; query: string } | null {
  const m = /(^|[\s(])@([^\s@]*)$/.exec(text.slice(0, caret));
  return m ? { start: caret - m[2].length - 1, query: m[2] } : null;
}

/** Candidates whose name contains every typed word, prefix matches first. */
export function filterMentionCandidates(list: MentionCandidate[], query: string, limit = 8): MentionCandidate[] {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  const scored: { c: MentionCandidate; rank: number }[] = [];
  for (const c of list) {
    const name = c.name.toLowerCase();
    if (!words.every((w) => name.includes(w) || c.user.includes(w))) continue;
    const rank = !words.length || name.startsWith(words[0]) || name.split(/\s+/).some((p) => p.startsWith(words[0])) ? 0 : 1;
    scored.push({ c, rank });
  }
  return scored.sort((a, b) => a.rank - b.rank).slice(0, limit).map((s) => s.c);
}

/** Replace each picked `@Name` in the draft with the `@<digits>` token WhatsApp expects. */
export function expandMentions(text: string, picked: MentionCandidate[]): string {
  let out = text;
  // Longest names first, so "@Ann Lee" is not eaten by "@Ann".
  for (const c of [...picked].sort((a, b) => b.name.length - a.name.length)) {
    const re = new RegExp(`(^|[^\\w@])@${c.name.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}(?![\\w])`, 'g');
    out = out.replace(re, (_m, pre: string) => `${pre}@${c.user}`);
  }
  return out;
}
