import { test } from 'node:test';
import assert from 'node:assert/strict';
import { build } from 'esbuild';

const result = await build({ entryPoints: ['src/lib/whatsappLinks.ts'], bundle: true, platform: 'node', format: 'esm', write: false });
const { phoneDigitsFromRemoteId, whatsappChatUrl, whatsappCallFallback } = await import(
  `data:text/javascript;base64,${Buffer.from(result.outputFiles[0].text).toString('base64')}`
);

test('a phone JID gives the digits a call link needs', () => {
  assert.equal(phoneDigitsFromRemoteId('447700900123@s.whatsapp.net'), '447700900123');
  assert.equal(phoneDigitsFromRemoteId('447700900123:12@s.whatsapp.net'), '447700900123');
  assert.equal(phoneDigitsFromRemoteId('15550100@c.us'), '15550100');
  assert.equal(phoneDigitsFromRemoteId('+44 7700 900123'), '447700900123');
});

test('a LID, a group and a channel never look like a phone number', () => {
  assert.equal(phoneDigitsFromRemoteId('123456789012345@lid'), null);
  assert.equal(phoneDigitsFromRemoteId('120363000000000000@g.us'), null);
  assert.equal(phoneDigitsFromRemoteId('120363000000000000@newsletter'), null);
  assert.equal(phoneDigitsFromRemoteId('r1'), null);
  assert.equal(phoneDigitsFromRemoteId(''), null);
  assert.equal(phoneDigitsFromRemoteId(null), null);
});

test('the fallback opens the chat when it has a number and says so either way', () => {
  const known = whatsappCallFallback('audio', '447700900123@s.whatsapp.net');
  assert.equal(known.url, 'https://wa.me/447700900123');
  assert.match(known.message, /Audio calls start in WhatsApp/);

  const video = whatsappCallFallback('video', '447700900123@s.whatsapp.net');
  assert.match(video.message, /Video calls start in WhatsApp/);

  const unknown = whatsappCallFallback('audio', 'r1');
  assert.equal(unknown.url, null);
  assert.match(unknown.message, /no number for this contact/);
  assert.equal(whatsappChatUrl('r1'), null);
});
