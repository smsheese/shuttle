import { test } from 'node:test';
import assert from 'node:assert/strict';
import { build } from 'esbuild';

const result = await build({ entryPoints: ['src/lib/tweakcn.ts'], bundle: true, platform: 'node', format: 'esm', write: false });
const { parseTweakcnId, themeToCss } = await import(`data:text/javascript;base64,${Buffer.from(result.outputFiles[0].text).toString('base64')}`);
const id = 'cmjgilzlg000404ju2wgs7uj9';

test('accepts a theme ID and both tweakcn URL forms', () => {
  for (const input of [id, `https://tweakcn.com/themes/${id}`, `https://tweakcn.com/r/themes/${id}?x=1`]) {
    assert.equal(parseTweakcnId(input), id);
  }
  assert.equal(parseTweakcnId(`https://tweakcn.com.evil.example/themes/${id}`), null);
});

test('maps fetched light and dark colors into Shuttle tokens', () => {
  const css = themeToCss({ cssVars: { light: { primary: '#123456' }, dark: { primary: '#abcdef' } } });
  assert.match(css, /--accent: #123456/);
  assert.match(css, /--accent: #abcdef/);
});
