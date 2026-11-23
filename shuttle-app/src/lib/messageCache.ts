import type { Message } from './types';

/**
 * Recent messages per conversation, so opening a chat paints at once. Reading an entry makes
 * it the most recently used one; adding past the chat cap evicts the least recently used.
 * Every open still re-reads the database and replaces the entry, so a stale row shows briefly.
 */
export class MessageCache {
  private readonly entries = new Map<string, Message[]>();

  constructor(
    private readonly maxChats: number,
    private readonly maxRows: number,
  ) {}

  get size(): number {
    return this.entries.size;
  }

  has(conversationId: string): boolean {
    return this.entries.has(conversationId);
  }

  get(conversationId: string): Message[] | undefined {
    const rows = this.entries.get(conversationId);
    if (rows) {
      this.entries.delete(conversationId);
      this.entries.set(conversationId, rows);
    }
    return rows;
  }

  set(conversationId: string, rows: Message[]): void {
    this.entries.delete(conversationId);
    this.entries.set(conversationId, rows.slice(-this.maxRows));
    for (const oldest of this.entries.keys()) {
      if (this.entries.size <= this.maxChats) break;
      this.entries.delete(oldest);
    }
  }
}
