//! Where media lives.
//!
//! * **Download cache** (app-private): `<data_dir>/media_cache/<account>/{media,avatars,thumbs}`.
//!   Every automatic or on-demand download, avatar and message preview goes here, like
//!   Telegram's `tdata/temp_data`. Retention only ever deletes inside `media/` of this tree
//!   (and Shuttle's own GOWA download scratch folder).
//! * **Save folder** (user-visible): defaults to `<Documents>/shuttle`. Only files the user
//!   explicitly saves are copied there, and never over an existing file.
//! * **Legacy root**: older builds wrote downloads to `<Documents>/shuttle/...`. Those files
//!   stay where they are and stay readable, but nothing new is written there and retention
//!   never touches them.

use chrono::{DateTime, Local};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// `<Documents>/shuttle`: the default save folder and the legacy download root.
pub fn shuttle_documents_root() -> PathBuf {
    dirs::document_dir()
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("shuttle")
}

/// Root of the app-private download cache.
pub fn media_cache_root(data_dir: &Path) -> PathBuf {
    data_dir.join("media_cache")
}

/// Where "Copy media" stages the files it puts on the clipboard (see `clipboard.rs`). Not an
/// account folder: retention only visits `<account>/media/`, and the clipboard code prunes it.
pub fn clipboard_stage_dir(data_dir: &Path) -> PathBuf {
    media_cache_root(data_dir).join("clipboard")
}

/// Per-account cache folder. Keyed by account id (not its label) so renames never move files.
/// Must match the connector's `files_dir()` (`safe_dir_name`).
pub fn account_cache_root(data_dir: &Path, account_id: &str) -> PathBuf {
    media_cache_root(data_dir).join(safe_dir_name(account_id))
}

pub fn ensure_account_dirs(root: &Path) -> io::Result<()> {
    for sub in ["media", "avatars", "thumbs"] {
        fs::create_dir_all(root.join(sub))?;
    }
    Ok(())
}

/// Folders media files may be *read* from: the cache, then the legacy Documents root.
pub fn media_read_roots(data_dir: &Path) -> Vec<PathBuf> {
    vec![media_cache_root(data_dir), shuttle_documents_root()]
}

/// Canonicalize before checking the scope so a symlink cannot escape the media roots.
pub fn validated_media_path(path: &Path, roots: &[PathBuf]) -> Result<PathBuf, String> {
    let file = path.canonicalize().map_err(|e| e.to_string())?;
    if !file.is_file() {
        return Err("Media file not found".into());
    }
    for root in roots {
        if let Ok(root) = root.canonicalize() {
            if file.starts_with(root) {
                return Ok(file);
            }
        }
    }
    Err("Media file is outside Shuttle's media folders".into())
}

/// True when `file` (canonical) is inside the download cache, not the legacy folder.
pub fn is_in_cache(file: &Path, data_dir: &Path) -> bool {
    match media_cache_root(data_dir).canonicalize() {
        Ok(root) => file.starts_with(root),
        Err(_) => false,
    }
}

/// Folders an older build used for one account's downloads and previews.
///
/// Older builds wrote to `<Documents>/shuttle/<account id>`, and before that to a folder per
/// connector and account label. Both still sit under the legacy root that `media_read_roots`
/// allows reads from.
pub fn legacy_account_dirs(connector_label: &str, account_id: &str, account_name: &str) -> Vec<PathBuf> {
    legacy_account_dirs_in(&shuttle_documents_root(), connector_label, account_id, account_name)
}

/// The same lookup against an explicit legacy root.
pub fn legacy_account_dirs_in(
    root: &Path,
    connector_label: &str,
    account_id: &str,
    account_name: &str,
) -> Vec<PathBuf> {
    let mut out = vec![root.join(safe_dir_name(account_id))];
    let wanted = account_name.trim();
    if wanted.is_empty() {
        return out;
    }
    let wanted_connector = connector_label.trim().to_lowercase();
    if let Ok(entries) = fs::read_dir(root) {
        for entry in entries.flatten() {
            if !entry.path().is_dir() {
                continue;
            }
            if entry.file_name().to_string_lossy().to_lowercase() != wanted_connector {
                continue;
            }
            let by_name = entry.path().join(wanted);
            if by_name.is_dir() {
                out.push(by_name);
            }
        }
    }
    out
}

/// Copy the previews an older build left outside the cache into `thumbs_dir`.
///
/// The originals stay where they are: a copy can only ever add a file the UI may not have
/// had, and the connector's own backfill skips a preview that already exists.
pub fn migrate_legacy_previews(legacy_dirs: &[PathBuf], thumbs_dir: &Path) -> usize {
    let mut copied = 0;
    for dir in legacy_dirs {
        let Ok(entries) = fs::read_dir(dir.join("thumbs")) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("jpg") {
                continue;
            }
            let Some(name) = path.file_name() else {
                continue;
            };
            let target = thumbs_dir.join(name);
            if target.exists() {
                continue;
            }
            if fs::copy(&path, &target).is_ok() {
                copied += 1;
            }
        }
    }
    copied
}

