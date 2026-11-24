//! Link previews and map tiles for the message list.
//!
//! The webview never fetches a remote page or image. This module does it, with guards:
//! https/http only, every resolved address must be public (checked again on each redirect and
//! pinned into the request so a second DNS answer cannot swap it), at most three redirects,
//! a hard timeout, and byte caps on the page and on images. Results are cached in
//! `<data_dir>/media_cache/previews` (outside any account folder, so retention never visits
//! it) and the folder is pruned to a size cap.

use std::fs;
use std::io::Cursor;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use regex::Regex;
use reqwest::{redirect::Policy, Url};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const USER_AGENT: &str = concat!("Shuttle/", env!("CARGO_PKG_VERSION"), " (desktop messaging app)");
/// Pages are read only up to `</head>`, but YouTube's head alone is about 860 KB, so the
/// ceiling is generous. Nothing past `</head>` is ever read.
const MAX_HTML_BYTES: usize = 2 * 1024 * 1024;
const MAX_IMAGE_BYTES: usize = 1024 * 1024;
const MAX_TILE_BYTES: usize = 200 * 1024;
const MAX_REDIRECTS: usize = 3;
const TIMEOUT: Duration = Duration::from_secs(4);
const HIT_TTL_SECS: u64 = 7 * 24 * 3600;
const MISS_TTL_SECS: u64 = 24 * 3600;
const CACHE_CAP_BYTES: u64 = 50 * 1024 * 1024;
const THUMB_EDGE: u32 = 320;
const TILE_ZOOM: u32 = 15;
const MAP_W: u32 = 260;
const MAP_H: u32 = 120;

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct LinkPreview {
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub site: Option<String>,
    /// Channel or author, when the page names one (YouTube does).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    /// Publication date as the page states it (ISO 8601), e.g. a video's upload date.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub published: Option<String>,
    /// Absolute path of the cached thumbnail, readable through `read_message_media`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_path: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct CacheEntry {
    fetched_at: u64,
    preview: Option<LinkPreview>,
}

pub fn previews_dir(data_dir: &Path) -> PathBuf {
    crate::media_store::media_cache_root(data_dir).join("previews")
}

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn key_for(text: &str) -> String {
    let digest = Sha256::digest(format!("v2|{text}").as_bytes());
    digest.iter().take(16).map(|b| format!("{b:02x}")).collect()
}

/// True only for addresses that belong to the public internet.
pub fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_public_v4(v4),
        IpAddr::V6(v6) => {
            if let Some(mapped) = v6.to_ipv4_mapped() {
                return is_public_v4(mapped);
            }
            let seg = v6.segments();
            !(v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || (seg[0] & 0xfe00) == 0xfc00
                || (seg[0] & 0xffc0) == 0xfe80
                || (seg[0] == 0x2001 && seg[1] == 0x0db8)
                || v6 == Ipv6Addr::LOCALHOST)
        }
    }
}

fn is_public_v4(ip: Ipv4Addr) -> bool {
    let o = ip.octets();
    !(ip.is_private()
        || ip.is_loopback()
        || ip.is_link_local()
        || ip.is_broadcast()
        || ip.is_unspecified()
        || ip.is_multicast()
        || ip.is_documentation()
        || o[0] == 0
        || (o[0] == 100 && (64..=127).contains(&o[1]))
        || (o[0] == 192 && o[1] == 0 && o[2] == 0)
        || (o[0] == 198 && (o[1] == 18 || o[1] == 19))
        || o[0] >= 240)
}

struct Fetched {
    body: Vec<u8>,
    final_url: Url,
    content_type: String,
}

