/** The four things the composer can send as a file. */
export type ComposerFileKind = 'image' | 'video' | 'audio' | 'document';

/**
 * What the attach menu entry promises. `document` makes no promise about the type, so the file
 * decides what it is.
 */
export type FileKindHint = ComposerFileKind;

export interface ClassifiedFile {
  kind: ComposerFileKind;
  /** Never empty: an unknown type is `application/octet-stream`. */
  mime: string;
}

/** One file waiting in the composer, already read so Send does no work. */
export interface PendingAttachment {
  id: string;
  kind: ComposerFileKind;
  filename: string;
  mime: string;
  size: number;
  data_base64: string;
  /** A `data:` URL for an image, so the bar shows the picture without an extra read. */
  preview: string | null;
}

/** Extensions the platforms leave with an empty mime type, so the file still classifies. */
const EXT_MIME: Record<string, string> = {
  apk: 'application/vnd.android.package-archive',
  avif: 'image/avif',
  avi: 'video/x-msvideo',
  bmp: 'image/bmp',
  bz2: 'application/x-bzip2',
  csv: 'text/csv',
  doc: 'application/msword',
  docx: 'application/vnd.openxmlformats-officedocument.wordprocessingml.document',
  flac: 'audio/flac',
  gif: 'image/gif',
  gz: 'application/gzip',
  heic: 'image/heic',
  ics: 'text/calendar',
  jpeg: 'image/jpeg',
  jpg: 'image/jpeg',
  json: 'application/json',
  m4a: 'audio/mp4',
  mkv: 'video/x-matroska',
  mov: 'video/quicktime',
  mp3: 'audio/mpeg',
  mp4: 'video/mp4',
  oga: 'audio/ogg',
  ogg: 'audio/ogg',
  opus: 'audio/opus',
  pdf: 'application/pdf',
  png: 'image/png',
  ppt: 'application/vnd.ms-powerpoint',
  pptx: 'application/vnd.openxmlformats-officedocument.presentationml.presentation',
  rar: 'application/vnd.rar',
  tar: 'application/x-tar',
  tif: 'image/tiff',
  tiff: 'image/tiff',
  ts: 'video/mp2t',
  txt: 'text/plain',
  wav: 'audio/wav',
  webm: 'video/webm',
  webp: 'image/webp',
  xls: 'application/vnd.ms-excel',
  xlsx: 'application/vnd.openxmlformats-officedocument.spreadsheetml.sheet',
  zip: 'application/zip',
  '7z': 'application/x-7z-compressed',
};

export function fileExtension(filename: string): string {
  const dot = filename.lastIndexOf('.');
  return dot > 0 ? filename.slice(dot + 1).toLowerCase() : '';
}

export function guessMime(filename: string): string {
  return EXT_MIME[fileExtension(filename)] ?? 'application/octet-stream';
}

export function kindFromMime(mime: string): ComposerFileKind {
  const type = mime.toLowerCase();
  if (type.startsWith('image/')) return type === 'image/svg+xml' ? 'document' : 'image';
  if (type.startsWith('video/')) return 'video';
  if (type.startsWith('audio/')) return 'audio';
  return 'document';
}

/**
 * The kind and mime to send a file as. A Photo/Video/Audio entry means what it says, so an
 * extensionless or unusual file still goes out the way the person picked it. Document makes no
 * promise, so a PNG picked there arrives as a photo.
 */
export function classifyFile(filename: string, mime: string, hint?: FileKindHint): ClassifiedFile {
  const given = mime.trim().toLowerCase();
  const resolved = !given || given === 'application/octet-stream' ? guessMime(filename) : given;
  const kind = hint && hint !== 'document' ? hint : kindFromMime(resolved);
  return { kind, mime: resolved };
}

export function removeAttachment(
  items: PendingAttachment[],
  id: string
): PendingAttachment[] {
  return items.filter((item) => item.id !== id);
}
