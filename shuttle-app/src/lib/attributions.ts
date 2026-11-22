export interface AttributionEntry {
  name: string;
  role: string;
  license: string;
  url?: string;
  notes?: string;
}

export interface AttributionSection {
  title: string;
  entries: AttributionEntry[];
}

export const SHUTTLE_LICENSE = {
  name: 'Shuttle',
  license: 'AGPL-3.0-or-later',
  url: 'https://github.com/smsheese/shuttle/blob/main/LICENSE',
  summary:
    'Shuttle is free and open source under the GNU AGPL v3 or later. A separate commercial license is available for products that cannot meet the AGPL terms.',
  sourceUrl: 'https://github.com/smsheese/shuttle',
} as const;

export const ATTRIBUTION_SECTIONS: AttributionSection[] = [
  {
    title: 'Desktop application',
    entries: [
      {
        name: 'Tauri 2',
        role: 'Desktop shell, IPC, bundling',
        license: 'MIT OR Apache-2.0',
        url: 'https://github.com/tauri-apps/tauri',
      },
      {
        name: 'Svelte 5 / SvelteKit',
        role: 'User interface',
        license: 'MIT',
        url: 'https://github.com/sveltejs/svelte',
      },
      {
        name: 'Vite',
        role: 'Frontend build tool',
        license: 'MIT',
        url: 'https://github.com/vitejs/vite',
      },
      {
        name: 'emoji-picker-element',
        role: 'Emoji picker',
        license: 'Apache-2.0',
        url: 'https://github.com/nolanlawson/emoji-picker-element',
      },
      {
        name: 'Giphy JS SDK',
        role: 'GIF and sticker search',
        license: 'MIT',
        url: 'https://github.com/Giphy/giphy-js',
      },
      {
        name: 'SunCalc',
        role: 'Sunrise and sunset times for the automatic theme',
        license: 'BSD-2-Clause',
        url: 'https://github.com/mourner/suncalc',
      },
      {
        name: 'Rust ecosystem',
        role: 'Core app (rusqlite, tokio, serde, keyring, age, notify-rust, …)',
        license: 'MIT, Apache-2.0, BSD, ISC, Zlib, MPL-2.0 and others (per crate)',
        url: 'https://github.com/smsheese/shuttle',
        notes: 'Full crate list in shuttle-app/src-tauri/Cargo.lock',
      },
      {
        name: 'python-build-standalone',
        role: 'Embedded CPython runtime for connector sidecars',
        license: 'MPL-2.0 (runtime) · Python PSF License',
        url: 'https://github.com/astral-sh/python-build-standalone',
      },
    ],
  },
  {
    title: 'Connector backends',
    entries: [
      {
        name: 'GOWA (go-whatsapp-web-multidevice)',
        role: 'WhatsApp local gateway',
        license: 'MIT',
        url: 'https://github.com/aldinokemal/go-whatsapp-web-multidevice',
        notes: 'Uses whatsmeow (MPL-2.0)',
      },
      {
        name: 'TDLib',
        role: 'Telegram client library',
        license: 'Boost Software License 1.0',
        url: 'https://github.com/tdlib/td',
      },
      {
        name: 'signal-cli',
        role: 'Signal JSON-RPC client (bundled sidecar)',
        license: 'GPL-3.0',
        url: 'https://github.com/AsamK/signal-cli',
        notes: 'Source must remain available when redistributed. See licenses/signal-cli-GPL-3.0.txt in the app bundle.',
      },
      {
        name: 'fbchat',
        role: 'Messenger (unofficial API)',
        license: 'BSD-3-Clause',
        url: 'https://github.com/fbchat-dev/fbchat',
        notes: 'Unofficial Meta API — use at your own risk',
      },
      {
        name: 'instagrapi',
        role: 'Instagram DMs (unofficial API)',
        license: 'MIT',
        url: 'https://github.com/subzeroid/instagrapi',
        notes: 'Unofficial Meta API — use at your own risk',
      },
      {
        name: 'Matrix Client-Server API',
        role: 'Matrix messaging over HTTPS',
        license: 'Apache-2.0 (spec) · server-dependent',
        url: 'https://spec.matrix.org',
      },
      {
        name: 'Python standard library',
        role: 'Email IMAP/SMTP connector',
        license: 'PSF License',
        url: 'https://docs.python.org/3/license.html',
      },
    ],
  },
];

export const TRADEMARK_NOTICE =
  'WhatsApp, Telegram, Signal, Messenger, Instagram, Facebook, Matrix, and other network names/logos are trademarks of their respective owners. Shuttle is an independent project and is not affiliated with or endorsed by those companies.';
