/**
 * Pure helpers for the New > Contact picker: which rows count as real contacts, how a row is
 * labelled, and how a typed number is cleaned up. Raw JIDs never reach the screen.
 */
import { formatPhone } from '$lib/senderIdentity';
import type { Contact } from '$lib/types';

/** Rows drawn at once; the search field narrows the rest. */
export const CONTACT_ROW_LIMIT = 200;

/**
 * Digits of a typed phone number: spaces, dashes, brackets, a leading `+` and the `00`
 * international prefix are dropped. Null unless the rest is a plausible E.164 length (7 to 15).
 */
export function normalizePhoneInput(raw: string): string | null {
  let digits = raw.replace(/\D/g, '');
  if (digits.startsWith('00')) digits = digits.slice(2);
  return digits.length >= 7 && digits.length <= 15 ? digits : null;
}

/** True when the query reads as a phone number rather than a name. */
export function looksLikePhoneQuery(raw: string): boolean {
  const q = raw.trim();
  return /^\+?[\d\s().-]{3,}$/.test(q) && /\d{3}/.test(q);
}

/**
 * A real contact is saved in the address book or has a direct chat with us. Everyone else is
 * known only from group participation. The connector reports `saved`; rows without that fact
 * (older syncs) count as saved so nothing disappears.
 */
export function isRealContact(c: Contact): boolean {
  return c.metadata?.saved !== false || c.metadata?.has_chat === true;
}

function userPart(remoteId: string): string {
  return remoteId.split('@')[0];
}

/** "+974 …" for a phone JID, null for LIDs and anything else without a real number. */
export function contactPhone(c: Contact): string | null {
  if (!/@(s\.whatsapp\.net|c\.us)$/.test(c.remote_id)) return null;
  const user = userPart(c.remote_id).split(':')[0];
  return /^\d{5,}$/.test(user) ? formatPhone(user) : null;
}

/** The name to show: the stored name, or the formatted number when there is no real name. */
export function contactTitle(c: Contact): string {
  const name = (c.display_name ?? '').trim();
  const hasName = name && !/@(s\.whatsapp\.net|lid|c\.us|g\.us)$/.test(name) && !/^\+?[\d\s().-]{6,}$/.test(name);
  if (hasName) return name;
  return contactPhone(c) ?? (name && !name.includes('@') ? name : 'Unknown contact');
}

/** Second line: the number, only for a contact without a saved name (a push name is not one). */
export function contactSecondary(c: Contact): string | null {
  if (c.metadata?.saved !== false) return null;
  const phone = contactPhone(c);
  return phone && phone !== contactTitle(c) ? phone : null;
}

/** Contacts matching the query on name or number (never on the JID text itself). */
export function filterContacts(contacts: Contact[], query: string, includeUnknown: boolean): Contact[] {
  const q = query.trim().toLowerCase();
  const digits = q.replace(/\D/g, '');
  return contacts.filter((c) => {
    if (!includeUnknown && !isRealContact(c)) return false;
    if (!q) return true;
    if (c.display_name.toLowerCase().includes(q)) return true;
    return digits.length >= 3 && /@(s\.whatsapp\.net|c\.us)$/.test(c.remote_id) && userPart(c.remote_id).includes(digits);
  });
}
