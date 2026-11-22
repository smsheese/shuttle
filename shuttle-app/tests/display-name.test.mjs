import { test } from 'node:test';
import assert from 'node:assert/strict';
import { build } from 'esbuild';

const result = await build({ entryPoints: ['src/lib/displayName.ts'], bundle: true, platform: 'node', format: 'esm', write: false });
const { displayName, displayNameFor, isPlaceholderName, chatKind } = await import(
  `data:text/javascript;base64,${Buffer.from(result.outputFiles[0].text).toString('base64')}`
);

const channel = (title, names = {}) => ({ title, remote_id: '120363041234567890@newsletter', conversation_type: 'channel', metadata: { names } });
const group = (title, names = {}) => ({ title, remote_id: '120363041234567890@g.us', conversation_type: 'group', metadata: { names } });

test('a real name the connector reported wins over a placeholder title', () => {
  assert.equal(displayName(channel('Newsletter 120363041234567890', { subject: 'Shuttle News' })), 'Shuttle News');
  assert.equal(displayName(group('Group', { subject: 'Rent Appartment Riyadh' })), 'Rent Appartment Riyadh');
  assert.equal(displayName(channel('Channel', { subject: 'Product updates' })), 'Product updates');
});

test('a real title is kept, and a placeholder is never shown when nothing better exists', () => {
  assert.equal(displayName(channel('Design Weekly', { subject: 'Other' })), 'Design Weekly');
  assert.equal(displayName(channel('Newsletter 120363041234567890')), 'Unnamed channel');
  assert.equal(displayName(group('Group')), 'Unnamed group');
  assert.equal(displayName({ title: 'Group', remote_id: '120363041234567890@g.us' }), 'Unnamed group');
});

test('another account row with the same remote id supplies the name', () => {
  const mine = channel('Newsletter 120363041234567890');
  const theirs = { ...channel('Shuttle News'), id: 'other', account_id: 'wa-2' };
  assert.equal(displayNameFor({ ...mine, id: 'self' }, [theirs]), 'Shuttle News');
  assert.equal(displayNameFor({ ...mine, id: 'self' }, [{ ...theirs, title: 'Channel' }]), 'Unnamed channel');
  assert.equal(displayNameFor({ ...mine, id: 'self' }, [{ ...theirs, id: 'self' }]), 'Unnamed channel', 'a row is not its own sibling');
});

test('a direct chat keeps a formatted number, and a saved name outranks a bare id', () => {
  assert.equal(displayName({ title: '+1 555-0100', remote_id: '15550100@s.whatsapp.net', conversation_type: 'direct' }), '+1 555-0100');
  assert.equal(displayName({ title: '15550100', remote_id: '15550100@s.whatsapp.net', conversation_type: 'direct' }), '+15550100');
  assert.equal(displayName({ title: '15550100', remote_id: '15550100@s.whatsapp.net', conversation_type: 'direct', metadata: { names: { push: 'Sarah Chen' } } }), 'Sarah Chen');
});

test('a real newsletter that happens to start with the word is not a placeholder', () => {
  assert.equal(isPlaceholderName('Newsletter Weekly'), false);
  assert.equal(isPlaceholderName('Newsletter 123456'), true);
  assert.equal(isPlaceholderName('Group 5'), true);
  assert.equal(isPlaceholderName('   '), true);
  assert.equal(isPlaceholderName('120363041234567890@g.us'), true);
  assert.equal(chatKind({ remote_id: 'x@newsletter' }), 'channel');
  assert.equal(chatKind({ remote_id: 'x@g.us' }), 'group');
  assert.equal(chatKind({ remote_id: '15550100@s.whatsapp.net' }), 'direct');
});
