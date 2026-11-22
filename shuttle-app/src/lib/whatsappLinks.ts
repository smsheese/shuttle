/**
 * The links a WhatsApp chat can open, and the reason a call button has one.
 *
 * A direct chat's remote id is a JID. Only the phone JIDs carry a number a caller dials: a LID
 * carries a WhatsApp-internal id that looks like digits, so it must never reach a link.
 */

const PHONE_JIDS = new Set(['s.whatsapp.net', 'c.us']);

export function phoneDigitsFromRemoteId(remoteId: string | null | undefined): string | null {
  const raw = String(remoteId ?? '').trim().toLowerCase();
  if (!raw) return null;
  const [local, domain = ''] = raw.split('@');
  if (domain && !PHONE_JIDS.has(domain)) return null;
  const digits = local.split(':')[0].split('.')[0].replace(/\D/g, '');
  return digits.length >= 7 && digits.length <= 15 ? digits : null;
}

/** The chat in WhatsApp's own apps, for a number; null when the id is not a phone number. */
export function whatsappChatUrl(remoteId: string | null | undefined): string | null {
  const digits = phoneDigitsFromRemoteId(remoteId);
  return digits ? `https://wa.me/${digits}` : null;
}

/**
 * What a call button does for a network that places no calls itself. GOWA has no call route, so
 * the honest fallback is WhatsApp's own chat, where the phone or the desktop app starts the call.
 * The message says where the call happens either way.
 */
export function whatsappCallFallback(
  mode: 'audio' | 'video',
  remoteId: string | null | undefined
): { url: string | null; message: string } {
  const url = whatsappChatUrl(remoteId);
  const kind = mode === 'video' ? 'Video' : 'Audio';
  return {
    url,
    message: url
      ? `${kind} calls start in WhatsApp. Opening this chat there.`
      : `${kind} calls start in WhatsApp on your phone. Shuttle has no number for this contact.`,
  };
}
