use super::privacy::{is_globally_allowed_key, sanitize_map, sanitize_value};
use serde_json::{Map, Value};

pub const APP_EVENTS: &[&str] = &[
    "app_started",
    "app_ready",
    "app_closed",
    "onboarding_started",
    "onboarding_completed",
    "account_add_completed",
    "account_add_failed",
    "account_removed",
    "connector_sync_started",
    "connector_sync_completed",
    "connector_sync_failed",
    "connector_crashed",
    "database_initialized",
    "database_migration_completed",
    "database_error",
    "performance_snapshot",
    "search_used",
    "command_failed",
];

const APP_EVENT_PROPS: &[(&str, &[&str])] = &[
    ("app_started", &[]),
    ("app_ready", &["duration_ms"]),
    ("app_closed", &[]),
    ("onboarding_started", &[]),
    ("onboarding_completed", &[]),
    ("account_add_completed", &["connector_type"]),
    ("account_add_failed", &["connector_type", "error_category"]),
    ("account_removed", &["connector_type"]),
    (
        "connector_sync_started",
        &["connector_type", "duration_ms"],
    ),
    (
        "connector_sync_completed",
        &["connector_type", "duration_ms", "items_processed", "errors"],
    ),
    (
        "connector_sync_failed",
        &["connector_type", "duration_ms", "error_category"],
    ),
    ("connector_crashed", &["connector_type", "error_category"]),
    ("database_initialized", &["database_size_bucket"]),
    ("database_migration_completed", &[]),
    ("database_error", &["error_category"]),
    (
        "performance_snapshot",
        &[
            "foreground",
            "sample_count",
            "cpu_avg",
            "cpu_p95",
            "memory_avg_mb",
            "memory_p95_mb",
        ],
    ),
    ("search_used", &[]),
    ("command_failed", &["operation", "error_category"]),
];

const CONNECTOR_EVENTS: &[&str] = &[
    "sync_started",
    "sync_completed",
    "sync_failed",
    "crashed",
];

const CONNECTOR_TYPES: &[&str] = &[
    "whatsapp", "telegram", "signal", "messenger", "instagram", "email", "matrix",
];

pub fn validate_app_event(name: &str, props: &Map<String, Value>) -> Option<Map<String, Value>> {
    if !APP_EVENTS.contains(&name) {
        return None;
    }
    let allowed = APP_EVENT_PROPS
        .iter()
        .find(|(event, _)| *event == name)
        .map(|(_, keys)| *keys)
        .unwrap_or(&[] as &[&str]);
    Some(filter_props(props, allowed))
}

pub fn validate_connector_telemetry(payload: &Map<String, Value>) -> Option<Map<String, Value>> {
    let event = payload.get("event").and_then(|v| v.as_str())?;
    if !CONNECTOR_EVENTS.contains(&event) {
        return None;
    }
    let connector_type = payload.get("connector_type").and_then(|v| v.as_str())?;
    if !CONNECTOR_TYPES.contains(&connector_type) {
        return None;
    }
    let mut out = Map::new();
    out.insert("event".into(), Value::String(map_connector_event(event)));
    out.insert(
        "connector_type".into(),
        Value::String(connector_type.to_string()),
    );
    for key in ["duration_ms", "items_processed", "errors"] {
        if let Some(v) = payload.get(key) {
            if let Some(n) = v.as_u64() {
                if n <= 86_400_000 {
                    out.insert(key.into(), Value::Number(n.into()));
                }
            }
        }
    }
    Some(out)
}

fn map_connector_event(event: &str) -> String {
    match event {
        "sync_started" => "connector_sync_started".into(),
        "sync_completed" => "connector_sync_completed".into(),
        "sync_failed" => "connector_sync_failed".into(),
        "crashed" => "connector_crashed".into(),
        other => other.into(),
    }
}

pub fn validate_error_context(context: &Map<String, Value>) -> Map<String, Value> {
    let sanitized = sanitize_map(context);
    filter_props(
        &sanitized,
        &["operation", "error_category", "connector_type"],
    )
}

fn filter_props(props: &Map<String, Value>, allowed: &[&str]) -> Map<String, Value> {
    let mut out = Map::new();
    for (key, value) in props {
        if allowed.contains(&key.as_str()) || is_globally_allowed_key(key) {
            out.insert(key.clone(), sanitize_value(value));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn rejects_unknown_app_event() {
        let props = Map::new();
        assert!(validate_app_event("message_sent", &props).is_none());
    }

    #[test]
    fn accepts_connector_sync_completed() {
        let mut props = Map::new();
        props.insert("event".into(), json!("sync_completed"));
        props.insert("connector_type".into(), json!("telegram"));
        props.insert("duration_ms".into(), json!(1200));
        props.insert("items_processed".into(), json!(42));
        props.insert("account_id".into(), json!("must-not-leak"));
        let out = validate_connector_telemetry(&props).expect("valid");
        assert_eq!(out["connector_type"], "telegram");
        assert!(out.get("account_id").is_none());
    }
}
