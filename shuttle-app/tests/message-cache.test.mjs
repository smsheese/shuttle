import { test } from 'node:test';
import assert from 'node:assert/strict';
import { build } from 'esbuild';

const result = await build({ entryPoints: ['src/lib/messageCache.ts'], bundle: true, platform: 'node', format: 'esm', write: false });
const { MessageCache } = await import(`data:text/javascript;base64,${Buffer.from(result.outputFiles[0].text).toString('base64')}`);

const rows = (...ids) => ids.map((id) => ({ id }));

test('evicts the least recently used chat past the cap', () => {
  const cache = new MessageCache(3, 100);
  for (const id of ['a', 'b', 'c', 'd']) cache.set(id, rows(`${id}1`));
  assert.equal(cache.size, 3);
  assert.equal(cache.has('a'), false);
  assert.deepEqual(['b', 'c', 'd'], ['b', 'c', 'd'].filter((id) => cache.has(id)));
});

test('reading a chat makes it the most recently used', () => {
  const cache = new MessageCache(3, 100);
  cache.set('a', rows('a1'));
  cache.set('b', rows('b1'));
  cache.set('c', rows('c1'));
  cache.get('a');
  cache.set('d', rows('d1'));
  assert.equal(cache.has('b'), false);
  assert.equal(cache.has('a'), true);
});

test('keeps the newest rows of a chat and replaces older ones', () => {
  const cache = new MessageCache(3, 2);
  cache.set('a', rows('m1', 'm2', 'm3'));
  assert.deepEqual(cache.get('a').map((row) => row.id), ['m2', 'm3']);
  cache.set('a', rows('m4'));
  assert.deepEqual(cache.get('a').map((row) => row.id), ['m4']);
});
