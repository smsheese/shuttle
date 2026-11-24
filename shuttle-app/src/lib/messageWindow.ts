import type { Message, MessagePage } from './types';

/**
 * The slice of one chat that the app holds: `messages` is a contiguous run, oldest first, and
 * `hasOlder` / `hasNewer` say whether the database goes on above and below it. Scrolling past
 * either end reads the next page; a jump into history replaces the run and leaves both flags set.
 */
export interface MessageWindow {
  messages: Message[];
  hasOlder: boolean;
  hasNewer: boolean;
}

export function emptyWindow(): MessageWindow {
  return { messages: [], hasOlder: false, hasNewer: false };
}

/** The window a chat opens on (or a jump replaces the run with). */
export function openWindow(page: MessagePage): MessageWindow {
  return {
    messages: page.messages,
    hasOlder: page.has_older,
    hasNewer: page.has_newer,
  };
}

/** History above the run: older rows go in front, and the run keeps every row it had. */
export function prependOlder(window: MessageWindow, page: MessagePage): MessageWindow {
  return {
    messages: [...withoutDuplicates(page.messages, window.messages), ...window.messages],
    hasOlder: page.has_older,
    hasNewer: window.hasNewer,
  };
}

/** The live end below a run the reader scrolled into: rows go after it, nothing is dropped. */
export function appendNewer(window: MessageWindow, page: MessagePage): MessageWindow {
  const known = new Set(window.messages.map((m) => m.id));
  return {
    messages: [...window.messages, ...page.messages.filter((m) => !known.has(m.id))],
    hasOlder: window.hasOlder,
    hasNewer: page.has_newer,
  };
}

/**
 * A refresh for a window that reaches the newest message: the newest page updates the rows the
 * reader already holds and appends what arrived. A page that shares no row with the window means
 * the chat moved further than one page, and the window restarts from that page rather than
 * leaving a hole between the two.
 */
export function mergeNewest(window: MessageWindow, page: MessagePage): MessageWindow {
  const known = new Set(window.messages.map((m) => m.id));
  if (!page.messages.some((m) => known.has(m.id))) return openWindow(page);
  const fresh = new Map(page.messages.map((m) => [m.id, m]));
  const kept = window.messages.map((m) => fresh.get(m.id) ?? m);
  const added = page.messages.filter((m) => !known.has(m.id));
  return {
    messages: [...kept, ...added],
    hasOlder: window.hasOlder,
    hasNewer: page.has_newer,
  };
}

/**
 * A refresh for a window that stops short of the live end (the reader jumped into history): the
 * page starts at the window's newest row, so its rows replace that part of the run and anything
 * older the page did not reach stays as it is.
 */
export function reReadSpan(window: MessageWindow, page: MessagePage): MessageWindow {
  const start = page.messages.length
    ? window.messages.findIndex((m) => m.id === page.messages[0].id)
    : -1;
  if (start < 0) return window;
  const older = window.messages.slice(0, start);
  return {
    messages: older.length ? [...older, ...page.messages] : page.messages,
    hasOlder: older.length ? window.hasOlder : page.has_older,
    hasNewer: page.has_newer,
  };
}

/** One row replaced in place (an edit, a receipt, a downloaded attachment), or added at the end. */
export function replaceRow(window: MessageWindow, msg: Message): MessageWindow {
  const index = window.messages.findIndex(
    (m) => m.id === msg.id || (!!msg.remote_id && m.remote_id === msg.remote_id)
  );
  if (index < 0) return { ...window, messages: [...window.messages, msg] };
  return {
    ...window,
    messages: window.messages.map((m, i) => (i === index ? msg : m)),
  };
}

export function removeRow(window: MessageWindow, messageId: string): MessageWindow {
  return {
    ...window,
    messages: window.messages.filter((m) => m.id !== messageId),
  };
}

function withoutDuplicates(incoming: Message[], held: Message[]): Message[] {
  if (held.length === 0) return incoming;
  // The next page can overlap the first row of the run when a message arrived while it loaded.
  const heldIds = new Set(held.map((m) => m.id));
  return incoming.filter((m) => !heldIds.has(m.id));
}
