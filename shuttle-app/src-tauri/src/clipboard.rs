//! Copy a downloaded media file to the system clipboard so it can be pasted into other apps.
//!
//! * **Images and stickers** go on the clipboard as image data (PNG) *and* as a file reference,
//!   so chat apps and image editors take the pixels while file managers take the file.
//! * **Everything else** (video, audio, documents) goes on as a file reference only, plus the
//!   path as plain text where the platform allows it.
//! * If the native copy fails the path is copied as text instead and the caller says so.
//! * The file reference never points at the cache file itself (named after the message id,
//!   often without an extension). It points at a **staged** hard link (or copy) with a
//!   readable name and the right extension under `<data dir>/media_cache/clipboard/<n>/`, so
//!   that is the name a paste produces. Staged files are pruned on every copy and at startup.
//!
//! Decoding happens only inside a copy call and nothing decoded is kept: a PNG file is passed
//! through untouched, other formats are transcoded once into the PNG bytes the clipboard owner
//! has to serve. See `docs/platforms.md` for the per-OS behaviour and the crate choice.

use std::fs;
use std::io::{self, BufReader, Cursor, Read};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use clipboard_rs::{Clipboard, ClipboardContent, ClipboardContext};
use parking_lot::Mutex;

use crate::media_store::MediaNameHint;

/// Images above this many pixels are copied as a file only (decoding one would spike memory).
const MAX_IMAGE_PIXELS: u64 = 64_000_000;

const PNG_MAGIC: &[u8] = b"\x89PNG\r\n\x1a\n";

/// Staged copies kept after a copy (the new one included). A paste can happen long after the
/// copy, but only the newest copy is ever on the clipboard.
const KEEP_STAGED: usize = 5;
/// Staged copies kept at startup: a Windows/macOS clipboard outlives the app.
pub const KEEP_STAGED_AT_STARTUP: usize = 1;

/// What ended up on the clipboard; the UI words its toast from this.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CopyKind {
    /// Image data plus a file reference.
    Image,
    /// A file reference (what file managers and chat apps attach).
    File,
    /// Native copy was not possible: only the file path was copied as text.
    PathText,
}

/// Validate `path` against the media roots, stage it under a readable name in `stage_dir`,
/// then copy it. Blocking: run off the UI thread.
pub fn copy_media_file(
    path: &Path,
    roots: &[PathBuf],
    stage_dir: &Path,
    hint: &MediaNameHint,
) -> Result<CopyKind, String> {
    let file = crate::media_store::validated_media_path(path, roots)?;
    let name = crate::media_store::display_name(hint, &file);
    let is_image = is_image_ext(crate::media_store::effective_extension(&file).as_deref());
    // A staging failure (read-only disk, odd filesystem) still copies the real file.
    let clip_file = stage_for_clipboard(&file, stage_dir, &name).unwrap_or_else(|e| {
        tracing::warn!("clipboard: could not stage {name:?} ({e}); copying the cache file itself");
        file.clone()
    });
    let path_str = clean_path(&clip_file);

    let image = if is_image {
        match png_bytes(&file, MAX_IMAGE_PIXELS) {
            Ok(png) => Some(png),
            Err(e) => {
                tracing::debug!("clipboard: copying {} as a file only: {e}", file.display());
                None
            }
        }
    } else {
        None
    };
    let kind = if image.is_some() { CopyKind::Image } else { CopyKind::File };

    let native = with_clipboard(|ctx| ctx.set(contents(&path_str, image)));
    match native {
        Ok(()) => Ok(kind),
        Err(e) => {
            tracing::warn!("clipboard: native copy failed ({e}); falling back to the path as text");
            with_clipboard(|ctx| ctx.set_text(path_str.clone()))
                .map(|()| CopyKind::PathText)
                .map_err(|e2| format!("Could not access the clipboard: {e2}"))
        }
    }
}