/// Mark a cached file as used now. Retention measures idleness by mtime (atime is unreliable
/// with `noatime`/`relatime`), so opening a file keeps it. Skipped when the mtime is already
/// recent, which avoids a metadata write for every redraw.
pub fn touch_if_stale(path: &Path) {
    const FRESH: Duration = Duration::from_secs(3600);
    let fresh = fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| SystemTime::now().duration_since(t).ok())
        .is_some_and(|age| age < FRESH);
    if fresh {
        return;
    }
    if let Ok(file) = fs::OpenOptions::new().write(true).open(path) {
        let _ = file.set_modified(SystemTime::now());
    }
}

// --------------------------------------------------------------------------- saving

/// Folder for explicit saves: the configured one, else `<Documents>/shuttle`.
pub fn resolve_save_dir(configured: Option<&str>) -> PathBuf {
    match configured.map(str::trim).filter(|s| !s.is_empty()) {
        Some(dir) => PathBuf::from(dir),
        None => shuttle_documents_root(),
    }
}

/// A file name that is valid on Linux, Windows and macOS, never a path.
pub fn sanitize_file_name(name: &str) -> String {
    let last = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let cleaned: String = last
        .chars()
        .map(|c| {
            if c.is_control() || "<>:\"/\\|?*".contains(c) {
                '_'
            } else {
                c
            }
        })
        .collect();
    let trimmed = cleaned.trim().trim_end_matches(['.', ' ']);
    let mut name = trimmed.trim_start().to_string();
    if name.is_empty() || name.chars().all(|c| c == '.') {
        return "media".into();
    }
    // Windows reserves these names even with extensions; use the same rule on every OS.
    let stem = name.split('.').next().unwrap_or(&name).to_ascii_uppercase();
    let reserved = ["CON", "PRN", "AUX", "NUL"].contains(&stem.as_str())
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && matches!(stem.as_bytes()[3], b'1'..=b'9'));
    if reserved {
        name = format!("_{name}");
    }
    if name.len() > 200 {
        let ext = Path::new(&name)
            .extension()
            .and_then(|e| e.to_str())
            .filter(|e| e.len() <= 16)
            .map(|e| format!(".{e}"))
            .unwrap_or_default();
        let keep = 200 - ext.len();
        let mut cut = keep;
        while cut > 0 && !name.is_char_boundary(cut) {
            cut -= 1;
        }
        name = format!("{}{ext}", &name[..cut]);
    }
    name
}

/// `name (1).ext`, `name (2).ext`, ... for the n-th collision.
fn numbered_name(name: &str, n: u32) -> String {
    if n == 0 {
        return name.to_string();
    }
    let path = Path::new(name);
    match (
        path.file_stem().and_then(|s| s.to_str()),
        path.extension().and_then(|s| s.to_str()),
    ) {
        (Some(stem), Some(ext)) => format!("{stem} ({n}).{ext}"),
        _ => format!("{name} ({n})"),
    }
}

/// Copy `src` into `dir` as `name`. Never overwrites: an existing name gets ` (1)`, ` (2)`, ...
/// The destination is created with `create_new`, so two concurrent saves cannot collide either.
/// Returns the path actually written.
pub fn copy_without_overwrite(src: &Path, dir: &Path, name: &str) -> io::Result<PathBuf> {
    fs::create_dir_all(dir)?;
    let name = sanitize_file_name(name);
    for n in 0..10_000u32 {
        let dest = dir.join(numbered_name(&name, n));
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&dest)
        {
            Ok(mut out) => {
                let copied = fs::File::open(src).and_then(|mut input| io::copy(&mut input, &mut out));
                if let Err(e) = copied {
                    drop(out);
                    let _ = fs::remove_file(&dest);
                    return Err(e);
                }
                return Ok(dest);
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "too many files with this name",
    ))
}

/// Show `path` selected in the OS file manager.
///
/// Linux: `org.freedesktop.FileManager1.ShowItems` over D-Bus (what the opener plugin does),
/// else `xdg-open` on the parent folder. Windows: Explorer's select. macOS: Finder reveal.
/// Each platform falls back to its command-line equivalent if the first attempt fails.
pub fn reveal_in_file_manager(path: &Path) -> Result<(), String> {
    if !path.is_file() {
        return Err("The saved file no longer exists".into());
    }
    let primary = tauri_plugin_opener::reveal_item_in_dir(path).map_err(|e| e.to_string());
    if primary.is_ok() {
        return Ok(());
    }
    let fallback = reveal_fallback(path);
    fallback.or(primary)
}

