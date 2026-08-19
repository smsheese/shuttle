mod events;
mod performance;
mod privacy;
mod system;

pub use performance::PerformanceSampler;

use crate::config::ConfigStore;
use crate::db::Database;
use events::{validate_app_event, validate_connector_telemetry, validate_error_context};
use performance::PerformanceSnapshot;
use privacy::scrub_string;
use serde_json::{json, Map, Value};
use std::sync::Arc;
use std::time::Duration;
use system::{global_context, merge_context};

const POSTHOG_BATCH_INTERVAL: Duration = Duration::from_secs(30);
const MAX_POSTHOG_QUEUE: usize = 256;

struct PostHogEvent {
    event: String,
    properties: Map<String, Value>,
}

pub struct TelemetryManager {
    app_version: String,
    installation_id: String,
    config: Arc<ConfigStore>,
    db: Arc<Database>,
    global_context: Map<String, Value>,
    sentry_guard: parking_lot::Mutex<Option<sentry::ClientInitGuard>>,
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
            app_version: app_version.to_string(),
            installation_id,
            config: config.clone(),
            db,
            global_context: global_context(app_version),
            sentry_guard: parking_lot::Mutex::new(None),
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
        self.configure_sentry(privacy.crash_reports);
        if !privacy.usage_diagnostics {
            self.posthog_queue.lock().clear();
        }
    }

    fn configure_sentry(&self, enabled: bool) {
        let mut guard = self.sentry_guard.lock();
        *guard = None;
        if !enabled {
            return;
        }
        let Some(dsn) = crate::env::sentry_dsn() else {
            return;
        };
        let release = format!("shuttle@{}", self.app_version);
        let environment = crate::env::telemetry_environment();
        let init = sentry::init((
            dsn,
            sentry::ClientOptions {
                release: Some(release.into()),
                environment: Some(environment.into()),
                traces_sample_rate: 0.1,
                send_default_pii: false,
                attach_stacktrace: true,
                before_send: Some(Arc::new(|mut event| {
                    if let Some(ref mut request) = event.request {
                        request.url = request.url.as_ref().map(|u| {
                            sentry::protocol::Url::parse(&privacy::sanitize_url_string(&u.to_string()))
                                .unwrap_or_else(|_| u.clone())
                        });
                        request
                            .headers
                            .retain(|k, _| !privacy::is_sensitive_key(k));
                    }
                    for extra in event.extra.values_mut() {
                        if let sentry::protocol::Value::String(s) = extra {
                            *extra = sentry::protocol::Value::String(scrub_string(s));
                        }
                    }
                    Some(event)
                })),
                ..Default::default()
            },
        ));
        *guard = Some(init);
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

    pub fn error(&self, message: &str, context: Map<String, Value>) {
        let privacy = self.config.get().privacy;
        let validated = validate_error_context(&context);
        if privacy.crash_reports {
            sentry::capture_message(message, sentry::Level::Error);
        }
        if privacy.usage_diagnostics {
            let mut props = validated;
            props.insert("error_category".into(), json!(message));
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
            if let Some(name) = validated.get("event").and_then(|v| v.as_str()).map(str::to_string) {
                self.track(&name, validated);
            }
        }
    }

    pub fn set_foreground(&self, foreground: bool) {
        self.sampler().set_foreground(foreground);
    }

    pub fn emit_database_initialized(&self) {
        let size = std::fs::metadata(self.db.data_dir().join("app.sqlite"))
            .map(|m| m.len())
            .unwrap_or(0);
        let accounts = self.db.list_accounts().map(|a| a.len() as u64).unwrap_or(0);
        let mut props = Map::new();
        props.insert(
            "database_size_bucket".into(),
            json!(system::bucket_bytes(size)),
        );
        props.insert("accounts_total".into(), json!(system::bucket_count(accounts)));
        props.insert("connector_count".into(), json!(system::bucket_count(accounts)));
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
        self.track("performance_snapshot", props);
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
        };
        config.save(cfg).unwrap();
        let mgr = TelemetryManager::new("0.1.0", "id".into(), config, db);
        mgr.track("app_started", Map::new());
        assert_eq!(mgr.posthog_queue.lock().len(), 1);
    }
}
