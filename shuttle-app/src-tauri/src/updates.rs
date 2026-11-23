//! Update check, download and installer launch for the About page.
//!
//! The check asks GitHub's public release API for this repository and sends nothing else: no
//! account, no installation id, no chat data. See PRIVACY.md.

use crate::connectors::AppEvent;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tauri::Emitter;

const RELEASE_URL: &str = "https://api.github.com/repos/smsheese/shuttle/releases/latest";
const USER_AGENT: &str = "shuttle-app";
const CHECK_TTL: Duration = Duration::from_secs(60 * 60);

// Each variant is constructed on the platform it names, so a per-platform build sees the
// others as unconstructed.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Platform {
    LinuxDeb,
    LinuxRpm,
    LinuxAppImage,
    Windows,
    MacOs,
    Unsupported,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Arch {
    X64,
    Arm64,
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateAsset {
    pub name: String,
    pub url: String,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct UpdateCheck {
    pub current: String,
    pub latest: String,
    pub available: bool,
    pub notes_url: String,
    pub asset: Option<UpdateAsset>,
    /// `SHA256SUMS` asset of the same release, when the publisher ships one.
    pub checksums_url: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct RawRelease {
    #[serde(default)]
    tag_name: String,
    #[serde(default)]
    html_url: String,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    assets: Vec<RawAsset>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct RawAsset {
    name: String,
    browser_download_url: String,
    #[serde(default)]
    size: u64,
}

/// `latest` is newer than `current`. Unparseable tags never count as an update.
pub(crate) fn is_newer(latest: &str, current: &str) -> bool {
    match (parse_version(latest), parse_version(current)) {
        (Some(latest), Some(current)) => latest > current,
        _ => false,
    }
}

fn parse_version(raw: &str) -> Option<semver::Version> {
    semver::Version::parse(raw.trim().trim_start_matches('v')).ok()
}

pub(crate) fn current_arch() -> Arch {
    #[cfg(target_arch = "x86_64")]
    {
        return Arch::X64;
    }
    #[cfg(target_arch = "aarch64")]
    {
        return Arch::Arm64;
    }
    #[allow(unreachable_code)]
    Arch::Other
}

pub(crate) fn current_platform() -> Platform {
    #[cfg(target_os = "windows")]
    {
        return Platform::Windows;
    }
    #[cfg(target_os = "macos")]
    {
        return Platform::MacOs;
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        return linux_platform();
    }
    #[allow(unreachable_code)]
    Platform::Unsupported
}

/// Debian-family systems take the `.deb`, RPM-family the `.rpm`, everything else the AppImage.
fn linux_platform() -> Platform {
    if Path::new("/var/lib/dpkg/status").exists() {
        Platform::LinuxDeb
    } else if Path::new("/var/lib/rpm").exists() || Path::new("/usr/lib/sysimage/rpm").exists() {
        Platform::LinuxRpm
    } else {
        Platform::LinuxAppImage
    }
}

/// The release file this machine can install, or `None` when the release ships none for it.
///
/// A release also carries the previous version's files, so the file naming `version` wins before
/// architecture does. macOS ships one universal image, which is why the arch pass can fall through.
pub(crate) fn select_asset(
    assets: &[RawAsset],
    platform: Platform,
    arch: Arch,
    version: &str,
) -> Option<RawAsset> {
    let suffixes: &[&str] = match platform {
        Platform::LinuxDeb => &[".deb"],
        Platform::LinuxRpm => &[".rpm"],
        Platform::LinuxAppImage => &[".appimage"],
        Platform::Windows => &["-setup.exe"],
        Platform::MacOs => &[".dmg"],
        Platform::Unsupported => return None,
    };
    let arch_tokens: &[&str] = match arch {
        Arch::X64 => &["amd64", "x86_64", "x64"],
        Arch::Arm64 => &["arm64", "aarch64"],
        Arch::Other => &[],
    };
    let version = version.trim().trim_start_matches('v');
    let for_platform: Vec<&RawAsset> = assets
        .iter()
        .filter(|asset| {
            let name = asset.name.to_lowercase();
            suffixes.iter().any(|suffix| name.ends_with(suffix))
        })
        .collect();
    let for_version: Vec<&RawAsset> = for_platform
        .iter()
        .copied()
        .filter(|asset| version.is_empty() || asset.name.contains(version))
        .collect();
    let pool = if for_version.is_empty() {
        for_platform
    } else {
        for_version
    };
    let named_for_arch = pool.iter().find(|asset| {
        let name = asset.name.to_lowercase();
        arch_tokens.iter().any(|token| name.contains(token))
    });
    named_for_arch.or_else(|| pool.first()).copied().cloned()
}

/// Hash listed for `filename` in a `SHA256SUMS` file, if it lists one.
pub(crate) fn sha_expected_from_sums(sums: &str, filename: &str) -> Option<String> {
    sums.lines().find_map(|line| {
        let mut fields = line.split_whitespace();
        let hash = fields.next()?;
        let name = fields.next()?.trim_start_matches('*');
        (hash.len() == 64 && name == filename).then(|| hash.to_lowercase())
    })
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let mut file = fs::File::open(path).map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn verify_download(path: &Path, expected: Option<&str>) -> Result<(), String> {
    let Some(expected) = expected else {
        return Err("The release does not publish a checksum for this installer, so it was not run.".into());
    };
    let actual = sha256_file(path)?;
    if actual.eq_ignore_ascii_case(expected.trim()) {
        Ok(())
    } else {
        Err("The downloaded update does not match the checksum the release publishes. Nothing was run.".into())
    }
}

pub struct UpdateManager {
    http: reqwest::Client,
    cache_dir: PathBuf,
    last_check: Mutex<Option<(Instant, UpdateCheck)>>,
    downloaded: Mutex<Option<PathBuf>>,
}

impl UpdateManager {
    pub fn new(cache_dir: PathBuf) -> Self {
        Self {
            http: reqwest::Client::new(),
            cache_dir,
            last_check: Mutex::new(None),
            downloaded: Mutex::new(None),
        }
    }

    fn cached(&self) -> Option<UpdateCheck> {
        let guard = self.last_check.lock();
        let (checked_at, check) = guard.as_ref()?;
        (checked_at.elapsed() < CHECK_TTL).then(|| check.clone())
    }

    pub async fn check(&self, force: bool) -> Result<UpdateCheck, String> {
        if !force {
            if let Some(check) = self.cached() {
                return Ok(check);
            }
        }
        let release = self.fetch_release().await?;
        let current = env!("CARGO_PKG_VERSION").to_string();
        let latest = release.tag_name.trim_start_matches('v').to_string();
        let asset = select_asset(&release.assets, current_platform(), current_arch(), &latest)
            .map(|asset| UpdateAsset {
                name: asset.name,
                url: asset.browser_download_url,
                size: asset.size,
            });
        let checksums_url = release
            .assets
            .iter()
            .find(|asset| asset.name.eq_ignore_ascii_case("SHA256SUMS"))
            .map(|asset| asset.browser_download_url.clone());
        let check = UpdateCheck {
            available: is_newer(&latest, &current),
            notes_url: release.html_url,
            current,
            latest,
            asset,
            checksums_url,
        };
        *self.last_check.lock() = Some((Instant::now(), check.clone()));
        Ok(check)
    }

    async fn fetch_release(&self) -> Result<RawRelease, String> {
        let response = self
            .http
            .get(RELEASE_URL)
            .header(reqwest::header::USER_AGENT, USER_AGENT)
            .header(reqwest::header::ACCEPT, "application/vnd.github+json")
            .send()
            .await
            .map_err(|_| "Could not reach GitHub. Check your connection and try again.".to_string())?;
        let status = response.status();
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS
            || status == reqwest::StatusCode::FORBIDDEN
        {
            return Err("GitHub is limiting update checks from this computer. Try again later.".into());
        }
        if status == reqwest::StatusCode::NOT_FOUND {
            return Err("GitHub has no published release of Shuttle yet.".into());
        }
        if !status.is_success() {
            return Err(format!("GitHub answered with HTTP {}.", status.as_u16()));
        }
        let release: RawRelease = response
            .json()
            .await
            .map_err(|_| "GitHub sent a release Shuttle could not read.".to_string())?;
        if release.draft || release.prerelease {
            return Err("The newest release is a draft or a prerelease, so Shuttle ignores it.".into());
        }
        Ok(release)
    }

    pub async fn download(&self, app: &tauri::AppHandle) -> Result<String, String> {
        let check = match self.cached() {
            Some(check) => check,
            None => self.check(false).await?,
        };
        let asset = check
            .asset
            .ok_or_else(|| "This release has no installer for this computer.".to_string())?;
        fs::create_dir_all(&self.cache_dir).map_err(|e| e.to_string())?;
        let dest = self.cache_dir.join(safe_file_name(&asset.name));
        if dest.exists() {
            fs::remove_file(&dest).ok();
        }
        self.stream_to(&asset, &dest, app).await?;
        let expected = match check.checksums_url.as_deref() {
            Some(url) => self.expected_hash(url, &asset.name).await?,
            None => None,
        };
        if let Err(e) = verify_download(&dest, expected.as_deref()) {
            fs::remove_file(&dest).ok();
            return Err(e);
        }
        *self.downloaded.lock() = Some(dest.clone());
        Ok(dest.to_string_lossy().into_owned())
    }

    async fn expected_hash(&self, sums_url: &str, asset_name: &str) -> Result<Option<String>, String> {
        let response = self
            .http
            .get(sums_url)
            .header(reqwest::header::USER_AGENT, USER_AGENT)
            .send()
            .await
            .map_err(|_| "Could not fetch the release checksums. Nothing was run.".to_string())?;
        if !response.status().is_success() {
            return Err("Could not fetch the release checksums. Nothing was run.".into());
        }
        let sums = response
            .text()
            .await
            .map_err(|_| "Could not read the release checksums. Nothing was run.".to_string())?;
        Ok(sha_expected_from_sums(&sums, asset_name))
    }

    async fn stream_to(
        &self,
        asset: &UpdateAsset,
        dest: &Path,
        app: &tauri::AppHandle,
    ) -> Result<(), String> {
        use futures_util::StreamExt;
        let response = self
            .http
            .get(&asset.url)
            .header(reqwest::header::USER_AGENT, USER_AGENT)
            .send()
            .await
            .map_err(|_| "Could not reach GitHub. Check your connection and try again.".to_string())?;
        if !response.status().is_success() {
            return Err(format!(
                "The download failed with HTTP {}.",
                response.status().as_u16()
            ));
        }
        let total = if asset.size > 0 {
            asset.size
        } else {
            response.content_length().unwrap_or(0)
        };
        let mut file = fs::File::create(dest).map_err(|e| e.to_string())?;
        let mut done = 0u64;
        emit_progress(app, done, total);
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|_| "The download stopped before it finished.".to_string())?;
            file.write_all(&chunk).map_err(|e| e.to_string())?;
            done += chunk.len() as u64;
            emit_progress(app, done, total);
        }
        Ok(())
    }

    /// Launch the downloaded installer. Never elevates: the OS shows its own prompt when the
    /// package needs it.
    pub fn run_installer(&self) -> Result<(), String> {
        let path = self
            .downloaded
            .lock()
            .clone()
            .ok_or_else(|| "Download the update before running it.".to_string())?;
        if !path.exists() {
            return Err("The downloaded update is gone. Download it again.".into());
        }
        launch_installer(&path)
    }
}

fn emit_progress(app: &tauri::AppHandle, bytes_done: u64, bytes_total: u64) {
    let _ = app.emit(
        "shuttle-event",
        AppEvent {
            kind: "update.download.progress".into(),
            payload: serde_json::json!({
                "bytes_done": bytes_done,
                "bytes_total": bytes_total,
            }),
        },
    );
}

fn safe_file_name(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_ascii_alphanumeric() || ".-_".contains(c) { c } else { '_' })
        .collect()
}

#[cfg(target_os = "windows")]
fn launch_installer(path: &Path) -> Result<(), String> {
    std::process::Command::new(path)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Could not start the installer: {e}"))
}

#[cfg(target_os = "macos")]
fn launch_installer(path: &Path) -> Result<(), String> {
    std::process::Command::new("open")
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Could not open the disk image: {e}"))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn launch_installer(path: &Path) -> Result<(), String> {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default()
        .to_lowercase();
    if name.ends_with(".appimage") {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755))
            .map_err(|e| format!("Could not make the AppImage executable: {e}"))?;
        return std::process::Command::new(path)
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("Could not start the AppImage: {e}"));
    }
    open_with_desktop_handler(path)
}

