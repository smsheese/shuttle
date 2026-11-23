mod install;
mod manifest;
mod python;
mod registry;

pub use manifest::{platform_key, Manifest};
pub use registry::InstalledComponent;

use crate::connectors::AppEvent;
use install::{
    download_file, extract_archive, finalize_native_layout, install_path_for_component,
    normalize_python_runtime, verify_sha256,
};
use manifest::{manifest_cache_path, manifest_url};
use parking_lot::Mutex;
use python::{detect_system_python, find_managed_python, python_deps_dir, python_version};
use registry::Registry as ComponentRegistry;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::broadcast;

#[derive(Debug, Clone, serde::Serialize)]
pub struct ComponentRequirement {
    pub id: String,
    pub label: String,
    pub size: u64,
    pub installed: bool,
    pub optional: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ConnectorRequirements {
    pub connector_id: String,
    pub components: Vec<ComponentRequirement>,
    pub total_download_bytes: u64,
}

/// One connector network and the components it needs. The constructor of installs and the
/// About page both read this table, so a new network gets its row in one place.
struct ConnectorComponents {
    id: &'static str,
    label: &'static str,
    components: &'static [&'static str],
}

const CONNECTORS: &[ConnectorComponents] = &[
    ConnectorComponents {
        id: "whatsapp",
        label: "WhatsApp",
        components: &["script:whatsapp", "native:gowa"],
    },
    ConnectorComponents {
        id: "telegram",
        label: "Telegram",
        components: &["script:telegram", "native:tdlib"],
    },
    ConnectorComponents {
        id: "signal",
        label: "Signal",
        components: &["script:signal", "native:signal-cli"],
    },
    ConnectorComponents {
        id: "messenger",
        label: "Messenger",
        components: &[
            "script:messenger",
            "script:shuttle_ipc",
            "python-runtime",
            "python:deps:messenger",
        ],
    },
    ConnectorComponents {
        id: "instagram",
        label: "Instagram",
        components: &[
            "script:instagram",
            "script:shuttle_ipc",
            "python-runtime",
            "python:deps:instagram",
        ],
    },
    ConnectorComponents {
        id: "matrix",
        label: "Matrix",
        components: &["script:matrix", "script:shuttle_ipc", "python-runtime"],
    },
    ConnectorComponents {
        id: "email",
        label: "Email",
        components: &["script:email", "script:shuttle_ipc", "python-runtime"],
    },
];

