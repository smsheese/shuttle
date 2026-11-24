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
    pub sleep: SleepConfig,
    #[serde(default)]
    pub channel_styles: HashMap<String, ChannelStyle>,
    #[serde(default)]
    pub media: MediaConfig,
    #[serde(default)]
    pub ai: AiConfig,
    #[serde(default)]
    pub integrations: IntegrationsConfig,
    #[serde(default)]
    pub performance: PerformanceConfig,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            appearance: AppearanceConfig::default(),
            notifications: NotificationConfig::default(),
            privacy: PrivacyConfig::default(),
            sleep: SleepConfig::default(),
            channel_styles: default_channel_styles(),
            media: MediaConfig::default(),
            ai: AiConfig::default(),
            integrations: IntegrationsConfig::default(),
            performance: PerformanceConfig::default(),
        }
    }
}

/// Runtime tuning for the WebView process. Editing `config.json` is the only way to change it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceConfig {
    /// Linux: shrink WebKit's caches and drop WebGL (nothing in the UI uses it). On by default.
    #[serde(default = "default_true")]
    pub low_memory_webview: bool,
    /// Destroy the main window's WebView once it has sat hidden in the tray, so its renderer
    /// process exits. Showing the window again rebuilds it. On by default.
    #[serde(default = "default_true")]
    pub unload_hidden_window: bool,
    /// How long it stays hidden before that happens, in seconds.
    #[serde(default = "default_unload_hidden_after_secs")]
    pub unload_hidden_after_secs: u32,
}

impl Default for PerformanceConfig {
    fn default() -> Self {
        Self {
            low_memory_webview: true,
            unload_hidden_window: true,
            unload_hidden_after_secs: default_unload_hidden_after_secs(),
        }
    }
}

/// The user's own OpenRouter account, used for translation and later AI features.
/// An empty `api_key` = AI is off and no text leaves the device.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AiConfig {
    #[serde(default)]
    pub openrouter_api_key: String,
    /// OpenRouter model id such as `google/gemini-2.5-flash`.
    #[serde(default)]
    pub model: String,
}

/// Keys for optional third-party services the user brings.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IntegrationsConfig {
    /// Enables GIF and sticker search.
    #[serde(default)]
    pub giphy_api_key: String,
}

/// Where saved media goes and how long the app-private download cache is kept.
///
/// Downloads (auto or on demand) live in `<data_dir>/media_cache`; only a file the user
/// explicitly saves ends up in `save_dir`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaConfig {
    /// Folder for explicit saves. `None` = `<Documents>/shuttle`.
    #[serde(default)]
    pub save_dir: Option<String>,
    /// Show the native save dialog for every save instead of using `save_dir`.
    #[serde(default)]
    pub ask_each_time: bool,
    /// Delete cached downloads unused for this many days. `None` = keep forever.
    #[serde(default = "default_keep_days")]
    pub keep_days: Option<u32>,
}

pub const DEFAULT_MEDIA_KEEP_DAYS: u32 = 60;

fn default_keep_days() -> Option<u32> {
    Some(DEFAULT_MEDIA_KEEP_DAYS)
}

impl Default for MediaConfig {
    fn default() -> Self {
        Self {
            save_dir: None,
            ask_each_time: false,
            keep_days: default_keep_days(),
        }
    }
}

