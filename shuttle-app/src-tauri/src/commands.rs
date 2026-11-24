use crate::components::ComponentManager;
use crate::config::{AppConfig, ConfigStore};
use crate::connectors::{AppEvent, ConnectorManager};
use crate::db::Database;
use crate::models::*;
use crate::notifications;
use crate::telemetry::TelemetryManager;
use crate::updates::{UpdateCheck, UpdateManager};
use age::secrecy::SecretString;
use age::{Decryptor, Encryptor};
use parking_lot::Mutex;
use std::fs;
use std::io::{Cursor, Read, Write};
use std::iter;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;
use tempfile::tempdir;

use tokio::sync::broadcast;

pub struct AppState {
    pub db: Arc<Database>,
    pub connectors: Arc<ConnectorManager>,
    pub components: Arc<ComponentManager>,
    pub config: Arc<ConfigStore>,
    pub telemetry: Arc<TelemetryManager>,
    pub updates: Arc<UpdateManager>,
}

#[tauri::command]
pub fn list_accounts(state: State<'_, AppState>) -> Result<Vec<Account>, String> {
    let mut accounts = state.db.list_accounts().map_err(|e| e.to_string())?;
    for account in &mut accounts {
        if account.metadata.get("avatar_data").is_none() {
            if let Ok(Some(avatar)) = state.db.self_avatar(account) {
                if let Some(metadata) = account.metadata.as_object_mut() {
                    metadata.insert("avatar_data".into(), serde_json::Value::String(avatar));
                }
            }
        }
    }
    Ok(accounts)
}

#[tauri::command]
pub fn get_connector_requirements(
    state: State<'_, AppState>,
    connector_id: String,
) -> Result<crate::components::ConnectorRequirements, String> {
    state.components.get_connector_requirements(&connector_id)
}

#[tauri::command]
pub fn get_installed_components(
    state: State<'_, AppState>,
) -> Vec<crate::components::InstalledComponent> {
    state.components.installed_components()
}

/// Version and platform facts the About page shows and the feedback diagnostics line uses.
#[derive(Debug, Clone, serde::Serialize)]
pub struct AppInfo {
    pub version: String,
    pub os: String,
    pub platform: String,
}

#[tauri::command]
pub fn app_info() -> AppInfo {
    AppInfo {
        version: env!("CARGO_PKG_VERSION").to_string(),
        os: std::env::consts::OS.to_string(),
        platform: crate::components::platform_key().to_string(),
    }
}

#[tauri::command]
pub async fn components_overview(
    state: State<'_, AppState>,
) -> Result<crate::components::ComponentsOverview, String> {
    Ok(state.components.components_overview().await)
}

#[tauri::command]
pub async fn check_for_update(
    state: State<'_, AppState>,
    force: bool,
) -> Result<UpdateCheck, String> {
    state.updates.check(force).await
}

#[tauri::command]
pub async fn download_update(app: AppHandle, state: State<'_, AppState>) -> Result<String, String> {
    state.updates.download(&app).await
}

#[tauri::command]
pub fn run_update_installer(state: State<'_, AppState>) -> Result<(), String> {
    state.updates.run_installer()
}

#[tauri::command]
pub async fn ensure_connector_components(
    state: State<'_, AppState>,
    connector_id: String,
) -> Result<(), String> {
    state.components.clear_cancel();
    state
        .components
        .ensure_connector_installed_async(&connector_id)
        .await
}

#[tauri::command]
pub fn cancel_component_install(state: State<'_, AppState>) -> Result<(), String> {
    state.components.cancel_install();
    Ok(())
}

#[tauri::command]
pub fn list_connectors(state: State<'_, AppState>) -> Vec<ConnectorInfo> {
    state.connectors.list_connectors()
}

#[tauri::command]
pub fn create_account(
    state: State<'_, AppState>,
    connector_id: String,
    name: String,
) -> Result<Account, String> {
    let account = state
        .db
        .create_account(&connector_id, &name)
        .map_err(|e| e.to_string())?;
    let mut props = serde_json::Map::new();
    props.insert("connector_type".into(), serde_json::json!(connector_id));
    state.telemetry.track("account_add_completed", props);
    Ok(account)
}

#[tauri::command]
pub fn delete_account(state: State<'_, AppState>, account_id: String) -> Result<(), String> {
    let connector_id = state
        .db
        .get_account(&account_id)
        .map(|a| a.connector_id)
        .unwrap_or_default();
    state.connectors.wipe_account(&account_id, &connector_id);
    state
        .db
        .delete_account(&account_id)
        .map_err(|e| e.to_string())?;
    let mut props = serde_json::Map::new();
    props.insert("connector_type".into(), serde_json::json!(connector_id));
    state.telemetry.track("account_removed", props);
    Ok(())
}

#[tauri::command]
pub fn update_account(
    state: State<'_, AppState>,
    account_id: String,
    patch: AccountPatch,
) -> Result<Account, String> {
    if patch.disabled == Some(true) {
        state.connectors.stop_connector(&account_id);
        let _ = state
            .db
            .update_account_status(&account_id, AccountStatus::Disconnected);
    }
    let account = state
        .db
        .patch_account(&account_id, &patch)
        .map_err(|e| e.to_string())?;
    if patch.disabled == Some(false) {
        let creds = crate::secrets::load(state.connectors.data_dir(), &account_id);
        let connectors = state.connectors.clone();
        let connector_id = account.connector_id.clone();
        let id = account_id.clone();
        tauri::async_runtime::spawn(async move {
            let _ = connectors.start_connector(&connector_id, &id, creds);
        });
    }
    Ok(account)
}

#[tauri::command]
pub fn wake_account(state: State<'_, AppState>, account_id: String) -> Result<String, String> {
    state.connectors.wake_account(&account_id)
}

#[tauri::command]
pub fn set_active_account(
    state: State<'_, AppState>,
    account_id: Option<String>,
) -> Result<(), String> {
    state.connectors.set_active_account(account_id);
    Ok(())
}

#[tauri::command]
pub fn list_conversations(
    state: State<'_, AppState>,
    account_id: Option<String>,
    workspace_id: Option<String>,
    priority_group: Option<String>,
    archived_only: Option<bool>,
    offset: Option<i64>,
    limit: Option<i64>,
) -> Result<Vec<Conversation>, String> {
    let started = std::time::Instant::now();
    let result = state
        .db
        .list_conversations(
            account_id.as_deref(),
            workspace_id.as_deref(),
            priority_group.as_deref(),
            archived_only.unwrap_or(false),
            offset.unwrap_or(0),
            limit.unwrap_or(30),
        )
        .map_err(|e| e.to_string());
    state.telemetry.time("chat_list", started);
    result
}