#[cfg(all(unix, not(target_os = "macos")))]
fn open_with_desktop_handler(path: &Path) -> Result<(), String> {
    let xdg = std::process::Command::new("xdg-open").arg(path).spawn();
    if xdg.is_ok() {
        return Ok(());
    }
    std::process::Command::new("gio")
        .arg("open")
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|_| {
            "Could not open the package with your system installer. Open it from your downloads folder."
                .to_string()
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset(name: &str) -> RawAsset {
        RawAsset {
            name: name.into(),
            browser_download_url: format!("https://example.test/{name}"),
            size: 10,
        }
    }

    #[test]
    fn newer_patch_minor_and_major_are_updates() {
        assert!(is_newer("0.6.2", "0.6.1"));
        assert!(is_newer("0.7.0", "0.6.9"));
        assert!(is_newer("1.0.0", "0.9.9"));
        assert!(is_newer("v0.6.2", "0.6.1"));
    }

    #[test]
    fn same_or_older_version_is_not_an_update() {
        assert!(!is_newer("0.6.1", "0.6.1"));
        assert!(!is_newer("0.6.0", "0.6.1"));
        assert!(!is_newer("1.0.0-beta.1", "1.0.0"));
        assert!(is_newer("1.0.0", "1.0.0-beta.1"));
    }

    #[test]
    fn unreadable_tags_never_count_as_updates() {
        assert!(!is_newer("latest", "0.6.1"));
        assert!(!is_newer("", "0.6.1"));
    }

    #[test]
    fn linux_picks_deb_rpm_or_appimage() {
        let assets = vec![
            asset("Shuttle_0.6.2_amd64.deb"),
            asset("Shuttle-0.6.2-1.x86_64.rpm"),
            asset("Shuttle_0.6.2_amd64.AppImage"),
        ];
        assert_eq!(
            select_asset(&assets, Platform::LinuxDeb, Arch::X64, "0.6.2").map(|a| a.name),
            Some("Shuttle_0.6.2_amd64.deb".to_string())
        );
        assert_eq!(
            select_asset(&assets, Platform::LinuxRpm, Arch::X64, "0.6.2").map(|a| a.name),
            Some("Shuttle-0.6.2-1.x86_64.rpm".to_string())
        );
        assert_eq!(
            select_asset(&assets, Platform::LinuxAppImage, Arch::X64, "0.6.2").map(|a| a.name),
            Some("Shuttle_0.6.2_amd64.AppImage".to_string())
        );
    }

    #[test]
    fn windows_and_macos_pick_the_setup_for_this_arch() {
        let windows = vec![
            asset("Shuttle_0.6.2_x64-setup.exe"),
            asset("Shuttle_0.6.2_arm64-setup.exe"),
        ];
        assert_eq!(
            select_asset(&windows, Platform::Windows, Arch::Arm64, "0.6.2").map(|a| a.name),
            Some("Shuttle_0.6.2_arm64-setup.exe".to_string())
        );
        assert_eq!(
            select_asset(&windows, Platform::Windows, Arch::X64, "0.6.2").map(|a| a.name),
            Some("Shuttle_0.6.2_x64-setup.exe".to_string())
        );
        let macos = vec![asset("Shuttle_0.6.2_aarch64.dmg"), asset("Shuttle_0.6.2_x64.dmg")];
        assert_eq!(
            select_asset(&macos, Platform::MacOs, Arch::Arm64, "0.6.2").map(|a| a.name),
            Some("Shuttle_0.6.2_aarch64.dmg".to_string())
        );
    }

    #[test]
    fn the_previous_version_files_in_one_release_are_skipped() {
        // Release v0.6.1 really does carry 0.6.0 installers alongside its own.
        let assets = vec![
            asset("Shuttle_0.6.0_amd64.deb"),
            asset("Shuttle_0.6.0_amd64.AppImage"),
            asset("Shuttle_0.6.1_amd64.deb"),
            asset("Shuttle_0.6.1_amd64.AppImage"),
        ];
        assert_eq!(
            select_asset(&assets, Platform::LinuxDeb, Arch::X64, "0.6.1").map(|a| a.name),
            Some("Shuttle_0.6.1_amd64.deb".to_string())
        );
        assert_eq!(
            select_asset(&assets, Platform::LinuxAppImage, Arch::X64, "v0.6.1").map(|a| a.name),
            Some("Shuttle_0.6.1_amd64.AppImage".to_string())
        );
    }

    #[test]
    fn a_universal_macos_image_is_used_when_no_file_names_the_arch() {
        let assets = vec![asset("Shuttle_0.6.1_universal.dmg")];
        assert_eq!(
            select_asset(&assets, Platform::MacOs, Arch::Arm64, "0.6.1").map(|a| a.name),
            Some("Shuttle_0.6.1_universal.dmg".to_string())
        );
    }

    #[test]
    fn a_release_without_an_installer_for_this_platform_selects_nothing() {
        let assets = vec![asset("Shuttle_0.6.2_amd64.deb")];
        assert!(select_asset(&assets, Platform::MacOs, Arch::X64, "0.6.2").is_none());
        assert!(select_asset(&assets, Platform::Unsupported, Arch::X64, "0.6.2").is_none());
    }

    #[test]
    fn checksum_line_is_read_from_a_sums_file() {
        let deb = "a".repeat(64);
        let appimage = "b".repeat(64);
        let sums = format!("{deb}  Shuttle_0.6.2_amd64.deb\n{appimage} *Shuttle_0.6.2_amd64.AppImage\n");
        assert_eq!(
            sha_expected_from_sums(&sums, "Shuttle_0.6.2_amd64.deb").as_deref(),
            Some(deb.as_str())
        );
        assert_eq!(
            sha_expected_from_sums(&sums, "Shuttle_0.6.2_amd64.AppImage").as_deref(),
            Some(appimage.as_str())
        );
        assert_eq!(sha_expected_from_sums(&sums, "other.deb"), None);
        assert_eq!(sha_expected_from_sums("not-a-checksum  Shuttle.deb", "Shuttle.deb"), None);
    }

    #[test]
    fn a_download_that_matches_its_checksum_is_accepted() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Shuttle.deb");
        fs::write(&path, b"installer bytes").unwrap();
        let real = sha256_file(&path).unwrap();
        assert!(verify_download(&path, Some(&real)).is_ok());
        assert!(verify_download(&path, Some(&real.to_uppercase())).is_ok());
        assert!(verify_download(&path, None).is_err());
    }

    #[test]
    fn a_download_that_fails_its_checksum_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Shuttle.deb");
        fs::write(&path, b"installer bytes").unwrap();
        let error = verify_download(&path, Some(&"0".repeat(64))).unwrap_err();
        assert!(error.contains("checksum"));
    }

    #[test]
    fn installer_file_names_lose_path_separators() {
        assert_eq!(safe_file_name("../Shuttle_0.6.2_amd64.deb"), ".._Shuttle_0.6.2_amd64.deb");
    }
}