async fn fetch_guarded(
    start: &Url,
    max_bytes: usize,
    accept: &str,
    stop_after: Option<&[u8]>,
) -> Result<Fetched, String> {
    let mut url = start.clone();
    for _ in 0..=MAX_REDIRECTS {
        if url.scheme() != "http" && url.scheme() != "https" {
            return Err("unsupported scheme".into());
        }
        let host = url.host_str().ok_or("no host")?.to_string();
        let port = url.port_or_known_default().ok_or("no port")?;
        let addrs: Vec<SocketAddr> = tokio::net::lookup_host((host.as_str(), port))
            .await
            .map_err(|e| e.to_string())?
            .collect();
        if addrs.is_empty() || addrs.iter().any(|a| !is_public_ip(a.ip())) {
            return Err("address is not public".into());
        }
        let mut builder = reqwest::Client::builder()
            .redirect(Policy::none())
            .timeout(TIMEOUT)
            .user_agent(USER_AGENT)
            .no_proxy();
        if host.parse::<IpAddr>().is_err() {
            builder = builder.resolve(&host, addrs[0]);
        }
        let client = builder.build().map_err(|e| e.to_string())?;
        let mut resp = client
            .get(url.clone())
            .header(reqwest::header::ACCEPT, accept)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if resp.status().is_redirection() {
            let next = resp
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|v| v.to_str().ok())
                .ok_or("redirect without location")?;
            url = url.join(next).map_err(|e| e.to_string())?;
            continue;
        }
        if !resp.status().is_success() {
            return Err(format!("status {}", resp.status()));
        }
        let content_type = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_ascii_lowercase();
        let mut body = Vec::new();
        while let Some(chunk) = resp.chunk().await.map_err(|e| e.to_string())? {
            let room = max_bytes.saturating_sub(body.len());
            let take = chunk.len().min(room);
            body.extend_from_slice(&chunk[..take]);
            if body.len() >= max_bytes {
                break;
            }
            if let Some(marker) = stop_after {
                let from = body.len().saturating_sub(take + marker.len());
                if body[from..].windows(marker.len()).any(|w| w.eq_ignore_ascii_case(marker)) {
                    break;
                }
            }
        }
        return Ok(Fetched { body, final_url: url, content_type });
    }
    Err("too many redirects".into())
}

fn decode_entities(raw: &str) -> String {
    let named = raw
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&nbsp;", " ");
    let num = Regex::new(r"&#(x[0-9a-fA-F]+|\d+);").expect("static regex");
    num.replace_all(&named, |c: &regex::Captures| {
        let s = &c[1];
        let code = match s.strip_prefix('x') {
            Some(hex) => u32::from_str_radix(hex, 16).ok(),
            None => s.parse::<u32>().ok(),
        };
        code.and_then(char::from_u32).map(String::from).unwrap_or_default()
    })
    .into_owned()
}