/// One long-lived context. On X11 the clipboard owner has to stay alive to serve pastes (and
/// it is a thread plus two X connections), so it is created on first use and reused rather
/// than rebuilt per copy. A failed construction is not cached.
fn with_clipboard<T>(
    f: impl FnOnce(&ClipboardContext) -> clipboard_rs::Result<T>,
) -> Result<T, String> {
    static CTX: Mutex<Option<ClipboardContext>> = Mutex::new(None);
    let mut guard = CTX.lock();
    if guard.is_none() {
        *guard = Some(ClipboardContext::new().map_err(|e| e.to_string())?);
    }
    f(guard.as_ref().expect("initialised above")).map_err(|e| e.to_string())
}

// ------------------------------------------------------------------ per-OS payloads

/// Linux (X11 and Wayland): every target is written explicitly so both backends offer the
/// same set. `text/uri-list` is what GTK/Qt/Electron and most chat apps read,
/// `x-special/gnome-copied-files` is what Nautilus/Nemo/Caja/Thunar-style managers read, and
/// the KDE "not a cut" hint keeps Dolphin from treating the paste as a move.
#[cfg(target_os = "linux")]
fn contents(path: &str, png: Option<Vec<u8>>) -> Vec<ClipboardContent> {
    let mut out = Vec::with_capacity(5);
    let is_image = png.is_some();
    if let Some(png) = png {
        out.push(ClipboardContent::Other("image/png".into(), png));
    }
    out.push(ClipboardContent::Other("text/uri-list".into(), uri_list(path).into_bytes()));
    out.push(ClipboardContent::Other(
        "x-special/gnome-copied-files".into(),
        gnome_copied_files(path).into_bytes(),
    ));
    out.push(ClipboardContent::Other("application/x-kde-cutselection".into(), b"0".to_vec()));
    // A plain-text path would win over the file in text fields; for images that would be
    // surprising, so only non-image files carry it.
    if !is_image {
        out.push(ClipboardContent::Text(path.to_owned()));
    }
    out
}

/// Windows: image first (setting an image clears the clipboard), then the registered `PNG`
/// format (keeps transparency for Chromium/Electron apps), then `CF_HDROP`.
#[cfg(target_os = "windows")]
fn contents(path: &str, png: Option<Vec<u8>>) -> Vec<ClipboardContent> {
    use clipboard_rs::{RustImage, RustImageData};
    let mut out = Vec::with_capacity(4);
    let is_image = png.is_some();
    if let Some(png) = png {
        if let Ok(img) = RustImageData::from_bytes(&png) {
            out.push(ClipboardContent::Image(img));
        }
        out.push(ClipboardContent::Other("PNG".into(), png));
    }
    out.push(ClipboardContent::Files(vec![path.to_owned()]));
    if !is_image {
        out.push(ClipboardContent::Text(path.to_owned()));
    }
    out
}

/// macOS: PNG on the pasteboard plus the file URL (clipboard-rs writes files as NSURLs).
#[cfg(target_os = "macos")]
fn contents(path: &str, png: Option<Vec<u8>>) -> Vec<ClipboardContent> {
    use clipboard_rs::{RustImage, RustImageData};
    let mut out = Vec::with_capacity(2);
    if let Some(png) = png {
        if let Ok(img) = RustImageData::from_bytes(&png) {
            out.push(ClipboardContent::Image(img));
        }
    }
    out.push(ClipboardContent::Files(vec![path.to_owned()]));
    out
}

// ------------------------------------------------------------------ pure helpers

/// The path as the clipboard should carry it. On Windows `canonicalize` yields `\\?\C:\x`;
/// Explorer and most apps reject that form in `CF_HDROP`, so the prefix is dropped.
fn clean_path(path: &Path) -> String {
    strip_verbatim_prefix(&path.to_string_lossy())
}

fn strip_verbatim_prefix(s: &str) -> String {
    if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
        return format!(r"\\{rest}");
    }
    s.strip_prefix(r"\\?\").unwrap_or(s).to_owned()
}

