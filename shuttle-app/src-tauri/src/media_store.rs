use std::path::{Path, PathBuf};

/// User-visible files: ~/Documents/shuttle/{account_id}/media|avatars
pub fn shuttle_documents_root() -> PathBuf {
    dirs::document_dir()
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("shuttle")
}

pub fn account_files_root(account_id: &str) -> PathBuf {
    shuttle_documents_root().join(safe_dir_name(account_id))
}

pub fn account_media_dir(account_id: &str) -> PathBuf {
    account_files_root(account_id).join("media")
}

pub fn account_avatars_dir(account_id: &str) -> PathBuf {
    account_files_root(account_id).join("avatars")
}

pub fn ensure_account_dirs(account_id: &str) -> std::io::Result<()> {
    std::fs::create_dir_all(account_media_dir(account_id))?;
    std::fs::create_dir_all(account_avatars_dir(account_id))?;
    Ok(())
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

pub fn guess_mime(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase().as_str() {
        "png" => "image/png",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "mp4" => "video/mp4",
        "ogg" => "audio/ogg",
        "mp3" => "audio/mpeg",
        "pdf" => "application/pdf",
        "jpg" | "jpeg" => "image/jpeg",
        _ => "application/octet-stream",
    }
}

pub fn read_as_data_url(path: &str) -> Result<String, String> {
    let p = Path::new(path);
    if !p.is_file() {
        return Err("Media file not found".into());
    }
    let raw = std::fs::read(p).map_err(|e| e.to_string())?;
    let mime = guess_mime(p);
    Ok(format!("data:{mime};base64,{}", base64_encode(&raw)))
}

fn base64_encode(raw: &[u8]) -> String {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    STANDARD.encode(raw)
}

fn safe_dir_name(value: &str) -> String {
    value
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
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
