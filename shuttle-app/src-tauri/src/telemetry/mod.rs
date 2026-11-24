mod events;
mod performance;
mod privacy;
mod system;

pub use performance::PerformanceSampler;
use std::time::Instant;

use crate::config::ConfigStore;
use crate::db::Database;
use events::{validate_app_event, validate_connector_telemetry};
use performance::PerformanceSnapshot;
use serde_json::{json, Map, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use system::{global_context, merge_context};

const POSTHOG_BATCH_INTERVAL: Duration = Duration::from_secs(30);
const MAX_POSTHOG_QUEUE: usize = 256;

// The SDK's panic hook is process-wide. Consent is checked again on its worker
// immediately before sending; raw panic payloads and stack traces never leave it.
static CRASH_CONSENT: AtomicBool = AtomicBool::new(false);
static ERROR_CLIENT: std::sync::Once = std::sync::Once::new();

fn error_category(message: &str) -> &'static str {
    match message {
        "frontend_exception" => "frontend_exception",
        "frontend_unhandled_rejection" => "frontend_unhandled_rejection",
        _ => "application_error",
    }
}

fn safe_exception(
    event: posthog_rs::Event,
    context: &Map<String, Value>,
    consent: bool,
) -> Option<posthog_rs::Event> {
    if !consent || event.event_name() != "$exception" {
        return None;
    }
    let category = event
        .properties()
        .get("shuttle_error_category")
        .and_then(Value::as_str)
        .map(error_category)
        .unwrap_or("rust_panic");
    let mut safe = posthog_rs::Event::new_anon("$exception");
    safe.insert_prop(
        "$exception_list",
        json!([{ "type": category, "value": category, "mechanism": { "type": if category == "rust_panic" { "panic" } else { "generic" }, "handled": category != "rust_panic", "synthetic": false } }]),
    )
    .ok()?;
    // Only compiler source paths inside our crate are safe to retain.
    if category == "rust_panic" {
        if let Some(file) = event
            .properties()
            .get("$exception_panic_file")
            .and_then(Value::as_str)
            .map(|p| p.replace('\\', "/"))
            .filter(|p| {
                p.starts_with("src/")
                    && !p.contains("..")
                    && p.bytes()
                        .all(|c| c.is_ascii_alphanumeric() || b"/_-.".contains(&c))
            })
        {
            if let Some(line) = event
                .properties()
                .get("$exception_panic_line")
                .and_then(Value::as_u64)
            {
                safe.insert_prop("source_file", &file).ok()?;
                safe.insert_prop("source_line", line).ok()?;
                safe.insert_prop(
                    "$exception_fingerprint",
                    format!("rust_panic:{file}:{line}"),
                )
                .ok()?;
            }
        }
    }
    safe.insert_prop(
        "$exception_level",
        if category == "rust_panic" {
            "fatal"
        } else {
            "error"
        },
    )
    .ok()?;
    safe.insert_prop("$process_person_profile", false).ok()?;
    for (key, value) in context {
        safe.insert_prop(key, value).ok()?;
    }
    Some(safe)
}

fn init_posthog_errors(context: &Map<String, Value>) {
    let Some(api_key) = crate::env::posthog_api_key() else {
        return;
    };
    let context = context.clone();
    ERROR_CLIENT.call_once(|| {
        let error_options = posthog_rs::ErrorTrackingOptionsBuilder::default()
            .capture_panics(true)
            .capture_stacktrace(false)
            .build()
            .expect("error options");
        let mut builder = posthog_rs::ClientOptionsBuilder::default();
        builder
            .api_key(api_key)
            .host(crate::env::posthog_host())
            .request_timeout_seconds(2)
            .error_tracking(error_options)
            .before_send(move |event| {
                safe_exception(event, &context, CRASH_CONSENT.load(Ordering::SeqCst))
            });
        if let Ok(options) = builder.build() {
            // The SDK uses a blocking HTTP runtime, which must be created outside
            // Tauri's async runtime when the user changes consent in Settings.
            let _ = std::thread::spawn(move || posthog_rs::init_global(options)).join();
        }
    });
}

