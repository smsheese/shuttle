import { test } from 'node:test';
import assert from 'node:assert/strict';
import { build } from 'esbuild';

const result = await build({ entryPoints: ['src/lib/micErrors.ts'], bundle: true, platform: 'node', format: 'esm', write: false });
const { desktopOs, microphoneErrorMessage } = await import(`data:text/javascript;base64,${Buffer.from(result.outputFiles[0].text).toString('base64')}`);

const named = (name) => Object.assign(new Error('not allowed'), { name });

const MAC = 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko)';
const WINDOWS = 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36 Edg/120.0.0.0';
const LINUX = 'Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/605.1.15 (KHTML, like Gecko)';

test('the user agent names the OS that owns the microphone switch', () => {
  assert.equal(desktopOs(MAC), 'macos');
  assert.equal(desktopOs(WINDOWS), 'windows');
  assert.equal(desktopOs(LINUX), 'linux');
});

test('a macOS denial sends the person to the macOS switch', () => {
  const message = microphoneErrorMessage(named('NotAllowedError'), MAC);
  assert.match(message, /System Settings > Privacy & Security > Microphone/);
});

test('a Windows denial sends the person to the Windows switch', () => {
  const message = microphoneErrorMessage(named('NotAllowedError'), WINDOWS);
  assert.match(message, /Settings > Privacy & security > Microphone/);
  assert.match(message, /Let desktop apps access your microphone/);
});

test('on Linux a denial never blames an app setting that does not exist', () => {
  const message = microphoneErrorMessage(named('NotAllowedError'), LINUX);
  assert.doesNotMatch(message, /Privacy|Settings >|system settings/i);
  assert.match(message, /microphone/);
});

test('a missing device says no microphone was found', () => {
  assert.match(microphoneErrorMessage(named('NotFoundError'), LINUX), /No microphone was found/);
  assert.match(microphoneErrorMessage(named('DevicesNotFoundError'), MAC), /No microphone was found/);
});

test('a device another app holds says so instead of blaming settings', () => {
  const message = microphoneErrorMessage(named('NotReadableError'), WINDOWS);
  assert.match(message, /Another app is using the microphone/);
  assert.doesNotMatch(message, /Settings >/);
});

test('an unexpected failure keeps the browser message', () => {
  const message = microphoneErrorMessage(new Error('stream blew up'), LINUX);
  assert.match(message, /The microphone could not start: stream blew up/);
});
