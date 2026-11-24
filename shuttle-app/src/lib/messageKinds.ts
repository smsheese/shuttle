import type { Message } from './types';

export interface ContactPayload {
  kind: 'contact';
  name: string;
  phones: string[];
  vcard?: string;
}

export interface LocationPayload {
  kind: 'location';
  lat: number;
  lng: number;
  name?: string;
  address?: string;
  live?: boolean;
}

export type RichPayload = ContactPayload | LocationPayload;

const CONTACT_BODY = /^Contact:\s*(.+?)(?:\s*\(([^()]+)\))?\s*$/s;
const MAPS_BODY =
  /^<?https?:\/\/(?:maps\.google\.com\/?|(?:www\.)?google\.com\/maps\/?)\?(?:[^\s]*&)?(?:q|query|ll)=(-?\d{1,3}(?:\.\d+)?),(-?\d{1,3}(?:\.\d+)?)[^\s]*>?$/i;

function str(v: unknown): string | undefined {
  return typeof v === 'string' && v.trim() ? v.trim() : undefined;
}

function validCoords(lat: number, lng: number): boolean {
  return Number.isFinite(lat) && Number.isFinite(lng) && Math.abs(lat) <= 90 && Math.abs(lng) <= 180;
}

function fromMetadata(meta: Record<string, unknown> | undefined): RichPayload | null {
  const contact = meta?.contact as Record<string, unknown> | undefined;
  if (contact && typeof contact === 'object') {
    const phones = Array.isArray(contact.phones)
      ? contact.phones.map(str).filter((p): p is string => !!p)
      : [];
    const name = str(contact.name) ?? phones[0];
    if (name) return { kind: 'contact', name, phones, vcard: str(contact.vcard) };
  }
  const loc = meta?.location as Record<string, unknown> | undefined;
  if (loc && typeof loc === 'object') {
    const lat = Number(loc.lat);
    const lng = Number(loc.lng);
    if (validCoords(lat, lng)) {
      return {
        kind: 'location',
        lat,
        lng,
        name: str(loc.name),
        address: str(loc.address),
        live: loc.live === true,
      };
    }
  }
  return null;
}

/**
 * The structured card a message should render as, or null for ordinary text and media.
 * Structured connector facts win; stored messages from before they existed (GOWA flattens
 * a contact to `Contact: Name (+phone)` and a location to a Google Maps link) are read
 * from the body, so no backfill is needed.
 */
export function richPayload(msg: Pick<Message, 'body' | 'metadata'>): RichPayload | null {
  const meta = msg.metadata;
  const structured = fromMetadata(meta);
  if (structured) return structured;
  if (typeof meta?.media_type === 'string' && meta.media_type && meta.media_type !== 'text') {
    return null;
  }
  const body = (msg.body || '').trim();
  const loc = body.match(MAPS_BODY);
  if (loc) {
    const lat = Number(loc[1]);
    const lng = Number(loc[2]);
    if (validCoords(lat, lng)) return { kind: 'location', lat, lng };
  }
  const contact = body.match(CONTACT_BODY);
  if (contact && !body.includes('\n')) {
    const name = contact[1].trim();
    const phone = contact[2]?.trim();
    if (name) return { kind: 'contact', name, phones: phone ? [phone] : [] };
  }
  return null;
}

const URL_RE = /\bhttps?:\/\/[^\s<>"']+/i;
const TRAILING = /[.,;:!?)\]}'"]+$/;

/** The first web link in a message body, trimmed of sentence punctuation, or null. */
export function firstUrl(text: string | null | undefined): string | null {
  const m = (text || '').match(URL_RE);
  if (!m) return null;
  const url = m[0].replace(TRAILING, '');
  try {
    const u = new URL(url);
    return u.protocol === 'http:' || u.protocol === 'https:' ? u.href : null;
  } catch {
    return null;
  }
}

/** Digits only, for building a `…@s.whatsapp.net` id; null unless E.164-length. */
export function phoneDigits(raw: string): string | null {
  let digits = raw.replace(/\D/g, '');
  if (digits.startsWith('00')) digits = digits.slice(2);
  return digits.length >= 7 && digits.length <= 15 ? digits : null;
}

export function osmUrl(lat: number, lng: number, zoom = 16): string {
  return `https://www.openstreetmap.org/?mlat=${lat}&mlon=${lng}#map=${zoom}/${lat}/${lng}`;
}

/** A vCard for "Save contact": the connector's own when present, else one built from the card. */
export function vcardFor(c: ContactPayload): string {
  if (c.vcard) return c.vcard;
  const lines = ['BEGIN:VCARD', 'VERSION:3.0', `FN:${c.name.replace(/[\r\n]/g, ' ')}`];
  for (const p of c.phones) lines.push(`TEL;TYPE=CELL:${p.replace(/[\r\n]/g, ' ')}`);
  lines.push('END:VCARD');
  return lines.join('\r\n');
}

export interface VideoEmbed {
  provider: 'youtube' | 'vimeo';
  id: string;
  /** Player URL for an iframe; autoplay is added when it is opened. */
  embedUrl: string;
  watchUrl: string;
}

const YT_ID = /^[\w-]{11}$/;

/** A YouTube or Vimeo link the chat can play in place, or null for any other URL. */
export function videoEmbed(raw: string | null | undefined): VideoEmbed | null {
  let u: URL;
  try {
    u = new URL(raw || '');
  } catch {
    return null;
  }
  const host = u.hostname.replace(/^(www|m|music)\./, '').toLowerCase();
  let ytId: string | null = null;
  if (host === 'youtu.be') ytId = u.pathname.split('/')[1] ?? null;
  else if (host === 'youtube.com' || host === 'youtube-nocookie.com') {
    if (u.pathname === '/watch') ytId = u.searchParams.get('v');
    else {
      const m = u.pathname.match(/^\/(?:embed|shorts|live|v)\/([\w-]{11})/);
      ytId = m ? m[1] : null;
    }
  }
  if (ytId && YT_ID.test(ytId)) {
    return {
      provider: 'youtube',
      id: ytId,
      embedUrl: `https://www.youtube-nocookie.com/embed/${ytId}?rel=0&playsinline=1`,
      watchUrl: `https://www.youtube.com/watch?v=${ytId}`,
    };
  }
  if (host === 'vimeo.com' || host === 'player.vimeo.com') {
    const m = u.pathname.match(/\/(\d{5,12})(?:\/|$)/);
    if (m) {
      return {
        provider: 'vimeo',
        id: m[1],
        embedUrl: `https://player.vimeo.com/video/${m[1]}?dnt=1`,
        watchUrl: `https://vimeo.com/${m[1]}`,
      };
    }
  }
  return null;
}
