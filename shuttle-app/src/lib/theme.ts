import type { AppConfig } from './types';

const TWEAKCN_MAP: Record<string, string> = {
  '--background': '--bg-main',
  '--card': '--bg-panel',
  '--popover': '--bg-sidebar',
  '--foreground': '--text',
  '--muted-foreground': '--text-muted',
  '--primary': '--accent',
  '--border': '--border',
  '--input': '--bg-input',
  '--radius': '--radius-md',
};

export function resolvedScheme(scheme: string): 'light' | 'dark' {
  if (scheme === 'light' || scheme === 'dark') return scheme;
  if (typeof window !== 'undefined' && window.matchMedia('(prefers-color-scheme: light)').matches) {
    return 'light';
  }
  return 'dark';
}

export function applyAppConfig(cfg: AppConfig) {
  if (typeof document === 'undefined') return;
  const root = document.documentElement;
  const theme = resolvedScheme(cfg.appearance.color_scheme);
  root.dataset.theme = theme;
  root.dataset.preset = cfg.appearance.theme_id || 'shuttle';
  root.style.colorScheme = theme;

  let el = document.getElementById('shuttle-tweakcn');
  const css = cfg.appearance.tweakcn_css?.trim();
  if (css) {
    if (!el) {
      el = document.createElement('style');
      el.id = 'shuttle-tweakcn';
      document.head.appendChild(el);
    }
    el.textContent = `:root {\n${mapTweakcn(css)}\n}\n${css}`;
    root.dataset.preset = 'custom';
  } else if (el) {
    el.remove();
  }
}

function mapTweakcn(css: string): string {
  const lines: string[] = [];
  for (const [from, to] of Object.entries(TWEAKCN_MAP)) {
    const re = new RegExp(`${from.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}\\s*:\\s*([^;]+);`);
    const m = css.match(re);
    if (m) lines.push(`${to}: ${m[1].trim()};`);
  }
  const font = css.match(/--font-sans\s*:\s*([^;]+);/);
  if (font) lines.push(`--font: ${font[1].trim()};`);
  return lines.join('\n');
}
