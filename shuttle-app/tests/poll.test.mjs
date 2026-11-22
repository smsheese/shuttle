import { test } from 'node:test';
import assert from 'node:assert/strict';
import { build } from 'esbuild';

const result = await build({ entryPoints: ['src/lib/poll.ts'], bundle: true, platform: 'node', format: 'esm', write: false });
const { pollDataFromMessage, pollVoteTotal, pollShare, pollInitials, pollVoterName } = await import(
  `data:text/javascript;base64,${Buffer.from(result.outputFiles[0].text).toString('base64')}`
);

const pollMessage = (poll) => ({
  id: 'm1',
  conversation_id: 'c1',
  remote_id: 'r1',
  sender_id: null,
  sender_name: 'You',
  direction: 'outbound',
  body: 'Where shall we eat?',
  timestamp: '2025-03-01T10:00:00+00:00',
  status: 'sent',
  metadata: { media_type: 'poll', poll },
});

const stored = {
  question: 'Where shall we eat?',
  multiple: false,
  options: [
    { id: 'h1', text: 'Pizza', votes: 2, voters: [{ id: 'a@s.whatsapp.net', name: 'Ann Lee' }, { id: 'b@s.whatsapp.net' }], mine: false },
    { id: 'h2', text: 'Sushi', votes: 1, voters: [{ id: 'self@s.whatsapp.net', name: 'You' }], mine: true },
    { id: 'h3', text: 'Tacos', votes: 0, voters: [], mine: false },
  ],
};

test('a stored poll message parses into the card model', () => {
  const poll = pollDataFromMessage(pollMessage(stored));
  assert.ok(poll);
  assert.equal(poll.question, 'Where shall we eat?');
  assert.equal(poll.multiple, false);
  assert.deepEqual(
    poll.options.map((o) => [o.text, o.votes, o.mine]),
    [
      ['Pizza', 2, false],
      ['Sushi', 1, true],
      ['Tacos', 0, false],
    ]
  );
  assert.equal(poll.options[0].voters.length, 2);
});

test('a message that is not a poll has no poll data', () => {
  assert.equal(pollDataFromMessage({ metadata: {} }), null, 'no metadata.poll');
  assert.equal(pollDataFromMessage({ metadata: { poll: { options: [] } } }), null, 'no options');
  assert.equal(
    pollDataFromMessage({ metadata: { poll: { options: [{ text: 'no id' }] } } }),
    null,
    'an option without an id cannot be voted for'
  );
  assert.equal(
    pollDataFromMessage({ metadata: { poll: { options: 'nope' } } }),
    null,
    'options must be a list'
  );
});

test('the bar is the share of all votes, and an empty poll has no bars', () => {
  const poll = pollDataFromMessage(pollMessage(stored));
  const total = pollVoteTotal(poll);
  assert.equal(total, 3);
  assert.equal(pollShare(poll.options[0], total), 2 / 3);
  assert.equal(pollShare(poll.options[2], total), 0);
  assert.equal(pollShare(poll.options[0], 0), 0, 'no votes anywhere: no bar');
});

test('a vote count with no voter list still counts', () => {
  const poll = pollDataFromMessage(
    pollMessage({ question: 'Q', options: [{ id: 'h1', text: 'A', votes: 4, voters: [] }] })
  );
  assert.equal(poll.options[0].votes, 4);
  assert.equal(pollVoteTotal(poll), 4);
});

test('voter chips read as initials, names, and "You"', () => {
  assert.equal(pollInitials({ id: 'a@s.whatsapp.net', name: 'Ann Lee' }), 'AL');
  assert.equal(pollInitials({ id: 'a@s.whatsapp.net', name: 'Ann' }), 'A');
  assert.equal(pollInitials({ id: '447700900123@s.whatsapp.net' }), '4');
  assert.equal(pollInitials({ id: '@s.whatsapp.net' }), '?');
  assert.equal(pollVoterName({ id: 'a@s.whatsapp.net', name: 'Ann Lee' }, false), 'Ann Lee');
  assert.equal(pollVoterName({ id: 'a@s.whatsapp.net' }, true), 'You');
  assert.equal(pollVoterName({ id: '447700900123@s.whatsapp.net' }, false), '447700900123');
});

test('a poll sent from this app draws from its question and labels until the network reports it', () => {
  const sent = pollDataFromMessage({
    metadata: { media_type: 'poll', question: 'Where shall we eat?', options: ['Pizza', 'Sushi'] },
  });
  assert.ok(sent, 'a sent poll is still a poll');
  assert.equal(sent.question, 'Where shall we eat?');
  assert.deepEqual(
    sent.options.map((o) => [o.id, o.text, o.votes]),
    [
      ['Pizza', 'Pizza', 0],
      ['Sushi', 'Sushi', 0],
    ]
  );
  assert.equal(pollVoteTotal(sent), 0);
});

test('a full definition wins over the sent labels', () => {
  const both = pollDataFromMessage({
    metadata: {
      question: 'Where?',
      options: ['Pizza', 'Sushi'],
      poll: { question: 'Where?', multiple: true, options: [{ id: 'h1', text: 'Pizza', votes: 2, voters: [] }] },
    },
  });
  assert.deepEqual(both.options.map((o) => o.id), ['h1'], 'the network ids are used once they arrive');
  assert.equal(both.multiple, true);
});