#[tauri::command]
pub fn count_conversations(
    state: State<'_, AppState>,
    account_id: Option<String>,
    workspace_id: Option<String>,
    priority_group: Option<String>,
    archived_only: Option<bool>,
) -> Result<i64, String> {
    state
        .db
        .count_conversations(
            account_id.as_deref(),
            workspace_id.as_deref(),
            priority_group.as_deref(),
            archived_only.unwrap_or(false),
        )
        .map_err(|e| e.to_string())
}

/// One page of a chat's messages, oldest first, with whether the chat has more on each side.
#[tauri::command]
pub fn get_messages(
    state: State<'_, AppState>,
    conversation_id: String,
    limit: Option<i64>,
    anchor: Option<MessageAnchor>,
) -> Result<MessagePage, String> {
    let started = std::time::Instant::now();
    let result = state
        .db
        .message_page(
            &conversation_id,
            anchor.as_ref().unwrap_or(&MessageAnchor::Newest),
            limit.unwrap_or(MESSAGE_PAGE_ROWS),
        )
        .map_err(|e| e.to_string());
    state.telemetry.time("chat_open", started);
    result
}

/// The default page a chat opens with.
pub const MESSAGE_PAGE_ROWS: i64 = 50;

#[tauri::command]
pub async fn send_message(
    state: State<'_, AppState>,
    account_id: String,
    conversation_id: String,
    text: String,
    reply_to_message_id: Option<String>,
) -> Result<Message, String> {
    state
        .connectors
        .send_message(
            &account_id,
            &conversation_id,
            &text,
            reply_to_message_id.as_deref(),
            false,
        )
        .await
}

/// React to, edit or delete messages of one chat. Returns one outcome per message id.
#[tauri::command]
pub async fn apply_message_op(
    state: State<'_, AppState>,
    conversation_id: String,
    message_ids: Vec<String>,
    op: crate::connectors::MessageOp,
) -> Result<Vec<crate::connectors::MessageOpResult>, String> {
    state
        .connectors
        .apply_message_op(&conversation_id, &message_ids, op)
        .await
}

/// Translate messages of one chat into `target` (a language code) with the user's OpenRouter model.
#[tauri::command]
pub async fn translate_text(
    state: State<'_, AppState>,
    conversation_id: String,
    message_ids: Vec<String>,
    target: String,
) -> Result<Vec<crate::translate::Translation>, crate::translate::TranslateError> {
    use crate::translate::{translate, TranslateError, Translation};
    let cfg = state.config.get().ai;
    if cfg.openrouter_api_key.trim().is_empty() {
        return Err(TranslateError::NotConfigured);
    }
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(45))
        .build()
        .map_err(|e| TranslateError::Failed {
            message: e.to_string(),
        })?;
    let one = |message_id: String| {
        let (db, client, cfg, conversation_id, target) =
            (&state.db, &client, &cfg, &conversation_id, &target);
        async move {
            let text = db
                .get_message(conversation_id, &message_id)
                .map(|m| m.body)
                .unwrap_or_default();
            let outcome = if text.trim().is_empty() {
                Err("Nothing to translate".to_string())
            } else {
                translate(client, cfg, &text, target)
                    .await
                    .map_err(|e| match e {
                        TranslateError::NotConfigured => "Translation is not set up".to_string(),
                        TranslateError::Failed { message } => message,
                    })
            };
            Translation {
                message_id,
                text: outcome.as_ref().ok().cloned(),
                error: outcome.err(),
            }
        }
    };
    Ok(futures_util::future::join_all(message_ids.into_iter().map(one)).await)
}

#[tauri::command]
pub async fn send_attachment(
    state: State<'_, AppState>,
    account_id: String,
    conversation_id: String,
    kind: String,
    caption: Option<String>,
    filename: Option<String>,
    mime: Option<String>,
    data_base64: Option<String>,
    latitude: Option<f64>,
    longitude: Option<f64>,
    question: Option<String>,
    options: Option<Vec<String>>,
    max_answer: Option<i32>,
    reply_to_message_id: Option<String>,
) -> Result<Message, String> {
    state
        .connectors
        .send_attachment(
            &account_id,
            &conversation_id,
            &kind,
            caption.as_deref(),
            filename.as_deref(),
            mime.as_deref(),
            data_base64.as_deref(),
            latitude,
            longitude,
            question.as_deref(),
            options.unwrap_or_default(),
            max_answer,
            reply_to_message_id.as_deref(),
            None,
            false,
        )
        .await
}

#[tauri::command]
pub fn mark_read(
    state: State<'_, AppState>,
    conversation_id: String,
    send_remote: Option<bool>,
) -> Result<(), String> {
    let conv = state
        .db
        .get_conversation(&conversation_id)
        .map_err(|e| e.to_string())?;
    let account = state
        .db
        .get_account(&conv.account_id)
        .map_err(|e| e.to_string())?;
    state
        .db
        .mark_conversation_read(&conversation_id)
        .map_err(|e| e.to_string())?;
    let remote = send_remote.unwrap_or_else(|| notifications::should_send_receipt(&account, &conv));
    if remote {
        let _ = state
            .connectors
            .mark_read_remote(&conv.account_id, &conversation_id);
    }
    Ok(())
}