#[cfg(target_os = "windows")]
fn reveal_fallback(path: &Path) -> Result<(), String> {
    std::process::Command::new("explorer")
        .arg(format!("/select,{}", path.display()))
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[cfg(target_os = "macos")]
fn reveal_fallback(path: &Path) -> Result<(), String> {
    std::process::Command::new("open")
        .arg("-R")
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn reveal_fallback(path: &Path) -> Result<(), String> {
    let parent = path.parent().ok_or("File has no parent folder")?;
    std::process::Command::new("xdg-open")
        .arg(parent)
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

// --------------------------------------------------------------------------- cache upkeep

/// Result of a cache purge.
#[derive(Debug, Default, Clone)]
pub struct Purged {
    /// Cached message-media files that were deleted (their DB `media_path` must be cleared).
    pub media_files: Vec<PathBuf>,
    pub files: u64,
    pub bytes: u64,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct CacheStats {
    pub files: u64,
    pub bytes: u64,
}

/// Folders holding downloaded message media: each account's `media/` plus Shuttle's own GOWA
/// download scratch folder (GOWA keeps a copy of every download there). Avatars and message
/// previews are small, are re-used constantly, and are not "downloaded media".
fn media_dirs(data_dir: &Path) -> (Vec<PathBuf>, PathBuf) {
    let mut dirs = Vec::new();
    if let Ok(entries) = fs::read_dir(media_cache_root(data_dir)) {
        for entry in entries.flatten() {
            if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                dirs.push(entry.path().join("media"));
            }
        }
    }
    (dirs, data_dir.join("gowa").join("statics").join("media"))
}

/// Regular files directly under `dir` (symlinks are never followed or returned).
fn plain_files(dir: &Path) -> Vec<(PathBuf, fs::Metadata)> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|e| {
            let meta = fs::symlink_metadata(e.path()).ok()?;
            meta.is_file().then(|| (e.path(), meta))
        })
        .collect()
}

pub fn cache_stats(data_dir: &Path) -> CacheStats {
    let (cache_dirs, gowa_dir) = media_dirs(data_dir);
    let mut stats = CacheStats::default();
    for dir in cache_dirs.iter().chain(std::iter::once(&gowa_dir)) {
        for (_, meta) in plain_files(dir) {
            stats.files += 1;
            stats.bytes += meta.len();
        }
    }
    stats
}

/// Delete cached downloads unused for `keep` (or all of them when `None`).
///
/// Only touches regular files directly inside `<cache>/<account>/media/` and Shuttle's GOWA
/// scratch folder. The save folder, the legacy Documents folder, avatars and previews are never
/// visited. In-flight transfers (`*.part`) and fresh GOWA files are left alone.
pub fn purge_cache(data_dir: &Path, keep: Option<Duration>, now: SystemTime) -> Purged {
    const IN_FLIGHT: Duration = Duration::from_secs(600);
    let (cache_dirs, gowa_dir) = media_dirs(data_dir);
    let mut out = Purged::default();
    let idle_enough = |meta: &fs::Metadata, min_age: Duration| -> bool {
        let age = meta
            .modified()
            .ok()
            .and_then(|t| now.duration_since(t).ok())
            .unwrap_or(Duration::ZERO);
        age >= min_age && keep.is_none_or(|k| age >= k)
    };
    for dir in &cache_dirs {
        for (path, meta) in plain_files(dir) {
            let is_part = path.extension().is_some_and(|e| e == "part");
            let min_age = if is_part { IN_FLIGHT } else { Duration::ZERO };
            if !idle_enough(&meta, min_age) {
                continue;
            }
            if fs::remove_file(&path).is_ok() {
                out.files += 1;
                out.bytes += meta.len();
                if !is_part {
                    out.media_files.push(path);
                }
            }
        }
    }
    for (path, meta) in plain_files(&gowa_dir) {
        if idle_enough(&meta, IN_FLIGHT) && fs::remove_file(&path).is_ok() {
            out.files += 1;
            out.bytes += meta.len();
        }
    }
    out
}

// --------------------------------------------------------------------------- file types

/// File extension (no dot) for well-known signatures in a file's first bytes.
pub fn detect_extension(head: &[u8]) -> Option<&'static str> {
    let starts = |sig: &[u8]| head.starts_with(sig);
    let at = |off: usize, sig: &[u8]| head.get(off..off + sig.len()) == Some(sig);
    Some(if starts(b"\xff\xd8\xff") {
        "jpg"
    } else if starts(b"\x89PNG\r\n\x1a\n") {
        "png"
    } else if starts(b"GIF87a") || starts(b"GIF89a") {
        "gif"
    } else if starts(b"RIFF") && at(8, b"WEBP") {
        "webp"
    } else if starts(b"RIFF") && at(8, b"WAVE") {
        "wav"
    } else if starts(b"BM") && head.len() > 14 {
        "bmp"
    } else if starts(b"%PDF") {
        "pdf"
    } else if starts(b"OggS") {
        "ogg"
    } else if starts(b"ID3") {
        "mp3"
    } else if starts(b"\x1a\x45\xdf\xa3") {
        "webm"
    } else if starts(b"#!AMR\n") {
        "amr"
    } else if at(4, b"ftyp") {
        match head.get(8..12) {
            Some(b"qt  ") => "mov",
            Some(b"heic" | b"heix" | b"mif1" | b"msf1") => "heic",
            Some(b"M4A " | b"M4B ") => "m4a",
            Some(b) if b.starts_with(b"3gp") => "3gp",
            _ => "mp4",
        }
    } else {
        return None;
    })
}

/// `detect_extension` on the first bytes of `path`.
pub fn sniff_extension(path: &Path) -> Option<&'static str> {
    use std::io::Read;
    let mut head = [0u8; 16];
    let mut file = fs::File::open(path).ok()?;
    let mut n = 0;
    while n < head.len() {
        match file.read(&mut head[n..]) {
            Ok(0) | Err(_) => break,
            Ok(k) => n += k,
        }
    }
    detect_extension(&head[..n])
}

