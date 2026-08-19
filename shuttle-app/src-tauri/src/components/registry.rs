use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct InstalledComponent {
    pub id: String,
    pub version: Option<String>,
    pub sha256: Option<String>,
    pub path: String,
    #[serde(default)]
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Registry {
    pub components: HashMap<String, InstalledComponent>,
}

impl Registry {
    pub fn load(path: &Path) -> Self {
        if !path.exists() {
            return Self::default();
        }
        fs::read_to_string(path)
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let raw = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        fs::write(path, raw).map_err(|e| e.to_string())
    }

    pub fn is_installed(&self, id: &str) -> bool {
        self.components
            .get(id)
            .map(|c| PathBuf::from(&c.path).exists())
            .unwrap_or(false)
    }

    pub fn path_for(&self, id: &str) -> Option<PathBuf> {
        self.components
            .get(id)
            .map(|c| PathBuf::from(&c.path))
            .filter(|p| p.exists())
    }

    pub fn mark(&mut self, id: &str, path: PathBuf, sha256: Option<String>, source: &str) {
        self.components.insert(
            id.to_string(),
            InstalledComponent {
                id: id.to_string(),
                version: None,
                sha256,
                path: path.to_string_lossy().into_owned(),
                source: source.to_string(),
            },
        );
    }
}
