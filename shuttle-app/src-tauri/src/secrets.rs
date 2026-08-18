//! OS-backed credential storage. Passwords never go in SQLite or the frontend.

use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

const SERVICE: &str = "com.shuttle.app";

#[cfg(unix)]
fn restrict_file(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
}

#[cfg(not(unix))]
fn restrict_file(_path: &Path) {}

fn fallback_path(data_dir: &Path, account_id: &str) -> PathBuf {
    let dir = data_dir.join("secrets");
    let _ = fs::create_dir_all(&dir);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&dir, fs::Permissions::from_mode(0o700));
    }
    dir.join(format!("{account_id}.json"))
}

fn persistable(credentials: &Value) -> Value {
    let mut out = serde_json::Map::new();
    if let Some(obj) = credentials.as_object() {
        for (k, v) in obj {
            if matches!(
                k.as_str(),
                "code" | "verification_code" | "captcha" | "two_factor_code"
            ) {
                continue;
            }
            if v.is_null() {
                continue;
            }
            if v.as_str().is_some_and(|s| s.is_empty()) {
                continue;
            }
            out.insert(k.clone(), v.clone());
        }
    }
    Value::Object(out)
}

fn merge(base: Value, overlay: &Value) -> Value {
    let mut map = base.as_object().cloned().unwrap_or_default();
    if let Some(obj) = overlay.as_object() {
        for (k, v) in obj {
            if v.is_null() || v.as_str().is_some_and(str::is_empty) {
                continue;
            }
            map.insert(k.clone(), v.clone());
        }
    }
    Value::Object(map)
}

pub fn load(data_dir: &Path, account_id: &str) -> Value {
    if let Ok(entry) = keyring::Entry::new(SERVICE, account_id) {
        if let Ok(raw) = entry.get_password() {
            if let Ok(v) = serde_json::from_str::<Value>(&raw) {
                return v;
            }
        }
    }
    let path = fallback_path(data_dir, account_id);
    fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(Value::Object(Default::default()))
}

pub fn save(data_dir: &Path, account_id: &str, credentials: &Value) -> Result<(), String> {
    let merged = persistable(&merge(load(data_dir, account_id), credentials));
    let raw = serde_json::to_string(&merged).map_err(|e| e.to_string())?;
    match keyring::Entry::new(SERVICE, account_id) {
        Ok(entry) => match entry.set_password(&raw) {
            Ok(()) => {
                let _ = fs::remove_file(fallback_path(data_dir, account_id));
                return Ok(());
            }
            Err(err) => tracing::warn!("OS keyring unavailable ({err}); using 0600 file fallback"),
        },
        Err(err) => tracing::warn!("OS keyring unavailable ({err}); using 0600 file fallback"),
    }
    let path = fallback_path(data_dir, account_id);
    fs::write(&path, raw).map_err(|e| e.to_string())?;
    restrict_file(&path);
    Ok(())
}

pub fn delete(data_dir: &Path, account_id: &str) {
    if let Ok(entry) = keyring::Entry::new(SERVICE, account_id) {
        let _ = entry.delete_credential();
    }
    let _ = fs::remove_file(fallback_path(data_dir, account_id));
}