fn clean(raw: &str, max_chars: usize) -> Option<String> {
    let text = decode_entities(raw);
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.is_empty() {
        None
    } else {
        Some(text.chars().take(max_chars).collect())
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct PageMeta {
    pub title: Option<String>,
    pub description: Option<String>,
    pub site: Option<String>,
    pub author: Option<String>,
    pub published: Option<String>,
    pub image: Option<String>,
}

/// Read the Open Graph / Twitter / plain HTML facts out of a page's head.
pub fn parse_page_meta(html: &str) -> PageMeta {
    let head_end = html.to_ascii_lowercase().find("</head>").unwrap_or(html.len());
    let head = &html[..head_end];
    let meta_tag = Regex::new(r"(?is)<meta\s[^>]*>").expect("static regex");
    let link_name = Regex::new(r#"(?is)<link\s[^>]*itemprop\s*=\s*["']name["'][^>]*>"#).expect("static regex");
    let attr = |tag: &str, name: &str| -> Option<String> {
        let re = Regex::new(&format!(r#"(?is)\b{name}\s*=\s*(?:"([^"]*)"|'([^']*)')"#)).ok()?;
        re.captures(tag).and_then(|c| c.get(1).or_else(|| c.get(2))).map(|m| m.as_str().to_string())
    };
    let mut found: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for m in meta_tag.find_iter(head) {
        let tag = m.as_str();
        let key = attr(tag, "property").or_else(|| attr(tag, "name")).or_else(|| attr(tag, "itemprop"));
        let content = attr(tag, "content");
        if let (Some(k), Some(c)) = (key, content) {
            found.entry(k.to_ascii_lowercase()).or_insert(c);
        }
    }
    let pick = |keys: &[&str]| keys.iter().find_map(|k| found.get(*k)).cloned();
    let title_tag = Regex::new(r"(?is)<title[^>]*>(.*?)</title>").expect("static regex");
    let title = pick(&["og:title", "twitter:title"])
        .or_else(|| title_tag.captures(head).map(|c| c[1].to_string()));
    PageMeta {
        title: title.and_then(|t| clean(&t, 200)),
        description: pick(&["og:description", "twitter:description", "description"])
            .and_then(|d| clean(&d, 300)),
        site: pick(&["og:site_name"]).and_then(|s| clean(&s, 80)),
        author: link_name
            .find(head)
            .and_then(|m| attr(m.as_str(), "content"))
            .or_else(|| pick(&["author"]))
            .and_then(|a| clean(&a, 80)),
        published: pick(&["article:published_time", "datepublished", "uploaddate", "og:video:release_date"])
            .map(|d| d.trim().to_string())
            .filter(|d| d.len() >= 10 && d.as_bytes()[..4].iter().all(u8::is_ascii_digit) && d.as_bytes()[4] == b'-')
            .map(|d| d.chars().take(40).collect()),
        image: pick(&["og:image", "og:image:url", "twitter:image", "twitter:image:src"])
            .map(|i| decode_entities(i.trim())),
    }
}

fn read_cache<T: for<'de> Deserialize<'de>>(path: &Path) -> Option<T> {
    serde_json::from_slice(&fs::read(path).ok()?).ok()
}

fn prune_cache(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    let mut files: Vec<(PathBuf, u64, SystemTime)> = entries
        .flatten()
        .filter_map(|e| {
            let meta = e.metadata().ok()?;
            meta.is_file().then(|| (e.path(), meta.len(), meta.modified().unwrap_or(UNIX_EPOCH)))
        })
        .collect();
    let mut total: u64 = files.iter().map(|f| f.1).sum();
    if total <= CACHE_CAP_BYTES {
        return;
    }
    files.sort_by_key(|f| f.2);
    for (path, size, _) in files {
        if total <= CACHE_CAP_BYTES * 9 / 10 {
            break;
        }
        if fs::remove_file(&path).is_ok() {
            total = total.saturating_sub(size);
        }
    }
}

fn save_thumbnail(bytes: &[u8], dest: &Path) -> Result<(), String> {
    let mut reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| e.to_string())?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(6000);
    limits.max_image_height = Some(6000);
    limits.max_alloc = Some(96 * 1024 * 1024);
    reader.limits(limits);
    let img = reader.decode().map_err(|e| e.to_string())?;
    img.thumbnail(THUMB_EDGE, THUMB_EDGE)
        .to_rgb8()
        .save_with_format(dest, image::ImageFormat::Jpeg)
        .map_err(|e| e.to_string())
}

/// Preview of the page at `raw_url`, from the cache when fresh. `Ok(None)` means the page has
/// nothing to preview (or could not be fetched); that outcome is cached for a day.
pub async fn link_preview(data_dir: &Path, raw_url: &str) -> Result<Option<LinkPreview>, String> {
    let url = Url::parse(raw_url).map_err(|e| e.to_string())?;
    if url.scheme() != "http" && url.scheme() != "https" {
        return Err("unsupported scheme".into());
    }
    let dir = previews_dir(data_dir);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let key = key_for(url.as_str());
    let cache_path = dir.join(format!("{key}.json"));
    if let Some(entry) = read_cache::<CacheEntry>(&cache_path) {
        let ttl = if entry.preview.is_some() { HIT_TTL_SECS } else { MISS_TTL_SECS };
        if now_secs().saturating_sub(entry.fetched_at) < ttl {
            return Ok(entry.preview);
        }
    }
    let preview = build_preview(&dir, &key, &url).await;
    let entry = CacheEntry { fetched_at: now_secs(), preview: preview.clone() };
    if let Ok(json) = serde_json::to_vec(&entry) {
        let _ = fs::write(&cache_path, json);
    }
    prune_cache(&dir);
    Ok(preview)
}

async fn build_preview(dir: &Path, key: &str, url: &Url) -> Option<LinkPreview> {
    let page = fetch_guarded(url, MAX_HTML_BYTES, "text/html,application/xhtml+xml", Some(b"</head>"))
        .await
        .ok()?;
    if !page.content_type.starts_with("text/html") && !page.content_type.contains("xhtml") {
        return None;
    }
    let meta = parse_page_meta(&String::from_utf8_lossy(&page.body));
    if meta.title.is_none() && meta.description.is_none() {
        return None;
    }
    let mut image_path = None;
    if let Some(image) = meta.image.as_deref().and_then(|i| page.final_url.join(i).ok()) {
        if let Ok(got) = fetch_guarded(&image, MAX_IMAGE_BYTES, "image/*", None).await {
            let dest = dir.join(format!("{key}.jpg"));
            if got.content_type.starts_with("image/") && save_thumbnail(&got.body, &dest).is_ok() {
                image_path = Some(dest.to_string_lossy().into_owned());
            }
        }
    }
    let site = meta
        .site
        .or_else(|| page.final_url.host_str().map(|h| h.trim_start_matches("www.").to_string()));
    Some(LinkPreview {
        url: url.to_string(),
        title: meta.title,
        description: meta.description,
        site,
        author: meta.author,
        published: meta.published,
        image_path,
    })
}

/// Fractional OSM tile coordinates of a point at `zoom`.
pub fn tile_coords(lat: f64, lng: f64, zoom: u32) -> (f64, f64) {
    let n = f64::from(1u32 << zoom);
    let lat = lat.clamp(-85.05112878, 85.05112878).to_radians();
    let x = (lng + 180.0) / 360.0 * n;
    let y = (1.0 - lat.tan().asinh() / std::f64::consts::PI) / 2.0 * n;
    (x, y)
}

/// A small map image centred on the point, stitched from four OpenStreetMap tiles and cached.
/// Returns the image's absolute path.
pub async fn map_tile(data_dir: &Path, lat: f64, lng: f64) -> Result<Option<String>, String> {
    if !(lat.is_finite() && lng.is_finite()) || lat.abs() > 90.0 || lng.abs() > 180.0 {
        return Err("invalid coordinates".into());
    }
    let dir = previews_dir(data_dir);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let dest = dir.join(format!("map_{}.jpg", key_for(&format!("{lat:.5},{lng:.5}"))));
    if dest.exists() {
        return Ok(Some(dest.to_string_lossy().into_owned()));
    }
    let n = 1i64 << TILE_ZOOM;
    let (fx, fy) = tile_coords(lat, lng, TILE_ZOOM);
    let x0 = (fx - 0.5).floor() as i64;
    let y0 = ((fy - 0.5).floor() as i64).clamp(0, n - 2);
    let mut canvas = image::RgbImage::new(512, 512);
    for dy in 0..2i64 {
        for dx in 0..2i64 {
            let tx = (x0 + dx).rem_euclid(n);
            let ty = y0 + dy;
            let url = Url::parse(&format!("https://tile.openstreetmap.org/{TILE_ZOOM}/{tx}/{ty}.png"))
                .map_err(|e| e.to_string())?;
            let got = match fetch_guarded(&url, MAX_TILE_BYTES, "image/png", None).await {
                Ok(g) if g.content_type.starts_with("image/") => g,
                _ => return Ok(None),
            };
            let tile = image::load_from_memory(&got.body).map_err(|e| e.to_string())?.to_rgb8();
            image::imageops::replace(&mut canvas, &tile, dx * 256, dy * 256);
        }
    }
    let px = ((fx - x0 as f64) * 256.0).round() as i64;
    let py = ((fy - y0 as f64) * 256.0).round() as i64;
    let left = (px - i64::from(MAP_W) / 2).clamp(0, 512 - i64::from(MAP_W)) as u32;
    let top = (py - i64::from(MAP_H) / 2).clamp(0, 512 - i64::from(MAP_H)) as u32;
    let crop = image::imageops::crop_imm(&canvas, left, top, MAP_W, MAP_H).to_image();
    crop.save_with_format(&dest, image::ImageFormat::Jpeg).map_err(|e| e.to_string())?;
    prune_cache(&dir);
    Ok(Some(dest.to_string_lossy().into_owned()))
}

// ---- Video embed host -------------------------------------------------------------------
//
// YouTube refuses to play inside a page whose origin is not http(s) ("Error 153"), and the
// webview's own origin is `tauri://localhost`. The player is therefore put inside a one-line
// wrapper page served from `http://127.0.0.1:<port>`, which gives YouTube a normal referrer.
// The listener starts on the first play, binds loopback only, and serves nothing but that
// wrapper for ids that pass validation.

static EMBED_HOST: tokio::sync::OnceCell<String> = tokio::sync::OnceCell::const_new();

fn embed_page(provider: &str, id: &str) -> Option<String> {
    let src = match provider {
        "youtube" if id.len() == 11 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-') => {
            format!("https://www.youtube-nocookie.com/embed/{id}?autoplay=1&playsinline=1&rel=0")
        }
        "vimeo" if (5..=12).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_digit()) => {
            format!("https://player.vimeo.com/video/{id}?autoplay=1&dnt=1")
        }
        _ => return None,
    };
    Some(format!(
        "<!doctype html><meta charset=utf-8><meta name=referrer content=strict-origin-when-cross-origin>\
<style>html,body{{margin:0;height:100%;background:#000;overflow:hidden}}iframe{{border:0;width:100%;height:100%}}</style>\
<iframe src=\"{src}\" allow=\"autoplay; encrypted-media; picture-in-picture; fullscreen\" allowfullscreen referrerpolicy=strict-origin-when-cross-origin></iframe>"
    ))
}

/// The HTTP response for one request line such as `GET /embed/youtube/dQw4w9WgXcQ HTTP/1.1`.
fn embed_response(request_line: &str) -> Vec<u8> {
    let mut parts = request_line.split_whitespace();
    let (method, path) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
    let page = if method == "GET" {
        let path = path.split('?').next().unwrap_or("");
        let mut seg = path.trim_start_matches('/').split('/');
        match (seg.next(), seg.next(), seg.next(), seg.next()) {
            (Some("embed"), Some(provider), Some(id), None) => embed_page(provider, id),
            _ => None,
        }
    } else {
        None
    };
    match page {
        Some(html) => format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nCache-Control: no-store\r\nReferrer-Policy: strict-origin-when-cross-origin\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{html}",
            html.len()
        )
        .into_bytes(),
        None => b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec(),
    }
}

async fn serve_embed(mut stream: tokio::net::TcpStream) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut buf = [0u8; 2048];
    let read = tokio::time::timeout(Duration::from_secs(3), stream.read(&mut buf)).await;
    let Ok(Ok(n)) = read else { return };
    let head = String::from_utf8_lossy(&buf[..n]);
    let line = head.lines().next().unwrap_or("");
    let _ = stream.write_all(&embed_response(line)).await;
    let _ = stream.shutdown().await;
}