/// One component row of the About page. `size` stays `None` when the manifest is out of reach,
/// so an offline machine still gets a full list instead of an error page.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ComponentStatus {
    pub id: String,
    pub label: String,
    pub installed: bool,
    pub optional: bool,
    pub version: Option<String>,
    pub size: Option<u64>,
    pub source: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ConnectorComponentStatus {
    pub connector_id: String,
    pub connector_label: String,
    pub components: Vec<ComponentStatus>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ComponentsOverview {
    pub platform: String,
    /// Version the app expects its manifest to match.
    pub expected_version: String,
    pub manifest_version: Option<String>,
    pub manifest_available: bool,
    pub connectors: Vec<ConnectorComponentStatus>,
}

pub struct ComponentManager {
    components_root: PathBuf,
    registry_path: PathBuf,
    registry: Mutex<ComponentRegistry>,
    http: reqwest::Client,
    cancel_flag: Arc<AtomicBool>,
    event_tx: broadcast::Sender<AppEvent>,
}

impl ComponentManager {
    pub fn new(data_dir: &Path, event_tx: broadcast::Sender<AppEvent>) -> Self {
        let components_root = data_dir.join("components");
        std::fs::create_dir_all(&components_root).ok();
        std::fs::create_dir_all(components_root.join("scripts")).ok();
        std::fs::create_dir_all(components_root.join("native")).ok();
        let registry_path = components_root.join("installed.json");
        let registry = ComponentRegistry::load(&registry_path);
        Self {
            components_root,
            registry_path,
            registry: Mutex::new(registry),
            http: reqwest::Client::new(),
            cancel_flag: Arc::new(AtomicBool::new(false)),
            event_tx,
        }
    }

    pub fn root(&self) -> &Path {
        &self.components_root
    }

    pub fn scripts_dir(&self) -> PathBuf {
        self.root().join("scripts")
    }

    pub fn cancel_install(&self) {
        self.cancel_flag.store(true, Ordering::SeqCst);
    }

    pub fn clear_cancel(&self) {
        self.cancel_flag.store(false, Ordering::SeqCst);
    }

    pub fn installed_components(&self) -> Vec<InstalledComponent> {
        self.registry.lock().components.values().cloned().collect()
    }

    pub fn connector_component_ids(connector_id: &str) -> Vec<&'static str> {
        Self::connector_row(connector_id)
            .map(|row| row.components.to_vec())
            .unwrap_or_default()
    }

    fn connector_row(connector_id: &str) -> Option<&'static ConnectorComponents> {
        CONNECTORS.iter().find(|row| row.id == connector_id)
    }

    fn component_label(id: &str) -> String {
        match id {
            "python-runtime" => "Python runtime".into(),
            "native:gowa" => "WhatsApp helper (GOWA)".into(),
            "native:tdlib" => "Telegram library (TDLib)".into(),
            "native:signal-cli" => "Signal CLI".into(),
            "python:deps:messenger" => "Messenger libraries".into(),
            "python:deps:instagram" => "Instagram libraries".into(),
            id if id.starts_with("script:") => {
                let name = id.strip_prefix("script:").unwrap_or(id);
                format!("{} connector", capitalize(name))
            }
            other => other.to_string(),
        }
    }

    fn resolve_requirements(&self, connector_id: &str) -> Result<ConnectorRequirements, String> {
        if dev_has_connector(connector_id) {
            return Ok(ConnectorRequirements {
                connector_id: connector_id.to_string(),
                components: Self::connector_component_ids(connector_id)
                    .into_iter()
                    .map(|id| ComponentRequirement {
                        id: id.to_string(),
                        label: Self::component_label(id),
                        size: 0,
                        installed: true,
                        optional: false,
                    })
                    .collect(),
                total_download_bytes: 0,
            });
        }
        let manifest = self.load_manifest_sync()?;
        let platform = platform_key();
        let mut components = Vec::new();
        let mut total_download_bytes = 0u64;
        for id in Self::connector_component_ids(connector_id) {
            let optional = id == "python-runtime" && detect_system_python().is_some();
            let installed = if id == "python-runtime" {
                self.python_runtime_ready()
            } else {
                self.registry.lock().is_installed(id)
            };
            let (size, label) = if installed {
                (0, Self::component_label(id))
            } else if optional {
                (0, Self::component_label(id))
            } else if let Some(artifact) = manifest.artifact(platform, id) {
                total_download_bytes += artifact.size;
                (artifact.size, Self::component_label(id))
            } else if dev_component_exists(id) {
                (0, Self::component_label(id))
            } else {
                return Err(format!(
                    "Component {id} not available for platform {platform}"
                ));
            };
            components.push(ComponentRequirement {
                id: id.to_string(),
                label,
                size,
                installed: installed || optional || dev_component_exists(id),
                optional,
            });
        }
        Ok(ConnectorRequirements {
            connector_id: connector_id.to_string(),
            components,
            total_download_bytes,
        })
    }

    pub fn get_connector_requirements(
        &self,
        connector_id: &str,
    ) -> Result<ConnectorRequirements, String> {
        self.resolve_requirements(connector_id)
    }

    /// Every connector's components with what the About page shows. Never fails on a missing
    /// manifest: sizes and manifest version read as unknown instead.
    pub async fn components_overview(&self) -> ComponentsOverview {
        let platform = platform_key();
        let manifest = self.fetch_manifest().await.ok();
        let registry = self.registry.lock().clone();
        let python_ready = self.python_runtime_ready();
        let connectors = CONNECTORS
            .iter()
            .map(|row| ConnectorComponentStatus {
                connector_id: row.id.to_string(),
                connector_label: row.label.to_string(),
                components: row
                    .components
                    .iter()
                    .map(|id| {
                        let artifact = manifest.as_ref().and_then(|m| m.artifact(platform, id));
                        let optional = *id == "python-runtime" && !python_ready;
                        let managed = registry.components.get(*id);
                        let installed = if *id == "python-runtime" {
                            python_ready
                        } else {
                            registry.is_installed(id) || dev_component_exists(id)
                        };
                        ComponentStatus {
                            id: (*id).to_string(),
                            label: Self::component_label(id),
                            installed,
                            optional,
                            version: managed
                                .and_then(|c| c.version.clone())
                                .or_else(|| artifact.and_then(|a| a.version.clone())),
                            size: artifact.map(|a| a.size).filter(|size| *size > 0),
                            source: managed.map(|c| c.source.clone()),
                        }
                    })
                    .collect(),
            })
            .collect();
        ComponentsOverview {
            platform: platform.to_string(),
            expected_version: env!("CARGO_PKG_VERSION").to_string(),
            manifest_version: manifest.as_ref().map(|m| m.shuttle_version.clone()),
            manifest_available: manifest.is_some(),
            connectors,
        }
    }

    pub fn ensure_connector_installed(&self, connector_id: &str) -> Result<(), String> {
        if dev_has_connector(connector_id) {
            return Ok(());
        }
        let requirements = self.resolve_requirements(connector_id)?;
        for component in requirements.components {
            if component.installed {
                continue;
            }
            if component.optional {
                self.ensure_system_python_registered()?;
                continue;
            }
            self.install_component(&component.id)?;
        }
        Ok(())
    }

    pub async fn ensure_connector_installed_async(&self, connector_id: &str) -> Result<(), String> {
        if dev_has_connector(connector_id) {
            return Ok(());
        }
        let requirements = self.resolve_requirements(connector_id)?;
        for component in requirements.components {
            if self.cancel_flag.load(Ordering::SeqCst) {
                return Err("Component install cancelled".into());
            }
            if component.installed {
                continue;
            }
            if component.optional {
                self.ensure_system_python_registered()?;
                continue;
            }
            self.install_component_async(&component.id).await?;
        }
        Ok(())
    }

    fn ensure_system_python_registered(&self) -> Result<(), String> {
        if self.python_runtime_ready() {
            return Ok(());
        }
        let Some((bin, args)) = detect_system_python() else {
            return Err("Compatible system Python 3.12+ not found".into());
        };
        let mut path = PathBuf::from(&bin);
        if !args.is_empty() {
            // Store launcher info in registry path field as bin|arg1|arg2
            path = PathBuf::from(format!("{}|{}", bin, args.join("|")));
        }
        let mut registry = self.registry.lock();
        registry.mark(
            "python-runtime",
            path,
            None,
            "system",
            python_version(&bin, &args),
        );
        registry.save(&self.registry_path)?;
        Ok(())
    }

    fn python_runtime_ready(&self) -> bool {
        if self.registry.lock().is_installed("python-runtime") {
            return true;
        }
        find_managed_python(&self.components_root.join("python")).is_some()
    }

    fn load_manifest_sync(&self) -> Result<Manifest, String> {
        let cache = manifest_cache_path(&self.components_root);
        if cache.exists() {
            if let Ok(manifest) = Manifest::load(&cache) {
                if manifest.shuttle_version == env!("CARGO_PKG_VERSION") {
                    return Ok(manifest);
                }
            }
        }
        let url = manifest_url().ok_or_else(|| {
            "Component manifest URL not configured (SHUTTLE_COMPONENTS_BASE_URL)".to_string()
        })?;
        let response = tauri::async_runtime::block_on(async {
            self.http
                .get(&url)
                .send()
                .await
                .map_err(|e| format!("Failed to fetch manifest: {e}"))
        })?;
        if !response.status().is_success() {
            return Err(format!(
                "Failed to fetch manifest: HTTP {}",
                response.status().as_u16()
            ));
        }
        let body = tauri::async_runtime::block_on(async {
            response.text().await.map_err(|e| e.to_string())
        })?;
        let manifest: Manifest =
            serde_json::from_str(&body).map_err(|e| format!("Invalid manifest JSON: {e}"))?;
        if manifest.schema != 1 {
            return Err(format!("Unsupported manifest schema {}", manifest.schema));
        }
        if manifest.shuttle_version != env!("CARGO_PKG_VERSION") {
            return Err(format!(
                "Manifest version {} does not match app version {}",
                manifest.shuttle_version,
                env!("CARGO_PKG_VERSION")
            ));
        }
        manifest.save(&cache)?;
        Ok(manifest)
    }

    async fn fetch_manifest(&self) -> Result<Manifest, String> {
        let cache = manifest_cache_path(&self.components_root);
        if cache.exists() {
            if let Ok(manifest) = Manifest::load(&cache) {
                if manifest.shuttle_version == env!("CARGO_PKG_VERSION") {
                    return Ok(manifest);
                }
            }
        }
        let url = manifest_url().ok_or_else(|| {
            "Component manifest URL not configured (SHUTTLE_COMPONENTS_BASE_URL)".to_string()
        })?;
        let response = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("Failed to fetch manifest: {e}"))?;
        if !response.status().is_success() {
            return Err(format!(
                "Failed to fetch manifest: HTTP {}",
                response.status().as_u16()
            ));
        }
        let body = response.text().await.map_err(|e| e.to_string())?;
        let manifest: Manifest =
            serde_json::from_str(&body).map_err(|e| format!("Invalid manifest JSON: {e}"))?;
        if manifest.schema != 1 {
            return Err(format!("Unsupported manifest schema {}", manifest.schema));
        }
        if manifest.shuttle_version != env!("CARGO_PKG_VERSION") {
            return Err(format!(
                "Manifest version {} does not match app version {}",
                manifest.shuttle_version,
                env!("CARGO_PKG_VERSION")
            ));
        }
        manifest.save(&cache)?;
        Ok(manifest)
    }

    fn install_component(&self, component_id: &str) -> Result<(), String> {
        tauri::async_runtime::block_on(self.install_component_async(component_id))
    }

    async fn install_component_async(&self, component_id: &str) -> Result<(), String> {
        if component_id == "python-runtime"
            && detect_system_python().is_some() {
                return self.ensure_system_python_registered();
            }
        if self.registry.lock().is_installed(component_id) {
            return Ok(());
        }
        if dev_component_exists(component_id) {
            return Ok(());
        }

        let manifest = self.fetch_manifest().await?;
        let platform = platform_key();
        let artifact = manifest
            .artifact(platform, component_id)
            .ok_or_else(|| format!("Component {component_id} missing for {platform}"))?
            .clone();

        self.emit_progress(component_id, 0, artifact.size, "starting");

        let temp_dir = tempfile::tempdir().map_err(|e| e.to_string())?;
        let filename = artifact
            .url
            .rsplit('/')
            .next()
            .filter(|s| !s.is_empty())
            .unwrap_or("artifact");
        let archive_path = temp_dir.path().join(filename);
        let component_id_owned = component_id.to_string();
        let cancel = self.cancel_flag.clone();
        let event_tx = self.event_tx.clone();
        let client = self.http.clone();
        let artifact_for_download = artifact.clone();

        download_file(
            &client,
            &artifact_for_download,
            &archive_path,
            move |update| {
                if cancel.load(Ordering::SeqCst) {
                    return;
                }
                let _ = event_tx.send(AppEvent {
                    kind: "component.install.progress".into(),
                    payload: serde_json::json!({
                        "component_id": component_id_owned,
                        "bytes_done": update.bytes_done,
                        "bytes_total": update.bytes_total,
                        "phase": update.phase,
                    }),
                });
            },
        )
        .await?;

        if self.cancel_flag.load(Ordering::SeqCst) {
            return Err("Component install cancelled".into());
        }

        verify_sha256(&archive_path, &artifact.sha256)?;
        self.emit_progress(component_id, artifact.size, artifact.size, "extracting");

        let dest = install_path_for_component(&self.components_root, component_id);
        if dest.exists() {
            if dest.is_dir() {
                std::fs::remove_dir_all(&dest).ok();
            } else {
                std::fs::remove_file(&dest).ok();
            }
        }

        if component_id.starts_with("script:") {
            let dest = install_path_for_component(&self.components_root, component_id);
            if let Some(parent) = dest.parent() {
                extract_archive(&archive_path, parent)?;
            } else {
                extract_archive(&archive_path, &dest)?;
            }
        } else if component_id == "python-runtime" {
            extract_archive(&archive_path, &dest)?;
            normalize_python_runtime(&dest)?;
        } else if component_id.starts_with("python:deps:") {
            extract_archive(&archive_path, &dest)?;
        } else if component_id.starts_with("native:") {
            extract_archive(&archive_path, &dest)?;
            let final_path = finalize_native_layout(component_id, &dest)?;
            let mut registry = self.registry.lock();
            let managed_version = artifact
                .version
                .clone()
                .or_else(|| Some(manifest.shuttle_version.clone()));
            registry.mark(
                component_id,
                final_path,
                Some(artifact.sha256.clone()),
                "managed",
                managed_version,
            );
            registry.save(&self.registry_path)?;
            self.emit_progress(component_id, artifact.size, artifact.size, "complete");
            return Ok(());
        } else {
            extract_archive(&archive_path, &dest)?;
        }

        let installed_path = if component_id == "python-runtime" {
            find_managed_python(&dest).unwrap_or(dest)
        } else {
            dest
        };

        let mut registry = self.registry.lock();
        let managed_version = artifact
            .version
            .clone()
            .or_else(|| Some(manifest.shuttle_version.clone()));
        registry.mark(
            component_id,
            installed_path,
            Some(artifact.sha256.clone()),
            "managed",
            managed_version,
        );
        registry.save(&self.registry_path)?;
        self.emit_progress(component_id, artifact.size, artifact.size, "complete");
        Ok(())
    }

    fn emit_progress(&self, component_id: &str, done: u64, total: u64, phase: &str) {
        let _ = self.event_tx.send(AppEvent {
            kind: "component.install.progress".into(),
            payload: serde_json::json!({
                "component_id": component_id,
                "bytes_done": done,
                "bytes_total": total,
                "phase": phase,
            }),
        });
    }

    pub fn gowa_binary(&self) -> PathBuf {
        if let Ok(p) = std::env::var("SHUTTLE_GOWA_BIN") {
            return PathBuf::from(p);
        }
        if let Some(path) = self.registry.lock().path_for("native:gowa") {
            return path;
        }
        let bundled = self.components_root.join("native/gowa/whatsapp");
        if bundled.exists() {
            return bundled;
        }
        dev_path(&["gowa", "whatsapp"])
    }

    pub fn tdlib_path(&self) -> PathBuf {
        if let Ok(p) = std::env::var("SHUTTLE_TDLIB") {
            return PathBuf::from(p);
        }
        if let Some(path) = self.registry.lock().path_for("native:tdlib") {
            return path;
        }
        for name in tdlib_names() {
            let bundled = self.components_root.join("native/tdlib").join(name);
            if bundled.exists() {
                return bundled;
            }
        }
        dev_path(&["tdlib", tdlib_names()[0]])
    }

    pub fn signal_cli(&self) -> PathBuf {
        if let Ok(p) = std::env::var("SHUTTLE_SIGNAL_CLI") {
            return PathBuf::from(p);
        }
        if let Some(path) = self.registry.lock().path_for("native:signal-cli") {
            return path;
        }
        for rel in ["signal-cli", "signal-cli.exe", "runtime/bin/signal-cli"] {
            let candidate = self.components_root.join("native/signal").join(rel);
            if candidate.exists() {
                return candidate;
            }
        }
        dev_path(&["signal", "signal-cli"])
    }

    pub fn connector_script(&self, connector_id: &str) -> Result<PathBuf, String> {
        let script_name = format!("{connector_id}-connector.py");
        let managed = self.scripts_dir().join(&script_name);
        if managed.exists() {
            return Ok(managed);
        }
        if let Some(path) = self
            .registry
            .lock()
            .path_for(&format!("script:{connector_id}"))
        {
            if path.exists() {
                return Ok(path);
            }
        }
        let dev = dev_path(&[&script_name]);
        if dev.exists() {
            return Ok(dev);
        }
        Err(format!(
            "Connector script not found for {connector_id}. Install components first."
        ))
    }

    pub fn shuttle_ipc_script(&self) -> Option<PathBuf> {
        let managed = self.scripts_dir().join("shuttle_ipc.py");
        if managed.exists() {
            return Some(managed);
        }
        let dev = dev_path(&["shuttle_ipc.py"]);
        if dev.exists() {
            return Some(dev);
        }
        None
    }

    pub fn python_launcher(&self) -> (String, Vec<String>) {
        if let Ok(p) = std::env::var("SHUTTLE_PYTHON") {
            return (p, Vec::new());
        }
        if let Some(entry) = self.registry.lock().components.get("python-runtime") {
            if entry.source == "system" {
                let parts: Vec<&str> = entry.path.split('|').collect();
                if !parts.is_empty() {
                    let bin = parts[0].to_string();
                    let args = parts.iter().skip(1).map(|s| (*s).to_string()).collect();
                    return (bin, args);
                }
            }
            if PathBuf::from(&entry.path).exists() {
                return (entry.path.clone(), Vec::new());
            }
        }
        if let Some(path) = find_managed_python(&self.root().join("python")) {
            return (path.to_string_lossy().into_owned(), Vec::new());
        }
        if let Some((bin, args)) = detect_system_python() {
            return (bin, args);
        }
        if cfg!(windows) {
            ("py".into(), vec!["-3".into()])
        } else {
            ("python3".into(), Vec::new())
        }
    }

    pub fn pythonpath_for_connector(
        &self,
        connector_id: &str,
        script_dir: &Path,
    ) -> Option<std::ffi::OsString> {
        let mut paths = vec![script_dir.to_path_buf()];
        if let Some(ipc) = self
            .shuttle_ipc_script()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        {
            if ipc != script_dir.to_path_buf() {
                paths.push(ipc);
            }
        }
        if let Some(deps) = python_deps_dir(&self.components_root, connector_id) {
            paths.push(deps);
        }
        std::env::join_paths(paths).ok()
    }
}