/// RFC 3986 percent-encoding of a path into a `file://` URI. Unreserved characters and `/`
/// stay literal; every other byte (spaces, `#`, `%`, non-ASCII) is escaped.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn file_uri(path: &str) -> String {
    let mut out = String::with_capacity(path.len() + 8);
    out.push_str("file://");
    for b in path.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// `text/uri-list` body: one URI per line, CRLF terminated (RFC 2483).
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn uri_list(path: &str) -> String {
    format!("{}\r\n", file_uri(path))
}

/// `x-special/gnome-copied-files` body: the action (`copy`) then the URIs, LF separated and
/// without a trailing newline.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn gnome_copied_files(path: &str) -> String {
    format!("copy\n{}", file_uri(path))
}

fn is_image_ext(ext: Option<&str>) -> bool {
    matches!(ext, Some("png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp"))
}

// ------------------------------------------------------------------ staging

/// Hard-link (else copy) `src` to `<stage_dir>/<n>/<name>` and return that path. Every staged
/// file gets its own numbered folder so equal names never collide and pruning is by age.
/// Older staged copies beyond `KEEP_STAGED - 1` are removed first.
fn stage_for_clipboard(src: &Path, stage_dir: &Path, name: &str) -> io::Result<PathBuf> {
    fs::create_dir_all(stage_dir)?;
    prune_staging(stage_dir, KEEP_STAGED - 1);
    let mut id = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis());
    let name = crate::media_store::sanitize_file_name(name);
    loop {
        let folder = stage_dir.join(id.to_string());
        match fs::create_dir(&folder) {
            Ok(()) => {
                let dest = folder.join(&name);
                // A hard link duplicates nothing; it fails across filesystems (the legacy
                // Documents folder) and on some network/FAT volumes, so fall back to a copy.
                let linked = fs::hard_link(src, &dest).or_else(|_| fs::copy(src, &dest).map(|_| ()));
                return match linked {
                    Ok(()) => Ok(dest),
                    Err(e) => {
                        let _ = fs::remove_dir_all(&folder);
                        Err(e)
                    }
                };
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => id += 1,
            Err(e) => return Err(e),
        }
    }
}

/// Keep the newest `keep` staged folders and delete the rest. Only numbered folders (what
/// `stage_for_clipboard` creates) are touched, and symlinks are never followed.
pub fn prune_staging(stage_dir: &Path, keep: usize) {
    let Ok(entries) = fs::read_dir(stage_dir) else { return };
    let mut staged: Vec<(u128, PathBuf)> = entries
        .flatten()
        .filter_map(|e| {
            let id = e.file_name().to_str()?.parse::<u128>().ok()?;
            let meta = fs::symlink_metadata(e.path()).ok()?;
            meta.is_dir().then(|| (id, e.path()))
        })
        .collect();
    staged.sort_by(|a, b| b.0.cmp(&a.0));
    for (_, folder) in staged.into_iter().skip(keep) {
        let _ = fs::remove_dir_all(folder);
    }
}

