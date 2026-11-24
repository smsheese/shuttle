import { test } from 'node:test';
import assert from 'node:assert/strict';
import { build } from 'esbuild';

const result = await build({ entryPoints: ['src/lib/messageWindow.ts'], bundle: true, platform: 'node', format: 'esm', write: false });
const {
  emptyWindow,
  openWindow,
  prependOlder,
  appendNewer,
  mergeNewest,
  reReadSpan,
  replaceRow,
  removeRow,
} = await import(`data:text/javascript;base64,${Buffer.from(result.outputFiles[0].text).toString('base64')}`);

const rows = (...ids) => ids.map((id) => ({ id, body: id }));
const page = (ids, hasOlder, hasNewer) => ({
  messages: rows(...ids),
  has_older: hasOlder,
  has_newer: hasNewer,
});
const ids = (window) => window.messages.map((m) => m.id);

test('a chat opens on its newest page', () => {
  const window = openWindow(page(['g', 'h', 'i'], true, false));
  assert.deepEqual(ids(window), ['g', 'h', 'i']);
  assert.equal(window.hasOlder, true);
  assert.equal(window.hasNewer, false);
});

test('older history goes in front and keeps the rows already held', () => {
  const window = prependOlder(openWindow(page(['d', 'e', 'f'], true, false)), page(['a', 'b', 'c'], false, true));
  assert.deepEqual(ids(window), ['a', 'b', 'c', 'd', 'e', 'f']);
  assert.equal(window.hasOlder, false, 'the oldest page ends the history');
  assert.equal(window.hasNewer, false, 'the live end is still below the window');
});

test('a page that overlaps the run repeats no row', () => {
  const window = prependOlder(openWindow(page(['c', 'd'], true, false)), page(['a', 'b', 'c'], false, true));
  assert.deepEqual(ids(window), ['a', 'b', 'c', 'd']);
});

test('the live end extends a run the reader scrolled into history for', () => {
  const jumped = openWindow(page(['b', 'c'], true, true));
  const window = appendNewer(jumped, page(['d', 'e', 'f'], true, false));
  assert.deepEqual(ids(window), ['b', 'c', 'd', 'e', 'f']);
  assert.equal(window.hasNewer, false, 'the window reaches the newest message again');
  assert.equal(window.hasOlder, true, 'history above the run is untouched');
});

test('a refresh updates the rows it re-reads and appends what arrived', () => {
  const window = mergeNewest(openWindow(page(['a', 'b', 'c'], true, false)), page(['b', 'c', 'd'], true, false));
  assert.deepEqual(ids(window), ['a', 'b', 'c', 'd']);
  assert.equal(window.hasOlder, true, 'older pages the reader loaded are never dropped');
  assert.equal(window.hasNewer, false);
});

test('a refresh replaces rows it knows with the stored version', () => {
  const held = openWindow({ messages: [{ id: 'a', body: 'old' }, { id: 'b', body: 'old' }], has_older: false, has_newer: false });
  const window = mergeNewest(held, { messages: [{ id: 'b', body: 'new' }], has_older: false, has_newer: false });
  assert.deepEqual(window.messages.map((m) => m.body), ['old', 'new']);
});

test('a chat that moved further than one page restarts from the newest page', () => {
  const window = mergeNewest(openWindow(page(['a', 'b'], true, false)), page(['z1', 'z2'], true, false));
  assert.deepEqual(ids(window), ['z1', 'z2']);
  assert.equal(window.hasOlder, true);
});

test('re-reading the span of a window that stopped short of the live end keeps older rows', () => {
  const jumped = openWindow(page(['c', 'd', 'e'], true, true));
  const grown = prependOlder(jumped, page(['a', 'b'], false, true));
  const window = reReadSpan(grown, page(['c', 'd', 'e'], false, false));
  assert.deepEqual(ids(window), ['a', 'b', 'c', 'd', 'e']);
  assert.equal(window.hasOlder, false);
  assert.equal(window.hasNewer, false, 'the live end moved into the window');
});

test('a span the page does not cover leaves the window alone', () => {
  const window = openWindow(page(['a', 'b'], false, true));
  assert.deepEqual(ids(reReadSpan(window, page(['x', 'y'], true, true))), ['a', 'b']);
});

test('an edited row replaces the held one and a new row goes to the end', () => {
  let window = openWindow(page(['a', 'b'], false, false));
  window = replaceRow(window, { id: 'a', body: 'edited' });
  assert.deepEqual(window.messages.map((m) => m.body), ['edited', 'b']);
  window = replaceRow(window, { id: 'c', body: 'new' });
  assert.deepEqual(ids(window), ['a', 'b', 'c']);
});

test('a removed row leaves the window and the flags alone', () => {
  const window = removeRow(emptyWindow(), 'nope');
  assert.deepEqual(ids(window), []);
  assert.deepEqual(ids(removeRow(openWindow(page(['a', 'b'], true, true)), 'a')), ['b']);
});
