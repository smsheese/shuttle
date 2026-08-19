use std::path::PathBuf;
use std::process::Command;

const MIN_PYTHON: (u32, u32) = (3, 12);

pub fn system_python_candidates() -> Vec<(String, Vec<String>)> {
    if cfg!(windows) {
        vec![("py".into(), vec!["-3".into()])]
    } else {
        vec![
            ("python3".into(), Vec::new()),
            ("python".into(), Vec::new()),
        ]
    }
}

pub fn detect_system_python() -> Option<(String, Vec<String>)> {
    for (bin, args) in system_python_candidates() {
        if !python_meets_minimum(&bin, &args) {
            continue;
        }
        if !python_import_probe(&bin, &args) {
            continue;
        }
        return Some((bin, args));
    }
    None
}

fn python_meets_minimum(bin: &str, extra_args: &[String]) -> bool {
    let mut cmd = Command::new(bin);
    for arg in extra_args {
        cmd.arg(arg);
    }
    let output = match cmd.args(["-c", "import sys; print(f'{sys.version_info[0]}.{sys.version_info[1]}')"]).output()
    {
        Ok(o) if o.status.success() => o,
        _ => return false,
    };
    let version = String::from_utf8_lossy(&output.stdout);
    let version = version.trim();
    let Some((major, minor)) = parse_version(version) else {
        return false;
    };
    major > MIN_PYTHON.0 || (major == MIN_PYTHON.0 && minor >= MIN_PYTHON.1)
}

fn python_import_probe(bin: &str, extra_args: &[String]) -> bool {
    let mut cmd = Command::new(bin);
    for arg in extra_args {
        cmd.arg(arg);
    }
    cmd.args(["-c", "import ssl, sqlite3"]).status().map(|s| s.success()).unwrap_or(false)
}

fn parse_version(raw: &str) -> Option<(u32, u32)> {
    let mut parts = raw.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    Some((major, minor))
}

pub fn managed_python_candidates(root: &PathBuf) -> Vec<PathBuf> {
    if cfg!(windows) {
        vec![
            root.join("python.exe"),
            root.join("python").join("python.exe"),
            root.join("python").join("install").join("python.exe"),
        ]
    } else {
        vec![
            root.join("python").join("bin").join("python3"),
            root.join("python").join("bin").join("python"),
            root.join("bin").join("python3"),
        ]
    }
}

pub fn find_managed_python(root: &PathBuf) -> Option<PathBuf> {
    managed_python_candidates(root).into_iter().find(|p| p.exists())
}

pub fn python_deps_dir(components_root: &PathBuf, connector_id: &str) -> Option<PathBuf> {
    let dep = match connector_id {
        "messenger" => Some("messenger"),
        "instagram" => Some("instagram"),
        _ => None,
    }?;
    let path = components_root.join("python-deps").join(dep);
    if path.exists() { Some(path) } else { None }
}
