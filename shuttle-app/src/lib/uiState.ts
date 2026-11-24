/**
 * What the window looked like when it went away, so a rebuild from the tray (or a restart) puts
 * the same chat, filters and half-written replies back. Kept in one small localStorage key and
 * never read across devices. Every access is guarded: a full or blocked localStorage must not
 * take the UI down.
 */

export interface UiState {
  workspaceId: string | null;
  accountId: string | null;
  conversationId: string | null;
  /** Unsent composer text per conversation id. */
  drafts: Record<string, string>;
}

const KEY = 'shuttle.ui-state';
const MAX_DRAFTS = 40;

const empty = (): UiState => ({ workspaceId: null, accountId: null, conversationId: null, drafts: {} });

function text(value: unknown): string | null {
  return typeof value === 'string' && value ? value : null;
}

function read(): UiState {
  try {
    const raw = localStorage.getItem(KEY);
    if (!raw) return empty();
    const parsed: unknown = JSON.parse(raw);
    if (!parsed || typeof parsed !== 'object') return empty();
    const record = parsed as Record<string, unknown>;
    const drafts: Record<string, string> = {};
    if (record.drafts && typeof record.drafts === 'object') {
      for (const [id, body] of Object.entries(record.drafts as Record<string, unknown>)) {
        if (typeof body === 'string' && body) drafts[id] = body;
      }
    }
    return {
      workspaceId: text(record.workspaceId),
      accountId: text(record.accountId),
      conversationId: text(record.conversationId),
      drafts,
    };
  } catch {
    return empty();
  }
}

let state: UiState = read();

function write(): void {
  try {
    localStorage.setItem(KEY, JSON.stringify(state));
  } catch {
    // Private browsing or a full quota: the window just rebuilds without its last view.
  }
}

export function loadUiState(): UiState {
  return state;
}

/** Remember which filters and chat are open. */
export function saveUiPlace(patch: Pick<UiState, 'workspaceId' | 'accountId' | 'conversationId'>): void {
  state = { ...state, ...patch };
  write();
}

/** Remember one conversation's unsent text, or forget it when the box is empty. */
export function saveDraft(conversationId: string, body: string): void {
  const drafts = { ...state.drafts };
  if (body) {
    delete drafts[conversationId];
    drafts[conversationId] = body;
  } else {
    delete drafts[conversationId];
  }
  const kept = Object.keys(drafts).slice(-MAX_DRAFTS);
  const trimmed: Record<string, string> = {};
  for (const id of kept) trimmed[id] = drafts[id];
  state = { ...state, drafts: trimmed };
  write();
}

export function draftFor(conversationId: string | null): string {
  return conversationId ? state.drafts[conversationId] ?? '' : '';
}