/// Base URL (`http://127.0.0.1:<port>`) of the embed host, started on first use.
pub async fn embed_host() -> Result<String, String> {
    EMBED_HOST
        .get_or_try_init(|| async {
            let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
                .await
                .map_err(|e| e.to_string())?;
            let port = listener.local_addr().map_err(|e| e.to_string())?.port();
            tokio::spawn(async move {
                while let Ok((stream, _)) = listener.accept().await {
                    tokio::spawn(serve_embed(stream));
                }
            });
            Ok::<String, String>(format!("http://127.0.0.1:{port}"))
        })
        .await
        .cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_and_special_addresses_are_refused() {
        for ip in [
            "127.0.0.1", "10.1.2.3", "192.168.0.9", "172.16.5.5", "169.254.169.254", "100.64.0.1",
            "0.0.0.0", "224.0.0.1", "::1", "fe80::1", "fd00::1", "::ffff:10.0.0.1",
        ] {
            assert!(!is_public_ip(ip.parse().unwrap()), "{ip} must be refused");
        }
        for ip in ["8.8.8.8", "1.1.1.1", "2606:4700:4700::1111"] {
            assert!(is_public_ip(ip.parse().unwrap()), "{ip} must be allowed");
        }
    }

    #[test]
    fn open_graph_meta_wins_over_plain_title() {
        let html = r#"<html><head><title>Plain</title>
            <meta property="og:title" content="Rent &amp; Sale &#8211; Riyadh">
            <meta name='description' content='  Two   lines
            of text '>
            <meta property="og:image" content="/img/a.png">
            <meta property="og:site_name" content="Listings"></head><body></body></html>"#;
        let m = parse_page_meta(html);
        assert_eq!(m.title.as_deref(), Some("Rent & Sale \u{2013} Riyadh"));
        assert_eq!(m.description.as_deref(), Some("Two lines of text"));
        assert_eq!(m.image.as_deref(), Some("/img/a.png"));
        assert_eq!(m.site.as_deref(), Some("Listings"));
    }

    #[test]
    fn video_pages_give_a_channel_and_an_upload_date() {
        let html = r#"<head><meta property="og:title" content="T">
            <link itemprop="name" content="Rick Astley">
            <meta itemprop="datePublished" content="2009-10-24T23:57:33-07:00">
            <meta itemprop="name" content="T"></head>"#;
        let m = parse_page_meta(html);
        assert_eq!(m.author.as_deref(), Some("Rick Astley"));
        assert_eq!(m.published.as_deref(), Some("2009-10-24T23:57:33-07:00"));
        let none = parse_page_meta("<head><meta itemprop=\"datePublished\" content=\"soon\"></head>");
        assert_eq!(none.published, None);
    }

    #[test]
    fn falls_back_to_the_title_tag() {
        let m = parse_page_meta("<head><title> Hello </title></head>");
        assert_eq!(m.title.as_deref(), Some("Hello"));
        assert_eq!(m.description, None);
    }

    #[test]
    fn meta_after_the_head_is_ignored() {
        let m = parse_page_meta("<head></head><body><meta property=\"og:title\" content=\"late\"></body>");
        assert_eq!(m.title, None);
    }

    #[test]
    fn tile_coordinates_match_the_slippy_map_formula() {
        let (x, y) = tile_coords(0.0, 0.0, 1);
        assert!((x - 1.0).abs() < 1e-9 && (y - 1.0).abs() < 1e-9);
        let (x, y) = tile_coords(24.6803947, 46.7077217, 15);
        assert_eq!(x.floor() as u32, 20_635);
        assert_eq!(y.floor() as u32, 14_064);
    }

    #[tokio::test]
    async fn loopback_urls_are_never_fetched() {
        let dir = std::env::temp_dir().join(format!("shuttle-lp-{}", now_secs()));
        let got = link_preview(&dir, "http://127.0.0.1:1/x").await;
        assert_eq!(got, Ok(None));
        let _ = fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    #[ignore = "needs the internet"]
    async fn live_page_and_map_tile() {
        let dir = std::env::temp_dir().join(format!("shuttle-live-{}", now_secs()));
        let p = link_preview(&dir, "https://github.com/").await.unwrap().expect("preview");
        let yt = link_preview(&dir, "https://www.youtube.com/watch?v=dQw4w9WgXcQ").await.unwrap().expect("youtube");
        assert!(yt.title.as_deref().unwrap_or("").contains("Rick Astley"), "{yt:?}");
        assert!(yt.image_path.is_some(), "{yt:?}");
        assert_eq!(yt.author.as_deref(), Some("Rick Astley"), "{yt:?}");
        assert!(yt.published.as_deref().unwrap_or("").starts_with("2009-10-2"), "{yt:?}");
        assert!(p.title.is_some(), "{p:?}");
        let tile = map_tile(&dir, 24.6803947, 46.7077217).await.unwrap().expect("tile");
        assert!(fs::metadata(&tile).unwrap().len() > 1000);
        println!("{p:?}\n{tile}");
    }

    #[test]
    fn embed_pages_only_exist_for_valid_ids() {
        let ok = String::from_utf8(embed_response("GET /embed/youtube/dQw4w9WgXcQ HTTP/1.1")).unwrap();
        assert!(ok.starts_with("HTTP/1.1 200") && ok.contains("youtube-nocookie.com/embed/dQw4w9WgXcQ"));
        let vimeo = String::from_utf8(embed_response("GET /embed/vimeo/76979871?x=1 HTTP/1.1")).unwrap();
        assert!(vimeo.contains("player.vimeo.com/video/76979871"));
        for bad in [
            "GET /embed/youtube/short HTTP/1.1",
            "GET /embed/youtube/\"onload=alert(1)x HTTP/1.1",
            "GET /embed/vimeo/abc12345 HTTP/1.1",
            "GET /embed/evil/dQw4w9WgXcQ HTTP/1.1",
            "GET /embed/youtube/dQw4w9WgXcQ/extra HTTP/1.1",
            "POST /embed/youtube/dQw4w9WgXcQ HTTP/1.1",
            "GET / HTTP/1.1",
        ] {
            assert!(String::from_utf8(embed_response(bad)).unwrap().starts_with("HTTP/1.1 404"), "{bad}");
        }
    }

    #[tokio::test]
    async fn embed_host_serves_on_loopback() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let base = embed_host().await.unwrap();
        assert!(base.starts_with("http://127.0.0.1:"));
        let addr = base.trim_start_matches("http://");
        let mut c = tokio::net::TcpStream::connect(addr).await.unwrap();
        c.write_all(b"GET /embed/youtube/dQw4w9WgXcQ HTTP/1.1\r\nHost: x\r\n\r\n").await.unwrap();
        let mut out = String::new();
        c.read_to_string(&mut out).await.unwrap();
        assert!(out.starts_with("HTTP/1.1 200") && out.contains("<iframe"), "{out}");
    }
}
