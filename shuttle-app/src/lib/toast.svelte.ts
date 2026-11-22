/** Tiny global toast queue (bottom-right). */

export interface Toast {
  id: number;
  kind: 'saved' | 'error' | 'info';
  text: string;
  /** For `saved`: clicking the path reveals this file in the OS file manager. */
  path?: string;
}

export const TOAST_MS = 3000;

let nextId = 1;
export const toasts = $state<{ items: Toast[] }>({ items: [] });

function push(toast: Omit<Toast, 'id'>, ms: number = TOAST_MS): number {
  const id = nextId++;
  toasts.items = [...toasts.items, { ...toast, id }];
  setTimeout(() => dismissToast(id), ms);
  return id;
}

export function dismissToast(id: number) {
  toasts.items = toasts.items.filter((t) => t.id !== id);
}

export const toast = {
  saved: (path: string) => push({ kind: 'saved', text: 'Saved to', path }),
  error: (text: string) => push({ kind: 'error', text }),
  info: (text: string) => push({ kind: 'info', text }),
  /** Short confirmation for clipboard copies. */
  copied: (text = 'Copied to clipboard') => push({ kind: 'info', text }, 2000),
};
