import { GiphyFetch } from '@giphy/js-fetch-api';
import type { IGif } from '@giphy/js-types';

export interface GiphyItem {
  id: string;
  title: string;
  previewUrl: string;
  fullUrl: string;
  width: number;
  height: number;
}

let _gf: GiphyFetch | null = null;

function gf(): GiphyFetch | null {
  const key = import.meta.env.VITE_GIPHY_API_KEY as string | undefined;
  if (!key?.trim()) return null;
  if (!_gf) _gf = new GiphyFetch(key.trim());
  return _gf;
}

export function giphyConfigured(): boolean {
  return gf() !== null;
}

function fromIGif(gif: IGif): GiphyItem {
  const images = gif.images;
  // fixed_width_downsampled is webp, small, fast to load for preview grid
  const preview =
    images.fixed_width_downsampled ??
    images.fixed_width ??
    images.downsized_small ??
    images.original;
  const orig = images.original ?? images.downsized_large ?? images.fixed_width;
  const fullUrl = (orig as { url: string }).url;
  return {
    id: String(gif.id),
    title: gif.title || 'GIF',
    previewUrl: (preview as { webp?: string; url: string }).webp || preview.url,
    fullUrl,
    width: Number(preview.width) || 200,
    height: Number(preview.height) || 200,
  };
}

export const PAGE_SIZE = 15;

export async function fetchTrendingGifs(offset = 0): Promise<GiphyItem[]> {
  const client = gf();
  if (!client) return [];
  const { data } = await client.trending({ offset, limit: PAGE_SIZE, rating: 'pg-13' });
  return data.map(fromIGif);
}

export async function searchGifs(query: string, offset = 0): Promise<GiphyItem[]> {
  const q = query.trim();
  if (!q) return fetchTrendingGifs(offset);
  const client = gf();
  if (!client) return [];
  const { data } = await client.search(q, { offset, limit: PAGE_SIZE, rating: 'pg-13' });
  return data.map(fromIGif);
}

export async function fetchTrendingStickers(offset = 0): Promise<GiphyItem[]> {
  const client = gf();
  if (!client) return [];
  const { data } = await client.trending({ offset, limit: PAGE_SIZE, type: 'stickers', rating: 'pg-13' });
  return data.map(fromIGif);
}

export async function searchStickers(query: string, offset = 0): Promise<GiphyItem[]> {
  const q = query.trim();
  if (!q) return fetchTrendingStickers(offset);
  const client = gf();
  if (!client) return [];
  const { data } = await client.search(q, { offset, limit: PAGE_SIZE, type: 'stickers', rating: 'pg-13' });
  return data.map(fromIGif);
}

export async function giphyUrlToBase64(url: string): Promise<{ data_base64: string; mime: string }> {
  const { invoke } = await import('@tauri-apps/api/core');
  const data_base64 = await invoke<string>('fetch_url_bytes', { url });
  // Derive mime from URL extension
  const mime = url.includes('.webp') ? 'image/webp' : 'image/gif';
  return { data_base64, mime };
}
