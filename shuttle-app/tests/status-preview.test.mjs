import { test } from 'node:test';
import assert from 'node:assert/strict';
import { build } from 'esbuild';

const result = await build({ entryPoints: ['src/lib/statusPreview.ts'], bundle: true, platform: 'node', format: 'esm', write: false });
const {
  inlineSrc,
  isBareUrl,
  isEmojiOnlyText,
  linkPreviewHost,
  linkPreviewTitle,
  orderedPosts,
  safeExternalUrl,
} = await import(`data:text/javascript;base64,${Buffer.from(result.outputFiles[0].text).toString('base64')}`);

const post = (id, timestamp, extra = {}) => ({ id, media_type: 'image', timestamp, ...extra });

test('the first slide is the oldest post, whatever order the feed arrives in', () => {
  const newestFirst = { posts: [post('c', '2026-01-01T12:00:00Z'), post('b', '2026-01-01T11:00:00Z'), post('a', '2026-01-01T10:00:00Z')] };
  assert.deepEqual(orderedPosts(newestFirst).map((p) => p.id), ['a', 'b', 'c']);
  const oldestFirst = { posts: [post('a', '2026-01-01T10:00:00Z'), post('b', '2026-01-01T11:00:00Z')] };
  assert.deepEqual(orderedPosts(oldestFirst).map((p) => p.id), ['a', 'b']);
});

test('an unordered or timestamp-less status keeps its own order', () => {
  const mixed = { posts: [post('a', '2026-01-01T10:00:00Z'), post('b', null)] };
  assert.deepEqual(orderedPosts(mixed).map((p) => p.id), ['a', 'b']);
  assert.deepEqual(orderedPosts({}).map((p) => p.id), []);
});

test('a preview the webview can draw directly is passed through', () => {
  assert.equal(inlineSrc('data:image/png;base64,AAAA'), 'data:image/png;base64,AAAA');
  assert.equal(inlineSrc('blob:http://localhost/abc'), 'blob:http://localhost/abc');
  assert.equal(inlineSrc('/home/u/.local/share/shuttle/media_cache/x.jpg'), null);
  assert.equal(inlineSrc(null), null);
});

test('only web and mail links become clickable', () => {
  assert.equal(safeExternalUrl('https://www.instagram.com/reel/abc'), 'https://www.instagram.com/reel/abc');
  assert.equal(safeExternalUrl('javascript:alert(1)'), null);
  assert.equal(safeExternalUrl('file:///etc/passwd'), null);
  assert.equal(safeExternalUrl(''), null);
});

test('the link card is labelled with the host and the preview title', () => {
  assert.equal(linkPreviewHost('https://www.instagram.com/reel/Cxyz/?utm=1'), 'instagram.com');
  assert.equal(linkPreviewHost('facebook.com/watch?v=1'), 'facebook.com');
  assert.equal(linkPreviewTitle({ url: 'https://www.instagram.com/reel/Cxyz/', title: 'A reel by maya' }), 'A reel by maya');
  assert.equal(linkPreviewTitle({ url: 'https://www.facebook.com/watch?v=1', title: null }), 'facebook.com');
  assert.equal(linkPreviewTitle(null), 'Link');
});

test('a body that is only a link is not drawn as raw text', () => {
  assert.equal(isBareUrl('https://www.instagram.com/reel/Cxyz/'), true);
  assert.equal(isBareUrl('www.facebook.com/watch?v=1'), true);
  assert.equal(isBareUrl('look at this https://example.com'), false);
  assert.equal(isBareUrl('hello'), false);
});

test('emoji-only text is drawn large', () => {
  assert.equal(isEmojiOnlyText('🔥'), true);
  assert.equal(isEmojiOnlyText('🔥 🎉'), true);
  assert.equal(isEmojiOnlyText('🇬🇧'), true);
  assert.equal(isEmojiOnlyText('👍🏽'), true);
  assert.equal(isEmojiOnlyText('fire 🔥'), false);
  assert.equal(isEmojiOnlyText(''), false);
});
