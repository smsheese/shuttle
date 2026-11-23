import { test } from 'node:test';
import assert from 'node:assert/strict';
import { build } from 'esbuild';

const result = await build({
  entryPoints: ['src/lib/windowTitle.ts'],
  bundle: true,
  platform: 'node',
  format: 'esm',
  write: false,
  external: ['@tauri-apps/api/core', '@tauri-apps/api/window', '@tauri-apps/api/app'],
});
const { windowTitle } = await import(`data:text/javascript;base64,${Buffer.from(result.outputFiles[0].text).toString('base64')}`);

test('title is the app name and version with no unread count', () => {
  assert.equal(windowTitle('0.6.1', 0), 'Shuttle 0.6.1');
});

test('title leads with the unread count', () => {
  assert.equal(windowTitle('0.6.1', 3), '(3) Shuttle 0.6.1');
});
