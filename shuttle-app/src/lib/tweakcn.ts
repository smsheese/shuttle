import defaultThemeJson from './tweakcn/default-theme.json';
import type { AppConfig } from './types';

export const DEFAULT_TWEAKCN_THEME_ID = 'cmlhfpjhw000004l4f4ax3m7z';

export const LEGACY_THEME_PRESETS = ['shuttle', 'zinc', 'ocean', 'twilight'] as const;

export type LegacyThemePreset = (typeof LEGACY_THEME_PRESETS)[number];

export interface TweakcnTheme {
  name?: string;
  cssVars: {
    theme?: Record<string, string>;
    light: Record<string, string>;
    dark: Record<string, string>;
  };
}

const SYSTEM_FONTS = new Set([
  'system-ui',
  'sans-serif',
  'serif',
  'monospace',
  'ui-sans-serif',
  'ui-monospace',
  'ui-serif',
  'Georgia',
]);

/** Parse a tweakcn share URL or raw theme id. */
export function parseTweakcnId(input: string): string | null {
  const trimmed = input.trim();
  if (!trimmed) return null;
  const urlMatch = trimmed.match(/tweakcn\.com\/r\/themes\/([a-z0-9]+)/i);
  if (urlMatch) return urlMatch[1];
  if (/^[a-z0-9]{16,}$/i.test(trimmed)) return trimmed;
  return null;
}

export function isLegacyPreset(themeId: string): themeId is LegacyThemePreset {
  return (LEGACY_THEME_PRESETS as readonly string[]).includes(themeId);
}

export function isTweakcnThemeId(themeId: string): boolean {
  return parseTweakcnId(themeId) !== null;
}

function cssVarName(key: string): string {
  return key.startsWith('--') ? key : `--${key}`;
}

function pick(vars: Record<string, string>, key: string): string | undefined {
  return vars[key] ?? vars[cssVarName(key)];
}

function shuttleMappings(vars: Record<string, string>): string[] {
  const lines: string[] = [];
  const background = pick(vars, 'background');
  const card = pick(vars, 'card');
  const sidebar = pick(vars, 'sidebar') ?? pick(vars, 'popover');
  const foreground = pick(vars, 'foreground');
  const mutedForeground = pick(vars, 'muted-foreground');
  const secondaryForeground = pick(vars, 'secondary-foreground');
  const primary = pick(vars, 'primary');
  const accent = pick(vars, 'accent');
  const muted = pick(vars, 'muted');
  const border = pick(vars, 'border');
  const input = pick(vars, 'input');
  const radius = pick(vars, 'radius');
  const fontSans = pick(vars, 'font-sans');

  if (background) lines.push(`  --bg-main: ${background};`);
  if (card) lines.push(`  --bg-panel: ${card};`);
  if (sidebar) lines.push(`  --bg-sidebar: ${sidebar};`);
  if (foreground) lines.push(`  --text: ${foreground};`);
  if (mutedForeground) lines.push(`  --text-muted: ${mutedForeground};`);
  if (secondaryForeground) lines.push(`  --text-secondary: ${secondaryForeground};`);
  if (primary) {
    lines.push(`  --accent: ${primary};`);
    lines.push(`  --accent-hover: color-mix(in oklch, ${primary} 88%, black);`);
    lines.push(`  --bg-bubble-out: ${primary};`);
  }
  if (accent) {
    lines.push(`  --accent-muted: color-mix(in oklch, ${accent} 18%, transparent);`);
  } else if (primary) {
    lines.push(`  --accent-muted: color-mix(in oklch, ${primary} 15%, transparent);`);
  }
  if (border) {
    lines.push(`  --border: ${border};`);
    lines.push(`  --border-subtle: color-mix(in oklch, ${border} 55%, transparent);`);
  }
  if (input) lines.push(`  --bg-input: ${input};`);
  if (muted) {
    lines.push(`  --bg-hover: ${muted};`);
    lines.push(
      `  --bg-active: color-mix(in oklch, ${muted} 82%, ${foreground ?? 'currentColor'});`
    );
    lines.push(`  --bg-bubble-in: ${muted};`);
  } else if (card) {
    lines.push(`  --bg-bubble-in: ${card};`);
  }
  if (radius) {
    lines.push(`  --radius-md: ${radius};`);
    lines.push(`  --radius-sm: calc(${radius} * 0.6);`);
    lines.push(`  --radius-lg: calc(${radius} * 1.2);`);
  }
  if (fontSans) lines.push(`  --font: ${fontSans};`);
  if (primary && input) {
    lines.push(`  --select-bg: color-mix(in oklch, ${primary} 10%, ${input});`);
    lines.push(`  --select-border: color-mix(in oklch, ${primary} 30%, ${border ?? input});`);
  } else if (input) {
    lines.push(`  --select-bg: ${input};`);
    if (border) lines.push(`  --select-border: ${border};`);
  }

  return lines;
}