struct PostHogEvent {
    event: String,
    properties: Map<String, Value>,
}

pub struct TelemetryManager {
    installation_id: String,
    config: Arc<ConfigStore>,
    db: Arc<Database>,
    global_context: Map<String, Value>,
    posthog_queue: parking_lot::Mutex<Vec<PostHogEvent>>,
    performance: Arc<PerformanceSampler>,
    http: reqwest::Client,
}

impl TelemetryManager {
    pub fn new(
        app_version: &str,
        installation_id: String,
        config: Arc<ConfigStore>,
        db: Arc<Database>,
    ) -> Arc<Self> {
        let manager = Arc::new(Self {
            installation_id,
            config: config.clone(),
            db,
            global_context: global_context(app_version),
            posthog_queue: parking_lot::Mutex::new(Vec::new()),
            performance: Arc::new(PerformanceSampler::new()),
            http: reqwest::Client::new(),
        });
        manager.apply_consent();
        manager
    }

    pub fn sampler(&self) -> Arc<PerformanceSampler> {
        self.performance.clone()
    }

    pub fn installation_id(&self) -> &str {
        &self.installation_id
    }

    pub fn apply_consent(&self) {
        let privacy = self.config.get().privacy;
        CRASH_CONSENT.store(privacy.crash_reports, Ordering::SeqCst);
        if privacy.crash_reports {
            init_posthog_errors(&self.global_context);
        }
        if !privacy.usage_diagnostics {
            self.posthog_queue.lock().clear();
        }
    }

    pub fn track(&self, event: &str, props: Map<String, Value>) {
        if !self.config.get().privacy.usage_diagnostics {
            return;
        }
        let Some(validated) = validate_app_event(event, &props) else {
            return;
        };
        let properties = merge_context(&self.global_context, &validated);
        let mut queue = self.posthog_queue.lock();
        if queue.len() >= MAX_POSTHOG_QUEUE {
            queue.remove(0);
        }
        queue.push(PostHogEvent {
            event: event.to_string(),
            properties,
        });
    }

    pub fn error(&self, message: &str, _context: Map<String, Value>) {
        let privacy = self.config.get().privacy;
        if privacy.crash_reports {
            let category = error_category(message);
            let error = std::io::Error::other(category);
            let options = posthog_rs::CaptureExceptionOptions::new()
                .property("shuttle_error_category", category)
                .expect("static category");
            let _ = posthog_rs::capture_exception_with(&error, options);
        }
        if privacy.usage_diagnostics {
            let mut props = Map::new();
            props.insert("error_category".into(), json!(error_category(message)));
            self.track("command_failed", props);
        }
    }

    pub fn track_operation(&self, operation: &str, props: Map<String, Value>) {
        let mut map = props;
        map.insert("operation".into(), json!(operation));
        self.track("performance_snapshot", map);
    }

    pub fn handle_connector_payload(&self, payload: Value) {
        let Some(obj) = payload.as_object() else {
            return;
        };
        if let Some(validated) = validate_connector_telemetry(obj) {
            if let Some(name) = validated
                .get("event")
                .and_then(|v| v.as_str())
                .map(str::to_string)
            {
                self.track(&name, validated);
            }
        }
    }

    pub fn time(&self, operation: &str, started: Instant) {
        if self.config.get().privacy.usage_diagnostics {
            self.performance.record_timing(operation, started.elapsed());
        }
    }

    pub fn set_foreground(&self, foreground: bool) {
        self.sampler().set_foreground(foreground);
    }

    pub fn emit_database_initialized(&self) {
        let mut props = Map::new();
        self.insert_data_profile(&mut props);
        self.track("database_initialized", props);
    }

