/**
 * How a group member is shown on a message bubble: a readable name (with fallbacks) and a
 * stable colour. Pure helpers, shared by every bubble kind so text, media and links agree.
 */
import { looksLikeJid } from '$lib/messageMedia';
import type { Conversation } from '$lib/types';

interface SenderFields {
  sender_id?: string | null;
  sender_name?: string | null;
}

/** "+91 97426 04484" for a 10-digit national number, "+<digits>" otherwise. */
export function formatPhone(digits: string): string {
  const d = digits.replace(/\D/g, '');
  if (!d) return '';
  if (d.length >= 11) return `+${d.slice(0, d.length - 10)} ${d.slice(-10, -5)} ${d.slice(-5)}`;
  return `+${d}`;
}

const PHONEISH = /^\+?[\d\s().-]{6,}$/;

/**
 * Name to print above an inbound group message: the push/contact name when there is one,
 * else the formatted phone number. Never a raw JID, never the group's own number.
 */
export function senderLabel(msg: SenderFields): string {
  const id = (msg.sender_id ?? '').trim();
  const user = id.split('@')[0];
  const name = (msg.sender_name ?? '').trim();
  const isPerson = id.endsWith('@s.whatsapp.net') || id.endsWith('@c.us') || (!!id && !id.includes('@'));
  const nameIsId = !name || looksLikeJid(name) || (!!user && name === user) || PHONEISH.test(name);
  if (!nameIsId) return name;
  if (isPerson && user) return formatPhone(user);
  if (!id && name && PHONEISH.test(name)) return formatPhone(name);
  // A LID, or a group JID stored by an older build: there is no number to show.
  return name && !looksLikeJid(name) && !(!!user && name === user) ? name : 'Group member';
}

/** Stable key per sender: the same person always gets the same colour. */
export function senderKey(msg: SenderFields): string {
  const id = (msg.sender_id ?? '').trim();
  return id ? id.split('@')[0] : (msg.sender_name ?? '').trim();
}

/** Hue 0-359 from an FNV-1a hash of the sender key. */
export function senderHue(key: string): number {
  let h = 0x811c9dc5;
  for (let i = 0; i < key.length; i++) {
    h ^= key.charCodeAt(i);
    h = Math.imul(h, 0x01000193) >>> 0;
  }
  return h % 360;
}

const PREVIEW_NAME_MAX = 14;

/**
 * "Samah: " for the chat-list preview of a group whose last message came from someone else.
 * Contact or push name first (first word, truncated), else the formatted phone number.
 * Empty for direct chats, channels, my own messages (the up arrow says it) and unknown senders.
 */
export function previewSenderPrefix(
  conv: Pick<
    Conversation,
    'conversation_type' | 'last_message_from_me' | 'last_message_sender_id' | 'last_message_sender_name'
  >
): string {
  if (conv.conversation_type !== 'group' || conv.last_message_from_me !== false) return '';
  const label = senderLabel({
    sender_id: conv.last_message_sender_id,
    sender_name: conv.last_message_sender_name,
  });
  if (!label || label === 'Group member') return '';
  const short = label.startsWith('+') ? label : (label.split(/\s+/)[0] ?? label);
  const clipped = short.length > PREVIEW_NAME_MAX ? `${short.slice(0, PREVIEW_NAME_MAX - 1)}…` : short;
  return `${clipped}: `;
}