fn tdlib_names() -> &'static [&'static str] {
    if cfg!(windows) {
        &["tdjson.dll", "libtdjson.dll"]
    } else if cfg!(target_os = "macos") {
        &["libtdjson.dylib", "tdjson.dylib"]
    } else {
        &["libtdjson.so", "tdjson.so"]
    }
}

fn dev_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../connectors")
}

fn dev_path(parts: &[&str]) -> PathBuf {
    let mut path = dev_root();
    for part in parts {
        path.push(part);
    }
    path
}

fn dev_component_exists(component_id: &str) -> bool {
    if !cfg!(debug_assertions) {
        return false;
    }
    match component_id {
        "python-runtime" => {
            detect_system_python().is_some() || dev_path(&["python-runtime/current"]).exists()
        }
        "native:gowa" => dev_path(&["gowa", "whatsapp"]).exists(),
        "native:tdlib" => tdlib_names()
            .iter()
            .any(|n| dev_path(&["tdlib", n]).exists()),
        "native:signal-cli" => dev_path(&["signal", "signal-cli"]).exists(),
        "python:deps:messenger" | "python:deps:instagram" => {
            dev_path(&["python-runtime/current"]).exists()
        }
        id if id.starts_with("script:") => {
            let name = id.strip_prefix("script:").unwrap_or(id);
            let path = if name == "shuttle_ipc" {
                dev_root().join("shuttle_ipc.py")
            } else {
                dev_root().join(format!("{name}-connector.py"))
            };
            path.exists()
        }
        _ => false,
    }
}

