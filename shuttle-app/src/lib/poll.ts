import type { Message, PollData, PollOption, PollVoter } from './types';

/**
 * Polls as the UI reads them. The core stores `metadata.poll` (see `db/chat.rs`, the only
 * writer of poll state); everything here parses that shape and derives what a card shows.
 */

function text(value: unknown): string {
  return typeof value === 'string' ? value : '';
}

function count(value: unknown): number {
  return typeof value === 'number' && Number.isFinite(value) && value > 0 ? Math.floor(value) : 0;
}

function parseVoter(raw: unknown): PollVoter | null {
  if (!raw || typeof raw !== 'object') return null;
  const r = raw as Record<string, unknown>;
  const id = text(r.id).trim();
  if (!id) return null;
  const name = text(r.name).trim();
  return name ? { id, name } : { id };
}

function parseOption(raw: unknown): PollOption | null {
  if (!raw || typeof raw !== 'object') return null;
  const r = raw as Record<string, unknown>;
  const id = text(r.id).trim();
  if (!id) return null;
  const voters = Array.isArray(r.voters)
    ? r.voters.map(parseVoter).filter((v): v is PollVoter => v !== null)
    : [];
  return {
    id,
    text: text(r.text),
    votes: count(r.votes) || voters.length,
    voters,
    mine: r.mine === true,
  };
}

/** The poll carried by a message, or null when the message is not one. */
export function pollDataFromMessage(msg: Message): PollData | null {
  const raw = msg.metadata?.poll;
  if (raw && typeof raw === 'object') {
    const r = raw as Record<string, unknown>;
    const options = Array.isArray(r.options)
      ? r.options.map(parseOption).filter((o): o is PollOption => o !== null)
      : [];
    if (options.length) {
      return {
        question: text(r.question) || text(msg.metadata?.question),
        multiple: r.multiple === true,
        options,
      };
    }
  }
  return sentPoll(msg);
}

/**
 * A poll this app sent stores the question and the option labels, without the network's option
 * hashes. Draw it as well: the network's own report of the same poll replaces this with the full
 * definition, and votes only ever arrive with it.
 */
function sentPoll(msg: Message): PollData | null {
  const labels = msg.metadata?.options;
  if (!Array.isArray(labels)) return null;
  const options: PollOption[] = labels
    .map((label) => text(label).trim())
    .filter(Boolean)
    .map((label) => ({ id: label, text: label, votes: 0, voters: [], mine: false }));
  if (!options.length) return null;
  return {
    question: text(msg.metadata?.question),
    multiple: false,
    options,
  };
}

/** Every vote cast in the poll, across its options. */
export function pollVoteTotal(poll: PollData): number {
  return poll.options.reduce((sum, o) => sum + o.votes, 0);
}

/** Share of the votes one option holds, 0 to 1. A poll with no votes leaves every bar empty. */
export function pollShare(option: PollOption, total: number): number {
  if (total <= 0 || option.votes <= 0) return 0;
  return Math.min(1, option.votes / total);
}

/** One or two initials for a voter chip, from the name, then from the id. */
export function pollInitials(voter: PollVoter): string {
  const name = (voter.name ?? '').trim();
  const source = name || voter.id.split('@')[0].replace(/[^\p{L}\p{N}]+/gu, ' ').trim();
  const words = source.split(/\s+/).filter(Boolean);
  if (!words.length) return '?';
  const first = [...words[0]][0] ?? '?';
  const second = words.length > 1 ? ([...words[words.length - 1]][0] ?? '') : '';
  return (first + second).toUpperCase();
}

/** A voter's display name, falling back to "You" or the id's user part. */
export function pollVoterName(voter: PollVoter, mine: boolean): string {
  if (voter.name) return voter.name;
  return mine ? 'You' : voter.id.split('@')[0];
}
