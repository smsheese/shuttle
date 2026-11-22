import { test } from 'node:test';
import assert from 'node:assert/strict';
import { build } from 'esbuild';

const result = await build({ entryPoints: ['src/lib/recorder.ts'], bundle: true, platform: 'node', format: 'esm', write: false });
const { recorderNext, recorderControls, formatClock, peaksFromChannelData, recordErrorMessage, WAVE_BARS } = await import(
  `data:text/javascript;base64,${Buffer.from(result.outputFiles[0].text).toString('base64')}`
);

const STATES = ['idle', 'recording', 'paused', 'recorded', 'error'];

test('the recorder walks the states a take needs', () => {
  let state = 'idle';
  for (const action of ['start', 'pause', 'resume', 'stop', 'discard']) {
    state = recorderNext(state, action);
  }
  assert.equal(state, 'idle');
  assert.equal(recorderNext('idle', 'pause'), null, 'nothing to pause before recording');
  assert.equal(recorderNext('recorded', 'stop'), null, 'stopping twice does nothing');
  assert.equal(recorderNext('recorded', 'start'), null, 'the mic is gone while reviewing');
});

test('a failure lands in error and a retry starts a fresh take', () => {
  assert.equal(recorderNext('recording', 'fail'), 'error');
  assert.equal(recorderNext('idle', 'fail'), 'error');
  assert.equal(recorderNext('error', 'start'), 'recording');
  assert.equal(recorderNext('error', 'discard'), 'idle');
  assert.equal(recorderNext('error', 'stop'), null);
});

test('every state answers every action without throwing', () => {
  for (const state of STATES) {
    for (const action of ['start', 'pause', 'resume', 'stop', 'discard', 'fail']) {
      const next = recorderNext(state, action);
      assert.ok(next === null || STATES.includes(next), `${state} + ${action} gave ${next}`);
    }
  }
});

test('the controls table shows exactly the buttons a state offers', () => {
  assert.deepEqual(recorderControls('idle'), {
    mic: true,
    stack: false,
    pausing: false,
    player: false,
    strip: false,
    inputDisabled: false,
    sendable: false,
  });
  assert.deepEqual(recorderControls('recording'), {
    mic: false,
    stack: true,
    pausing: true,
    player: false,
    strip: true,
    inputDisabled: true,
    sendable: false,
  });
  assert.deepEqual(recorderControls('paused'), {
    mic: false,
    stack: true,
    pausing: false,
    player: false,
    strip: true,
    inputDisabled: true,
    sendable: false,
  });
  assert.deepEqual(recorderControls('recorded'), {
    mic: false,
    stack: false,
    pausing: false,
    player: true,
    strip: false,
    inputDisabled: true,
    sendable: true,
  });
  assert.deepEqual(recorderControls('error'), {
    mic: true,
    stack: false,
    pausing: false,
    player: false,
    strip: false,
    inputDisabled: false,
    sendable: false,
  });
});

test('only the reviewing state can send a clip', () => {
  for (const state of STATES) {
    assert.equal(recorderControls(state).sendable, state === 'recorded', state);
  }
});

test('the clock counts minutes and zero pads seconds', () => {
  assert.equal(formatClock(0), '0:00');
  assert.equal(formatClock(999), '0:00');
  assert.equal(formatClock(1000), '0:01');
  assert.equal(formatClock(59_400), '0:59');
  assert.equal(formatClock(60_000), '1:00');
  assert.equal(formatClock(3_723_000), '62:03');
  assert.equal(formatClock(-5), '0:00');
});

test('peaks keep one bar per bucket and scale to the loudest one', () => {
  const quiet = new Float32Array([0, 0.1, 0, -0.2]);
  const loud = new Float32Array([0, 0.5, 0, -1]);
  const peaks = peaksFromChannelData([quiet, loud], 4);
  assert.equal(peaks.length, 4);
  assert.equal(Math.max(...peaks), 1);
  assert.equal(peaks[0], 0.06, 'silence keeps a floor so the bar still draws');
  assert.ok(peaks.every((peak) => peak >= 0.06 && peak <= 1));
});

test('peaks are empty rather than broken when there is no audio to read', () => {
  assert.deepEqual(peaksFromChannelData([], 8), []);
  assert.deepEqual(peaksFromChannelData([new Float32Array(0)], 8), []);
  assert.deepEqual(peaksFromChannelData([new Float32Array([0.2])], 0), []);
  assert.equal(peaksFromChannelData([new Float32Array(1000)]).length, WAVE_BARS);
});

test('the microphone errors say what a person can do next', () => {
  const mac = 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15';
  const denied = recordErrorMessage(Object.assign(new Error('denied'), { name: 'NotAllowedError' }), mac);
  assert.match(denied, /macOS has blocked the microphone/);
  assert.match(denied, /System Settings > Privacy & Security > Microphone/);
  assert.doesNotMatch(
    recordErrorMessage(Object.assign(new Error('denied'), { name: 'NotAllowedError' }), 'X11; Linux x86_64'),
    /settings/i,
  );
  assert.match(recordErrorMessage(Object.assign(new Error('none'), { name: 'NotFoundError' })), /No microphone/);
  assert.match(recordErrorMessage(Object.assign(new Error('busy'), { name: 'NotReadableError' })), /Another app/);
  assert.match(recordErrorMessage(new Error('boom')), /boom/);
  assert.match(recordErrorMessage('not an error'), /not an error/);
});
