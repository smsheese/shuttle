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
  if (typeof document === 'undefined') return;
  const root = document.documentElement;
  const themeMode = resolvedScheme(cfg.appearance.color_scheme);
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