/// A plausible extension: 1-8 ASCII letters/digits, lower-cased.
fn clean_extension(ext: &str) -> Option<String> {
    (!ext.is_empty() && ext.len() <= 8 && ext.bytes().all(|b| b.is_ascii_alphanumeric()))
        .then(|| ext.to_ascii_lowercase())
}

fn name_extension(name: &str) -> Option<String> {
    Path::new(name).extension().and_then(|e| e.to_str()).and_then(clean_extension)
}

/// The extension a cached file should be shown with: its own when it has a real one, else
/// what its bytes say. Files cached by older builds carry none (named after the message id),
/// and `.bin` is only the connector's last-resort guess for documents.
pub fn effective_extension(path: &Path) -> Option<String> {
    let own = path.file_name().and_then(|n| n.to_str()).and_then(name_extension);
    match own.as_deref() {
        Some("bin") | None => sniff_extension(path).map(str::to_owned).or(own),
        Some(_) => own,
    }
}

/// What the UI knows about a media message, used to name a copied or saved file.
#[derive(Debug, Default, Clone)]
pub struct MediaNameHint {
    /// `image`, `video`, `audio`, `sticker`, `document`, ... (normalised by the UI).
    pub kind: Option<String>,
    /// The file name WhatsApp reported (documents only; photos and videos get a readable
    /// name because their reported name is just the message id).
    pub filename: Option<String>,
    /// Message time, RFC 3339.
    pub timestamp: Option<String>,
    /// The account's name in Shuttle.
    pub account_name: Option<String>,
    /// The chat's title.
    pub chat_name: Option<String>,
}

/// Longest one half of a generated name (`<account>` or `<chat>`) may be.
const NAME_PART_MAX: usize = 40;

/// One half of a generated name: valid on Linux, Windows and macOS, whitespace collapsed,
/// capped, and never empty (an unusable name takes `fallback`). A path separator inside a
/// name is just a character, so it becomes a space instead of cutting the name short.
fn name_part(raw: Option<&str>, fallback: &str) -> String {
    let flattened = raw
        .unwrap_or("")
        .chars()
        .map(|c| if c == '/' || c == '\\' { ' ' } else { c })
        .collect::<String>();
    let collapsed = flattened.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.is_empty() {
        return fallback.to_string();
    }
    let safe = sanitize_file_name(&collapsed);
    let mut cut = safe.len().min(NAME_PART_MAX);
    while cut > 0 && !safe.is_char_boundary(cut) {
        cut -= 1;
    }
    let safe = safe[..cut].trim_matches(['.', ' ']);
    if safe.is_empty() {
        fallback.to_string()
    } else {
        safe.to_string()
    }
}

/// The message time in local time, or `now` when it is missing or unparsable.
fn message_time(timestamp: Option<&str>, now: DateTime<Local>) -> DateTime<Local> {
    timestamp
        .and_then(|t| DateTime::parse_from_rfc3339(t.trim()).ok())
        .map(|t| t.with_timezone(&Local))
        .unwrap_or(now)
}

/// `<account>-<chat>-<yymmdd>-<hhmm>.<ext>` in local time: the one place a saved or copied
/// media file's name is built. The date and time are the message's, never the save moment.
/// `now` is only used when `timestamp` is missing or unparsable.
pub fn media_file_name(
    account: Option<&str>,
    chat: Option<&str>,
    timestamp: Option<&str>,
    ext: Option<&str>,
    now: DateTime<Local>,
) -> String {
    let when = message_time(timestamp, now);
    let base = format!(
        "{}-{}-{}",
        name_part(account, "Shuttle"),
        name_part(chat, "Chat"),
        when.format("%y%m%d-%H%M")
    );
    let ext = ext
        .map(|e| e.trim_start_matches('.'))
        .filter(|e| !e.is_empty() && e.chars().all(|c| c.is_ascii_alphanumeric()));
    match ext {
        Some(ext) => format!("{base}.{ext}"),
        None => base,
    }
}

/// A file name with the right extension for a cached media file.
///
/// Documents keep their own name (an extension is added only when it has none). Everything
/// else is named like `Work-Mum-261112-1325.jpg` from the account, the chat and the message
/// time.
pub fn display_name(hint: &MediaNameHint, source: &Path) -> String {
    let kind = hint.kind.as_deref().map(str::to_ascii_lowercase);
    let reported = hint.filename.as_deref().map(str::trim).filter(|n| !n.is_empty());
    let is_document = matches!(kind.as_deref(), Some("document")) || (kind.is_none() && reported.is_some());
    let source_ext = effective_extension(source);
    if let (true, Some(name)) = (is_document, reported) {
        let name = sanitize_file_name(name);
        return match (name_extension(&name), source_ext) {
            (None, Some(ext)) => format!("{name}.{ext}"),
            _ => name,
        };
    }
    media_file_name(
        hint.account_name.as_deref(),
        hint.chat_name.as_deref(),
        hint.timestamp.as_deref(),
        source_ext.as_deref(),
        Local::now(),
    )
}

pub fn mime_to_ext(mime: &str) -> &'static str {
    match mime {
        "image/png" => ".png",
        "image/jpeg" | "image/jpg" => ".jpg",
        "image/webp" => ".webp",
        "image/gif" => ".gif",
        "video/mp4" => ".mp4",
        "audio/ogg" => ".ogg",
        "audio/mpeg" => ".mp3",
        "application/pdf" => ".pdf",
        _ => ".bin",
    }
}

