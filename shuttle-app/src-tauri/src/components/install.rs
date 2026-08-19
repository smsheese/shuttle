use crate::components::manifest::ManifestArtifact;
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{copy, Read, Write};
use std::path::{Path, PathBuf};
use tar::Archive;

pub struct ProgressUpdate {
    pub bytes_done: u64,
    pub bytes_total: u64,
    pub phase: &'static str,
}

pub fn sha256_file(path: &Path) -> Result<String, String> {
    let mut file = File::open(path).map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 8192];
    loop {
        let read = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

pub async fn download_file(
    client: &reqwest::Client,
    artifact: &ManifestArtifact,
    dest: &Path,
    mut on_progress: impl FnMut(ProgressUpdate),
) -> Result<(), String> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let response = client
        .get(&artifact.url)
        .send()
        .await
        .map_err(|e| format!("Download failed: {e}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "Download failed: HTTP {}",
            response.status().as_u16()
        ));
    }
    let total = if artifact.size > 0 {
        artifact.size
    } else {
        response.content_length().unwrap_or(0)
    };
    let mut file = File::create(dest).map_err(|e| e.to_string())?;
    let mut downloaded = 0u64;
    on_progress(ProgressUpdate {
        bytes_done: 0,
        bytes_total: total,
        phase: "downloading",
    });
    let mut stream = response.bytes_stream();
    use futures_util::StreamExt;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("Download stream error: {e}"))?;
        file.write_all(&chunk).map_err(|e| e.to_string())?;
        downloaded += chunk.len() as u64;
        on_progress(ProgressUpdate {
            bytes_done: downloaded,
            bytes_total: total,
            phase: "downloading",
        });
    }
    Ok(())
}

pub fn verify_sha256(path: &Path, expected: &str) -> Result<(), String> {
    let actual = sha256_file(path)?;
    if actual.eq_ignore_ascii_case(expected.trim()) {
        Ok(())
    } else {
        Err(format!(
            "Checksum mismatch for {} (expected {expected}, got {actual})",
            path.display()
        ))
    }
}

pub fn extract_archive(archive_path: &Path, dest: &Path) -> Result<(), String> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let name = archive_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_lowercase();
    if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
        extract_tar_gz(archive_path, dest)
    } else if name.ends_with(".zip") {
        extract_zip(archive_path, dest)
    } else if name.ends_with(".py") {
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        fs::copy(archive_path, dest).map_err(|e| e.to_string())?;
        Ok(())
    } else {
        Err(format!("Unsupported archive format: {}", archive_path.display()))
    }
}

fn extract_tar_gz(archive_path: &Path, dest: &Path) -> Result<(), String> {
    fs::create_dir_all(dest).map_err(|e| e.to_string())?;
    let file = File::open(archive_path).map_err(|e| e.to_string())?;
    let decoder = flate2::read::GzDecoder::new(file);
    let mut archive = Archive::new(decoder);
    archive.unpack(dest).map_err(|e| e.to_string())
}

fn extract_zip(archive_path: &Path, dest: &Path) -> Result<(), String> {
    fs::create_dir_all(dest).map_err(|e| e.to_string())?;
    let file = File::open(archive_path).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
    for i in 0..archive.len() {
        let mut file = archive.by_index(i).map_err(|e| e.to_string())?;
        let outpath = match file.enclosed_name() {
            Some(path) => dest.join(path),
            None => continue,
        };
        if file.name().ends_with('/') {
            fs::create_dir_all(&outpath).map_err(|e| e.to_string())?;
        } else {
            if let Some(parent) = outpath.parent() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            let mut outfile = File::create(&outpath).map_err(|e| e.to_string())?;
            copy(&mut file, &mut outfile).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

pub fn install_path_for_component(components_root: &Path, component_id: &str) -> PathBuf {
    if component_id.starts_with("script:") {
        let name = component_id.strip_prefix("script:").unwrap_or(component_id);
        let filename = if name == "shuttle_ipc" {
            "shuttle_ipc.py".to_string()
        } else {
            format!("{name}-connector.py")
        };
        return components_root.join("scripts").join(filename);
    }
    if component_id.starts_with("native:") {
        let name = component_id.strip_prefix("native:").unwrap_or(component_id);
        return match name {
            "gowa" => components_root.join("native/gowa"),
            "tdlib" => components_root.join("native/tdlib"),
            "signal-cli" => components_root.join("native/signal"),
            _ => components_root.join("native").join(name),
        };
    }
    if component_id == "python-runtime" {
        return components_root.join("python");
    }
    if let Some(dep) = component_id.strip_prefix("python:deps:") {
        return components_root.join("python-deps").join(dep);
    }
    components_root.join("artifacts").join(component_id)
}

pub fn finalize_native_layout(component_id: &str, dest: &Path) -> Result<PathBuf, String> {
    if component_id == "native:gowa" {
        return finalize_binary(dest, &["whatsapp", "whatsapp.exe"]);
    }
    if component_id == "native:signal-cli" {
        for rel in ["signal-cli", "signal-cli.exe", "runtime/bin/signal-cli"] {
            let candidate = find_file_recursive(dest, rel);
            if let Some(path) = candidate {
                return Ok(path);
            }
        }
        return Err("signal-cli binary not found in archive".into());
    }
    if component_id == "native:tdlib" {
        for name in ["libtdjson.so", "tdjson.so", "libtdjson.dylib", "tdjson.dll"] {
            if let Some(path) = find_file_recursive(dest, name) {
                return Ok(path);
            }
        }
        return Err("tdjson library not found in archive".into());
    }
    Ok(dest.to_path_buf())
}

fn finalize_binary(dest: &Path, names: &[&str]) -> Result<PathBuf, String> {
    for name in names {
        if let Some(path) = find_file_recursive(dest, name) {
            return Ok(path);
        }
    }
    Err(format!("Binary not found under {}", dest.display()))
}

fn find_file_recursive(root: &Path, filename: &str) -> Option<PathBuf> {
    if root.is_file() && root.file_name()?.to_str()? == filename {
        return Some(root.to_path_buf());
    }
    let entries = fs::read_dir(root).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(found) = find_file_recursive(&path, filename) {
                return Some(found);
            }
        } else if path.file_name()?.to_str()? == filename {
            return Some(path);
        }
    }
    None
}

pub fn normalize_python_runtime(root: &Path) -> Result<(), String> {
    let install_dir = root.join("install");
    if install_dir.is_dir() {
        for entry in fs::read_dir(&install_dir).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let target = root.join(entry.file_name());
            if target.exists() {
                if target.is_dir() {
                    fs::remove_dir_all(&target).ok();
                } else {
                    fs::remove_file(&target).ok();
                }
            }
            fs::rename(entry.path(), &target).map_err(|e| e.to_string())?;
        }
        fs::remove_dir_all(&install_dir).ok();
    }
    Ok(())
}
