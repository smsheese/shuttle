import {
  copyMediaToClipboard,
  downloadMessageMediaAndWait,
  saveMediaFile,
  type MediaNameInfo,
  type MediaNameOwner,
} from './api';
import { mediaFilename, mediaKindFromMessage, mediaPathFromMessage } from './messageMedia';
import { dismissToast, toast } from './toast.svelte';
import type { Account, AutoDownloadGroup, AutoDownloadPrefs, Conversation, Message } from './types';

/** Account label and chat title for a conversation, in one place so every save and copy agrees. */
export function mediaNameOwner(
  conversation: Pick<Conversation, 'account_id' | 'title'> | null | undefined,
  accounts: readonly Account[]
): MediaNameOwner {
  if (!conversation) return {};
  return {
    accountName: accounts.find((a) => a.id === conversation.account_id)?.name ?? null,
    chatName: conversation.title || null,
  };
}

/** Built-in auto-download choices when neither the chat nor the account decided. */
export const AUTO_DOWNLOAD_DEFAULTS: Required<{ [K in AutoDownloadGroup]: boolean }> = {
  images: true,
  videos: false,
  files: false,
};

/** Stickers always load; voice notes and audio follow Files; contacts, polls etc. never download. */
export function autoDownloadGroup(kind: string | null | undefined): AutoDownloadGroup | null {
  switch ((kind ?? '').toLowerCase()) {
    case 'image':
    case 'photo':
    case 'sticker':
      return 'images';
    case 'video':
      return 'videos';
    case 'document':
    case 'audio':
    case 'ptt':
      return 'files';
    default:
      return null;
  }
}

/** Effective choice for one group: chat override, else account default, else built-in. */
export function effectiveAutoDownload(
  group: AutoDownloadGroup,
  chat?: AutoDownloadPrefs | null,
  account?: AutoDownloadPrefs | null
): boolean {
  return chat?.[group] ?? account?.[group] ?? AUTO_DOWNLOAD_DEFAULTS[group];
}

/** Whether a message's media should download by itself (voice play always downloads on demand). */
export function shouldAutoDownload(
  kind: string | null | undefined,
  chat?: AutoDownloadPrefs | null,
  account?: AutoDownloadPrefs | null
): boolean {
  // A sticker is not a file the reader chooses to fetch: it is the message. It always loads.
  if ((kind ?? '').toLowerCase() === 'sticker') return true;
  const group = autoDownloadGroup(kind);
  return group !== null && effectiveAutoDownload(group, chat, account);
}

/**
 * Save a message's media: download it first when needed, copy it to the save folder (or the
 * path the native dialog picks), then toast where it went. Never navigates the webview.
 */
export async function saveMessageMediaFile(
  accountId: string,
  conversationId: string,
  msg: Message,
  owner: MediaNameOwner = {}
): Promise<void> {
  try {
    let source = mediaPathFromMessage(msg);
    if (!source) {
      const downloaded = await downloadMessageMediaAndWait(accountId, conversationId, msg.id);
      source = mediaPathFromMessage(downloaded);
    }
    if (!source) throw new Error('The media is not available to save.');
    const saved = await saveMediaFile(source, mediaNameInfo(msg, owner));
    if (saved) toast.saved(saved);
  } catch (e) {
    toast.error(`Could not save: ${e instanceof Error ? e.message : String(e)}`);
  }
}

/** What names a copied or saved file: its kind, WhatsApp's file name and the message time. */
export function mediaNameInfo(msg: Message, owner: MediaNameOwner = {}): MediaNameInfo {
  return {
    kind: mediaKindFromMessage(msg),
    filename: mediaFilename(msg),
    timestamp: msg.timestamp,
    accountName: owner.accountName ?? null,
    chatName: owner.chatName ?? null,
  };
}

/** Save an already-downloaded file (lightbox / contact media). */
export async function saveCachedMediaFile(path: string, name: MediaNameInfo = {}): Promise<void> {
  try {
    const saved = await saveMediaFile(path, name);
    if (saved) toast.saved(saved);
  } catch (e) {
    toast.error(`Could not save: ${e instanceof Error ? e.message : String(e)}`);
  }
}

/**
 * Copy an already-downloaded media file to the system clipboard (native route, so it pastes
 * into other apps) and toast the result. Never silent: an unsupported OS gets the path as
 * text and says so.
 */
export async function copyCachedMediaToClipboard(
  path: string,
  name: MediaNameInfo = {}
): Promise<void> {
  try {
    const kind = await copyMediaToClipboard(path, name);
    if (kind === 'path_text') {
      toast.copied('Copied the file path (this system could not copy the file itself)');
    } else {
      toast.copied();
    }
  } catch (e) {
    toast.error(`Could not copy: ${e instanceof Error ? e.message : String(e)}`);
  }
}

/** Copy a message's media to the clipboard, downloading it first when needed. */
export async function copyMessageMediaToClipboard(
  accountId: string,
  conversationId: string,
  msg: Message,
  owner: MediaNameOwner = {}
): Promise<void> {
  let source = mediaPathFromMessage(msg);
  if (!source) {
    const pending = toast.info('Downloading to copy…');
    try {
      const downloaded = await downloadMessageMediaAndWait(accountId, conversationId, msg.id);
      source = mediaPathFromMessage(downloaded);
    } catch (e) {
      dismissToast(pending);
      toast.error(`Could not copy: ${e instanceof Error ? e.message : String(e)}`);
      return;
    }
    dismissToast(pending);
  }
  if (!source) {
    toast.error('Could not copy: the media is not available.');
    return;
  }
  await copyCachedMediaToClipboard(source, mediaNameInfo(msg, owner));
}
