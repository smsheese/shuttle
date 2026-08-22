//! Ensure at most one OS process per connector_id (kill orphans from prior Shuttle runs).

use std::fs;
use std::path::{Path, PathBuf};
#[cfg(any(windows, target_os = "macos"))]
use std::process::Command;

pub fn lock_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("locks")
}

pub fn pid_path(data_dir: &Path, connector_id: &str) -> PathBuf {
    lock_dir(data_dir).join(format!("{connector_id}.pid"))
}

/// Kill any leftover sidecar recorded in the pidfile, then clear the file.
pub fn reclaim(data_dir: &Path, connector_id: &str) {
    let path = pid_path(data_dir, connector_id);
    let Ok(raw) = fs::read_to_string(&path) else {
        return;
    };
    let pid = match raw.trim().parse::<i32>() {
        Ok(p) if p > 1 => p,
        _ => {
            let _ = fs::remove_file(&path);
            return;
        }
    };
    if pid_alive(pid) && cmdline_looks_like_connector(pid, connector_id) {
        tracing::warn!(
            "killing orphaned {connector_id} connector pid {pid} from previous Shuttle run"
        );
        kill_pid(pid);
    }
    let _ = fs::remove_file(&path);
}

pub fn write_pid(data_dir: &Path, connector_id: &str, pid: u32) {
    let dir = lock_dir(data_dir);
    let _ = fs::create_dir_all(&dir);
    let path = pid_path(data_dir, connector_id);
    if let Err(e) = fs::write(&path, format!("{pid}\n")) {
        tracing::warn!("failed to write connector pidfile {}: {e}", path.display());
    }
}

pub fn clear_pid(data_dir: &Path, connector_id: &str) {
    let _ = fs::remove_file(pid_path(data_dir, connector_id));
}

/// Stop the Shuttle-managed GOWA singleton (if any) recorded under data_dir/gowa.
pub fn stop_gowa(data_dir: &Path) {
    let state_path = data_dir.join("gowa").join("runtime.json");
    let Ok(raw) = fs::read_to_string(&state_path) else {
        return;
    };
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(&raw) {
        if let Some(pid) = value.get("pid").and_then(|v| v.as_i64()).map(|p| p as i32) {
            if pid > 1 && pid_alive(pid) {
                tracing::info!("stopping GOWA pid {pid} with Shuttle shutdown");
                kill_pid(pid);
            }
        }
    }
    let _ = fs::remove_file(&state_path);
}

#[cfg(unix)]
fn pid_alive(pid: i32) -> bool {
    unsafe { libc::kill(pid, 0) == 0 }
}

#[cfg(windows)]
fn pid_alive(pid: i32) -> bool {
    Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/NH"])
        .output()
        .map(|o| {
            let out = String::from_utf8_lossy(&o.stdout);
            out.contains(&pid.to_string()) && !out.to_ascii_lowercase().contains("no tasks")
        })
        .unwrap_or(false)
}

#[cfg(not(any(unix, windows)))]
fn pid_alive(pid: i32) -> bool {
    pid > 1
}

#[cfg(target_os = "linux")]
fn cmdline_looks_like_connector(pid: i32, connector_id: &str) -> bool {
    let path = format!("/proc/{pid}/cmdline");
    let Ok(bytes) = fs::read(&path) else {
        return false;
    };
    cmdline_matches(&String::from_utf8_lossy(&bytes).replace('\0', " "), connector_id)
}

#[cfg(target_os = "macos")]
fn cmdline_looks_like_connector(pid: i32, connector_id: &str) -> bool {
    let Ok(output) = Command::new("ps")
        .args(["-ww", "-p", &pid.to_string(), "-o", "command="])
        .output()
    else {
        return false;
    };
    cmdline_matches(&String::from_utf8_lossy(&output.stdout), connector_id)
}

#[cfg(windows)]
fn cmdline_looks_like_connector(pid: i32, connector_id: &str) -> bool {
    let script = format!(
        "(Get-CimInstance Win32_Process -Filter \"ProcessId={pid}\" -ErrorAction SilentlyContinue).CommandLine"
    );
    let Ok(output) = Command::new("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &script,
        ])
        .output()
    else {
        return false;
    };
    cmdline_matches(&String::from_utf8_lossy(&output.stdout), connector_id)
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn cmdline_looks_like_connector(_pid: i32, _connector_id: &str) -> bool {
    true
}

fn cmdline_matches(cmd: &str, connector_id: &str) -> bool {
    let needle = format!("{connector_id}-connector");
    cmd.contains(&needle) || (cmd.contains("connectors/") && cmd.contains(connector_id))
}

#[cfg(unix)]
fn kill_pid(pid: i32) {
    unsafe {
        let _ = libc::kill(pid, libc::SIGTERM);
    }
    for _ in 0..20 {
        if !pid_alive(pid) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    unsafe {
        let _ = libc::kill(pid, libc::SIGKILL);
    }
}

#[cfg(windows)]
fn kill_pid(pid: i32) {
    let _ = Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .output();
}

#[cfg(not(any(unix, windows)))]
fn kill_pid(_pid: i32) {}
