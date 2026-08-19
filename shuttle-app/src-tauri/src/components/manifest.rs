use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestArtifact {
    pub url: String,
    pub sha256: String,
    #[serde(default)]
    pub size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub schema: u32,
    pub shuttle_version: String,
    pub platforms: HashMap<String, HashMap<String, ManifestArtifact>>,
}

impl Manifest {
    pub fn load(path: &Path) -> Result<Self, String> {
        let raw = fs::read_to_string(path).map_err(|e| e.to_string())?;
        serde_json::from_str(&raw).map_err(|e| format!("Invalid manifest: {e}"))
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let raw = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        fs::write(path, raw).map_err(|e| e.to_string())
    }

    pub fn artifact(&self, platform: &str, component_id: &str) -> Option<&ManifestArtifact> {
        self.platforms
            .get(platform)
            .and_then(|p| p.get(component_id))
    }
}

pub fn manifest_url() -> Option<String> {
    let base = option_env!("SHUTTLE_COMPONENTS_BASE_URL")?;
    let trimmed = base.trim();
    if trimmed.is_empty() {
        return None;
    }
    let version = env!("CARGO_PKG_VERSION");
    Some(format!(
        "{}/v{}/manifest.json",
        trimmed.trim_end_matches('/'),
        version
    ))
}

pub fn platform_key() -> &'static str {
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        return "linux-x86_64";
    }
    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    {
        return "linux-arm64";
    }
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    {
        return "macos-x86_64";
    }
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        return "macos-arm64";
    }
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    {
        return "windows-x86_64";
    }
    #[cfg(all(target_os = "windows", target_arch = "aarch64"))]
    {
        return "windows-arm64";
    }
    #[allow(unreachable_code)]
    "unknown"
}

pub fn manifest_cache_path(components_root: &Path) -> PathBuf {
    components_root.join("manifest.json")
}