/// PNG bytes for an image file: a PNG is passed through without decoding, anything else is
/// decoded once and re-encoded. Refuses images over `max_pixels`.
fn png_bytes(path: &Path, max_pixels: u64) -> Result<Vec<u8>, String> {
    let reader = image::ImageReader::new(BufReader::new(fs::File::open(path).map_err(|e| e.to_string())?))
        .with_guessed_format()
        .map_err(|e| e.to_string())?;
    let (w, h) = reader.into_dimensions().map_err(|e| e.to_string())?;
    if u64::from(w) * u64::from(h) > max_pixels {
        return Err(format!("image is too large to copy as pixels ({w}x{h})"));
    }

    let mut head = [0u8; 8];
    let is_png = fs::File::open(path)
        .and_then(|mut f| f.read_exact(&mut head))
        .is_ok()
        && head == PNG_MAGIC;
    if is_png {
        return fs::read(path).map_err(|e| e.to_string());
    }

    // Not `image::open`: it trusts the file extension, and cached files may have none.
    let decoded = image::ImageReader::new(BufReader::new(fs::File::open(path).map_err(|e| e.to_string())?))
        .with_guessed_format()
        .map_err(|e| e.to_string())?
        .decode()
        .map_err(|e| e.to_string())?;
    let mut out = Cursor::new(Vec::new());
    decoded.write_to(&mut out, image::ImageFormat::Png).map_err(|e| e.to_string())?;
    Ok(out.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_uri_escapes_what_must_be_escaped() {
        assert_eq!(file_uri("/home/a/b.png"), "file:///home/a/b.png");
        assert_eq!(file_uri("/tmp/my file #1%.jpg"), "file:///tmp/my%20file%20%231%25.jpg");
        assert_eq!(file_uri("/tmp/é.png"), "file:///tmp/%C3%A9.png");
        assert_eq!(
            file_uri("/tmp/a/TFSBS (You)-1.jpg"),
            "file:///tmp/a/TFSBS%20%28You%29-1.jpg"
        );
    }

    #[test]
    fn uri_list_is_crlf_terminated() {
        assert_eq!(uri_list("/a b"), "file:///a%20b\r\n");
    }

    #[test]
    fn gnome_payload_is_action_then_uri_without_trailing_newline() {
        assert_eq!(gnome_copied_files("/a b"), "copy\nfile:///a%20b");
    }

    #[test]
    fn verbatim_prefix_is_stripped() {
        assert_eq!(strip_verbatim_prefix(r"\\?\C:\Users\me\a.png"), r"C:\Users\me\a.png");
        assert_eq!(strip_verbatim_prefix(r"\\?\UNC\srv\share\a.png"), r"\\srv\share\a.png");
        assert_eq!(strip_verbatim_prefix(r"C:\a.png"), r"C:\a.png");
        assert_eq!(strip_verbatim_prefix("/home/a.png"), "/home/a.png");
    }

    #[test]
    fn image_detection_is_by_extension() {
        assert!(is_image_ext(Some("png")));
        assert!(is_image_ext(Some("webp")));
        assert!(!is_image_ext(Some("mp4")));
        assert!(!is_image_ext(None));
    }

    fn staged_ids(dir: &Path) -> Vec<String> {
        let mut ids: Vec<String> = fs::read_dir(dir)
            .map(|d| d.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect())
            .unwrap_or_default();
        ids.sort();
        ids
    }

    #[test]
    fn staging_gives_the_file_a_readable_name_without_duplicating_it() {
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join("cache");
        fs::create_dir_all(&cache).unwrap();
        let src = cache.join("AC798AA7D07D4EE0B6BAA3F981297348");
        fs::write(&src, b"\xff\xd8\xff\xe0\0\x10JFIFpixels").unwrap();
        let stage = dir.path().join("stage");
        let staged =
            stage_for_clipboard(&src, &stage, "Work-Mum-261112-1325.jpg").unwrap();
        assert_eq!(staged.file_name().unwrap(), "Work-Mum-261112-1325.jpg");
        assert_eq!(fs::read(&staged).unwrap(), fs::read(&src).unwrap());
        assert!(staged.starts_with(&stage));
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            assert_eq!(fs::metadata(&src).unwrap().nlink(), 2, "same inode, not a second copy");
        }
        // The same name again lands in its own folder instead of clashing.
        let again =
            stage_for_clipboard(&src, &stage, "Work-Mum-261112-1325.jpg").unwrap();
        assert_ne!(staged.parent(), again.parent());
        assert!(staged.exists() && again.exists());
        // Names are made safe for every OS.
        let odd = stage_for_clipboard(&src, &stage, "a/b:c.jpg").unwrap();
        assert_eq!(odd.file_name().unwrap(), "b_c.jpg");
    }

    #[test]
    fn staging_keeps_only_the_latest_few_and_leaves_foreign_entries_alone() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("a.bin");
        fs::write(&src, b"x").unwrap();
        let stage = dir.path().join("stage");
        fs::create_dir_all(&stage).unwrap();
        fs::write(stage.join("notes.txt"), b"mine").unwrap();
        fs::create_dir_all(stage.join("keepme")).unwrap();
        let mut last = None;
        for _ in 0..(KEEP_STAGED + 4) {
            last = Some(stage_for_clipboard(&src, &stage, "f.bin").unwrap());
        }
        let numbered: Vec<_> = staged_ids(&stage)
            .into_iter()
            .filter(|n| n.parse::<u128>().is_ok())
            .collect();
        assert_eq!(numbered.len(), KEEP_STAGED);
        assert!(last.unwrap().exists(), "the newest copy survives");
        assert!(stage.join("notes.txt").exists() && stage.join("keepme").is_dir());
        // Startup keeps just the newest one.
        prune_staging(&stage, KEEP_STAGED_AT_STARTUP);
        let numbered: Vec<_> = staged_ids(&stage)
            .into_iter()
            .filter(|n| n.parse::<u128>().is_ok())
            .collect();
        assert_eq!(numbered.len(), KEEP_STAGED_AT_STARTUP);
        // Pruning a folder that does not exist is a no-op.
        prune_staging(&dir.path().join("missing"), 0);
    }

    fn write_img(dir: &Path, name: &str, fmt: image::ImageFormat, w: u32, h: u32) -> PathBuf {
        let path = dir.join(name);
        let img = image::DynamicImage::ImageRgb8(image::RgbImage::new(w, h));
        img.save_with_format(&path, fmt).unwrap();
        path
    }

    #[test]
    fn png_is_passed_through_and_jpeg_is_transcoded() {
        let dir = tempfile::tempdir().unwrap();
        let png = write_img(dir.path(), "a.png", image::ImageFormat::Png, 4, 3);
        let jpg = write_img(dir.path(), "a.jpg", image::ImageFormat::Jpeg, 4, 3);
        assert_eq!(png_bytes(&png, 1000).unwrap(), fs::read(&png).unwrap());
        let out = png_bytes(&jpg, 1000).unwrap();
        assert_eq!(&out[..8], PNG_MAGIC);
        assert_ne!(out, fs::read(&jpg).unwrap());
    }

    #[test]
    fn extensionless_jpeg_is_transcoded_by_its_content() {
        let dir = tempfile::tempdir().unwrap();
        let bare = write_img(dir.path(), "AC798AA7D07D4EE0B6BAA3F981297348", image::ImageFormat::Jpeg, 4, 3);
        let out = png_bytes(&bare, 1000).unwrap();
        assert_eq!(&out[..8], PNG_MAGIC);
    }

    #[test]
    fn oversized_images_are_refused_before_decoding() {
        let dir = tempfile::tempdir().unwrap();
        let png = write_img(dir.path(), "big.png", image::ImageFormat::Png, 40, 30);
        assert!(png_bytes(&png, 1000).is_err());
        assert!(png_bytes(&png, 1200).is_ok());
    }

    #[test]
    fn copy_rejects_paths_outside_the_media_roots_before_touching_the_clipboard() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("cache");
        fs::create_dir_all(&root).unwrap();
        let inside = root.join("a.txt");
        fs::write(&inside, "x").unwrap();
        let outside = dir.path().join("secret.txt");
        fs::write(&outside, "x").unwrap();
        let roots = vec![root.clone()];

        let stage = dir.path().join("stage");
        let hint = MediaNameHint::default();
        assert!(copy_media_file(&outside, &roots, &stage, &hint).unwrap_err().contains("outside"));
        assert!(copy_media_file(&root, &roots, &stage, &hint).is_err(), "a folder is not a media file");
        assert!(copy_media_file(&root.join("missing.png"), &roots, &stage, &hint).is_err());
        assert!(!stage.exists(), "nothing is staged for a refused path");
        // The accepted path is the canonical one inside the root.
        assert!(crate::media_store::validated_media_path(&inside, &roots).is_ok());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_payload_has_image_only_for_images_and_text_only_for_files() {
        let names = |c: &[ClipboardContent]| -> Vec<String> {
            c.iter()
                .map(|c| match c {
                    ClipboardContent::Other(n, _) => n.clone(),
                    ClipboardContent::Text(_) => "text".into(),
                    _ => "other".into(),
                })
                .collect()
        };
        let img = names(&contents("/a.png", Some(vec![1])));
        assert_eq!(img[0], "image/png");
        assert!(img.contains(&"text/uri-list".to_string()));
        assert!(img.contains(&"x-special/gnome-copied-files".to_string()));
        assert!(!img.contains(&"text".to_string()));
        let file = names(&contents("/a.mp4", None));
        assert!(!file.contains(&"image/png".to_string()));
        assert!(file.contains(&"text".to_string()));
    }

    /// Manual check against a live Linux session: `cargo test live_ -- --ignored --nocapture`
    /// (needs `xclip` on X11 or `wl-paste` on Wayland to read the targets back).
    #[cfg(target_os = "linux")]
    #[test]
    #[ignore]
    fn live_linux_copy_lists_expected_targets() {
        use std::process::Command;
        let dir = tempfile::tempdir().unwrap();
        let png = write_img(dir.path(), "my pic.png", image::ImageFormat::Png, 4, 3);
        let vid = dir.path().join("clip.mp4");
        fs::write(&vid, b"not really a video").unwrap();
        let roots = vec![dir.path().to_path_buf()];
        let stage = dir.path().join("stage");
        let hint = MediaNameHint {
            kind: Some("image".into()),
            filename: None,
            timestamp: Some("2026-11-12T13:25:04+00:00".into()),
            account_name: Some("Work".into()),
            chat_name: Some("Mum".into()),
        };
        let read = |args: &[&str]| -> Vec<u8> {
            let (bin, a): (&str, Vec<&str>) = if std::env::var_os("WAYLAND_DISPLAY").is_some() {
                ("wl-paste", args.to_vec())
            } else {
                ("xclip", [&["-selection", "clipboard", "-o"][..], args].concat())
            };
            Command::new(bin).args(a).output().unwrap().stdout
        };
        let targets = |_: ()| -> String {
            let out = if std::env::var_os("WAYLAND_DISPLAY").is_some() {
                read(&["--list-types"])
            } else {
                read(&["-t", "TARGETS"])
            };
            String::from_utf8_lossy(&out).into_owned()
        };

        assert_eq!(copy_media_file(&png, &roots, &stage, &hint).unwrap(), CopyKind::Image);
        std::thread::sleep(std::time::Duration::from_millis(300));
        let t = targets(());
        println!("image targets:\n{t}");
        for want in ["image/png", "text/uri-list", "x-special/gnome-copied-files"] {
            assert!(t.contains(want), "missing {want}");
        }
        let mime = |m: &str| read(&["-t", m]);
        assert_eq!(&mime("image/png")[..8], PNG_MAGIC);
        println!("uri-list: {:?}", String::from_utf8_lossy(&mime("text/uri-list")));
        println!("gnome: {:?}", String::from_utf8_lossy(&mime("x-special/gnome-copied-files")));

        // The reported bug: a cached JPEG named after the message id, with no extension.
        let bare = write_img(
            dir.path(),
            "AC798AA7D07D4EE0B6BAA3F981297348",
            image::ImageFormat::Jpeg,
            4,
            3,
        );
        assert_eq!(copy_media_file(&bare, &roots, &stage, &hint).unwrap(), CopyKind::Image);
        std::thread::sleep(std::time::Duration::from_millis(300));
        let t = targets(());
        assert!(t.contains("image/png"), "an extensionless JPEG is still copied as pixels");
        let uris = String::from_utf8_lossy(&mime("text/uri-list")).into_owned();
        println!("extensionless uri-list: {uris:?}");
        assert!(uris.contains("Work-Mum-261112-1625.jpg"), "{uris}");
        assert!(!uris.contains("AC798AA7"), "{uris}");
        assert!(uris.chars().all(|c| c != ' '), "no raw spaces in a URI: {uris}");

        assert_eq!(copy_media_file(&vid, &roots, &stage, &hint).unwrap(), CopyKind::File);
        std::thread::sleep(std::time::Duration::from_millis(300));
        let t = targets(());
        println!("file targets:\n{t}");
        assert!(t.contains("text/uri-list") && !t.contains("image/png"));
        println!("text: {:?}", String::from_utf8_lossy(&read(&["-t", "UTF8_STRING"])));
    }
}