    pub fn spawn_background_tasks(self: &Arc<Self>) {
        let weak = Arc::downgrade(self);
        tauri::async_runtime::spawn(async move {
            let mut batch_tick = tokio::time::interval(POSTHOG_BATCH_INTERVAL);
            loop {
                let sample_wait = weak
                    .upgrade()
                    .map(|mgr| mgr.sampler().sample_interval())
                    .unwrap_or(Duration::from_secs(60));
                tokio::select! {
                    _ = tokio::time::sleep(sample_wait) => {
                        if let Some(mgr) = weak.upgrade() {
                            if mgr.config.get().privacy.usage_diagnostics {
                                let perf = mgr.sampler();
                                perf.record_sample();
                                if let Some(snapshot) = perf.maybe_snapshot() {
                                    mgr.track_performance_snapshot(snapshot);
                                }
                            }
                        }
                    }
                    _ = batch_tick.tick() => {
                        if let Some(mgr) = weak.upgrade() {
                            mgr.flush_posthog().await;
                        }
                    }
                }
            }
        });
    }

    /// Buckets only, so a timing or CPU number can be read against the load that produced it.
    fn insert_data_profile(&self, props: &mut Map<String, Value>) {
        let size = std::fs::metadata(self.db.data_dir().join("app.sqlite"))
            .map(|m| m.len())
            .unwrap_or(0);
        props.insert("database_size_bucket".into(), json!(system::bucket_bytes(size)));
        if let Ok(profile) = self.db.data_profile() {
            props.insert("accounts_total".into(), json!(system::bucket_accounts(profile.accounts)));
            props.insert(
                "conversation_count_bucket".into(),
                json!(system::bucket_large_count(profile.conversations)),
            );
            props.insert(
                "message_count_bucket".into(),
                json!(system::bucket_large_count(profile.messages)),
            );
            props.insert(
                "largest_account_message_bucket".into(),
                json!(system::bucket_large_count(profile.largest_account_messages)),
            );
        }
    }

    fn track_performance_snapshot(&self, snapshot: PerformanceSnapshot) {
        let mut props = Map::new();
        props.insert("foreground".into(), json!(snapshot.foreground));
        props.insert(
            "sample_count".into(),
            json!(system::bucket_count(snapshot.sample_count)),
        );
        props.insert("cpu_avg".into(), json!(snapshot.cpu_avg));
        props.insert("cpu_p95".into(), json!(snapshot.cpu_p95));
        props.insert(
            "memory_avg_mb".into(),
            json!(snapshot.memory_avg_mb.round() as u64),
        );
        props.insert(
            "memory_p95_mb".into(),
            json!(snapshot.memory_p95_mb.round() as u64),
        );
        props.insert("sidecar_cpu_avg".into(), json!(snapshot.sidecar_cpu_avg));
        props.insert(
            "sidecar_memory_avg_mb".into(),
            json!(snapshot.sidecar_memory_avg_mb.round() as u64),
        );
        props.insert(
            "sidecar_memory_p95_mb".into(),
            json!(snapshot.sidecar_memory_p95_mb.round() as u64),
        );
        props.insert("sidecar_count".into(), json!(snapshot.sidecar_count));
        self.insert_data_profile(&mut props);
        self.track("performance_snapshot", props);
        for timing in self.performance.drain_timings() {
            let mut t = Map::new();
            t.insert("operation".into(), json!(timing.operation));
            t.insert("count".into(), json!(timing.count));
            t.insert("p50_ms".into(), json!(timing.p50_ms));
            t.insert("p95_ms".into(), json!(timing.p95_ms));
            t.insert("max_ms".into(), json!(timing.max_ms));
            self.insert_data_profile(&mut t);
            self.track("operation_timing", t);
        }
    }

