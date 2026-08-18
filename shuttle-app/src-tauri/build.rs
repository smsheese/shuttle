fn main() {
    load_dotenv_for_build();
    emit_embedded("SENTRY_DSN", "SHUTTLE_SENTRY_DSN");
    emit_embedded("POSTHOG_API_KEY", "SHUTTLE_POSTHOG_API_KEY");
    emit_embedded("POSTHOG_HOST", "SHUTTLE_POSTHOG_HOST");
    emit_embedded("SHUTTLE_BUILD_CHANNEL", "SHUTTLE_EMBEDDED_BUILD_CHANNEL");
    emit_embedded("SHUTTLE_GIT_COMMIT", "SHUTTLE_EMBEDDED_GIT_COMMIT");
    println!("cargo:rerun-if-env-changed=SENTRY_DSN");
    println!("cargo:rerun-if-env-changed=POSTHOG_API_KEY");
    println!("cargo:rerun-if-env-changed=POSTHOG_HOST");
    println!("cargo:rerun-if-env-changed=SHUTTLE_BUILD_CHANNEL");
    println!("cargo:rerun-if-env-changed=SHUTTLE_GIT_COMMIT");
    tauri_build::build()
}

fn load_dotenv_for_build() {
    let manifest = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    for path in [
        manifest.join(".env"),
        manifest.join("..").join(".env"),
        manifest.join("..").join("..").join(".env"),
    ] {
        if path.is_file() {
            println!("cargo:rerun-if-changed={}", path.display());
            let _ = dotenvy::from_path(&path);
            break;
        }
    }
}

fn emit_embedded(from: &str, into: &str) {
    if let Ok(value) = std::env::var(from) {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            println!("cargo:rustc-env={into}={trimmed}");
        }
    }
}