fn dev_has_connector(connector_id: &str) -> bool {
    if !cfg!(debug_assertions) {
        return false;
    }
    ComponentManager::connector_component_ids(connector_id)
        .iter()
        .all(|id| dev_component_exists(id))
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manager(dir: &Path) -> ComponentManager {
        let (event_tx, _) = broadcast::channel(16);
        ComponentManager::new(dir, event_tx)
    }

    fn write_registry(dir: &Path, id: &str, version: Option<&str>, path: &Path) {
        std::fs::create_dir_all(dir.join("components")).unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"installed").unwrap();
        let entry = serde_json::json!({
            "id": id,
            "version": version,
            "sha256": null,
            "path": path.to_string_lossy(),
            "source": "managed",
        });
        let raw = serde_json::json!({ "components": { id: entry } });
        std::fs::write(
            dir.join("components").join("installed.json"),
            serde_json::to_string(&raw).unwrap(),
        )
        .unwrap();
    }

    fn row<'a>(overview: &'a ComponentsOverview, connector: &str) -> &'a ConnectorComponentStatus {
        overview
            .connectors
            .iter()
            .find(|c| c.connector_id == connector)
            .expect("connector row")
    }

    #[test]
    fn overview_lists_every_network_with_its_components() {
        let dir = tempfile::tempdir().unwrap();
        let overview = tauri::async_runtime::block_on(manager(dir.path()).components_overview());
        let ids: Vec<&str> = overview
            .connectors
            .iter()
            .map(|c| c.connector_id.as_str())
            .collect();
        assert_eq!(
            ids,
            vec!["whatsapp", "telegram", "signal", "messenger", "instagram", "matrix", "email"]
        );
        let telegram: Vec<&str> = row(&overview, "telegram")
            .components
            .iter()
            .map(|c| c.id.as_str())
            .collect();
        assert_eq!(telegram, vec!["script:telegram", "native:tdlib"]);
        assert_eq!(row(&overview, "telegram").connector_label, "Telegram");
    }

    #[test]
    fn an_unreachable_manifest_leaves_sizes_unknown_without_failing() {
        let dir = tempfile::tempdir().unwrap();
        let overview = tauri::async_runtime::block_on(manager(dir.path()).components_overview());
        assert!(!overview.manifest_available);
        assert!(overview.manifest_version.is_none());
        assert_eq!(overview.expected_version, env!("CARGO_PKG_VERSION"));
        for connector in &overview.connectors {
            for component in &connector.components {
                assert!(component.size.is_none(), "{} has a size", component.id);
            }
        }
    }

    #[test]
    fn an_installed_component_reports_its_recorded_version() {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("scripts").join("whatsapp-connector.py");
        write_registry(dir.path(), "script:whatsapp", Some("0.6.1"), &script);
        let overview = tauri::async_runtime::block_on(manager(dir.path()).components_overview());
        let whatsapp = row(&overview, "whatsapp");
        let installed = whatsapp
            .components
            .iter()
            .find(|c| c.id == "script:whatsapp")
            .unwrap();
        assert!(installed.installed);
        assert_eq!(installed.version.as_deref(), Some("0.6.1"));
        assert_eq!(installed.source.as_deref(), Some("managed"));
    }

    #[test]
    fn a_component_installed_before_versions_were_recorded_still_reads_as_installed() {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("scripts").join("telegram-connector.py");
        write_registry(dir.path(), "script:telegram", None, &script);
        let overview = tauri::async_runtime::block_on(manager(dir.path()).components_overview());
        let telegram = row(&overview, "telegram")
            .components
            .iter()
            .find(|c| c.id == "script:telegram")
            .unwrap();
        assert!(telegram.installed);
        assert!(telegram.version.is_none());
    }
}