    async fn flush_posthog(&self) {
        if !self.config.get().privacy.usage_diagnostics {
            return;
        }
        let Some(api_key) = crate::env::posthog_api_key() else {
            return;
        };
        let host = crate::env::posthog_host();
        let batch: Vec<PostHogEvent> = {
            let mut queue = self.posthog_queue.lock();
            if queue.is_empty() {
                return;
            }
            queue.drain(..).collect()
        };
        let events: Vec<Value> = batch
            .into_iter()
            .map(|item| {
                let mut properties = item.properties;
                let environment = crate::env::telemetry_environment();
                properties.insert("environment".into(), json!(environment.clone()));
                properties.insert("$environment".into(), json!(environment));
                json!({
                    "event": item.event,
                    "distinct_id": self.installation_id(),
                    "properties": properties,
                })
            })
            .collect();
        let body = json!({ "api_key": api_key, "batch": events });
        let url = format!("{host}/batch/");
        let _ = self.http.post(url).json(&body).send().await;
    }
}

#[cfg(test)]
mod consent_tests {
    use super::*;
    use crate::config::{AppConfig, PrivacyConfig};

    #[test]
    fn exceptions_drop_without_consent_and_discard_private_payloads() {
        let mut event = posthog_rs::Event::new_anon("$exception");
        event
            .insert_prop("$exception_list", json!([{ "value": "secret chat text" }]))
            .unwrap();
        event
            .insert_prop("$exception_panic_file", "/home/alice/private.rs")
            .unwrap();
        let context = global_context("1.2.3");
        assert!(safe_exception(event.clone(), &context, false).is_none());
        let safe = safe_exception(event, &context, true).unwrap();
        let serialized = serde_json::to_string(&safe).unwrap();
        assert!(!serialized.contains("secret chat text"));
        assert!(!serialized.contains("alice"));
        assert!(serialized.contains("rust_panic"));
        assert!(serialized.contains("shuttle-desktop"));
    }

    #[test]
    fn panic_location_is_limited_to_crate_relative_paths() {
        let mut event = posthog_rs::Event::new_anon("$exception");
        event
            .insert_prop("$exception_panic_file", "src/db/chat.rs")
            .unwrap();
        event.insert_prop("$exception_panic_line", 42).unwrap();
        let safe = safe_exception(event, &global_context("1"), true).unwrap();
        assert_eq!(
            safe.properties().get("source_file"),
            Some(&json!("src/db/chat.rs"))
        );
        assert_eq!(safe.properties().get("source_line"), Some(&json!(42)));
    }

    #[test]
    fn exception_categories_do_not_forward_free_text() {
        assert_eq!(error_category("frontend_exception"), "frontend_exception");
        assert_eq!(
            error_category("secret message alice@example.com"),
            "application_error"
        );
        let mut event = posthog_rs::Event::new_anon("$exception");
        event
            .insert_prop("shuttle_error_category", "frontend_exception")
            .unwrap();
        let safe = safe_exception(event, &global_context("1"), true).unwrap();
        assert!(serde_json::to_string(&safe)
            .unwrap()
            .contains("frontend_exception"));
    }

    #[test]
    fn diagnostics_disabled_drops_posthog_events() {
        let dir = tempfile::tempdir().unwrap();
        let db = Arc::new(Database::open(dir.path()).unwrap());
        let config = Arc::new(ConfigStore::open(dir.path(), db.clone()));
        let mgr = TelemetryManager::new("0.1.0", "id".into(), config, db);
        mgr.track("app_started", Map::new());
        assert!(mgr.posthog_queue.lock().is_empty());
    }

    #[test]
    fn diagnostics_enabled_queues_events() {
        let dir = tempfile::tempdir().unwrap();
        let db = Arc::new(Database::open(dir.path()).unwrap());
        let config = Arc::new(ConfigStore::open(dir.path(), db.clone()));
        let mut cfg = AppConfig::default();
        cfg.privacy = PrivacyConfig {
            crash_reports: false,
            usage_diagnostics: true,
            ..PrivacyConfig::default()
        };
        config.save(cfg).unwrap();
        let mgr = TelemetryManager::new("0.1.0", "id".into(), config, db);
        mgr.track("app_started", Map::new());
        assert_eq!(mgr.posthog_queue.lock().len(), 1);
    }
}
