<script lang="ts">
  import { WAVE_BARS, formatClock } from '$lib/recorder';

  interface Props {
    src: string;
    peaks: number[];
    durationMs: number;
    ondiscard: () => void;
  }

  let { src, peaks, durationMs, ondiscard }: Props = $props();

  let audioEl = $state<HTMLAudioElement | null>(null);
  let playing = $state(false);
  let progress = $state(0);
  let nowMs = $state(0);

  const bars = $derived(peaks.length ? peaks : Array.from({ length: WAVE_BARS }, () => 0.06));
  const duration = $derived(clipMs());

  /**
   * A MediaRecorder webm carries no duration until something plays it, so the audio element can
   * report `Infinity`. Fall back to the clock the recorder measured. Milliseconds.
   */
  function clipMs(): number {
    const reported = audioEl?.duration;
    return reported && Number.isFinite(reported) && reported > 0 ? reported * 1000 : durationMs;
  }

  function togglePlay() {
    const audio = audioEl;
    if (!audio) return;
    if (audio.paused) void audio.play();
    else audio.pause();
  }

  function seekTo(ratio: number) {
    const audio = audioEl;
    if (!audio) return;
    const clamped = Math.min(1, Math.max(0, ratio));
    audio.currentTime = (clipMs() / 1000) * clamped;
    progress = clamped;
    nowMs = clipped(clamped * duration);
  }

  /** A webm without a duration header may take the seek past what it can report. */
  function clipped(ms: number): number {
    return Math.min(duration, Math.max(0, ms));
  }

  function seekAt(event: MouseEvent) {
    const box = (event.currentTarget as HTMLElement).getBoundingClientRect();
    if (!box.width) return;
    seekTo((event.clientX - box.left) / box.width);
  }

  function seekKey(event: KeyboardEvent) {
    if (event.key === 'ArrowRight') {
      event.preventDefault();
      seekTo(progress + 0.05);
    } else if (event.key === 'ArrowLeft') {
      event.preventDefault();
      seekTo(progress - 0.05);
    }
  }

  function onTimeUpdate() {
    const audio = audioEl;
    if (!audio) return;
    nowMs = clipped(audio.currentTime * 1000);
    progress = Math.min(1, (audio.currentTime * 1000) / Math.max(1, clipMs()));
  }
</script>

<div class="clip">
  <button
    type="button"
    class="clip-play"
    onclick={togglePlay}
    aria-label={playing ? 'Pause the recording' : 'Play the recording'}
    title={playing ? 'Pause' : 'Play'}
  >
    {#if playing}
      <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><path d="M7 4h4v16H7zM13 4h4v16h-4z"/></svg>
    {:else}
      <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><path d="M8 5v14l11-7z"/></svg>
    {/if}
  </button>

  <div
    class="clip-track"
    role="slider"
    tabindex="0"
    aria-label="Seek within the recording"
    aria-valuemin="0"
    aria-valuemax={Math.round(duration / 1000)}
    aria-valuenow={Math.round(nowMs / 1000)}
    aria-valuetext={formatClock(nowMs)}
    onclick={seekAt}
    onkeydown={seekKey}
  >
    <span class="bars">
      {#each bars as peak, at (at)}
        <span class="bar" class:played={at / bars.length < progress} style="height: {Math.round(5 + peak * 21)}px"></span>
      {/each}
    </span>
    <span class="playhead" style="left: {progress * 100}%"></span>
  </div>

  <span class="clip-clock">{formatClock(nowMs)} / {formatClock(duration)}</span>

  <button
    type="button"
    class="clip-trash"
    onclick={ondiscard}
    aria-label="Discard the recording"
    title="Discard"
  >
    <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><path d="M3 6h18M8 6V4h8v2M19 6l-1 14H6L5 6M10 11v5M14 11v5"/></svg>
  </button>

  <audio
    bind:this={audioEl}
    src={src}
    preload="metadata"
    onplay={() => (playing = true)}
    onpause={() => (playing = false)}
    ontimeupdate={onTimeUpdate}
    onended={() => {
      playing = false;
      progress = 1;
      nowMs = duration;
    }}
  ></audio>
</div>

<style>
  .clip {
    display: flex;
    align-items: center;
    gap: 8px;
    flex: 1;
    min-width: 0;
    padding: 4px 0;
  }

  .clip-play {
    flex: none;
    width: 30px;
    height: 30px;
    border: none;
    border-radius: 50%;
    background: var(--accent);
    color: var(--text-on-accent, var(--primary-foreground, #ffffff));
    display: grid;
    place-items: center;
    cursor: pointer;
  }

  .clip-play:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }

  .clip-track {
    position: relative;
    flex: 1;
    min-width: 0;
    height: 32px;
    display: flex;
    align-items: center;
    cursor: pointer;
    border-radius: var(--radius-sm);
  }

  .clip-track:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }

  .bars {
    display: flex;
    align-items: center;
    gap: 2px;
    width: 100%;
    height: 100%;
  }

  .bar {
    flex: 1 1 0;
    min-width: 1px;
    border-radius: 999px;
    background: var(--text-muted);
    opacity: 0.55;
    transition: background 90ms linear, opacity 90ms linear;
  }

  .bar.played {
    background: var(--accent);
    opacity: 1;
  }

  .playhead {
    position: absolute;
    top: 2px;
    bottom: 2px;
    width: 2px;
    border-radius: 999px;
    background: var(--accent);
    transform: translateX(-1px);
    pointer-events: none;
  }

  .clip-clock {
    flex: none;
    font-size: 11px;
    color: var(--text-muted);
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }

  .clip-trash {
    flex: none;
    width: 28px;
    height: 28px;
    border: none;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--text-secondary);
    display: grid;
    place-items: center;
    cursor: pointer;
  }

  .clip-trash:hover {
    background: var(--bg-hover);
    color: var(--destructive, #ef4444);
  }

  .clip-trash:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 1px;
  }

  audio {
    display: none;
  }
</style>
