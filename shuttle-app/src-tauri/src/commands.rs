use crate::components::ComponentManager;
use crate::config::{AppConfig, ConfigStore};
use crate::connectors::{AppEvent, ConnectorManager};
use crate::db::Database;
use crate::models::*;
use crate::notifications;
use crate::telemetry::TelemetryManager;
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
use tauri_plugin_opener::OpenerExt;
use tempfile::tempdir;

use tokio::sync::broadcast;

pub struct AppState {
    pub db: Arc<Database>,
    pub connectors: Arc<ConnectorManager>,
    pub components: Arc<ComponentManager>,
    pub config: Arc<ConfigStore>,
    pub telemetry: Arc<TelemetryManager>,
}

#[tauri::command]
pub fn list_accounts(state: State<'_, AppState>) -> Result<Vec<Account>, String> {
    state.db.list_accounts().map_err(|e| e.to_string())
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
    state.db.delete_account(&account_id).map_err(|e| e.to_string())?;
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
pub fn list_conversations(
    state: State<'_, AppState>,
    account_id: Option<String>,
    workspace_id: Option<String>,
    priority_group: Option<String>,
    archived_only: Option<bool>,
) -> Result<Vec<Conversation>, String> {
    state
        .db
        .list_conversations(
            account_id.as_deref(),
            workspace_id.as_deref(),
            priority_group.as_deref(),
            archived_only.unwrap_or(false),
        )
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_messages(
    state: State<'_, AppState>,
    conversation_id: String,
    limit: Option<i64>,
) -> Result<Vec<Message>, String> {
    state
        .db
        .list_messages(&conversation_id, limit.unwrap_or(100))
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn send_message(
    state: State<'_, AppState>,
    account_id: String,
    conversation_id: String,
    text: String,
) -> Result<Message, String> {
    state
        .connectors
        .send_message(&account_id, &conversation_id, &text)
        .await
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
    state.db.list_contacts(&account_id).map_err(|e| e.to_string())
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

#[tauri::command]
pub fn create_group(
    state: State<'_, AppState>,
    account_id: String,
    title: String,
    participants: Vec<String>,
) -> Result<(), String> {
    state.connectors.create_group(&account_id, &title, participants)
}

#[tauri::command]
pub fn download_message_media(
    state: State<'_, AppState>,
    account_id: String,
    conversation_id: String,
    message_id: String,
) -> Result<(), String> {
    state
        .connectors
        .download_message_media(&account_id, &conversation_id, &message_id)
}

#[tauri::command]
pub fn read_message_media(path: String) -> Result<String, String> {
    crate::media_store::read_as_data_url(&path)
}

#[tauri::command]
pub fn shuttle_files_root(account_id: String) -> Result<String, String> {
    Ok(crate::media_store::account_files_root(&account_id)
        .to_string_lossy()
        .into_owned())
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
pub fn search_conversations(state: State<'_, AppState>, query: String) -> Result<Vec<Conversation>, String> {
    let result = state.db.search(&query).map_err(|e| e.to_string());
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
    let result = state
        .db
        .search_messages(
            &query,
            scope,
            account_id.as_deref(),
            conversation_id.as_deref(),
        )
        .map_err(|e| e.to_string());
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
pub fn accept_call(state: State<'_, AppState>, account_id: String, call_id: String) -> Result<(), String> {
    state.connectors.accept_call(&account_id, &call_id)
}

#[tauri::command]
pub fn reject_call(state: State<'_, AppState>, account_id: String, call_id: String) -> Result<(), String> {
    state.connectors.reject_call(&account_id, &call_id)
}

#[tauri::command]
pub fn hangup_call(state: State<'_, AppState>, account_id: String, call_id: String) -> Result<(), String> {
    state.connectors.hangup_call(&account_id, &call_id)
}

#[tauri::command]
pub fn total_unread(state: State<'_, AppState>) -> Result<i64, String> {
    state.db.total_unread().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn connect_account(
    state: State<'_, AppState>,
    account_id: String,
    credentials: Option<serde_json::Value>,
) -> Result<String, String> {
    let account = state.db.get_account(&account_id).map_err(|e| e.to_string())?;
    if account.disabled {
        return Err("Account is disabled".into());
    }

    let incoming = credentials.unwrap_or(serde_json::json!({}));
    crate::secrets::save(&state.connectors.data_dir(), &account_id, &incoming)?;
    let merged = crate::secrets::load(&state.connectors.data_dir(), &account_id);

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
    crate::secrets::save(&state.connectors.data_dir(), &account_id, &credentials)?;
    state.connectors.submit_auth(&account_id, credentials)
}

#[tauri::command]
pub fn get_app_config(state: State<'_, AppState>) -> AppConfig {
    state.config.get()
}

#[tauri::command]
pub fn save_app_config(state: State<'_, AppState>, config: AppConfig) -> Result<AppConfig, String> {
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
    let resp = reqwest::get(&url)
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
    let bytes = resp.bytes().await.map_err(|e| format!("Read failed: {e}"))?;
    tracing::info!("[fetch_url_bytes] got {} bytes, status={status}", bytes.len());
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
pub fn telemetry_set_foreground(state: State<'_, AppState>, foreground: bool) -> Result<(), String> {
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
pub fn rename_workspace(state: State<'_, AppState>, id: String, name: String) -> Result<(), String> {
    state.db.rename_workspace(&id, &name).map_err(|e| e.to_string())
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
pub fn rename_priority_group(state: State<'_, AppState>, id: String, name: String) -> Result<(), String> {
    state
        .db
        .rename_priority_group(&id, &name)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_priority_group(state: State<'_, AppState>, id: String) -> Result<(), String> {
    state.db.delete_priority_group(&id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_todos(state: State<'_, AppState>, conversation_id: String) -> Result<Vec<ChatTodo>, String> {
    state.db.list_todos(&conversation_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn add_todo(
    state: State<'_, AppState>,
    conversation_id: String,
    account_id: String,
    body: String,
    due_at: Option<String>,
) -> Result<ChatTodo, String> {
    state
        .db
        .add_todo(&conversation_id, &account_id, &body, due_at.as_deref())
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
pub fn list_reminders(
    state: State<'_, AppState>,
    conversation_id: Option<String>,
) -> Result<Vec<Reminder>, String> {
    state
        .db
        .list_reminders(conversation_id.as_deref())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn create_reminder(
    state: State<'_, AppState>,
    conversation_id: String,
    account_id: String,
    fire_at: String,
    kind: Option<String>,
    note: Option<String>,
) -> Result<Reminder, String> {
    state
        .db
        .create_reminder(
            &conversation_id,
            &account_id,
            &fire_at,
            kind.as_deref().unwrap_or("nudge"),
            note.as_deref(),
        )
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_reminder(state: State<'_, AppState>, id: String) -> Result<(), String> {
    state.db.delete_reminder(&id).map_err(|e| e.to_string())
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
    state.db.create_forward_rule(&draft).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn update_forward_rule(
    state: State<'_, AppState>,
    id: String,
    patch: ForwardRulePatch,
) -> Result<ForwardRule, String> {
    state.db.patch_forward_rule(&id, &patch).map_err(|e| e.to_string())
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
    state.db.delete_scheduled_message(&id).map_err(|e| e.to_string())
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
) -> Result<BackupManifest, String> {
    let includes_messages = include_messages.unwrap_or(true);
    export_backup_bundle(state.connectors.data_dir(), &path, &password, includes_messages)
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
pub fn open_external(app: AppHandle, url: String) -> Result<(), String> {
    app.opener().open_url(&url, None::<&str>).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn open_devtools(app: AppHandle) {
    #[cfg(debug_assertions)]
    if let Some(win) = app.get_webview_window("main") {
        win.open_devtools();
    }
    let _ = app;
}

#[tauri::command]
pub async fn forward_message(
    state: State<'_, AppState>,
    dest_account_id: String,
    dest_conversation_id: String,
    text: String,
) -> Result<Message, String> {
    state
        .connectors
        .send_message(&dest_account_id, &dest_conversation_id, &text)
        .await
}

pub fn spawn_event_forwarder(app: AppHandle, connectors: Arc<ConnectorManager>) {
    let mut rx = connectors.subscribe();
    tauri::async_runtime::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(event) => {
                    let _ = app.emit("shuttle-event", &event);
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => break,
            }
        }
    });
}

fn spawn_reminder_loop(app: AppHandle, db: Arc<Database>) {
    tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(20));
        loop {
            interval.tick().await;
            let Ok(due) = db.due_reminders() else {
                continue;
            };
            for reminder in due {
                let title = db
                    .get_conversation(&reminder.conversation_id)
                    .ok()
                    .map(|c| c.title)
                    .unwrap_or_else(|| "Chat reminder".into());
                let body = reminder
                    .note
                    .clone()
                    .unwrap_or_else(|| format!("Reminder for {title}"));
                notifications::notify(&title, &body);
                let _ = db.mark_reminder_fired(&reminder.id);
                let _ = app.emit(
                    "shuttle-event",
                    AppEvent {
                        kind: "reminder.fired".into(),
                        payload: serde_json::json!({
                            "reminder_id": reminder.id,
                            "conversation_id": reminder.conversation_id,
                            "account_id": reminder.account_id,
                        }),
                    },
                );
            }
        }
    });
}

fn spawn_scheduled_message_loop(app: AppHandle, db: Arc<Database>, connectors: Arc<ConnectorManager>) {
    tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(20));
        loop {
            interval.tick().await;
            let Ok(due) = db.due_scheduled_messages() else {
                continue;
            };
            for msg in due {
                let sent = connectors
                    .send_message(&msg.dest_account_id, &msg.dest_conversation_id, &msg.body)
                    .await;
                if sent.is_ok() {
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
    let telemetry = TelemetryManager::new(&app_version, installation_id, config.clone(), db.clone());
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
    connectors.resume_saved_accounts();

    AppState {
        db,
        connectors,
        components,
        config,
        telemetry,
    }
}

pub static APP_HANDLE: Mutex<Option<AppHandle>> = Mutex::new(None);

fn export_backup_bundle(
    data_dir: &Path,
    output_path: &str,
    password: &str,
    include_messages: bool,
) -> Result<BackupManifest, String> {
    let temp = tempdir().map_err(|e| e.to_string())?;
    let root = temp.path().join("shuttle-backup");
    fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    copy_backup_subset(data_dir, &root, include_messages).map_err(|e| e.to_string())?;

    let manifest = BackupManifest {
        exported_at: chrono::Utc::now().to_rfc3339(),
        includes_messages: include_messages,
    };
    fs::write(
        root.join("manifest.json"),
        serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;

    let mut tar_buf = Vec::new();
    {
        let mut builder = tar::Builder::new(&mut tar_buf);
        builder.append_dir_all("shuttle-backup", &root).map_err(|e| e.to_string())?;
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
    reader.read_to_end(&mut tar_buf).map_err(|e| e.to_string())?;

    let temp = tempdir().map_err(|e| e.to_string())?;
    let mut archive = tar::Archive::new(Cursor::new(tar_buf));
    archive.unpack(temp.path()).map_err(|e| e.to_string())?;
    let unpacked = temp.path().join("shuttle-backup");
    if !unpacked.exists() {
        return Err("Backup archive missing shuttle-backup root".into());
    }
    merge_backup_tree(&unpacked, data_dir).map_err(|e| e.to_string())
}

fn copy_backup_subset(src: &Path, dest: &Path, include_messages: bool) -> std::io::Result<()> {
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
    for dir_name in ["connectors", "gowa", "secrets"] {
        let from = src.join(dir_name);
        if from.exists() {
            copy_dir_recursive(&from, &dest.join(dir_name))?;
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

fn merge_backup_tree(src: &Path, dest: &Path) -> std::io::Result<()> {
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let from = entry.path();
        let to = dest.join(entry.file_name());
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