function blockForMode(
  modeVars: Record<string, string>,
  shared?: Record<string, string>
): string {
  const merged = { ...(shared ?? {}), ...modeVars };
  const shadcn = Object.entries(merged)
    .map(([key, value]) => `  ${cssVarName(key)}: ${value};`)
    .join('\n');
  const shuttle = shuttleMappings(merged).join('\n');
  return `${shadcn}\n${shuttle}`;
}

/** Convert tweakcn registry JSON into Shuttle CSS with separate light/dark blocks. */
export function themeToCss(theme: TweakcnTheme): string {
  const shared = theme.cssVars.theme ?? {};
  const light = blockForMode(theme.cssVars.light, shared);
  const dark = blockForMode(theme.cssVars.dark, shared);
  return `:root[data-theme='light'] {\n${light}\n}\n:root[data-theme='dark'] {\n${dark}\n}\nbody {\n  font-family: var(--font);\n  letter-spacing: var(--tracking-normal, normal);\n}`;
}

export function bundledThemeForId(themeId: string): TweakcnTheme | null {
  if (themeId === DEFAULT_TWEAKCN_THEME_ID) {
    return defaultThemeJson as TweakcnTheme;
  }
  return null;
}

export function bundledCssForId(themeId: string): string | null {
  const theme = bundledThemeForId(themeId);
  return theme ? themeToCss(theme) : null;
}

function parseFontFamilies(value: string): string[] {
  return value
    .split(',')
    .map((part) => part.trim().replace(/^['"]|['"]$/g, ''))
    .filter(Boolean);
}

function isLoadableGoogleFont(family: string): boolean {
  if (!family || SYSTEM_FONTS.has(family)) return false;
  return /^[A-Za-z0-9 ]+$/.test(family);
}

/** Inject or update Google Fonts stylesheet for theme typography. */
export function loadThemeFonts(theme: TweakcnTheme): void {
  if (typeof document === 'undefined') return;
  const families = new Set<string>();
  for (const bucket of [
    theme.cssVars.theme,
    theme.cssVars.light,
    theme.cssVars.dark,
  ]) {
    if (!bucket) continue;
    for (const key of ['font-sans', 'font-mono', 'font-serif']) {
      const value = bucket[key];
      if (!value) continue;
      for (const family of parseFontFamilies(value)) {
        if (isLoadableGoogleFont(family)) families.add(family);
      }
    }
  }
  const linkId = 'shuttle-theme-fonts';
  if (families.size === 0) {
    document.getElementById(linkId)?.remove();
    return;
  }
  const params = [...families]
    .map((family) => {
      const encoded = family.replace(/ /g, '+');
      const weights =
        family.toLowerCase().includes('mono') || family.toLowerCase().includes('jetbrains')
          ? 'wght@400;500;600'
          : 'wght@400;500;600;700';
      return `family=${encoded}:${weights}`;
    })
    .join('&');
  let link = document.getElementById(linkId) as HTMLLinkElement | null;
  if (!link) {
    link = document.createElement('link');
    link.id = linkId;
    link.rel = 'stylesheet';
    document.head.appendChild(link);
  }
  link.href = `https://fonts.googleapis.com/css2?${params}&display=swap`;
}

/** Inject Google Fonts from generated theme CSS (works after reload). */
export function loadFontsFromCss(css: string): void {
  const sans = css.match(/--font-sans:\s*([^;]+);/)?.[1];
  const mono = css.match(/--font-mono:\s*([^;]+);/)?.[1];
  if (!sans && !mono) return;
  loadThemeFonts({
    cssVars: {
      light: {
        ...(sans ? { 'font-sans': sans } : {}),
        ...(mono ? { 'font-mono': mono } : {}),
      },
      dark: {},
    },
  });
}
export async function ensureThemeConfig(
  cfg: AppConfig,
  fetchTheme: (id: string) => Promise<TweakcnTheme>
): Promise<AppConfig> {
  const themeId = cfg.appearance.theme_id;
  if (isLegacyPreset(themeId)) return cfg;
  const id = parseTweakcnId(themeId);
  if (!id) return cfg;
  if (cfg.appearance.tweakcn_css?.trim()) return cfg;

  const bundled = bundledThemeForId(id);
  if (bundled) {
    return {
      ...cfg,
      appearance: {
        ...cfg.appearance,
        theme_id: id,
        tweakcn_css: themeToCss(bundled),
      },
    };
  }

  const fetched = await fetchTheme(id);
  return {
    ...cfg,
    appearance: {
      ...cfg.appearance,
      theme_id: id,
      tweakcn_css: themeToCss(fetched),
    },
  };
}

export function themeDisplayName(themeId: string): string {
  if (isLegacyPreset(themeId)) {
    return themeId.charAt(0).toUpperCase() + themeId.slice(1);
  }
  if (themeId === DEFAULT_TWEAKCN_THEME_ID) return 'Light Green';
  return themeId.slice(0, 10) + '…';
}
