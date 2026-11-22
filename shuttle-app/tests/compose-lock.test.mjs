import { test } from 'node:test';
import assert from 'node:assert/strict';
import { build } from 'esbuild';

const result = await build({ entryPoints: ['src/lib/composeLock.ts'], bundle: true, platform: 'node', format: 'esm', write: false });
const { composeLocked } = await import(`data:text/javascript;base64,${Buffer.from(result.outputFiles[0].text).toString('base64')}`);

test('an announce group this account does not administer locks the composer', () => {
  assert.equal(composeLocked({ metadata: { announce_only: true, self_is_admin: false } }), true);
  assert.equal(composeLocked({ metadata: { announce_only: true } }), true, 'a missing admin fact is not an admin');
});

test('an admin keeps the composer, and a group open to members keeps it too', () => {
  assert.equal(composeLocked({ metadata: { announce_only: true, self_is_admin: true } }), false);
  assert.equal(composeLocked({ metadata: { announce_only: false, self_is_admin: false } }), false);
});

test('a chat that never reported the facts stays writable', () => {
  for (const metadata of [{}, { names: { contact: 'Bob' } }, null, undefined]) {
    assert.equal(composeLocked({ metadata }), false);
  }
});
