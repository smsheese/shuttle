/** Where the voice recorder is, and the only thing the composer renders from. */
import { microphoneErrorMessage } from './micErrors';

export type RecorderState = 'idle' | 'recording' | 'paused' | 'recorded' | 'error';

export type RecorderAction = 'start' | 'pause' | 'resume' | 'stop' | 'discard' | 'fail';

/** The one table of legal moves. An action missing from a state does nothing. */
const TRANSITIONS: Record<RecorderState, Partial<Record<RecorderAction, RecorderState>>> = {
  idle: { start: 'recording', fail: 'error' },
  recording: { pause: 'paused', stop: 'recorded', discard: 'idle', fail: 'error' },
  paused: { resume: 'recording', stop: 'recorded', discard: 'idle', fail: 'error' },
  recorded: { discard: 'idle' },
  error: { start: 'recording', discard: 'idle' },
};

/** The state after `action`, or null when the action does not apply there. */
export function recorderNext(state: RecorderState, action: RecorderAction): RecorderState | null {
  return TRANSITIONS[state][action] ?? null;
}

export interface RecorderControls {
  /** The mic button after Send, which starts a take. */
  mic: boolean;
  /** The pause/stop pair that fills the mic button's footprint during a take. */
  stack: boolean;
  /** The stack's top button pauses; otherwise it resumes. */
  pausing: boolean;
  /** The clip player in place of the text input. */
  player: boolean;
  /** The live strip (dot, clock, meter) in place of the text input. */
  strip: boolean;
  /** The text input and the attach tools are off during a take and while reviewing. */
  inputDisabled: boolean;
  /** Send can carry something: the pending files or the clip. */
  sendable: boolean;
}

const CONTROLS: Record<RecorderState, RecorderControls> = {
  idle: { mic: true, stack: false, pausing: false, player: false, strip: false, inputDisabled: false, sendable: false },
  recording: { mic: false, stack: true, pausing: true, player: false, strip: true, inputDisabled: true, sendable: false },
  paused: { mic: false, stack: true, pausing: false, player: false, strip: true, inputDisabled: true, sendable: false },
  recorded: { mic: false, stack: false, pausing: false, player: true, strip: false, inputDisabled: true, sendable: true },
  error: { mic: true, stack: false, pausing: false, player: false, strip: false, inputDisabled: false, sendable: false },
};

export function recorderControls(state: RecorderState): RecorderControls {
  return CONTROLS[state];
}

/** `m:ss`, the shape a voice note shows. */
export function formatClock(ms: number): string {
  const total = Math.max(0, Math.floor(ms / 1000));
  const minutes = Math.floor(total / 60);
  const seconds = total % 60;
  return `${minutes}:${String(seconds).padStart(2, '0')}`;
}

/** How many bars the waveform draws. */
export const WAVE_BARS = 42;

/**
 * Peak amplitude per bar, scaled so the loudest bar fills the height. A quiet take keeps its
 * shape instead of looking empty, and every bar keeps a floor so silence still draws.
 */
export function peaksFromChannelData(channels: Float32Array[], buckets = WAVE_BARS): number[] {
  const length = channels[0]?.length ?? 0;
  if (!length || buckets <= 0 || channels.length === 0) return [];
  const per = length / buckets;
  const peaks: number[] = [];
  let loudest = 0;
  for (let bucket = 0; bucket < buckets; bucket += 1) {
    const start = Math.floor(bucket * per);
    const end = Math.min(length, Math.max(start + 1, Math.floor((bucket + 1) * per)));
    let peak = 0;
    for (const channel of channels) {
      for (let i = start; i < end; i += 1) {
        const value = Math.abs(channel[i]);
        if (value > peak) peak = value;
      }
    }
    if (peak > loudest) loudest = peak;
    peaks.push(peak);
  }
  const scale = loudest > 0 ? 1 / loudest : 0;
  return peaks.map((peak) => Math.max(0.06, Math.min(1, peak * scale)));
}

/** What the browser said, in words a person can act on. */
export function recordErrorMessage(error: unknown, userAgent?: string): string {
  return microphoneErrorMessage(
    error,
    userAgent ?? (typeof navigator === 'undefined' ? '' : navigator.userAgent),
  );
}
