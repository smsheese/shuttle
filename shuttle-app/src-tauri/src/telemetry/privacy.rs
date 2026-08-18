use regex::Regex;
use serde_json::{Map, Value};
use std::sync::LazyLock;

static SENSITIVE_KEY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)(phone|email|username|user_?name|password|passwd|token|secret|cookie|auth|qr|message|body|content|account_?id|conversation_?id|chat_?id|remote_?id|sender|recipient|credential|api_?key|authorization|session|identity|distinct_?id|hostname|home_?dir|ip_?addr|mac_?addr|serial)",
    )
    .expect("regex")
});

static EMAIL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}").expect("regex"));

static PHONE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\+?\d[\d\s().-]{7,}\d").expect("regex"));

static UUID: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}")
        .expect("regex")
});

const GLOBAL_ALLOWED: &[&str] = &[
    "app_version",
    "build_channel",
    "environment",
    "release",
    "git_commit",
    "os",
    "os_version",
    "architecture",
    "cpu_core_count",
    "ram_bucket",
    "accounts_total",
    "connector_count",
    "database_size_bucket",
    "message_count_bucket",
    "duration_ms",
    "items_processed",
    "errors",
    "connector_type",
    "operation",
    "error_category",
    "foreground",
    "sample_count",
    "cpu_avg",
    "cpu_p95",
    "memory_avg_mb",
    "memory_p95_mb",
    "event",
];

pub fn sanitize_value(value: &Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(sanitize_map(map)),
        Value::Array(items) => Value::Array(items.iter().map(sanitize_value).collect()),
        Value::String(s) => Value::String(scrub_string(s)),
        other => other.clone(),
    }
}

pub fn sanitize_map(map: &Map<String, Value>) -> Map<String, Value> {
    let mut out = Map::new();
    for (key, value) in map {
        if is_sensitive_key(key) {
            continue;
        }
        let cleaned = match value {
            Value::String(s) => Value::String(scrub_string(s)),
            Value::Object(nested) => Value::Object(sanitize_map(nested)),
            Value::Array(items) => Value::Array(items.iter().map(sanitize_value).collect()),
            other => other.clone(),
        };
        if value_contains_sensitive(&cleaned) {
            continue;
        }
        out.insert(key.clone(), cleaned);
    }
    out
}

pub fn scrub_string(input: &str) -> String {
    let mut s = input.to_string();
    if looks_like_path_with_home(&s) {
        return "[path]".into();
    }
    if EMAIL.is_match(&s) {
        s = EMAIL.replace_all(&s, "[email]").into_owned();
    }
    if PHONE.is_match(&s) {
        s = PHONE.replace_all(&s, "[phone]").into_owned();
    }
    if UUID.is_match(&s) {
        s = UUID.replace_all(&s, "[uuid]").into_owned();
    }
    sanitize_url_string(&s)
}

pub fn sanitize_url_string(input: &str) -> String {
    if !(input.starts_with("http://") || input.starts_with("https://")) {
        return input.to_string();
    }
    let (base, query) = input.split_once('?').unwrap_or((input, ""));
    let mut out = base.to_string();
    if !query.is_empty() {
        let safe: Vec<&str> = query
            .split('&')
            .filter(|pair| {
                pair.split_once('=')
                    .map(|(k, _)| !is_sensitive_key(k))
                    .unwrap_or(true)
            })
            .collect();
        if !safe.is_empty() {
            out.push('?');
            out.push_str(&safe.join("&"));
        }
    }
    out
}

pub fn is_sensitive_key(key: &str) -> bool {
    SENSITIVE_KEY.is_match(key)
}

pub fn is_globally_allowed_key(key: &str) -> bool {
    GLOBAL_ALLOWED.contains(&key)
}

fn value_contains_sensitive(value: &Value) -> bool {
    match value {
        Value::String(s) => {
            EMAIL.is_match(s) || PHONE.is_match(s) || looks_like_path_with_home(s)
        }
        Value::Object(map) => map.keys().any(|k| is_sensitive_key(k)),
        _ => false,
    }
}

fn looks_like_path_with_home(s: &str) -> bool {
    s.contains("/home/")
        || s.contains("\\Users\\")
        || s.contains("/Users/")
        || s.starts_with("~/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn strips_sensitive_keys() {
        let input = json!({
            "app_version": "0.1.0",
            "account_id": "secret-id",
            "phone": "+1 555 0100",
            "duration_ms": 120
        });
        let out = sanitize_value(&input);
        assert_eq!(out["app_version"], "0.1.0");
        assert_eq!(out["duration_ms"], 120);
        assert!(out.get("account_id").is_none());
        assert!(out.get("phone").is_none());
    }

    #[test]
    fn scrubs_email_and_phone_in_strings() {
        let s = scrub_string("Contact alice@example.com or +1 555 123 4567");
        assert!(!s.contains("alice@example.com"));
        assert!(!s.contains("555 123"));
    }

    #[test]
    fn strips_sensitive_query_params() {
        let url = sanitize_url_string("https://example.com/auth?token=abc&build=1");
        assert!(!url.contains("token="));
        assert!(url.contains("build=1"));
    }
}
