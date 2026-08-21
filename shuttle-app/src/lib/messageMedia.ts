import { invoke } from '@tauri-apps/api/core';
import { isTauri } from './mock';
import type { Message } from './types';

const PREVIEW_LABELS: Record<string, string> = {
  image: '📷 Photo',
  photo: '📷 Photo',
  sticker: 'Sticker',
  video: '🎬 Video',
  audio: '🎵 Audio',
  ptt: '🎤 Voice message',
  document: '📎 Document',
  contact: '👤 Contact',
  poll: '📊 Poll',
  event: '📅 Event',
  location: '📍 Location',
};

const DOWNLOADABLE = new Set([
  'image',
  'photo',
  'sticker',
  'video',
  'audio',
  'ptt',
  'document',
]);

const mediaUrlCache = new Map<string, string>();

export function normalizeMediaKind(raw: string | null | undefined): string | null {
  if (!raw) return null;
  const k = raw.toLowerCase();
  if (k === 'ptt') return 'audio';
  if (k === 'photo') return 'image';
  return k;
}

export function mediaKindFromMessage(msg: Message): string | null {
  const fromMeta = msg.metadata?.media_type;
  if (typeof fromMeta === 'string' && fromMeta) {
    return normalizeMediaKind(fromMeta);
  }
  const match = (msg.body || '').match(
    /^\[(image|photo|sticker|video|audio|document|contact|poll|event|ptt|location)\]$/i
  );
  return match ? normalizeMediaKind(match[1]) : null;
}

export function mediaDataFromMessage(msg: Message): string | null {
  const data = msg.metadata?.media_data;
  return typeof data === 'string' && data.startsWith('data:') ? data : null;
}

export function mediaPathFromMessage(msg: Message): string | null {
  const path = msg.metadata?.media_path;
  return typeof path === 'string' && path.trim() ? path.trim() : null;
}

export async function resolveMediaUrl(msg: Message): Promise<string | null> {
  const inline = mediaDataFromMessage(msg);
  if (inline) return inline;
  const path = mediaPathFromMessage(msg);
  if (!path) return null;
  const cached = mediaUrlCache.get(path);
  if (cached) return cached;
  if (!isTauri()) return null;
  try {
    const url = await invoke<string>('read_message_media', { path });
    mediaUrlCache.set(path, url);
    return url;
  } catch {
    return null;
  }
}

export function mediaFilename(msg: Message): string | null {
  const name = msg.metadata?.filename;
  return typeof name === 'string' && name.trim() ? name.trim() : null;
}

export function mediaFailed(msg: Message): boolean {
  return msg.metadata?.media_error != null;
}

export function looksLikeJid(value: string): boolean {
  const s = (value || '').trim();
  if (!s || !s.includes('@')) return false;
  const [local, domain] = s.split('@', 2);
  if (!local || !domain) return false;
  if (['lid', 's.whatsapp.net', 'g.us', 'broadcast', 'newsletter'].includes(domain)) return true;
  return /^\+?\d+$/.test(local) && domain.includes('.');
}

/** True for garbage bodies like hex media keys, UUIDs, or other non-human strings */
function looksLikeGarbage(value: string): boolean {
  const s = value.trim();
  if (!s) return true;
  // Pure uppercase/lowercase hex ≥ 16 chars with no spaces
  if (/^[0-9A-Fa-f]{16,}$/.test(s)) return true;
  // UUID-like
  if (/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(s)) return true;
  // Base64-like blob (long, no spaces, mix of +/=)
  if (s.length > 32 && /^[A-Za-z0-9+/]+=*$/.test(s) && !/\s/.test(s)) return true;
  return false;
}

export function captionText(msg: Message): string {
  const kind = mediaKindFromMessage(msg);
  const body = (msg.body || '').trim();
  if (!body) return '';
  if (looksLikeJid(body) || looksLikeGarbage(body)) return '';
  if (!kind) return body;
  const bodyLower = body.toLowerCase();
  const isPlaceholder =
    bodyLower === `[${kind}]` ||
    bodyLower === `[image]` ||
    bodyLower === `[photo]` ||
    bodyLower === `[sticker]` ||
    bodyLower === `[video]` ||
    bodyLower === `[audio]` ||
    bodyLower === `[document]` ||
    bodyLower === `[location]` ||
    bodyLower === `[poll]` ||
    bodyLower === `[contact]`;
  if (isPlaceholder) return '';
  const raw = msg.metadata?.media_type;
  if (typeof raw === 'string' && bodyLower === `[${raw.toLowerCase()}]`) return '';
  return body;
}

export function messagePreview(body: string, metadata?: Record<string, unknown> | null): string {
  const meta = metadata ?? {};
  const rawType = meta.media_type;
  const mediaType =
    typeof rawType === 'string' ? normalizeMediaKind(rawType) : null;
  const text = (body || '').trim();

  if (mediaType) {
    const label = PREVIEW_LABELS[mediaType] ?? mediaType.charAt(0).toUpperCase() + mediaType.slice(1);
    const placeholder = `[${rawType}]`.toLowerCase();
    const isPlaceholder =
      !text ||
      text.toLowerCase() === placeholder ||
      text.toLowerCase() === `[${mediaType}]`;
    if (!isPlaceholder) return text;
    if (mediaType === 'document') {
      const filename = meta.filename;
      if (typeof filename === 'string' && filename.trim()) {
        return `📎 ${filename.trim()}`;
      }
    }
    return label;
  }

  return text;
}

export function previewFromMessage(msg: Message): string {
  return messagePreview(msg.body, msg.metadata);
}

export function isDownloadableMedia(msg: Message): boolean {
  return shouldDownloadMedia(mediaKindFromMessage(msg));
}

export function shouldDownloadMedia(kind: string | null): boolean {
  if (!kind) return false;
  return DOWNLOADABLE.has(kind) || kind === 'sticker';
}

export function mediaIconLabel(kind: string): string {
  return PREVIEW_LABELS[kind] ?? kind;
}

export function listPreview(preview: string | null | undefined): string {
  const text = (preview || '').trim();
  const match = text.match(/^\[(.+)\]$/);
  if (match) {
    return messagePreview(text, { media_type: match[1] });
  }
  return text;
}
