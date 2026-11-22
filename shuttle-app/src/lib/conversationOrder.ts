import type { Conversation } from './types';

/** Last-activity time in ms. Chats without a valid time return 0, so they sort last. */
export function convTime(conv: Conversation): number {
  if (!conv.last_message_at) return 0;
  const t = new Date(conv.last_message_at).getTime();
  return Number.isFinite(t) ? t : 0;
}

/**
 * Chat-list order, shared by every list: pinned first, then most recent activity
 * (chats without a time last), then title case-insensitively, then id.
 */
export function compareConversations(a: Conversation, b: Conversation): number {
  if (a.pinned !== b.pinned) return a.pinned ? -1 : 1;
  const byTime = convTime(b) - convTime(a);
  if (byTime !== 0) return byTime;
  const byTitle = (a.title || '').localeCompare(b.title || '', undefined, { sensitivity: 'base' });
  if (byTitle !== 0) return byTitle;
  return a.id < b.id ? -1 : a.id > b.id ? 1 : 0;
}
