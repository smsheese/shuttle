import { setDatetimeFormat } from '$lib/dateFormatPref.svelte';
import { setGiphyKey } from '$lib/giphy';
import * as SunCalc from 'suncalc';
import type { AppConfig } from './types';
import {
  bundledThemeForId,
  isLegacyPreset,
  isTweakcnThemeId,
  loadThemeFonts,
  loadFontsFromCss,
  parseTweakcnId,
  type TweakcnTheme,
} from './tweakcn';

export function resolvedScheme(scheme: string): 'light' | 'dark' {
  if (scheme === 'light' || scheme === 'dark') return scheme;
  if (typeof window !== 'undefined' && window.matchMedia('(prefers-color-scheme: light)').matches) {
    return 'light';
  }
  return 'dark';
}

export function scheduledScheme(appearance: AppConfig['appearance'], now = new Date()): 'light' | 'dark' {
  const mode = appearance.dark_schedule;
  if (!mode) return resolvedScheme(appearance.color_scheme);
  if (mode === 'always') return 'dark';
  if (mode === 'off') return 'light';
  if (mode === 'system') return resolvedScheme('system');
  if (mode === 'time') {
    const current = now.getHours() * 60 + now.getMinutes();
    const parse = (value: string) => { const [h, m] = value.split(':').map(Number); return h * 60 + m; };
    const from = parse(appearance.dark_from || '18:00');
    const to = parse(appearance.dark_to || '06:00');
    return (from === to ? false : from < to ? current >= from && current < to : current >= from || current < to) ? 'dark' : 'light';
  }
  const lat = appearance.dark_latitude, lon = appearance.dark_longitude;
  if (mode === 'sun' && lat != null && lon != null) {
    const events: { time: number; dark: boolean }[] = [];
    for (let offset = -2; offset <= 2; offset++) {
      const day = new Date(now.getTime() + offset * 86400000);
      const times = SunCalc.getTimes(day, lat, lon);
      if (times.sunrise) events.push({ time: times.sunrise.getTime(), dark: false });
      if (times.sunset) events.push({ time: times.sunset.getTime(), dark: true });
    }
    const latest = events.filter((e) => Number.isFinite(e.time) && e.time <= now.getTime()).sort((a, b) => b.time - a.time)[0];
    return latest ? (latest.dark ? 'dark' : 'light') : (SunCalc.getPosition(now, lat, lon).altitude < 0 ? 'dark' : 'light');
  }
  return 'light';
}

let currentConfig: AppConfig | undefined;
let scheduleTimer: ReturnType<typeof setInterval> | undefined;
let appliedMode: string | undefined;

function clearTweakcnStyle(): void {
  document.getElementById('shuttle-tweakcn')?.remove();
  document.getElementById('shuttle-theme-fonts')?.remove();
}

function applyTweakcnTheme(theme: TweakcnTheme, css: string): void {
  let el = document.getElementById('shuttle-tweakcn');
  if (!el) {
    el = document.createElement('style');
    el.id = 'shuttle-tweakcn';
    document.head.appendChild(el);
  }
  el.textContent = css;
  loadThemeFonts(theme);
  document.body.style.fontFamily = 'var(--font)';
}

function themeFromConfig(cfg: AppConfig): TweakcnTheme | null {
  const id = parseTweakcnId(cfg.appearance.theme_id);
  if (!id) return null;
  return bundledThemeForId(id);
}

/** Apply appearance config to the document (sync; expects tweakcn CSS to already be cached). */
export function applyAppConfig(cfg: AppConfig): void {
  setDatetimeFormat(cfg.appearance.datetime_format);
  setGiphyKey(cfg.integrations?.giphy_api_key);
  if (typeof document === 'undefined') return;
  const root = document.documentElement;
  currentConfig = cfg;
  if (!scheduleTimer && typeof window !== 'undefined') scheduleTimer = setInterval(() => { if (currentConfig && scheduledScheme(currentConfig.appearance) !== appliedMode) applyAppConfig(currentConfig); }, 60000);
  const themeMode = scheduledScheme(cfg.appearance);
  appliedMode = themeMode;
  root.dataset.theme = themeMode;
  root.style.colorScheme = themeMode;
  document.body.style.colorScheme = themeMode;

  const scale = Number(cfg.appearance.font_scale ?? 1);
  const clamped = Number.isFinite(scale) ? Math.min(1.5, Math.max(0.8, scale)) : 1;
  root.style.setProperty('--font-scale', String(clamped));

  const tweakcnCss = cfg.appearance.tweakcn_css?.trim();
  const usesTweakcn = !isLegacyPreset(cfg.appearance.theme_id) && isTweakcnThemeId(cfg.appearance.theme_id);

  if (usesTweakcn && tweakcnCss) {
    root.dataset.preset = 'tweakcn';
    const theme = themeFromConfig(cfg);
    if (theme) {
      applyTweakcnTheme(theme, tweakcnCss);
    } else {
      let el = document.getElementById('shuttle-tweakcn');
      if (!el) {
        el = document.createElement('style');
        el.id = 'shuttle-tweakcn';
        document.head.appendChild(el);
      }
      el.textContent = tweakcnCss;
      loadFontsFromCss(tweakcnCss);
      document.body.style.fontFamily = 'var(--font)';
    }
    return;
  }

  clearTweakcnStyle();
  root.dataset.preset = cfg.appearance.theme_id || 'shuttle';
  document.body.style.fontFamily = 'var(--font)';
}
