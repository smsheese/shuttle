import { test } from 'node:test';
import assert from 'node:assert/strict';
import { build } from 'esbuild';

const result = await build({ entryPoints: ['src/lib/messageKinds.ts'], bundle: true, platform: 'node', format: 'esm', write: false });
const { richPayload, firstUrl, phoneDigits, vcardFor, videoEmbed } = await import(
  `data:text/javascript;base64,${Buffer.from(result.outputFiles[0].text).toString('base64')}`
);

const msg = (body, metadata = {}) => ({ body, metadata });

test('flattened contact body becomes a contact card', () => {
  const p = richPayload(msg('Contact: Omer Hussain Bhai Brother Qatar (+974 7038 9309)'));
  assert.deepEqual(p, { kind: 'contact', name: 'Omer Hussain Bhai Brother Qatar', phones: ['+974 7038 9309'] });
});

test('contact without a number still renders', () => {
  assert.deepEqual(richPayload(msg('Contact: Zunaira')), { kind: 'contact', name: 'Zunaira', phones: [] });
});

test('bare Google Maps link becomes a location card', () => {
  const p = richPayload(msg('https://maps.google.com/?q=24.6803947,46.7077217'));
  assert.equal(p.kind, 'location');
  assert.equal(p.lat, 24.6803947);
  assert.equal(p.lng, 46.7077217);
});

test('a maps link inside a long listing is not a location message', () => {
  assert.equal(richPayload(msg('Location:\nhttps://maps.google.com/?q=24.65,46.74\nRent 40,000')), null);
});

test('media messages and plain text stay as they are', () => {
  assert.equal(richPayload(msg('Contact: x (1)', { media_type: 'video' })), null);
  assert.equal(richPayload(msg('hello there')), null);
});

test('structured metadata wins over the body', () => {
  const p = richPayload(msg('[location]', { location: { lat: 25.2, lng: 51.5, name: 'Villaggio', address: 'Doha' } }));
  assert.deepEqual(p, { kind: 'location', lat: 25.2, lng: 51.5, name: 'Villaggio', address: 'Doha', live: false });
});

test('out-of-range coordinates are rejected', () => {
  assert.equal(richPayload(msg('https://maps.google.com/?q=124.6,46.7')), null);
});

test('firstUrl takes the first link and drops sentence punctuation', () => {
  assert.equal(firstUrl('see https://example.com/a?b=1, and https://other.com'), 'https://example.com/a?b=1');
  assert.equal(firstUrl('no link'), null);
  assert.equal(firstUrl('javascript:alert(1)'), null);
});

test('phoneDigits and vcardFor', () => {
  assert.equal(phoneDigits('+974 7038 9309'), '97470389309');
  assert.equal(phoneDigits('12'), null);
  assert.match(vcardFor({ kind: 'contact', name: 'A', phones: ['+1 555 0100'] }), /FN:A\r\nTEL;TYPE=CELL:\+1 555 0100/);
});

test('YouTube and Vimeo links become embeds, other links do not', () => {
  assert.equal(videoEmbed('https://www.youtube.com/watch?v=dQw4w9WgXcQ&t=5').id, 'dQw4w9WgXcQ');
  assert.equal(videoEmbed('https://youtu.be/dQw4w9WgXcQ?si=x').provider, 'youtube');
  assert.equal(videoEmbed('https://m.youtube.com/shorts/dQw4w9WgXcQ').id, 'dQw4w9WgXcQ');
  assert.equal(videoEmbed('https://vimeo.com/76979871').embedUrl, 'https://player.vimeo.com/video/76979871?dnt=1');
  assert.equal(videoEmbed('https://www.youtube.com/watch?v=short'), null);
  assert.equal(videoEmbed('https://evil.com/watch?v=dQw4w9WgXcQ'), null);
  assert.equal(videoEmbed('https://youtube.com.evil.com/watch?v=dQw4w9WgXcQ'), null);
  assert.equal(videoEmbed('not a url'), null);
});
