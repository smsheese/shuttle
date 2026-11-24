import type { Message } from './types';

/**
 * Recent messages per conversation, so opening a chat paints at once. Reading an entry makes
 * it the most recently used one; adding past the chat cap evicts the least recently used.
 * Every open still re-reads the database and replaces the entry, so a stale row shows briefly.
 * An entry nobody read or wrote for `ttlMs` is dropped by `sweep`, so a chat looked at once does
 * not hold its rows for the rest of the session.
 */
export class MessageCache {
  private readonly entries = new Map<string, { rows: Message[]; touched: number }>();

  constructor(
    private readonly maxChats: number,
    private readonly maxRows: number,
    private readonly ttlMs = Infinity,
    private readonly now: () => number = Date.now,
  ) {}

  get size(): number {
    return this.entries.size;
  }

  has(conversationId: string): boolean {
    return this.entries.has(conversationId);
  }

  get(conversationId: string): Message[] | undefined {
    const entry = this.entries.get(conversationId);
    if (!entry) return undefined;
    if (this.expired(entry)) {
      this.entries.delete(conversationId);
      return undefined;
    }
    entry.touched = this.now();
    this.entries.delete(conversationId);
    this.entries.set(conversationId, entry);
    return entry.rows;
  }

  /** Drop every entry idle past the time limit. Returns how many went. */
  sweep(): number {
    let dropped = 0;
    for (const [id, entry] of this.entries) {
      if (this.expired(entry)) {
        this.entries.delete(id);
        dropped++;
      }
    }
    return dropped;
  }

  private expired(entry: { touched: number }): boolean {
    return this.now() - entry.touched >= this.ttlMs;
  }

  set(conversationId: string, rows: Message[]): void {
    this.entries.delete(conversationId);
    this.entries.set(conversationId, { rows: rows.slice(-this.maxRows), touched: this.now() });
    for (const oldest of this.entries.keys()) {
      if (this.entries.size <= this.maxChats) break;
      this.entries.delete(oldest);
    }
  }
}
