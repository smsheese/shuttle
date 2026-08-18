//! Telemetry and build-channel configuration.
//!
//! Resolution order (first non-empty wins):
//! 1. Process environment (including values loaded from `.env`)
//! 2. Values baked in at compile time (`option_env!` / `cargo:rustc-env`)

/// Load `.env` from the repo root, `shuttle-app/`, or the current directory.
/// Existing process env vars are not overwritten.
pub fn load_dotenv() {
    for path in candidate_env_files() {
        if path.is_file() {
            let _ = dotenvy::from_path(&path);
            tracing::debug!("loaded env file {}", path.display());
            break;
        }
    }
}

fn candidate_env_files() -> Vec<std::path::PathBuf> {
    let mut paths = Vec::new();
    if let Ok(cwd) = std::env::current_dir() {
        paths.push(cwd.join(".env"));
        paths.push(cwd.join("shuttle-app").join(".env"));
    }
    let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    paths.push(manifest.join(".env"));
    paths.push(manifest.join("..").join(".env"));
    paths.push(manifest.join("..").join("..").join(".env"));
    paths
}

pub fn sentry_dsn() -> Option<String> {
    runtime_or_embedded("SENTRY_DSN", option_env!("SHUTTLE_SENTRY_DSN"))
}

pub fn posthog_api_key() -> Option<String> {
    runtime_or_embedded("POSTHOG_API_KEY", option_env!("SHUTTLE_POSTHOG_API_KEY"))
}

pub fn posthog_host() -> String {
    runtime_or_embedded("POSTHOG_HOST", option_env!("SHUTTLE_POSTHOG_HOST"))
        .unwrap_or_else(|| "https://us.i.posthog.com".into())
}

pub fn build_channel() -> String {
    let raw = runtime_or_embedded(
        "SHUTTLE_BUILD_CHANNEL",
        option_env!("SHUTTLE_EMBEDDED_BUILD_CHANNEL"),
    )
    .unwrap_or_else(|| "testing".into());
    normalize_environment(&raw)
}

/// Sentry `environment` and PostHog `$environment`: `testing` or `production`.
pub fn telemetry_environment() -> String {
    build_channel()
}

fn normalize_environment(value: &str) -> String {
    match value.trim().to_ascii_lowercase().as_str() {
        "prod" | "production" => "production".into(),
        _ => "testing".into(),
    }
}

pub fn git_commit() -> Option<String> {
    runtime_or_embedded("SHUTTLE_GIT_COMMIT", option_env!("SHUTTLE_EMBEDDED_GIT_COMMIT"))
}

fn runtime_or_embedded(key: &str, embedded: Option<&str>) -> Option<String> {
    if let Ok(value) = std::env::var(key) {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    embedded
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_runtime_falls_through() {
        // Safety: tests run serially enough for this crate's env helper.
        std::env::remove_var("SENTRY_DSN");
        assert!(runtime_or_embedded("SENTRY_DSN", None).is_none());
        assert_eq!(
            runtime_or_embedded("SENTRY_DSN", Some("https://example.ingest.sentry.io/1")).unwrap(),
            "https://example.ingest.sentry.io/1"
        );
        assert_eq!(normalize_environment("prod"), "production");
        assert_eq!(normalize_environment("dev"), "testing");
    }
}