fn safe_dir_name(value: &str) -> String {
    value
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

pub fn phone_digits(value: &str) -> String {
    value.chars().filter(|c| c.is_ascii_digit()).collect()
}

pub fn jids_same(a: &str, b: &str) -> bool {
    let da = phone_digits(a);
    let db = phone_digits(b);
    !da.is_empty() && da == db
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_aged(path: &Path, bytes: &[u8], age: Duration) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
        let file = fs::OpenOptions::new().write(true).open(path).unwrap();
        file.set_modified(SystemTime::now() - age).unwrap();
    }

    #[test]
    fn media_file_access_stays_inside_the_media_roots() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = tmp.path().join("cache");
        let legacy = tmp.path().join("legacy");
        fs::create_dir_all(&cache).unwrap();
        fs::create_dir_all(&legacy).unwrap();
        let roots = vec![cache.clone(), legacy.clone()];
        let new_media = cache.join("clip.mp4");
        fs::write(&new_media, b"video").unwrap();
        let old_media = legacy.join("photo.jpg");
        fs::write(&old_media, b"photo").unwrap();
        let outside = tmp.path().join("private.txt");
        fs::write(&outside, b"private").unwrap();
        // Both the new cache and the legacy folder stay readable.
        assert!(validated_media_path(&new_media, &roots).is_ok());
        assert!(validated_media_path(&old_media, &roots).is_ok());
        assert!(validated_media_path(&outside, &roots).is_err());
        assert!(validated_media_path(&cache, &roots).is_err());
        #[cfg(unix)]
        {
            let link = cache.join("escape");
            std::os::unix::fs::symlink(&outside, &link).unwrap();
            assert!(validated_media_path(&link, &roots).is_err());
        }
    }

    #[test]
    fn magic_bytes_identify_common_media() {
        assert_eq!(detect_extension(b"\xff\xd8\xff\xe0\0\x10JFIF"), Some("jpg"));
        assert_eq!(detect_extension(b"\x89PNG\r\n\x1a\n\0\0"), Some("png"));
        assert_eq!(detect_extension(b"GIF89a\x01\0\x01\0"), Some("gif"));
        assert_eq!(detect_extension(b"RIFF\x24\0\0\0WEBPVP8 "), Some("webp"));
        assert_eq!(detect_extension(b"RIFF\x24\0\0\0WAVEfmt "), Some("wav"));
        assert_eq!(detect_extension(b"\0\0\0\x18ftypmp42\0\0\0\0"), Some("mp4"));
        assert_eq!(detect_extension(b"\0\0\0\x14ftypqt  \0\0\0\0"), Some("mov"));
        assert_eq!(detect_extension(b"\0\0\0\x18ftypheic\0\0\0\0"), Some("heic"));
        assert_eq!(detect_extension(b"\0\0\0\x18ftyp3gp4\0\0\0\0"), Some("3gp"));
        assert_eq!(detect_extension(b"OggS\0\x02"), Some("ogg"));
        assert_eq!(detect_extension(b"%PDF-1.7\n"), Some("pdf"));
        assert_eq!(detect_extension(b"just some text"), None);
        assert_eq!(detect_extension(b""), None);
        assert_eq!(detect_extension(b"\xff"), None);
    }

    #[test]
    fn extensionless_cache_files_get_their_type_from_the_bytes() {
        let tmp = tempfile::tempdir().unwrap();
        let bare = tmp.path().join("AC798AA7D07D4EE0B6BAA3F981297348");
        fs::write(&bare, b"\xff\xd8\xff\xe0\0\x10JFIF\0\x01").unwrap();
        assert_eq!(effective_extension(&bare).as_deref(), Some("jpg"));
        // A real extension is trusted; the connector's `.bin` guess is not.
        let named = tmp.path().join("A.PNG");
        fs::write(&named, b"whatever").unwrap();
        assert_eq!(effective_extension(&named).as_deref(), Some("png"));
        let guess = tmp.path().join("B.bin");
        fs::write(&guess, b"%PDF-1.4 ....").unwrap();
        assert_eq!(effective_extension(&guess).as_deref(), Some("pdf"));
        let unknown = tmp.path().join("C");
        fs::write(&unknown, b"plain").unwrap();
        assert_eq!(effective_extension(&unknown), None);
    }

    #[test]
    fn display_names_are_readable_and_carry_an_extension() {
        let tmp = tempfile::tempdir().unwrap();
        let bare = tmp.path().join("AC798AA7D07D4EE0B6BAA3F981297348");
        fs::write(&bare, b"\xff\xd8\xff\xe0\0\x10JFIF\0\x01").unwrap();
        let hint = |kind: &str, filename: Option<&str>| MediaNameHint {
            kind: Some(kind.into()),
            filename: filename.map(Into::into),
            timestamp: Some("2026-11-12T13:25:04+00:00".into()),
            ..Default::default()
        };
        let name = display_name(&hint("image", Some("AC798AA7D07D4EE0B6BAA3F981297348")), &bare);
        let local = local_stamp("2026-11-12T13:25:04+00:00");
        assert_eq!(name, format!("Shuttle-Chat-{local}.jpg"));
        assert!(!name.contains(':'));
        assert!(display_name(&hint("video", None), &tmp.path().join("v.mp4"))
            .starts_with("Shuttle-Chat-"));
        assert!(display_name(&hint("video", None), &tmp.path().join("v.mp4")).ends_with(".mp4"));
        // Documents keep their name; a missing extension is added from the file.
        let pdf = tmp.path().join("DOC1");
        fs::write(&pdf, b"%PDF-1.7").unwrap();
        assert_eq!(display_name(&hint("document", Some("Invoice 7.pdf")), &pdf), "Invoice 7.pdf");
        assert_eq!(display_name(&hint("document", Some("Invoice 7")), &pdf), "Invoice 7.pdf");
        assert_eq!(display_name(&hint("document", Some("a/b:c.pdf")), &pdf), "b_c.pdf");
        assert!(display_name(&hint("document", None), &pdf).starts_with("Shuttle-Chat-"));
        // No timestamp falls back to now, still well formed.
        let now = display_name(&MediaNameHint { kind: Some("image".into()), ..Default::default() }, &bare);
        let year = Local::now().format("%y").to_string();
        assert!(now.starts_with(&format!("Shuttle-Chat-{year}")) && now.ends_with(".jpg"));
    }

    fn local_stamp(rfc3339: &str) -> String {
        DateTime::parse_from_rfc3339(rfc3339)
            .unwrap()
            .with_timezone(&Local)
            .format("%y%m%d-%H%M")
            .to_string()
    }

    fn at(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> DateTime<Local> {
        use chrono::TimeZone;
        Local.with_ymd_and_hms(year, month, day, hour, minute, 0).unwrap()
    }

    #[test]
    fn media_file_names_carry_the_account_the_chat_and_the_message_time() {
        let now = at(2026, 6, 7, 8, 9);
        let name = media_file_name(
            Some("Work"),
            Some("Mum"),
            Some("2026-11-12T13:25:04+00:00"),
            Some("jpg"),
            now,
        );
        assert_eq!(name, format!("Work-Mum-{}.jpg", local_stamp("2026-11-12T13:25:04+00:00")));
        assert!(!name.contains(':'));
    }

    #[test]
    fn media_file_name_parts_are_safe_on_every_os() {
        let now = at(2026, 6, 7, 8, 9);
        let name = media_file_name(
            Some("  Work / Personal  "),
            Some("a:b*c?d\"e<f>g|h\u{7}"),
            None,
            Some("jpeg"),
            now,
        );
        for bad in ['/', '\\', ':', '*', '?', '"', '<', '>', '|', '\u{7}'] {
            assert!(!name.contains(bad), "{bad:?} survived in {name:?}");
        }
        assert!(name.starts_with("Work Personal-a_b_c_d_e_f_g_h_-"), "{name}");
        assert!(name.ends_with(".jpeg"));
        // A separator inside either name never cuts the other half off.
        assert!(media_file_name(Some("a/b"), Some("c\\d"), None, None, now).starts_with("a b-c d-"));
        // Trailing dots and spaces never reach the file name either.
        assert!(media_file_name(Some("Work. ."), Some("Mum ."), None, None, now).starts_with("Work-Mum-"));
        // Neither part can push the name past a sane length.
        let long = media_file_name(Some(&"a".repeat(200)), Some(&"b".repeat(200)), None, Some("jpg"), now);
        assert!(long.len() < 120, "{long}");
    }

    #[test]
    fn media_file_name_parts_fall_back_when_empty() {
        let now = at(2026, 6, 7, 8, 9);
        assert_eq!(media_file_name(None, None, None, Some("jpg"), now), "Shuttle-Chat-260607-0809.jpg");
        assert_eq!(media_file_name(Some(""), Some("   "), None, None, now), "Shuttle-Chat-260607-0809");
    }

    #[test]
    fn missing_or_unreadable_timestamps_fall_back_to_now() {
        let now = at(2026, 6, 7, 8, 9);
        assert_eq!(media_file_name(None, None, None, None, now), "Shuttle-Chat-260607-0809");
        assert_eq!(media_file_name(None, None, Some("yesterday"), None, now), "Shuttle-Chat-260607-0809");
    }

    #[test]
    fn media_file_name_pads_and_rolls_over_at_midnight() {
        let late = media_file_name(None, None, None, Some("jpg"), at(2026, 6, 30, 23, 59));
        assert_eq!(late, "Shuttle-Chat-260630-2359.jpg");
        let midnight = media_file_name(None, None, None, None, at(2026, 7, 1, 0, 0));
        assert_eq!(midnight, "Shuttle-Chat-260701-0000");
        let padded = media_file_name(None, None, None, None, at(2026, 1, 2, 3, 4));
        assert_eq!(padded, "Shuttle-Chat-260102-0304");
    }

    #[test]
    fn media_file_extension_comes_from_the_cached_file() {
        let tmp = tempfile::tempdir().unwrap();
        let now = at(2026, 6, 7, 8, 9);
        let hint = MediaNameHint {
            kind: Some("video".into()),
            filename: None,
            timestamp: Some("2026-11-12T13:25:04+00:00".into()),
            account_name: Some("Work".into()),
            chat_name: Some("Team".into()),
        };
        let local = local_stamp("2026-11-12T13:25:04+00:00");
        let mp4 = tmp.path().join("clip.mp4");
        fs::write(&mp4, b"x").unwrap();
        assert_eq!(display_name(&hint, &mp4), format!("Work-Team-{local}.mp4"));
        assert_eq!(media_file_name(Some("Work"), Some("Team"), None, None, now), "Work-Team-260607-0809");
        assert_eq!(media_file_name(Some("Work"), Some("Team"), None, Some(".mp4"), now), "Work-Team-260607-0809.mp4");
        assert_eq!(media_file_name(Some("Work"), Some("Team"), None, Some(""), now), "Work-Team-260607-0809");
        // An extension is not user text: anything but letters and digits is dropped.
        assert_eq!(media_file_name(None, None, None, Some("j p"), now), "Shuttle-Chat-260607-0809");
    }

    #[test]
    fn cache_layout_is_per_account_under_the_data_dir() {
        let data = Path::new("/data/shuttle");
        assert_eq!(media_cache_root(data), data.join("media_cache"));
        assert_eq!(
            account_cache_root(data, "acc/../1"),
            data.join("media_cache").join("acc____1")
        );
        let tmp = tempfile::tempdir().unwrap();
        let root = account_cache_root(tmp.path(), "a1");
        ensure_account_dirs(&root).unwrap();
        for sub in ["media", "avatars", "thumbs"] {
            assert!(root.join(sub).is_dir());
        }
        assert!(is_in_cache(
            &root.join("media").canonicalize().unwrap(),
            tmp.path()
        ));
    }

    #[test]
    fn save_never_overwrites_and_numbers_name_clashes() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src.bin");
        fs::write(&src, b"new").unwrap();
        let dir = tmp.path().join("saved");
        let first = copy_without_overwrite(&src, &dir, "photo.jpg").unwrap();
        let second = copy_without_overwrite(&src, &dir, "photo.jpg").unwrap();
        let third = copy_without_overwrite(&src, &dir, "photo.jpg").unwrap();
        assert_eq!(first.file_name().unwrap(), "photo.jpg");
        assert_eq!(second.file_name().unwrap(), "photo (1).jpg");
        assert_eq!(third.file_name().unwrap(), "photo (2).jpg");
        // An existing user file is untouched.
        fs::write(dir.join("report.pdf"), b"mine").unwrap();
        let saved = copy_without_overwrite(&src, &dir, "report.pdf").unwrap();
        assert_eq!(fs::read(dir.join("report.pdf")).unwrap(), b"mine");
        assert_eq!(fs::read(saved).unwrap(), b"new");
        // Names without an extension are numbered too.
        copy_without_overwrite(&src, &dir, "README").unwrap();
        let again = copy_without_overwrite(&src, &dir, "README").unwrap();
        assert_eq!(again.file_name().unwrap(), "README (1)");
    }

    #[test]
    fn suggested_names_cannot_escape_the_folder_or_break_windows() {
        assert_eq!(sanitize_file_name("../../etc/passwd"), "passwd");
        assert_eq!(sanitize_file_name("..\\..\\boot.ini"), "boot.ini");
        assert_eq!(sanitize_file_name("a:b*c?.png"), "a_b_c_.png");
        assert_eq!(sanitize_file_name("  .. "), "media");
        assert_eq!(sanitize_file_name(""), "media");
        assert_eq!(sanitize_file_name("CON.txt"), "_CON.txt");
        assert_eq!(sanitize_file_name("report. "), "report");
        let long = format!("{}.pdf", "x".repeat(400));
        let cleaned = sanitize_file_name(&long);
        assert!(cleaned.len() <= 200 && cleaned.ends_with(".pdf"));
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("s");
        fs::write(&src, b"x").unwrap();
        let dir = tmp.path().join("d");
        let saved = copy_without_overwrite(&src, &dir, "../escape.txt").unwrap();
        assert_eq!(saved.parent().unwrap(), dir);
    }

    #[test]
    fn previews_from_the_legacy_folder_are_copied_in_once() {
        let tmp = tempfile::tempdir().unwrap();
        let legacy = tmp.path().join("Documents/shuttle/Whatsapp/WA 1");
        let thumbs = tmp.path().join("cache/a1/thumbs");
        fs::create_dir_all(legacy.join("thumbs")).unwrap();
        fs::create_dir_all(&thumbs).unwrap();
        fs::write(legacy.join("thumbs/OLD1.jpg"), b"old").unwrap();
        fs::write(legacy.join("thumbs/OLD2.jpg"), b"old").unwrap();
        fs::write(legacy.join("thumbs/notes.txt"), b"not a preview").unwrap();
        // A preview the cache already has wins over the legacy copy.
        fs::write(thumbs.join("OLD2.jpg"), b"new").unwrap();

        let copied = migrate_legacy_previews(&[legacy.clone()], &thumbs);
        assert_eq!(copied, 1, "only the missing jpeg is copied in");
        assert_eq!(fs::read(thumbs.join("OLD1.jpg")).unwrap(), b"old");
        assert_eq!(fs::read(thumbs.join("OLD2.jpg")).unwrap(), b"new");
        assert!(!thumbs.join("notes.txt").exists());
        // The legacy file stays where it was.
        assert!(legacy.join("thumbs/OLD1.jpg").exists());
        assert_eq!(migrate_legacy_previews(&[legacy], &thumbs), 0);
    }

    #[test]
    fn legacy_account_dirs_follow_the_old_folder_layouts() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("Documents/shuttle");
        let by_connector = root.join("Whatsapp/WA 1");
        fs::create_dir_all(by_connector.join("thumbs")).unwrap();
        fs::create_dir_all(root.join("acc-a")).unwrap();

        let dirs = legacy_account_dirs_in(&root, "WhatsApp", "acc-a", "WA 1");
        assert!(dirs.contains(&root.join("acc-a")));
        assert!(dirs.contains(&by_connector), "the connector folder is matched by name");
        // A label nothing was written for adds no folder of its own.
        let none = legacy_account_dirs_in(&root, "WhatsApp", "acc-b", "WA 2");
        assert_eq!(none, vec![root.join("acc-b")]);
        // An empty label keeps only the account-id folder.
        assert_eq!(legacy_account_dirs_in(&root, "WhatsApp", "acc-a", "  "), vec![root.join("acc-a")]);
    }

    #[test]
    fn save_dir_defaults_to_documents_shuttle() {
        assert_eq!(resolve_save_dir(None), shuttle_documents_root());
        assert_eq!(resolve_save_dir(Some("  ")), shuttle_documents_root());
        assert_eq!(resolve_save_dir(Some("/tmp/out")), PathBuf::from("/tmp/out"));
        assert!(shuttle_documents_root().ends_with("shuttle"));
    }

    #[test]
    fn retention_deletes_only_idle_cache_media() {
        let tmp = tempfile::tempdir().unwrap();
        let data = tmp.path().join("data");
        let day = Duration::from_secs(86_400);
        let old = data.join("media_cache/a1/media/old.mp4");
        let fresh = data.join("media_cache/a1/media/fresh.jpg");
        let other_old = data.join("media_cache/a2/media/old.pdf");
        let avatar = data.join("media_cache/a1/avatars/me.jpg");
        let thumb = data.join("media_cache/a1/thumbs/t.jpg");
        let gowa_old = data.join("gowa/statics/media/leftover.jpg");
        write_aged(&old, b"1234", day * 90);
        write_aged(&fresh, b"12", day * 2);
        write_aged(&other_old, b"123456", day * 61);
        write_aged(&avatar, b"a", day * 400);
        write_aged(&thumb, b"t", day * 400);
        write_aged(&gowa_old, b"g", day * 90);
        // Everything outside the cache: the save folder, the legacy folder, other data.
        let saved = tmp.path().join("Documents/shuttle/saved.mp4");
        let legacy = tmp.path().join("Documents/shuttle/Whatsapp/Me/media/legacy.jpg");
        let db = data.join("app.sqlite");
        for p in [&saved, &legacy, &db] {
            write_aged(p, b"keep", day * 500);
        }

        let purged = purge_cache(&data, Some(day * 60), SystemTime::now());
        assert_eq!(purged.files, 3);
        assert_eq!(purged.bytes, 4 + 6 + 1);
        let mut deleted: Vec<_> = purged.media_files.clone();
        deleted.sort();
        let mut expect = vec![old.clone(), other_old.clone()];
        expect.sort();
        assert_eq!(deleted, expect);
        assert!(!old.exists() && !other_old.exists() && !gowa_old.exists());
        assert!(fresh.exists() && avatar.exists() && thumb.exists());
        for p in [&saved, &legacy, &db] {
            assert!(p.exists(), "{} must never be deleted", p.display());
        }
        let stats = cache_stats(&data);
        assert_eq!(stats, CacheStats { files: 1, bytes: 2 });

        // "Forever" keeps everything; clear-all removes the rest of the cache media only.
        assert_eq!(purge_cache(&data, None, SystemTime::now()).files, 1);
        assert!(!fresh.exists() && avatar.exists() && saved.exists() && legacy.exists());
    }

    #[test]
    fn retention_spares_in_flight_transfers_and_symlinks() {
        let tmp = tempfile::tempdir().unwrap();
        let data = tmp.path().join("data");
        let part = data.join("media_cache/a1/media/x.part");
        write_aged(&part, b"p", Duration::from_secs(5));
        let protected = tmp.path().join("outside.txt");
        write_aged(&protected, b"mine", Duration::from_secs(86_400 * 900));
        #[cfg(unix)]
        std::os::unix::fs::symlink(&protected, data.join("media_cache/a1/media/link.jpg")).unwrap();
        let purged = purge_cache(&data, None, SystemTime::now());
        assert_eq!(purged.files, 0);
        assert!(part.exists() && protected.exists());
    }

    #[test]
    fn touch_marks_idle_files_as_used() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("m.jpg");
        write_aged(&path, b"x", Duration::from_secs(86_400 * 30));
        touch_if_stale(&path);
        let age = SystemTime::now()
            .duration_since(fs::metadata(&path).unwrap().modified().unwrap())
            .unwrap();
        assert!(age < Duration::from_secs(60));
    }
}
