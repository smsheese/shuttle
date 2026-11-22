import type { StatusItem, StatusLinkPreview, StatusPost } from './types';

/** Max posts behind one sender's status before the rail card just shows a count. */
const INLINE_SRC = /^(data:|blob:|https?:)/i;
const BARE_URL = /^(https?:\/\/|www\.)\S+$/i;
const HOSTNAME = /^[a-z0-9.-]+\.[a-z]{2,}$/i;

/**
 * Status posts in viewing order, oldest first. The connector sends oldest first already; this
 * keeps the rail card and the viewer on the same slide when the order is not guaranteed.
 */
export function orderedPosts(item: Pick<StatusItem, 'posts'>): StatusPost[] {
  const posts = item.posts ?? [];
  if (posts.length < 2) return posts;
  if (posts.some((post) => !post.timestamp || !Number.isFinite(new Date(post.timestamp).getTime()))) return posts;
  return [...posts].sort((a, b) => new Date(a.timestamp as string).getTime() - new Date(b.timestamp as string).getTime());
}

/** A path the webview can draw without reading a local file (mock data, remote previews). */
export function inlineSrc(path: string | null | undefined): string | null {
  const text = (path ?? '').trim();
  return text && INLINE_SRC.test(text) ? text : null;
}

/** Only http(s) and mailto links are followed, so a stored `javascript:` or file path is inert. */
export function safeExternalUrl(url: string | null | undefined): string | null {
  const text = (url ?? '').trim();
  if (!text) return null;
  if (/^https?:\/\//i.test(text) || /^mailto:/i.test(text)) return text;
  return null;
}

/** Host of a link, without `www.`, for the label under a preview. */
export function linkPreviewHost(url: string | null | undefined): string {
  const text = (url ?? '').trim();
  if (!text) return '';
  try {
    const host = new URL(/^[a-z]+:\/\//i.test(text) ? text : `https://${text}`).hostname;
    return host.replace(/^www\./i, '');
  } catch {
    return text.replace(/^[a-z]+:\/\//i, '').split(/[/?#]/)[0].replace(/^www\./i, '');
  }
}

/** Title line for a link status: the preview title, then the host, then the path. */
export function linkPreviewTitle(link: StatusLinkPreview | null | undefined): string {
  const title = (link?.title ?? '').trim();
  if (title) return title;
  const host = linkPreviewHost(link?.url);
  if (host) return host;
  return 'Link';
}

/** True when a status body is nothing but a link, so it should not be drawn as raw text. */
export function isBareUrl(text: string | null | undefined): boolean {
  const value = (text ?? '').trim();
  if (!value) return false;
  if (BARE_URL.test(value)) return true;
  return HOSTNAME.test(value) && !/\s/.test(value);
}

/** Emoji, spaces, joiners and variation selectors only: drawn large and centred. */
export function isEmojiOnlyText(text: string | null | undefined): boolean {
  const value = (text ?? '').trim();
  if (!value || value.length > 24) return false;
  if (!/\p{Extended_Pictographic}|\p{Regional_Indicator}/u.test(value)) return false;
  return /^[\p{Extended_Pictographic}\p{Emoji_Component}\p{Regional_Indicator}\uFE0F\u200D\s]+$/u.test(value);
}