fn default_channel_styles() -> HashMap<String, ChannelStyle> {
    let mut m = HashMap::new();
    m.insert(
        "whatsapp".into(),
        ChannelStyle {
            tag: Some("#25D366".into()),
            background: None,
            font: None,
        },
    );
    m.insert(
        "telegram".into(),
        ChannelStyle {
            tag: Some("#2AABEE".into()),
            background: None,
            font: None,
        },
    );
    m.insert(
        "signal".into(),
        ChannelStyle {
            tag: Some("#3A76F0".into()),
            background: None,
            font: None,
        },
    );
    m.insert(
        "messenger".into(),
        ChannelStyle {
            tag: Some("#0084FF".into()),
            background: None,
            font: None,
        },
    );
    m.insert(
        "instagram".into(),
        ChannelStyle {
            tag: Some("#E1306C".into()),
            background: None,
            font: None,
        },
    );
    m.insert(
        "email".into(),
        ChannelStyle {
            tag: Some("#EA4335".into()),
            background: None,
            font: None,
        },
    );
    m.insert(
        "matrix".into(),
        ChannelStyle {
            tag: Some("#0DBD8B".into()),
            background: None,
            font: None,
        },
    );
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
    #[serde(default)] pub dark_schedule: Option<String>,
    #[serde(default)] pub dark_from: Option<String>,
    #[serde(default)] pub dark_to: Option<String>,
    #[serde(default)] pub dark_location: Option<String>,
    #[serde(default)] pub dark_latitude: Option<f64>,
    #[serde(default)] pub dark_longitude: Option<f64>,
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
            dark_schedule: None,
            dark_from: None, dark_to: None, dark_location: None, dark_latitude: None, dark_longitude: None,
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

/// Ferdium-style hibernation: stop idle sidecars, optionally wake to poll.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SleepConfig {
    #[serde(default)]
    pub enabled: bool,
    /// Idle minutes before a non-active account is stopped.
    #[serde(default = "default_sleep_after")]
    pub after_minutes: u32,
    /// While sleeping, wake this often to sync. `0` = only when the user opens the account.
    #[serde(default = "default_sleep_check")]
    pub check_minutes: u32,
}

impl Default for SleepConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            after_minutes: default_sleep_after(),
            check_minutes: default_sleep_check(),
        }
    }
}

fn default_sleep_after() -> u32 {
    5
}
fn default_sleep_check() -> u32 {
    15
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrivacyConfig {
    /// Send anonymous crash reports via PostHog when enabled.
    #[serde(default)]
    pub crash_reports: bool,
    /// Send anonymous usage and performance diagnostics via PostHog when enabled.
    #[serde(default)]
    pub usage_diagnostics: bool,
    /// Fetch a title, description and image for the first link in a message. The site sees
    /// this device's IP address when a message is opened.
    #[serde(default = "default_true")]
    pub link_previews: bool,
    /// Fetch a small OpenStreetMap image for shared locations. OpenStreetMap sees roughly
    /// where the shared point is, and this device's IP address.
    #[serde(default = "default_true")]
    pub map_tiles: bool,
}

impl Default for PrivacyConfig {
    fn default() -> Self {
        Self {
            crash_reports: false,
            usage_diagnostics: false,
            link_previews: true,
            map_tiles: true,
        }
    }
}

fn default_true() -> bool {
    true
}
fn default_unload_hidden_after_secs() -> u32 {
    120
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
        let _ = std::fs::write(
            &self.legacy_path,
            serde_json::to_string_pretty(&cfg).unwrap_or(json),
        );
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
        let _ = std::fs::write(
            legacy_path,
            serde_json::to_string_pretty(&cfg).unwrap_or(json),
        );
    }
    cfg
}

#[cfg(test)]
mod media_config_tests {
    use super::*;

    #[test]
    fn media_defaults_keep_sixty_days_and_save_to_documents() {
        // An old config without a `media` section gets the defaults.
        let cfg: AppConfig = serde_json::from_str("{}").unwrap();
        assert_eq!(cfg.media.keep_days, Some(60));
        assert!(!cfg.media.ask_each_time);
        assert!(cfg.media.save_dir.is_none());
        // A section with only some fields keeps 60 days for the rest.
        let cfg: AppConfig = serde_json::from_str(r#"{"media":{"ask_each_time":true}}"#).unwrap();
        assert_eq!(cfg.media.keep_days, Some(60));
        assert!(cfg.media.ask_each_time);
        // `null` is "keep forever", not "use the default".
        let cfg: AppConfig = serde_json::from_str(r#"{"media":{"keep_days":null}}"#).unwrap();
        assert_eq!(cfg.media.keep_days, None);
        // The retired per-kind retention block is ignored.
        let cfg: AppConfig =
            serde_json::from_str(r#"{"media_retention":{"images_days":3}}"#).unwrap();
        assert_eq!(cfg.media.keep_days, Some(60));
    }
}
