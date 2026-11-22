import type { Conversation } from './types';

/**
 * A chat that refuses this account's messages: a group where only admins may post.
 * The connector reports the two facts; a chat that never reported them stays writable,
 * so old rows, direct chats and other networks keep today's composer.
 */
export function composeLocked(conv: Pick<Conversation, 'metadata'>): boolean {
  const meta = conv.metadata;
  return meta?.announce_only === true && meta.self_is_admin !== true;
}
