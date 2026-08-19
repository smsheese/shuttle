use crate::db::Database;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default)]
    pub appearance: AppearanceConfig,
    #[serde(default)]
    pub notifications: NotificationConfig,
    #[serde(default)]
    pub privacy: PrivacyConfig,
    #[serde(default)]
    pub channel_styles: HashMap<String, ChannelStyle>,
    #[serde(default)]
    pub media_retention: MediaRetentionConfig,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            appearance: AppearanceConfig::default(),
            notifications: NotificationConfig::default(),
            privacy: PrivacyConfig::default(),
            channel_styles: default_channel_styles(),
            media_retention: MediaRetentionConfig::default(),
        }
    }
}

/// How long (in days) to keep downloaded media locally. `None` = keep forever.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaRetentionConfig {
    #[serde(default)]
    pub images_days: Option<u32>,
    #[serde(default)]
    pub videos_days: Option<u32>,
    #[serde(default)]
    pub audio_days: Option<u32>,
    #[serde(default)]
    pub documents_days: Option<u32>,
    #[serde(default)]
    pub stickers_days: Option<u32>,
    #[serde(default)]
    pub gifs_days: Option<u32>,
    #[serde(default)]
    pub voice_days: Option<u32>,
}

impl Default for MediaRetentionConfig {
    fn default() -> Self {
        Self {
            images_days: None,
            videos_days: None,
            audio_days: None,
            documents_days: None,
            stickers_days: None,
            gifs_days: None,
            voice_days: None,
        }
    }
}

fn default_channel_styles() -> HashMap<String, ChannelStyle> {
    let mut m = HashMap::new();
    m.insert("whatsapp".into(), ChannelStyle { tag: Some("#25D366".into()), background: None, font: None });
    m.insert("telegram".into(), ChannelStyle { tag: Some("#2AABEE".into()), background: None, font: None });
    m.insert("signal".into(), ChannelStyle { tag: Some("#3A76F0".into()), background: None, font: None });
    m.insert("messenger".into(), ChannelStyle { tag: Some("#0084FF".into()), background: None, font: None });
    m.insert("instagram".into(), ChannelStyle { tag: Some("#E1306C".into()), background: None, font: None });
    m.insert("email".into(), ChannelStyle { tag: Some("#EA4335".into()), background: None, font: None });
    m.insert("matrix".into(), ChannelStyle { tag: Some("#0DBD8B".into()), background: None, font: None });
    m
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppearanceConfig {
    /// `system`, `light`, or `dark`
    #[serde(default = "system_scheme")]
    pub color_scheme: String,
    #[serde(default = "default_theme")]
    pub theme_id: String,
    #[serde(default = "default_datetime_format")]
    pub datetime_format: String,
    #[serde(default = "default_font_scale")]
    pub font_scale: f64,
    #[serde(default)]
    pub tweakcn_css: Option<String>,
}

fn system_scheme() -> String {
    "light".into()
}
fn default_theme() -> String {
    "cmlhfpjhw000004l4f4ax3m7z".into()
}
fn default_datetime_format() -> String {
    "12h_full".into()
}
fn default_font_scale() -> f64 {
    1.0
}

impl Default for AppearanceConfig {
    fn default() -> Self {
        Self {
            color_scheme: system_scheme(),
            theme_id: default_theme(),
            datetime_format: default_datetime_format(),
            font_scale: default_font_scale(),
            tweakcn_css: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub quiet_hours_enabled: bool,
    #[serde(default = "default_quiet_start")]
    pub quiet_hours_start: String,
    #[serde(default = "default_quiet_end")]
    pub quiet_hours_end: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrivacyConfig {
    /// Send anonymous crash reports via Sentry when enabled.
    #[serde(default)]
    pub crash_reports: bool,
    /// Send anonymous usage and performance diagnostics via PostHog when enabled.
    #[serde(default)]
    pub usage_diagnostics: bool,
}

impl Default for PrivacyConfig {
    fn default() -> Self {
        Self {
            crash_reports: false,
            usage_diagnostics: false,
        }
    }
}

fn default_true() -> bool {
    true
}
fn default_quiet_start() -> String {
    "22:00".into()
}
fn default_quiet_end() -> String {
    "08:00".into()
}

impl Default for NotificationConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            quiet_hours_enabled: false,
            quiet_hours_start: default_quiet_start(),
            quiet_hours_end: default_quiet_end(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChannelStyle {
    #[serde(default)]
    pub tag: Option<String>,
    #[serde(default)]
    pub background: Option<String>,
    #[serde(default)]
    pub font: Option<String>,
}

pub struct ConfigStore {
    legacy_path: PathBuf,
    db: Arc<Database>,
    inner: Mutex<AppConfig>,
}

impl ConfigStore {
    pub fn open(data_dir: &Path, db: Arc<Database>) -> Self {
        let legacy_path = data_dir.join("config.json");
        let cfg = load_config(&db, &legacy_path);
        Self {
            legacy_path,
            db,
            inner: Mutex::new(cfg),
        }
    }

    pub fn get(&self) -> AppConfig {
        self.inner.lock().clone()
    }

    pub fn save(&self, cfg: AppConfig) -> Result<AppConfig, String> {
        let json = serde_json::to_string(&cfg).map_err(|e| e.to_string())?;
        self.db
            .set_app_setting("config", &json)
            .map_err(|e| e.to_string())?;
        // Keep config.json as a backward-compatible backup copy.
        let _ = std::fs::write(&self.legacy_path, serde_json::to_string_pretty(&cfg).unwrap_or(json));
        *self.inner.lock() = cfg.clone();
        Ok(cfg)
    }
}

fn load_config(db: &Database, legacy_path: &Path) -> AppConfig {
    if let Ok(Some(json)) = db.get_app_setting("config") {
        if let Ok(cfg) = serde_json::from_str(&json) {
            return cfg;
        }
    }
    if legacy_path.exists() {
        if let Ok(json) = std::fs::read_to_string(legacy_path) {
            if let Ok(cfg) = serde_json::from_str::<AppConfig>(&json) {
                let _ = db.set_app_setting("config", &json);
                return cfg;
            }
        }
    }
    let cfg = AppConfig::default();
    if let Ok(json) = serde_json::to_string(&cfg) {
        let _ = db.set_app_setting("config", &json);
        let _ = std::fs::write(legacy_path, serde_json::to_string_pretty(&cfg).unwrap_or(json));
    }
    cfg
}