#[tauri::command]
pub fn mark_unread(state: State<'_, AppState>, conversation_id: String) -> Result<(), String> {
    state
        .db
        .mark_unread(&conversation_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn update_conversation(
    state: State<'_, AppState>,
    conversation_id: String,
    patch: ConversationPatch,
) -> Result<Conversation, String> {
    state
        .db
        .patch_conversation(&conversation_id, &patch)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_contacts(
    state: State<'_, AppState>,
    account_id: String,
) -> Result<Vec<Contact>, String> {
    state
        .db
        .list_contacts(&account_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn start_conversation(
    state: State<'_, AppState>,
    account_id: String,
    remote_id: String,
    title: String,
) -> Result<Conversation, String> {
    state
        .connectors
        .start_conversation(&account_id, &remote_id, &title)
}

/// Ask the connector whether a typed number is on WhatsApp. Returns once the request is queued;
/// the answer arrives as a `number.checked` event with the same `request_id`.
#[tauri::command]
pub fn check_whatsapp_number(
    state: State<'_, AppState>,
    account_id: String,
    phone: String,
    request_id: String,
) -> Result<(), String> {
    state
        .connectors
        .check_number(&account_id, &phone, &request_id)
}

#[tauri::command]
pub fn create_group(
    state: State<'_, AppState>,
    account_id: String,
    title: String,
    participants: Vec<String>,
) -> Result<(), String> {
    state
        .connectors
        .create_group(&account_id, &title, participants)
}

/// Download one message's media into the app cache. `auto` marks a download nobody asked for
/// (a thread rendering); it is skipped unless the chat/account auto-download setting allows
/// the kind. Returns whether a download was requested.
#[tauri::command]
pub fn download_message_media(
    state: State<'_, AppState>,
    account_id: String,
    conversation_id: String,
    message_id: String,
    auto: Option<bool>,
) -> Result<bool, String> {
    state.connectors.download_message_media(
        &account_id,
        &conversation_id,
        &message_id,
        auto.unwrap_or(false),
    )
}

#[tauri::command]
pub fn download_status_media(
    state: State<'_, AppState>,
    account_id: String,
    message_id: String,
) -> Result<(), String> {
    state
        .connectors
        .download_status_media(&account_id, &message_id)
}

/// Title, description and image of the page behind a link. `None` when link previews are off
/// in Privacy settings, or the page has nothing to show.
#[tauri::command]
pub async fn get_link_preview(
    state: State<'_, AppState>,
    url: String,
) -> Result<Option<crate::link_preview::LinkPreview>, String> {
    if !state.config.get().privacy.link_previews {
        return Ok(None);
    }
    crate::link_preview::link_preview(state.db.data_dir(), &url).await
}

/// Base URL of the loopback page host that gives a YouTube or Vimeo player a normal http
/// origin. Started on the first video play.
#[tauri::command]
pub async fn get_embed_host() -> Result<String, String> {
    crate::link_preview::embed_host().await
}

/// Path of a small map image for a shared location. `None` when map images are off in
/// Privacy settings, or the tiles could not be fetched.
#[tauri::command]
pub async fn get_map_tile(
    state: State<'_, AppState>,
    lat: f64,
    lng: f64,
) -> Result<Option<String>, String> {
    if !state.config.get().privacy.map_tiles {
        return Ok(None);
    }
    crate::link_preview::map_tile(state.db.data_dir(), lat, lng).await
}

/// Validate a media/preview path (cache or legacy Documents folder) and mark it as used.
#[tauri::command]
pub fn read_message_media(state: State<'_, AppState>, path: String) -> Result<String, String> {
    let data_dir = state.db.data_dir();
    let file = crate::media_store::validated_media_path(
        std::path::Path::new(&path),
        &crate::media_store::media_read_roots(data_dir),
    )?;
    if crate::media_store::is_in_cache(&file, data_dir) {
        crate::media_store::touch_if_stale(&file);
    }
    Ok(file.to_string_lossy().into_owned())
}

/// Put a downloaded media file on the system clipboard (image data + file reference for
/// images, a file reference otherwise) so it pastes into other apps. The path must be inside
/// the media cache or the legacy Documents root. The file reference points at a staged link
/// named like `Work-Mum-261112-1325.jpg` (documents keep `filename`), built from `kind`,
/// `filename`, the account and chat names and the message `timestamp` (RFC 3339). Never decodes
/// or keeps anything beyond the call.
#[tauri::command]
pub async fn copy_media_to_clipboard(
    state: State<'_, AppState>,
    path: String,
    kind: Option<String>,
    filename: Option<String>,
    timestamp: Option<String>,
    account_name: Option<String>,
    chat_name: Option<String>,
) -> Result<crate::clipboard::CopyKind, String> {
    let data_dir = state.db.data_dir().to_path_buf();
    let roots = crate::media_store::media_read_roots(&data_dir);
    let stage = crate::media_store::clipboard_stage_dir(&data_dir);
    let hint = crate::media_store::MediaNameHint {
        kind,
        filename,
        timestamp,
        account_name,
        chat_name,
    };
    tauri::async_runtime::spawn_blocking(move || {
        crate::clipboard::copy_media_file(std::path::Path::new(&path), &roots, &stage, &hint)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Copy a downloaded file into the user's save folder (or where the native dialog says when
/// "ask each time" is on) and return the path written. Never overwrites: a name clash becomes
/// `name (1).ext`. `None` means the user cancelled the dialog. The caller downloads first.
#[tauri::command]
pub async fn save_media_file(
    app: AppHandle,
    state: State<'_, AppState>,
    source_path: String,
    suggested_name: Option<String>,
    kind: Option<String>,
    timestamp: Option<String>,
    account_name: Option<String>,
    chat_name: Option<String>,
) -> Result<Option<String>, String> {
    let data_dir = state.db.data_dir().to_path_buf();
    let source = crate::media_store::validated_media_path(
        std::path::Path::new(&source_path),
        &crate::media_store::media_read_roots(&data_dir),
    )?;
    let cfg = state.config.get().media;
    let name = crate::media_store::sanitize_file_name(&crate::media_store::display_name(
        &crate::media_store::MediaNameHint {
            kind,
            filename: suggested_name,
            timestamp,
            account_name,
            chat_name,
        },
        &source,
    ));
    let mut dir = crate::media_store::resolve_save_dir(cfg.save_dir.as_deref());
    let mut file_name = name;
    if cfg.ask_each_time {
        let start_dir = dir.clone();
        let dialog_app = app.clone();
        let wanted = file_name.clone();
        let picked = tauri::async_runtime::spawn_blocking(move || {
            let mut builder = dialog_app.dialog().file().set_file_name(&wanted);
            if start_dir.is_dir() {
                builder = builder.set_directory(&start_dir);
            }
            builder.blocking_save_file()
        })
        .await
        .map_err(|e| e.to_string())?;
        let Some(picked) = picked else {
            return Ok(None);
        };
        let chosen = picked.into_path().map_err(|e| e.to_string())?;
        let parent = chosen
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .ok_or("Choose a file name inside a folder")?
            .to_path_buf();
        file_name = chosen
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(&file_name)
            .to_string();
        dir = parent;
    } else if !dir.is_absolute() {
        return Err("The save folder is not an absolute path. Choose it again in Settings.".into());
    }
    let written = tauri::async_runtime::spawn_blocking(move || {
        crate::media_store::copy_without_overwrite(&source, &dir, &file_name)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| format!("Could not save the file: {e}"))?;
    Ok(Some(written.to_string_lossy().into_owned()))
}

/// Show a saved file selected in the OS file manager.
#[tauri::command]
pub async fn reveal_in_file_manager(path: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::media_store::reveal_in_file_manager(std::path::Path::new(&path))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[derive(serde::Serialize)]
pub struct MediaCacheInfo {
    pub bytes: u64,
    pub files: u64,
    /// Where explicit saves go right now (configured folder or `<Documents>/shuttle`).
    pub save_dir: String,
}

#[tauri::command]
pub async fn media_cache_info(state: State<'_, AppState>) -> Result<MediaCacheInfo, String> {
    let data_dir = state.db.data_dir().to_path_buf();
    let configured = state.config.get().media.save_dir;
    tauri::async_runtime::spawn_blocking(move || {
        let stats = crate::media_store::cache_stats(&data_dir);
        MediaCacheInfo {
            bytes: stats.bytes,
            files: stats.files,
            save_dir: crate::media_store::resolve_save_dir(configured.as_deref())
                .to_string_lossy()
                .into_owned(),
        }
    })
    .await
    .map_err(|e| e.to_string())
}

#[derive(serde::Serialize)]
pub struct MediaPurgeResult {
    pub files: u64,
    pub bytes: u64,
}

/// Delete every downloaded file from the cache now (saved files are never touched). Messages
/// keep their thumbnail and can download again.
#[tauri::command]
pub async fn clear_media_cache(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<MediaPurgeResult, String> {
    let db = state.db.clone();
    purge_media_cache(&app, db, None).await
}

/// Purge cached downloads idle for `keep` (everything when `None`), reset the affected
/// messages' `media_path` through the database, and tell the UI.
async fn purge_media_cache(
    app: &AppHandle,
    db: Arc<Database>,
    keep: Option<Duration>,
) -> Result<MediaPurgeResult, String> {
    let data_dir = db.data_dir().to_path_buf();
    let purged = tauri::async_runtime::spawn_blocking(move || {
        let purged = crate::media_store::purge_cache(&data_dir, keep, std::time::SystemTime::now());
        let paths: std::collections::HashSet<String> = purged
            .media_files
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        let affected = db.clear_media_paths(&paths).unwrap_or_default();
        (purged, affected)
    })
    .await
    .map_err(|e| e.to_string())?;
    let (purged, affected) = purged;
    if !affected.is_empty() {
        let _ = app.emit(
            "shuttle-event",
            AppEvent {
                kind: "media.purged".into(),
                payload: serde_json::json!({
                    "accounts": affected.iter().map(|(a, _)| a).collect::<Vec<_>>(),
                }),
            },
        );
    }
    Ok(MediaPurgeResult {
        files: purged.files,
        bytes: purged.bytes,
    })
}

/// Startup pass (after a short delay so launch stays fast), then about once a day.
fn spawn_media_cleanup_loop(app: AppHandle, db: Arc<Database>, config: Arc<ConfigStore>) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(45)).await;
        let mut interval = tokio::time::interval(Duration::from_secs(24 * 60 * 60));
        loop {
            interval.tick().await;
            let Some(days) = config.get().media.keep_days else {
                continue; // keep forever
            };
            let keep = Duration::from_secs(u64::from(days.max(1)) * 86_400);
            if let Err(e) = purge_media_cache(&app, db.clone(), Some(keep)).await {
                tracing::warn!("media cache cleanup failed: {e}");
            }
        }
    });
}

#[tauri::command]
pub fn fetch_conversation_avatar(
    state: State<'_, AppState>,
    account_id: String,
    conversation_id: String,
) -> Result<(), String> {
    state
        .connectors
        .fetch_conversation_avatar(&account_id, &conversation_id)
}

#[tauri::command]
pub fn sync_conversation(
    state: State<'_, AppState>,
    account_id: String,
    conversation_id: String,
) -> Result<(), String> {
    state
        .connectors
        .sync_conversation(&account_id, &conversation_id)
}

#[tauri::command]
pub fn search_conversations(
    state: State<'_, AppState>,
    query: String,
) -> Result<Vec<Conversation>, String> {
    let started = std::time::Instant::now();
    let result = state.db.search(&query).map_err(|e| e.to_string());
    state.telemetry.time("search_chats", started);
    if result.is_ok() && !query.is_empty() {
        state.telemetry.track("search_used", serde_json::Map::new());
    }
    result
}

#[tauri::command]
pub fn search_messages(
    state: State<'_, AppState>,
    query: String,
    scope: String,
    account_id: Option<String>,
    conversation_id: Option<String>,
) -> Result<SearchResults, String> {
    let scope = match scope.as_str() {
        "account" => SearchScope::Account,
        "conversation" => SearchScope::Conversation,
        _ => SearchScope::Global,
    };
    let started = std::time::Instant::now();
    let result = state
        .db
        .search_messages(
            &query,
            scope,
            account_id.as_deref(),
            conversation_id.as_deref(),
        )
        .map_err(|e| e.to_string());
    state.telemetry.time("search_messages", started);
    if result.is_ok() && !query.is_empty() {
        state.telemetry.track("search_used", serde_json::Map::new());
    }
    result
}

#[tauri::command]
pub fn star_message(
    state: State<'_, AppState>,
    message_id: String,
    starred: bool,
) -> Result<Message, String> {
    state
        .db
        .set_message_starred(&message_id, starred)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn pin_message(
    state: State<'_, AppState>,
    message_id: String,
    pinned: bool,
) -> Result<Message, String> {
    state
        .db
        .set_message_pinned(&message_id, pinned)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn fetch_contact_profile(
    state: State<'_, AppState>,
    account_id: String,
    conversation_id: String,
) -> Result<ContactProfileBundle, String> {
    let conv = state
        .db
        .get_conversation(&conversation_id)
        .map_err(|e| e.to_string())?;
    let profile = conv
        .metadata
        .get("contact_profile")
        .and_then(|v| serde_json::from_value::<ContactProfile>(v.clone()).ok())
        .unwrap_or_default();
    let _ = state
        .connectors
        .fetch_contact_profile(&account_id, &conversation_id);
    Ok(ContactProfileBundle {
        profile,
        media: state
            .db
            .list_messages_by_kind(&conversation_id, "media", 40)
            .map_err(|e| e.to_string())?,
        docs: state
            .db
            .list_messages_by_kind(&conversation_id, "docs", 40)
            .map_err(|e| e.to_string())?,
        links: state
            .db
            .list_messages_by_kind(&conversation_id, "links", 40)
            .map_err(|e| e.to_string())?,
        starred: state
            .db
            .list_messages_by_kind(&conversation_id, "starred", 40)
            .map_err(|e| e.to_string())?,
    })
}

/// A group member's direct chat on the same account, or their contact entry when there is no
/// chat. Nothing is created.
#[tauri::command]
pub fn sender_contact(
    state: State<'_, AppState>,
    account_id: String,
    sender_id: String,
) -> Result<SenderContact, String> {
    state
        .db
        .sender_contact(&account_id, &sender_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn start_call(
    state: State<'_, AppState>,
    account_id: String,
    conversation_id: String,
    mode: String,
    share_screen: Option<bool>,
) -> Result<CallState, String> {
    state.connectors.start_call(
        &account_id,
        &conversation_id,
        &mode,
        share_screen.unwrap_or(false),
    )
}

#[tauri::command]
pub fn accept_call(
    state: State<'_, AppState>,
    account_id: String,
    call_id: String,
) -> Result<(), String> {
    state.connectors.accept_call(&account_id, &call_id)
}

#[tauri::command]
pub fn reject_call(
    state: State<'_, AppState>,
    account_id: String,
    call_id: String,
) -> Result<(), String> {
    state.connectors.reject_call(&account_id, &call_id)
}

#[tauri::command]
pub fn hangup_call(
    state: State<'_, AppState>,
    account_id: String,
    call_id: String,
) -> Result<(), String> {
    state.connectors.hangup_call(&account_id, &call_id)
}

#[tauri::command]
pub fn unread_by_account(
    state: State<'_, AppState>,
) -> Result<std::collections::HashMap<String, i64>, String> {
    state.db.unread_by_account().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn total_unread(state: State<'_, AppState>) -> Result<i64, String> {
    state.db.total_unread().map_err(|e| e.to_string())
}

/// The call in progress, so a window rebuilt from the tray puts its call panel back.
#[tauri::command]
pub fn current_call() -> Option<CallState> {
    crate::main_window::current_call()
}

/// Live performance settings, read by the main-window lifecycle as it hides and rebuilds.
pub fn performance_config<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> crate::config::PerformanceConfig {
    app.try_state::<AppState>()
        .map(|state| state.config.get().performance)
        .unwrap_or_default()
}

#[tauri::command]
pub async fn connect_account(
    state: State<'_, AppState>,
    account_id: String,
    credentials: Option<serde_json::Value>,
) -> Result<String, String> {
    let account = state
        .db
        .get_account(&account_id)
        .map_err(|e| e.to_string())?;
    if account.disabled {
        return Err("Account is disabled".into());
    }

    let incoming = credentials.unwrap_or(serde_json::json!({}));
    crate::secrets::save(state.connectors.data_dir(), &account_id, &incoming)?;
    let merged = crate::secrets::load(state.connectors.data_dir(), &account_id);

    state
        .db
        .update_account_status(&account_id, AccountStatus::Connecting)
        .map_err(|e| e.to_string())?;

    state
        .connectors
        .start_connector(&account.connector_id, &account_id, merged)
}

#[tauri::command]
pub fn submit_auth(
    state: State<'_, AppState>,
    account_id: String,
    credentials: serde_json::Value,
) -> Result<(), String> {
    crate::secrets::save(state.connectors.data_dir(), &account_id, &credentials)?;
    state.connectors.submit_auth(&account_id, credentials)
}

#[tauri::command]
pub fn get_app_config(state: State<'_, AppState>) -> AppConfig {
    state.config.get()
}

#[tauri::command]
pub fn save_app_config(
    state: State<'_, AppState>,
    mut config: AppConfig,
) -> Result<AppConfig, String> {
    // Zero days would wipe the whole cache on the next run; one day is the minimum.
    config.media.keep_days = config.media.keep_days.map(|d| d.max(1));
    config.media.save_dir = config
        .media
        .save_dir
        .take()
        .map(|d| d.trim().to_string())
        .filter(|d| !d.is_empty());
    if let Some(dir) = &config.media.save_dir {
        if !std::path::Path::new(dir).is_absolute() {
            return Err("The save folder must be an absolute path".into());
        }
    }
    let saved = state.config.save(config)?;
    state.telemetry.apply_consent();
    Ok(saved)
}

#[tauri::command]
pub async fn fetch_tweakcn_theme(theme_id: String) -> Result<String, String> {
    let id = theme_id.trim();
    if id.is_empty() {
        return Err("Theme id is required".into());
    }
    if !id.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err("Invalid theme id".into());
    }
    let url = format!("https://tweakcn.com/r/themes/{id}");
    let resp = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| format!("Failed to create theme request: {e}"))?
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("Failed to fetch theme: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("Theme not found ({})", resp.status()));
    }
    let body = resp.text().await.map_err(|e| e.to_string())?;
    serde_json::from_str::<serde_json::Value>(&body)
        .map_err(|e| format!("Invalid theme JSON: {e}"))?;
    Ok(body)
}

#[tauri::command]
pub async fn fetch_url_bytes(url: String) -> Result<String, String> {
    // Only allow fetching from known safe CDN hosts (Giphy)
    let is_allowed = url.starts_with("https://media.giphy.com/")
        || url.starts_with("https://media0.giphy.com/")
        || url.starts_with("https://media1.giphy.com/")
        || url.starts_with("https://media2.giphy.com/")
        || url.starts_with("https://media3.giphy.com/")
        || url.starts_with("https://media4.giphy.com/")
        || url.starts_with("https://i.giphy.com/")
        || url.starts_with("https://upload.giphy.com/");
    if !is_allowed {
        tracing::warn!("[fetch_url_bytes] host not allowed: {url}");
        return Err(format!("Host not allowed for fetch_url_bytes: {url}"));
    }
    tracing::info!("[fetch_url_bytes] fetching {url}");
    let resp = reqwest::get(&url).await.map_err(|e| {
        tracing::error!("[fetch_url_bytes] fetch error: {e}");
        format!("Fetch failed: {e}")
    })?;
    let status = resp.status();
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| format!("Read failed: {e}"))?;
    tracing::info!(
        "[fetch_url_bytes] got {} bytes, status={status}",
        bytes.len()
    );
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    Ok(STANDARD.encode(&bytes))
}

#[tauri::command]
pub fn telemetry_track(
    state: State<'_, AppState>,
    event: String,
    props: serde_json::Map<String, serde_json::Value>,
) -> Result<(), String> {
    state.telemetry.track(&event, props);
    Ok(())
}

#[tauri::command]
pub fn telemetry_error(
    state: State<'_, AppState>,
    message: String,
    context: serde_json::Map<String, serde_json::Value>,
) -> Result<(), String> {
    state.telemetry.error(&message, context);
    Ok(())
}

#[tauri::command]
pub fn telemetry_performance(
    state: State<'_, AppState>,
    operation: String,
    props: serde_json::Map<String, serde_json::Value>,
) -> Result<(), String> {
    state.telemetry.track_operation(&operation, props);
    Ok(())
}

#[tauri::command]
pub fn telemetry_installation_id(state: State<'_, AppState>) -> String {
    state.telemetry.installation_id().to_string()
}

#[tauri::command]
pub fn telemetry_set_foreground(
    state: State<'_, AppState>,
    foreground: bool,
) -> Result<(), String> {
    state.telemetry.set_foreground(foreground);
    Ok(())
}

#[tauri::command]
pub fn list_workspaces(state: State<'_, AppState>) -> Result<Vec<Workspace>, String> {
    state.db.list_workspaces().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn create_workspace(state: State<'_, AppState>, name: String) -> Result<Workspace, String> {
    state.db.create_workspace(&name).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn rename_workspace(
    state: State<'_, AppState>,
    id: String,
    name: String,
) -> Result<(), String> {
    state
        .db
        .rename_workspace(&id, &name)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_workspace(state: State<'_, AppState>, id: String) -> Result<(), String> {
    state.db.delete_workspace(&id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_priority_groups(state: State<'_, AppState>) -> Result<Vec<PriorityGroup>, String> {
    state.db.list_priority_groups().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn create_priority_group(
    state: State<'_, AppState>,
    name: String,
    color: Option<String>,
) -> Result<PriorityGroup, String> {
    state
        .db
        .create_priority_group(&name, color.as_deref())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn rename_priority_group(
    state: State<'_, AppState>,
    id: String,
    name: String,
) -> Result<(), String> {
    state
        .db
        .rename_priority_group(&id, &name)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_priority_group(state: State<'_, AppState>, id: String) -> Result<(), String> {
    state
        .db
        .delete_priority_group(&id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_todos(
    state: State<'_, AppState>,
    conversation_id: String,
) -> Result<Vec<ChatTodo>, String> {
    state
        .db
        .list_todos(&conversation_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn add_todo(
    state: State<'_, AppState>,
    conversation_id: String,
    account_id: String,
    body: String,
    due_at: Option<String>,
    message_id: Option<String>,
    message_snippet: Option<String>,
) -> Result<ChatTodo, String> {
    state
        .db
        .add_todo_linked(
            &conversation_id,
            &account_id,
            &body,
            due_at.as_deref(),
            message_id.as_deref(),
            message_snippet.as_deref(),
        )
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_todo_done(state: State<'_, AppState>, id: String, done: bool) -> Result<(), String> {
    state.db.set_todo_done(&id, done).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_todo(state: State<'_, AppState>, id: String) -> Result<(), String> {
    state.db.delete_todo(&id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_notes(
    state: State<'_, AppState>,
    conversation_id: String,
) -> Result<Vec<ChatNote>, String> {
    state
        .db
        .list_notes(&conversation_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn add_note(
    state: State<'_, AppState>,
    conversation_id: String,
    account_id: String,
    body: String,
    message_id: Option<String>,
    message_snippet: Option<String>,
    schedule: Option<NoteSchedule>,
) -> Result<ChatNote, String> {
    state
        .db
        .create_note(
            &conversation_id,
            &account_id,
            &body,
            message_id.as_deref(),
            message_snippet.as_deref(),
            schedule.as_ref(),
        )
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn update_note(
    state: State<'_, AppState>,
    id: String,
    body: Option<String>,
    schedule: Option<NoteSchedule>,
    clear_schedule: Option<bool>,
) -> Result<ChatNote, String> {
    state
        .db
        .update_note(
            &id,
            body.as_deref(),
            schedule.as_ref(),
            clear_schedule.unwrap_or(false),
        )
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_note(state: State<'_, AppState>, id: String) -> Result<(), String> {
    state.db.delete_note(&id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_forward_rules(state: State<'_, AppState>) -> Result<Vec<ForwardRule>, String> {
    state.db.list_forward_rules().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn create_forward_rule(
    state: State<'_, AppState>,
    draft: ForwardRuleDraft,
) -> Result<ForwardRule, String> {
    state
        .db
        .create_forward_rule(&draft)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn update_forward_rule(
    state: State<'_, AppState>,
    id: String,
    patch: ForwardRulePatch,
) -> Result<ForwardRule, String> {
    state
        .db
        .patch_forward_rule(&id, &patch)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_forward_rule(state: State<'_, AppState>, id: String) -> Result<(), String> {
    state.db.delete_forward_rule(&id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_scheduled_messages(
    state: State<'_, AppState>,
    include_sent: Option<bool>,
) -> Result<Vec<ScheduledMessage>, String> {
    state
        .db
        .list_scheduled_messages(include_sent.unwrap_or(false))
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn schedule_message(
    state: State<'_, AppState>,
    draft: ScheduleMessageDraft,
) -> Result<ScheduledMessage, String> {
    state.db.schedule_message(&draft).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_scheduled_message(state: State<'_, AppState>, id: String) -> Result<(), String> {
    state
        .db
        .delete_scheduled_message(&id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn update_scheduled_message(
    state: State<'_, AppState>,
    id: String,
    body: Option<String>,
    send_at: Option<String>,
) -> Result<ScheduledMessage, String> {
    state
        .db
        .update_scheduled_message(&id, body.as_deref(), send_at.as_deref())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn export_backup(
    state: State<'_, AppState>,
    path: String,
    password: String,
    include_messages: Option<bool>,
    include_media: Option<bool>,
) -> Result<BackupManifest, String> {
    let includes_messages = include_messages.unwrap_or(true);
    let includes_media = include_media.unwrap_or(false);
    export_backup_bundle(
        state.connectors.data_dir(),
        &path,
        &password,
        includes_messages,
        includes_media,
    )
}

#[tauri::command]
pub fn restore_backup(
    state: State<'_, AppState>,
    path: String,
    password: String,
) -> Result<(), String> {
    restore_backup_bundle(state.connectors.data_dir(), &path, &password)
}

#[tauri::command]
pub fn restart_app(app: AppHandle) -> Result<(), String> {
    app.request_restart();
    Ok(())
}

#[tauri::command]
pub fn open_external(app: AppHandle, url: String) -> Result<(), String> {
    app.opener()
        .open_url(&url, None::<&str>)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn open_devtools(app: AppHandle) {
    #[cfg(debug_assertions)]
    if let Some(win) = app.get_webview_window("main") {
        win.open_devtools();
    }
    let _ = app;
}

/// Forward messages of one chat to chats on any account: each source message is sent again as
/// itself (text, or media with its caption). With `send_at` the forward is scheduled instead.
#[tauri::command]
pub async fn forward_messages(
    state: State<'_, AppState>,
    source_conversation_id: String,
    message_ids: Vec<String>,
    targets: Vec<crate::connectors::ForwardTarget>,
    send_at: Option<String>,
) -> Result<Vec<crate::connectors::ForwardResult>, String> {
    state
        .connectors
        .forward_messages(
            &source_conversation_id,
            &message_ids,
            &targets,
            send_at.as_deref(),
        )
        .await
}

pub fn spawn_event_forwarder(app: AppHandle, connectors: Arc<ConnectorManager>) {
    let mut rx = connectors.subscribe();
    tauri::async_runtime::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(event) => {
                    crate::main_window::note_event(&app, &event);
                    let _ = app.emit("shuttle-event", &event);
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => break,
            }
        }
    });
}

/// The one place note timers and alarms fire: poll for due notes, notify, mark fired, tell the UI.
fn spawn_reminder_loop(app: AppHandle, db: Arc<Database>) {
    tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(20));
        loop {
            interval.tick().await;
            let Ok(due) = db.due_notes() else {
                continue;
            };
            for note in due {
                let title = db
                    .get_conversation(&note.conversation_id)
                    .ok()
                    .map(|c| c.title)
                    .unwrap_or_else(|| "Chat reminder".into());
                let body = if note.body.trim().is_empty() {
                    note.message_snippet
                        .clone()
                        .unwrap_or_else(|| format!("Reminder for {title}"))
                } else {
                    note.body.clone()
                };
                notifications::notify(&title, &body);
                let _ = db.mark_note_fired(&note.id);
                let _ = app.emit(
                    "shuttle-event",
                    AppEvent {
                        kind: "reminder.fired".into(),
                        payload: serde_json::json!({
                            "note_id": note.id,
                            "conversation_id": note.conversation_id,
                            "account_id": note.account_id,
                        }),
                    },
                );
            }
        }
    });
}

fn spawn_scheduled_message_loop(
    app: AppHandle,
    db: Arc<Database>,
    connectors: Arc<ConnectorManager>,
) {
    tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(20));
        loop {
            interval.tick().await;
            let Ok(due) = db.due_scheduled_messages() else {
                continue;
            };
            for msg in due {
                let sent = if msg.forward {
                    connectors.send_scheduled_forward(&msg).await
                } else {
                    connectors
                        .send_message(
                            &msg.dest_account_id,
                            &msg.dest_conversation_id,
                            &msg.body,
                            None,
                            false,
                        )
                        .await
                        .map(drop)
                };
                match sent {
                    Ok(_) => {
                        let _ = db.mark_scheduled_message_sent(&msg.id);
                        let _ = app.emit(
                            "shuttle-event",
                            AppEvent {
                                kind: "scheduled_message.sent".into(),
                                payload: serde_json::json!({
                                    "scheduled_message_id": msg.id,
                                    "conversation_id": msg.dest_conversation_id,
                                    "account_id": msg.dest_account_id,
                                }),
                            },
                        );
                    }
                    Err(e) => {
                        let _ = db.mark_scheduled_message_attempt(&msg.id, &e, false);
                        let _ = app.emit(
                            "shuttle-event",
                            AppEvent {
                                kind: "scheduled_message.failed".into(),
                                payload: serde_json::json!({
                                    "scheduled_message_id": msg.id,
                                    "conversation_id": msg.dest_conversation_id,
                                    "account_id": msg.dest_account_id,
                                    "error": e,
                                }),
                            },
                        );
                    }
                }
            }
        }
    });
}

pub fn init_state(app: &AppHandle) -> AppState {
    let data_dir = dirs::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("shuttle");
    std::fs::create_dir_all(&data_dir).ok();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&data_dir, std::fs::Permissions::from_mode(0o700));
    }
    let db = Arc::new(Database::open(&data_dir).expect("Failed to open database"));
    if std::env::var("SHUTTLE_SEED_DEMO").ok().as_deref() == Some("1") {
        db.seed_demo_if_empty().ok();
    }
    let installation_id = db
        .ensure_installation_id()
        .expect("Failed to create installation id");
    let config = Arc::new(ConfigStore::open(&data_dir, db.clone()));

    let app_version = env!("CARGO_PKG_VERSION").to_string();
    let telemetry =
        TelemetryManager::new(&app_version, installation_id, config.clone(), db.clone());
    telemetry.track("app_started", serde_json::Map::new());
    telemetry.emit_database_initialized();
    telemetry.spawn_background_tasks();

    let (event_tx, _) = broadcast::channel(4096);
    let components = Arc::new(ComponentManager::new(&data_dir, event_tx.clone()));
    let connectors = Arc::new(ConnectorManager::new(
        db.clone(),
        config.clone(),
        data_dir.clone(),
        telemetry.clone(),
        components.clone(),
        event_tx.clone(),
    ));
    spawn_event_forwarder(app.clone(), connectors.clone());
    spawn_reminder_loop(app.clone(), db.clone());
    spawn_scheduled_message_loop(app.clone(), db.clone(), connectors.clone());
    spawn_media_cleanup_loop(app.clone(), db.clone(), config.clone());
    connectors.spawn_sleep_loop();
    connectors.resume_saved_accounts();

    let cache_dir = dirs::cache_dir()
        .unwrap_or_else(|| data_dir.clone())
        .join("shuttle")
        .join("updates");

    AppState {
        db,
        connectors,
        components,
        config,
        telemetry,
        updates: Arc::new(UpdateManager::new(cache_dir)),
    }
}

pub static APP_HANDLE: Mutex<Option<AppHandle>> = Mutex::new(None);

fn export_backup_bundle(
    data_dir: &Path,
    output_path: &str,
    password: &str,
    include_messages: bool,
    include_media: bool,
) -> Result<BackupManifest, String> {
    let temp = tempdir().map_err(|e| e.to_string())?;
    let root = temp.path().join("shuttle-backup");
    fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    copy_backup_subset(data_dir, &root, include_messages, include_media)
        .map_err(|e| e.to_string())?;
    if include_media {
        // The app-private download cache travels as `media_cache/` (see copy_backup_subset);
        // the legacy Documents folder, if present, as `user-media/`.
        let media_root = crate::media_store::shuttle_documents_root();
        if media_root.exists() {
            copy_dir_recursive(&media_root, &root.join("user-media")).map_err(|e| e.to_string())?;
        }
    }

    let manifest = BackupManifest {
        exported_at: chrono::Utc::now().to_rfc3339(),
        includes_messages: include_messages,
        includes_media: include_media,
    };
    fs::write(
        root.join("manifest.json"),
        serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;

    let mut tar_buf = Vec::new();
    {
        let mut builder = tar::Builder::new(&mut tar_buf);
        builder
            .append_dir_all("shuttle-backup", &root)
            .map_err(|e| e.to_string())?;
        builder.finish().map_err(|e| e.to_string())?;
    }

    let encryptor = Encryptor::with_user_passphrase(SecretString::new(password.to_string().into()));
    let file = fs::File::create(output_path).map_err(|e| e.to_string())?;
    let mut writer = encryptor.wrap_output(file).map_err(|e| e.to_string())?;
    writer.write_all(&tar_buf).map_err(|e| e.to_string())?;
    writer.finish().map_err(|e| e.to_string())?;
    Ok(manifest)
}

fn restore_backup_bundle(data_dir: &Path, input_path: &str, password: &str) -> Result<(), String> {
    let bytes = fs::read(input_path).map_err(|e| e.to_string())?;
    let decryptor = Decryptor::new(Cursor::new(bytes)).map_err(|e| e.to_string())?;
    if !decryptor.is_scrypt() {
        return Err("Backup was not encrypted with a passphrase".into());
    }
    let identity = age::scrypt::Identity::new(SecretString::new(password.to_string().into()));
    let mut reader = decryptor
        .decrypt(iter::once(&identity as &dyn age::Identity))
        .map_err(|e| e.to_string())?;
    let mut tar_buf = Vec::new();
    reader
        .read_to_end(&mut tar_buf)
        .map_err(|e| e.to_string())?;

    let temp = tempdir().map_err(|e| e.to_string())?;
    let mut archive = tar::Archive::new(Cursor::new(tar_buf));
    archive.unpack(temp.path()).map_err(|e| e.to_string())?;
    let unpacked = temp.path().join("shuttle-backup");
    if !unpacked.exists() {
        return Err("Backup archive missing shuttle-backup root".into());
    }
    let user_media = unpacked.join("user-media");
    let has_user_media = user_media.is_dir();
    merge_backup_tree(&unpacked, data_dir, has_user_media.then_some("user-media"))
        .map_err(|e| e.to_string())?;
    if has_user_media {
        let dest = crate::media_store::shuttle_documents_root();
        fs::create_dir_all(&dest).map_err(|e| e.to_string())?;
        merge_backup_tree(&user_media, &dest, None).map_err(|e| e.to_string())?;
    }
    restrict_restored_secrets(data_dir).map_err(|e| e.to_string())
}

fn copy_backup_subset(
    src: &Path,
    dest: &Path,
    include_messages: bool,
    include_media: bool,
) -> std::io::Result<()> {
    for name in [
        "config.json",
        "app.sqlite",
        "app.sqlite-wal",
        "app.sqlite-shm",
        "catalog.sqlite",
        "catalog.sqlite-wal",
        "catalog.sqlite-shm",
    ] {
        let from = src.join(name);
        if from.exists() {
            fs::copy(&from, dest.join(name))?;
        }
    }
    for dir_name in ["connectors", "secrets"] {
        let from = src.join(dir_name);
        if from.exists() {
            copy_dir_recursive(&from, &dest.join(dir_name))?;
        }
    }
    // GOWA's own folder holds the WhatsApp session and chat store; its `statics/media`
    // scratch copies of downloads are a re-creatable duplicate of the media cache.
    let gowa = src.join("gowa");
    if gowa.exists() {
        copy_dir_skipping(
            &gowa,
            &dest.join("gowa"),
            Path::new(""),
            &Path::new("statics").join("media"),
        )?;
    }
    if include_media {
        let cache = crate::media_store::media_cache_root(src);
        if cache.exists() {
            copy_dir_recursive(&cache, &dest.join("media_cache"))?;
        }
    }
    if include_messages {
        let from = src.join("accounts");
        if from.exists() {
            copy_dir_recursive(&from, &dest.join("accounts"))?;
        }
    }
    Ok(())
}

fn merge_backup_tree(src: &Path, dest: &Path, skip_dir: Option<&str>) -> std::io::Result<()> {
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let name = entry.file_name();
        if skip_dir.is_some_and(|skip| name == skip) {
            continue;
        }
        let file_type = entry.file_type()?;
        let from = entry.path();
        let to = dest.join(&name);
        if file_type.is_dir() {
            copy_dir_recursive(&from, &to)?;
        } else {
            if let Some(parent) = to.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(from, to)?;
        }
    }
    Ok(())
}

fn restrict_restored_secrets(data_dir: &Path) -> std::io::Result<()> {
    let secrets_dir = data_dir.join("secrets");
    if !secrets_dir.is_dir() {
        return Ok(());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&secrets_dir, fs::Permissions::from_mode(0o700));
        for entry in fs::read_dir(&secrets_dir)? {
            let entry = entry?;
            if entry.file_type()?.is_file() {
                let _ = fs::set_permissions(entry.path(), fs::Permissions::from_mode(0o600));
            }
        }
    }
    Ok(())
}

/// `copy_dir_recursive` without the sub-folder `skip` (a path relative to the copy root).
fn copy_dir_skipping(src: &Path, dest: &Path, rel: &Path, skip: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dest)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let rel_path = rel.join(entry.file_name());
        if rel_path == skip {
            continue;
        }
        let to = dest.join(entry.file_name());
        if file_type.is_dir() {
            copy_dir_skipping(&entry.path(), &to, &rel_path, skip)?;
        } else {
            fs::copy(entry.path(), to)?;
        }
    }
    Ok(())
}

fn copy_dir_recursive(src: &Path, dest: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dest)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let from = entry.path();
        let to = dest.join(entry.file_name());
        if file_type.is_dir() {
            copy_dir_recursive(&from, &to)?;
        } else {
            fs::copy(from, to)?;
        }
    }
    Ok(())
}
