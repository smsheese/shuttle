import { captionText, mediaKindFromMessage } from '$lib/messageMedia';
import type { Message, MessageActions, ReactionGroup } from '$lib/types';

/** A translation shown under a message while the chat is open. Never stored. */
export type TranslationView =
  | { state: 'loading' }
  | { state: 'done'; text: string }
  | { state: 'error'; message: string };

/** What the selection bar can do with the selected messages, in bar order. */
export const SELECTION_ACTIONS = [
  { id: 'copy', label: 'Copy' },
  { id: 'forward', label: 'Forward' },
  { id: 'star', label: 'Star' },
  { id: 'translate', label: 'Translate' },
  { id: 'delete', label: 'Delete' },
] as const;

export type SelectionAction = (typeof SELECTION_ACTIONS)[number]['id'];

/** The quick reaction bar of the message menu; the full picker covers the rest. */
export const QUICK_REACTIONS = ['👍', '❤️', '😂', '😮', '😢', '🙏'] as const;

/** Deleted for everyone: shown as a tombstone in its original place. */
export function isRevoked(msg: Message): boolean {
  return typeof msg.metadata?.revoked_at === 'string';
}

export function isEdited(msg: Message): boolean {
  return typeof msg.metadata?.edited_at === 'string' && !isRevoked(msg);
}

/** Which actions the menu offers for `msg`. Mirrors the core's guard; the core still decides. */
export interface OfferedActions {
  react: boolean;
  edit: boolean;
  deleteForMe: boolean;
  deleteForEveryone: boolean;
}

const NONE: OfferedActions = { react: false, edit: false, deleteForMe: false, deleteForEveryone: false };
const CAPTIONED = new Set(['image', 'video', 'document', 'gif']);

function within(windowSecs: number | null, sentAt: string, now: number): boolean {
  return windowSecs == null || now - Date.parse(sentAt) <= windowSecs * 1000;
}

export function offeredActions(
  actions: MessageActions | undefined,
  msg: Message,
  now: number = Date.now()
): OfferedActions {
  if (!actions || !msg.remote_id) return NONE;
  if (isRevoked(msg)) return { ...NONE, deleteForMe: actions.delete };
  const own = msg.direction === 'outbound';
  const kind = mediaKindFromMessage(msg);
  const editableText = !!captionText(msg) && (!kind || kind === 'text' || CAPTIONED.has(kind));
  return {
    react: actions.react,
    edit: actions.edit && own && editableText && within(actions.edit_window_secs, msg.timestamp, now),
    deleteForMe: actions.delete,
    deleteForEveryone:
      actions.delete_everyone && own && within(actions.delete_everyone_window_secs, msg.timestamp, now),
  };
}

/** The reaction groups after this account reacts with `emoji` (null removes its reaction). */
export function withMyReaction(groups: ReactionGroup[] | undefined, emoji: string | null): ReactionGroup[] {
  const rest = (groups ?? [])
    .map((g) => (g.mine ? { ...g, count: g.count - 1, names: g.names.filter((n) => n !== 'You'), mine: false } : g))
    .filter((g) => g.count > 0);
  if (!emoji) return rest;
  const hit = rest.find((g) => g.emoji === emoji);
  if (hit) return rest.map((g) => (g === hit ? { ...g, count: g.count + 1, names: [...g.names, 'You'], mine: true } : g));
  return [...rest, { emoji, count: 1, names: ['You'], mine: true }];
}

/** Reacting again with the emoji this account already gave removes it. */
export function toggledReaction(groups: ReactionGroup[] | undefined, emoji: string): string | null {
  return groups?.some((g) => g.mine && g.emoji === emoji) ? null : emoji;
}
