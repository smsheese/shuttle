import { test } from 'node:test';
import assert from 'node:assert/strict';
import { build } from 'esbuild';

const result = await build({ entryPoints: ['src/lib/attachments.ts'], bundle: true, platform: 'node', format: 'esm', write: false });
const { classifyFile, guessMime, kindFromMime, removeAttachment, fileExtension } = await import(
  `data:text/javascript;base64,${Buffer.from(result.outputFiles[0].text).toString('base64')}`
);

test('a file with no mime type is classified from its extension', () => {
  for (const [name, kind, mime] of [
    ['holiday.HEIC', 'image', 'image/heic'],
    ['clip.mkv', 'video', 'video/x-matroska'],
    ['take.flac', 'audio', 'audio/flac'],
    ['backup.zip', 'document', 'application/zip'],
    ['shuttle.apk', 'document', 'application/vnd.android.package-archive'],
    ['firmware.bin', 'document', 'application/octet-stream'],
    ['no-extension', 'document', 'application/octet-stream'],
  ]) {
    assert.deepEqual(classifyFile(name, ''), { kind, mime }, name);
  }
});

test('a browser that reports octet-stream for an unknown type still gets the real type', () => {
  assert.deepEqual(classifyFile('archive.zip', 'application/octet-stream'), {
    kind: 'document',
    mime: 'application/zip',
  });
});

test('the type the browser reported wins over the extension', () => {
  assert.deepEqual(classifyFile('mislabelled.pdf', 'image/webp'), { kind: 'image', mime: 'image/webp' });
});

test('the Photo, Video and Audio entries mean what they say', () => {
  assert.deepEqual(classifyFile('surprise.bin', '', 'video'), { kind: 'video', mime: 'application/octet-stream' });
  assert.deepEqual(classifyFile('surprise.bin', '', 'audio'), { kind: 'audio', mime: 'application/octet-stream' });
  assert.deepEqual(classifyFile('surprise.bin', '', 'image'), { kind: 'image', mime: 'application/octet-stream' });
});

test('Document makes no promise, so the file decides', () => {
  assert.deepEqual(classifyFile('shot.png', 'image/png', 'document'), { kind: 'image', mime: 'image/png' });
  assert.deepEqual(classifyFile('shuttle.apk', '', 'document'), {
    kind: 'document',
    mime: 'application/vnd.android.package-archive',
  });
});

test('an svg is a document, because WhatsApp would not show it as a photo', () => {
  assert.equal(classifyFile('logo.svg', 'image/svg+xml').kind, 'document');
});

test('kindFromMime covers the three media families and nothing else', () => {
  assert.equal(kindFromMime('image/png'), 'image');
  assert.equal(kindFromMime('video/mp4'), 'video');
  assert.equal(kindFromMime('audio/ogg'), 'audio');
  assert.equal(kindFromMime('application/pdf'), 'document');
  assert.equal(kindFromMime(''), 'document');
});

test('guessMime and fileExtension read the last dot only', () => {
  assert.equal(fileExtension('archive.backup.zip'), 'zip');
  assert.equal(fileExtension('.hidden'), '');
  assert.equal(fileExtension('plain'), '');
  assert.equal(guessMime('README'), 'application/octet-stream');
  assert.equal(guessMime('photo.jpeg'), 'image/jpeg');
});

test('removing one attachment keeps the rest in order', () => {
  const items = [
    { id: 'a', filename: 'a.png' },
    { id: 'b', filename: 'b.zip' },
    { id: 'c', filename: 'c.bin' },
  ];
  assert.deepEqual(
    removeAttachment(items, 'b').map((item) => item.id),
    ['a', 'c']
  );
  assert.deepEqual(removeAttachment(items, 'missing'), items);
});
