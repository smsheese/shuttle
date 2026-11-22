import { test } from 'node:test';
import assert from 'node:assert/strict';
import { build } from 'esbuild';
const result = await build({ entryPoints: ['src/lib/theme.ts'], bundle: true, platform: 'node', format: 'esm', write: false });
const { scheduledScheme } = await import(`data:text/javascript;base64,${Buffer.from(result.outputFiles[0].text).toString('base64')}`);
const config = { color_scheme: 'light', dark_schedule: 'time', dark_from: '18:00', dark_to: '06:00' };
test('fixed schedule crosses midnight and switches at boundaries', () => {
  for (const [hour, expected] of [[5,'dark'], [6,'light'], [17,'light'], [18,'dark']]) assert.equal(scheduledScheme(config, new Date(2026, 5, 1, hour)), expected);
});
test('sun schedule uses location rather than device local time', () => {
  const sun = { ...config, dark_schedule: 'sun', dark_latitude: 24.7136, dark_longitude: 46.6753 };
  assert.equal(scheduledScheme(sun, new Date('2026-06-01T09:00:00Z')), 'light');
  assert.equal(scheduledScheme(sun, new Date('2026-06-01T22:00:00Z')), 'dark');
});
