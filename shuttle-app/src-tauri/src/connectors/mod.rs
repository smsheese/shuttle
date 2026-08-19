mod protocol;

pub use protocol::*;

use crate::components::ComponentManager;
use crate::config::ConfigStore;
use crate::db::Database;
use crate::models::*;
use crate::notifications;
use crate::telemetry::TelemetryManager;
use chrono::{DateTime, Utc};
use parking_lot::Mutex;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{broadcast, mpsc};
use uuid::Uuid;

#[derive(Clone, serde::Serialize)]
pub struct AppEvent {
    pub kind: String,
    pub payload: serde_json::Value,
}

struct RunningConnector {
    child: Child,
    tx: mpsc::UnboundedSender<String>,
    generation: u64,
}

pub struct ConnectorManager {
    db: Arc<Database>,
    config: Arc<ConfigStore>,
    telemetry: Arc<TelemetryManager>,
    processes: Mutex<HashMap<String, RunningConnector>>,
    do_not_restart: Mutex<HashSet<String>>,
    generation: AtomicU64,
    event_tx: broadcast::Sender<AppEvent>,
    components: Arc<ComponentManager>,
    data_dir: PathBuf,
}

impl ConnectorManager {
    pub fn new(
        db: Arc<Database>,
        config: Arc<ConfigStore>,
        data_dir: PathBuf,
        telemetry: Arc<TelemetryManager>,
        components: Arc<ComponentManager>,
        event_tx: broadcast::Sender<AppEvent>,
    ) -> Self {
        Self {
            db,
            config,
            telemetry,
            processes: Mutex::new(HashMap::new()),
            do_not_restart: Mutex::new(HashSet::new()),
            generation: AtomicU64::new(0),
            event_tx,
            components,
            data_dir,
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<AppEvent> {
        self.event_tx.subscribe()
    }

    pub fn data_dir(&self) -> &PathBuf {
        &self.data_dir
    }

    pub fn resume_saved_accounts(self: &Arc<Self>) {
        let accounts = match self.db.list_accounts() {
            Ok(accounts) => accounts,
            Err(e) => {
                tracing::warn!("resume accounts: {e}");
                return;
            }
        };
        for account in accounts {
            if account.disabled {
                continue;
            }
            let connectors = Arc::clone(self);
            let connector_id = account.connector_id.clone();
            let account_id = account.id.clone();
            let creds = crate::secrets::load(&self.data_dir, &account_id);
            let _ = self
                .db
                .update_account_status(&account_id, AccountStatus::Connecting);
            self.emit(
                "account.status",
                serde_json::json!({
                    "account_id": account_id,
                    "status": "connecting",
                }),
            );
            tracing::info!(
                "resuming {} connector for account {}",
                connector_id,
                account.name
            );
            tauri::async_runtime::spawn(async move {
                if let Err(e) = connectors.start_connector(&connector_id, &account_id, creds) {
                    tracing::warn!("resume {account_id}: {e}");
                    let _ = connectors
                        .db
                        .update_account_status(&account_id, AccountStatus::Error);
                    connectors.emit(
                        "account.error",
                        serde_json::json!({
                            "account_id": account_id,
                            "message": e,
                        }),
                    );
                }
            });
        }
    }

    pub fn emit(&self, kind: &str, payload: serde_json::Value) {
        let _ = self.event_tx.send(AppEvent {
            kind: kind.to_string(),
            payload,
        });
    }

    pub fn list_connectors(&self) -> Vec<ConnectorInfo> {
        vec![
            ConnectorInfo {
                id: "whatsapp".into(),
                name: "WhatsApp".into(),
                description: "Scan QR code with WhatsApp on your phone".into(),
                auth_type: "qr".into(),
                capabilities: vec![
                    "text".into(),
                    "media".into(),
                    "read_receipts".into(),
                    "groups".into(),
                ],
            },
            ConnectorInfo {
                id: "telegram".into(),
                name: "Telegram".into(),
                description: "Log in with your phone number".into(),
                auth_type: "phone".into(),
                capabilities: vec![
                    "text".into(),
                    "media".into(),
                    "read_receipts".into(),
                    "groups".into(),
                    "channels".into(),
                    "calls:audio".into(),
                ],
            },
            ConnectorInfo {
                id: "signal".into(),
                name: "Signal".into(),
                description: "Register with your phone number".into(),
                auth_type: "phone".into(),
                capabilities: vec![
                    "text".into(),
                    "media".into(),
                    "read_receipts".into(),
                    "groups".into(),
                    "calls:audio".into(),
                ],
            },
            ConnectorInfo {
                id: "messenger".into(),
                name: "Messenger".into(),
                description: "Log in with Facebook email and password".into(),
                auth_type: "password".into(),
                capabilities: vec!["text".into(), "media".into(), "groups".into()],
            },
            ConnectorInfo {
                id: "instagram".into(),
                name: "Instagram".into(),
                description: "Log in to Instagram DMs".into(),
                auth_type: "password".into(),
                capabilities: vec!["text".into(), "media".into()],
            },
            ConnectorInfo {
                id: "email".into(),
                name: "Email".into(),
                description: "Connect IMAP and SMTP".into(),
                auth_type: "email".into(),
                capabilities: vec!["text".into()],
            },
            ConnectorInfo {
                id: "matrix".into(),
                name: "Matrix".into(),
                description: "Log in to a Matrix homeserver".into(),
                auth_type: "password".into(),
                capabilities: vec!["text".into(), "groups".into(), "channels".into()],
            },
        ]
    }

    pub fn components(&self) -> Arc<ComponentManager> {
        self.components.clone()
    }

    fn connector_script(&self, connector_id: &str) -> Result<PathBuf, String> {
        self.components().connector_script(connector_id)
    }

    fn spawn_connector_process(
        &self,
        connector_id: &str,
        account_id: &str,
    ) -> Result<tokio::process::Child, String> {
        let entry = self.connector_script(connector_id)?;
        let is_python = entry
            .extension()
            .is_some_and(|ext| ext == "py" || ext == "pyw");
        let mut command = if is_python {
            let (python, extra_args) = self.components.python_launcher();
            let mut cmd = Command::new(python);
            for arg in extra_args {
                cmd.arg(arg);
            }
            if let Some(parent) = entry.parent() {
                let merged = self
                    .components
                    .pythonpath_for_connector(connector_id, parent)
                    .or_else(|| std::env::var_os("PYTHONPATH"))
                    .unwrap_or_else(|| parent.as_os_str().into());
                cmd.env("PYTHONPATH", merged);
            }
            cmd.arg(&entry);
            cmd
        } else {
            Command::new(&entry)
        };
        let files_dir = crate::media_store::account_files_root(account_id);
        let _ = crate::media_store::ensure_account_dirs(account_id);
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env("SHUTTLE_ACCOUNT_ID", account_id)
            .env("SHUTTLE_DATA_DIR", &self.data_dir)
            .env("SHUTTLE_FILES_DIR", &files_dir)
            .env("SHUTTLE_GOWA_BIN", self.components.gowa_binary())
            .env("SHUTTLE_TDLIB", self.components.tdlib_path())
            .env("SHUTTLE_SIGNAL_CLI", self.components.signal_cli())
            .spawn()
            .map_err(|e| format!("Failed to spawn connector: {e}"))
    }

    pub fn start_connector(
        self: &Arc<Self>,
        connector_id: &str,
        account_id: &str,
        credentials: serde_json::Value,
    ) -> Result<String, String> {
        match self.db.get_account(account_id) {
            Ok(account) if account.disabled => return Err("Account is disabled".into()),
            Err(e) => return Err(e.to_string()),
            Ok(_) => {}
        }
        self.do_not_restart.lock().remove(account_id);
        self.components
            .ensure_connector_installed(connector_id)
            .map_err(|e| format!("Connector components not ready: {e}"))?;
        self.kill_running(account_id);
        let mut child = self.spawn_connector_process(connector_id, account_id)?;
        let generation = self.generation.fetch_add(1, Ordering::Relaxed) + 1;

        let stdout = child.stdout.take().ok_or("No stdout")?;
        let mut stdin = child.stdin.take().ok_or("No stdin")?;
        if let Some(stderr) = child.stderr.take() {
            tokio::spawn(async move {
                let mut reader = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = reader.next_line().await {
                    tracing::info!(target: "connector", "{line}");
                }
            });
        }
        let (tx, mut rx) = mpsc::unbounded_channel::<String>();

        tokio::spawn(async move {
            use tokio::io::AsyncWriteExt;
            while let Some(line) = rx.recv().await {
                if stdin.write_all(line.as_bytes()).await.is_err() {
                    break;
                }
                if stdin.flush().await.is_err() {
                    break;
                }
            }
        });

        let handshake = ConnectorRequest::Handshake {
            protocol_version: PROTOCOL_VERSION,
        };
        let line = encode_line(&handshake).map_err(|e| e.to_string())?;
        tx.send(line).map_err(|e| e.to_string())?;

        let db = self.db.clone();
        let config = self.config.clone();
        let telemetry = self.telemetry.clone();
        let event_tx = self.event_tx.clone();
        let account_id_owned = account_id.to_string();
        let stdin_tx = tx.clone();
        let this = Arc::clone(self);
        let connector_id_owned = connector_id.to_string();

        self.processes.lock().insert(
            account_id.to_string(),
            RunningConnector {
                child,
                tx: tx.clone(),
                generation,
            },
        );

        tokio::spawn(async move {
            let mut reader = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = reader.next_line().await {
                if let Ok(raw) = serde_json::from_str::<serde_json::Value>(&line) {
                    if raw.get("type").and_then(|v| v.as_str()) == Some("telemetry") {
                        telemetry.handle_connector_payload(raw);
                        continue;
                    }
                }
                if let Ok(ConnectorEvent::Event {
                    event,
                    account_id,
                    payload,
                }) = decode_line::<ConnectorEvent>(&line)
                {
                    if event == "account.connected" {
                        if let Ok(sync_line) = encode_line(&ConnectorRequest::SyncHistory {
                            account_id: account_id.clone(),
                        }) {
                            let _ = stdin_tx.send(sync_line);
                        }
                    }
                    handle_connector_event(&db, &config, &event_tx, &event, &account_id, payload).await;
                } else if let Ok(resp) = decode_line::<ConnectorResponse>(&line) {
                    match resp {
                        ConnectorResponse::AuthRequired {
                            method,
                            qr_data,
                            url,
                            message,
                        } => {
                            let _ = event_tx.send(AppEvent {
                                kind: "auth.required".into(),
                                payload: serde_json::json!({
                                    "account_id": account_id_owned,
                                    "method": method,
                                    "qr_data": qr_data,
                                    "url": url,
                                    "message": message,
                                }),
                            });
                        }
                        ConnectorResponse::Status {
                            account_id,
                            status,
                            identity,
                        } => {
                            let st = match status.as_str() {
                                "connected" => AccountStatus::Connected,
                                "connecting" => AccountStatus::Connecting,
                                "error" => AccountStatus::Error,
                                "awaiting_auth" => AccountStatus::AwaitingAuth,
                                _ => AccountStatus::Disconnected,
                            };
                            let _ = db.update_account_status(&account_id, st);
                            if let Some(id) = identity.as_deref() {
                                let _ = db.update_account_identity(&account_id, id);
                            }
                            let _ = event_tx.send(AppEvent {
                                kind: "account.status".into(),
                                payload: serde_json::json!({
                                    "account_id": account_id,
                                    "status": status,
                                    "identity": identity,
                                }),
                            });
                        }
                        ConnectorResponse::Error { message } => {
                            let _ = db.update_account_status(&account_id_owned, AccountStatus::Error);
                            let _ = event_tx.send(AppEvent {
                                kind: "account.error".into(),
                                payload: serde_json::json!({
                                    "account_id": account_id_owned,
                                    "message": message,
                                }),
                            });
                        }
                        ConnectorResponse::ContactProfile {
                            conversation_id,
                            profile,
                            ..
                        } => {
                            if let Some(remote) = conversation_id.as_deref() {
                                let _ = db.merge_conversation_metadata(
                                    &account_id_owned,
                                    remote,
                                    &serde_json::json!({ "contact_profile": profile }),
                                );
                                let _ = event_tx.send(AppEvent {
                                    kind: "contact.profile".into(),
                                    payload: serde_json::json!({
                                        "account_id": account_id_owned,
                                        "remote_id": remote,
                                        "profile": profile,
                                    }),
                                });
                            }
                        }
                        _ => {}
                    }
                }
            }
            this.on_connector_exit(&account_id_owned, &connector_id_owned, generation)
                .await;
        });

        let auth_req = ConnectorRequest::Authenticate {
            account_id: account_id.to_string(),
            credentials,
        };
        let line = encode_line(&auth_req).map_err(|e| e.to_string())?;
        tx.send(line).map_err(|e| e.to_string())?;

        Ok("Connector started".into())
    }

    pub fn submit_auth(&self, account_id: &str, credentials: serde_json::Value) -> Result<(), String> {
        self.send_to_connector(
            account_id,
            &ConnectorRequest::SubmitAuth {
                account_id: account_id.to_string(),
                credentials,
            },
        )
    }

    pub fn wipe_account(&self, account_id: &str, connector_id: &str) {
        self.stop_connector(account_id);
        crate::secrets::delete(&self.data_dir, account_id);
        let dir = self
            .data_dir
            .join("connectors")
            .join(connector_id)
            .join(account_id);
        let _ = std::fs::remove_dir_all(dir);
    }

    fn send_to_connector(&self, account_id: &str, req: &ConnectorRequest) -> Result<(), String> {
        let line = encode_line(req).map_err(|e| e.to_string())?;
        let map = self.processes.lock();
        let proc = map
            .get(account_id)
            .ok_or_else(|| "Connector is not running for this account".to_string())?;
        proc.tx.send(line).map_err(|e| e.to_string())
    }

    pub async fn send_message(
        &self,
        account_id: &str,
        conversation_id: &str,
        text: &str,
    ) -> Result<Message, String> {
        let conv = self.db.get_conversation(conversation_id).map_err(|e| e.to_string())?;
        let msg = Message {
            id: Uuid::new_v4().to_string(),
            conversation_id: conversation_id.to_string(),
            remote_id: None,
            sender_id: None,
            sender_name: Some("You".into()),
            direction: MessageDirection::Outbound,
            body: text.to_string(),
            timestamp: Utc::now(),
            status: MessageStatus::Pending,
            metadata: serde_json::json!({}),
            starred: false,
            pinned: false,
        };
        self.db.insert_message(&msg).map_err(|e| e.to_string())?;
        let conv = self.db.get_conversation(conversation_id).unwrap_or(conv);

        let _ = self.send_to_connector(
            account_id,
            &ConnectorRequest::SendMessage {
                account_id: account_id.to_string(),
                conversation_id: conv.remote_id.clone(),
                text: text.to_string(),
            },
        );

        self.emit(
            "message.sent",
            serde_json::json!({
                "account_id": account_id,
                "conversation_id": conversation_id,
                "conversation": conv,
                "message": msg,
            }),
        );

        Ok(msg)
    }

    pub async fn send_attachment(
        &self,
        account_id: &str,
        conversation_id: &str,
        kind: &str,
        caption: Option<&str>,
        filename: Option<&str>,
        mime: Option<&str>,
        data_base64: Option<&str>,
        latitude: Option<f64>,
        longitude: Option<f64>,
        question: Option<&str>,
        options: Vec<String>,
        max_answer: Option<i32>,
    ) -> Result<Message, String> {
        let conv = self.db.get_conversation(conversation_id).map_err(|e| e.to_string())?;
        let body = match kind {
            "location" => caption
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| "[Location]".into()),
            "poll" => question
                .filter(|s| !s.is_empty())
                .or(caption)
                .map(str::to_string)
                .unwrap_or_else(|| "[Poll]".into()),
            "ptt" | "audio" => caption
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| "[Audio]".into()),
            "image" | "gif" => caption
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| "[Image]".into()),
            "video" => caption
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| "[Video]".into()),
            "sticker" => "[Sticker]".into(),
            _ => caption
                .filter(|s| !s.is_empty())
                .or(filename)
                .map(str::to_string)
                .unwrap_or_else(|| "[Document]".into()),
        };
        let mut metadata = serde_json::json!({
            "media_type": kind,
        });
        let resolved_filename = filename.map(str::to_string).or_else(|| {
            mime.map(|m| format!("file{}", crate::media_store::mime_to_ext(m)))
        });
        if let Some(name) = resolved_filename.as_deref() {
            metadata["filename"] = serde_json::Value::String(name.to_string());
        }
        if let Some(m) = mime {
            metadata["mime"] = serde_json::Value::String(m.to_string());
        }
        if let (Some(lat), Some(lng)) = (latitude, longitude) {
            metadata["latitude"] = serde_json::json!(lat);
            metadata["longitude"] = serde_json::json!(lng);
        }
        if kind == "poll" {
            if let Some(q) = question {
                metadata["question"] = serde_json::Value::String(q.to_string());
            }
            metadata["options"] = serde_json::json!(options.clone());
        }
        if let Some(b64) = data_base64.filter(|s| !s.is_empty()) {
            let mime_type = mime.unwrap_or("application/octet-stream");
            metadata["media_data"] = serde_json::Value::String(format!(
                "data:{mime_type};base64,{b64}"
            ));
        }
        let msg = Message {
            id: Uuid::new_v4().to_string(),
            conversation_id: conversation_id.to_string(),
            remote_id: None,
            sender_id: None,
            sender_name: Some("You".into()),
            direction: MessageDirection::Outbound,
            body,
            timestamp: Utc::now(),
            status: MessageStatus::Pending,
            metadata,
            starred: false,
            pinned: false,
        };
        self.db.insert_message(&msg).map_err(|e| e.to_string())?;
        let conv = self.db.get_conversation(conversation_id).unwrap_or(conv);

        let b64_len = data_base64.map(|s| s.len()).unwrap_or(0);
        tracing::info!("[send_attachment] kind={kind} mime={} b64_len={b64_len} filename={:?} remote_id={}",
            mime.unwrap_or("none"), resolved_filename, conv.remote_id);
        if let Err(e) = self.send_to_connector(
            account_id,
            &ConnectorRequest::SendAttachment {
                account_id: account_id.to_string(),
                conversation_id: conv.remote_id.clone(),
                kind: kind.to_string(),
                caption: caption.map(str::to_string),
                text: caption.map(str::to_string),
                filename: resolved_filename,
                mime: mime.map(str::to_string),
                data_base64: data_base64.map(str::to_string),
                path: None,
                latitude,
                longitude,
                question: question.map(str::to_string),
                options,
                max_answer,
            },
        ) {
            tracing::error!("[send_attachment] send_to_connector failed: {e}");
        }

        self.emit(
            "message.sent",
            serde_json::json!({
                "account_id": account_id,
                "conversation_id": conversation_id,
                "conversation": conv,
                "message": msg,
            }),
        );

        Ok(msg)
    }

    pub fn mark_read_remote(&self, account_id: &str, conversation_id: &str) -> Result<(), String> {
        let conv = self.db.get_conversation(conversation_id).map_err(|e| e.to_string())?;
        self.send_to_connector(
            account_id,
            &ConnectorRequest::MarkRead {
                account_id: account_id.to_string(),
                conversation_id: conv.remote_id,
            },
        )
    }

    pub fn download_message_media(
        &self,
        account_id: &str,
        conversation_id: &str,
        message_id: &str,
    ) -> Result<(), String> {
        let conv = self.db.get_conversation(conversation_id).map_err(|e| e.to_string())?;
        let msg = self
            .db
            .get_message(conversation_id, message_id)
            .map_err(|e| e.to_string())?;
        let remote_id = msg
            .remote_id
            .filter(|s| !s.is_empty())
            .ok_or_else(|| "Message has no remote id".to_string())?;
        if self.send_to_connector(
            account_id,
            &ConnectorRequest::DownloadMedia {
                account_id: account_id.to_string(),
                conversation_id: conv.remote_id.clone(),
                message_id: remote_id.clone(),
            },
        ).is_err() {
            let _ = self.db.mark_message_media_error(conversation_id, &msg.id, "connector unavailable");
            return Err("Connector is not running for this account".into());
        }
        Ok(())
    }

    pub fn fetch_conversation_avatar(
        &self,
        account_id: &str,
        conversation_id: &str,
    ) -> Result<(), String> {
        let conv = self.db.get_conversation(conversation_id).map_err(|e| e.to_string())?;
        self.send_to_connector(
            account_id,
            &ConnectorRequest::FetchAvatar {
                account_id: account_id.to_string(),
                conversation_id: conv.remote_id,
            },
        )
    }

    pub fn sync_conversation(&self, account_id: &str, conversation_id: &str) -> Result<(), String> {
        let conv = self.db.get_conversation(conversation_id).map_err(|e| e.to_string())?;
        let since_message_id = self.db.last_inbound_remote_id(conversation_id).ok().flatten();
        self.send_to_connector(
            account_id,
            &ConnectorRequest::SyncChat {
                account_id: account_id.to_string(),
                conversation_id: conv.remote_id,
                since_message_id,
            },
        )
    }

    pub fn fetch_contact_profile(
        &self,
        account_id: &str,
        conversation_id: &str,
    ) -> Result<(), String> {
        let conv = self.db.get_conversation(conversation_id).map_err(|e| e.to_string())?;
        self.send_to_connector(
            account_id,
            &ConnectorRequest::FetchContactProfile {
                account_id: account_id.to_string(),
                conversation_id: conv.remote_id,
            },
        )
    }

    pub fn start_call(
        &self,
        account_id: &str,
        conversation_id: &str,
        mode: &str,
        share_screen: bool,
    ) -> Result<CallState, String> {
        let conv = self.db.get_conversation(conversation_id).map_err(|e| e.to_string())?;
        let call_id = Uuid::new_v4().to_string();
        self.send_to_connector(
            account_id,
            &ConnectorRequest::StartCall {
                account_id: account_id.to_string(),
                conversation_id: conv.remote_id.clone(),
                mode: mode.to_string(),
                share_screen,
            },
        )?;
        let state = CallState {
            call_id,
            conversation_id: conversation_id.to_string(),
            account_id: account_id.to_string(),
            direction: "outbound".into(),
            mode: mode.to_string(),
            status: "ringing".into(),
            remote_name: Some(conv.title.clone()),
        };
        self.emit("call.ringing", serde_json::to_value(&state).unwrap_or_default());
        Ok(state)
    }

    pub fn accept_call(&self, account_id: &str, call_id: &str) -> Result<(), String> {
        self.send_to_connector(
            account_id,
            &ConnectorRequest::AcceptCall {
                account_id: account_id.to_string(),
                call_id: call_id.to_string(),
            },
        )
    }

    pub fn reject_call(&self, account_id: &str, call_id: &str) -> Result<(), String> {
        self.send_to_connector(
            account_id,
            &ConnectorRequest::RejectCall {
                account_id: account_id.to_string(),
                call_id: call_id.to_string(),
            },
        )
    }

    pub fn hangup_call(&self, account_id: &str, call_id: &str) -> Result<(), String> {
        self.send_to_connector(
            account_id,
            &ConnectorRequest::HangupCall {
                account_id: account_id.to_string(),
                call_id: call_id.to_string(),
            },
        )
    }

    pub fn create_group(
        &self,
        account_id: &str,
        title: &str,
        participants: Vec<String>,
    ) -> Result<(), String> {
        self.send_to_connector(
            account_id,
            &ConnectorRequest::CreateGroup {
                account_id: account_id.to_string(),
                title: title.to_string(),
                participants,
            },
        )
    }

    pub fn start_conversation(
        &self,
        account_id: &str,
        remote_id: &str,
        title: &str,
    ) -> Result<Conversation, String> {
        let remote = normalize_remote_id(remote_id);
        let ctype = if remote.ends_with("@g.us") {
            ConversationType::Group
        } else {
            ConversationType::Direct
        };
        let label = if title.trim().is_empty() {
            remote.clone()
        } else {
            title.trim().to_string()
        };
        self.db
            .upsert_conversation(account_id, &remote, &label, ctype, None, None, None, None, false)
            .map_err(|e| e.to_string())
    }

    pub fn stop_connector(&self, account_id: &str) {
        self.do_not_restart.lock().insert(account_id.to_string());
        self.kill_running(account_id);
    }

    fn kill_running(&self, account_id: &str) {
        if let Some(mut running) = self.processes.lock().remove(account_id) {
            let _ = running.tx.send(
                encode_line(&ConnectorRequest::Shutdown).unwrap_or_else(|_| "{\"type\":\"shutdown\"}\n".into()),
            );
            let _ = running.child.start_kill();
        }
    }

    async fn on_connector_exit(self: &Arc<Self>, account_id: &str, connector_id: &str, generation: u64) {
        {
            let mut map = self.processes.lock();
            match map.get(account_id) {
                Some(running) if running.generation == generation => {
                    map.remove(account_id);
                }
                _ => return,
            }
        }
        if self.do_not_restart.lock().contains(account_id) {
            return;
        }
        let account = match self.db.get_account(account_id) {
            Ok(account) if !account.disabled => account,
            _ => return,
        };
        let _ = self
            .db
            .update_account_status(account_id, AccountStatus::Connecting);
        self.emit(
            "account.status",
            serde_json::json!({
                "account_id": account_id,
                "status": "connecting",
            }),
        );
        tracing::warn!(
            "connector for {} exited; restarting in 2s",
            account.name
        );
        tokio::time::sleep(Duration::from_secs(2)).await;
        if self.do_not_restart.lock().contains(account_id) {
            return;
        }
        if self
            .processes
            .lock()
            .get(account_id)
            .is_some_and(|running| running.generation != generation)
        {
            return;
        }
        let creds = crate::secrets::load(&self.data_dir, account_id);
        if let Err(e) = self.start_connector(connector_id, account_id, creds) {
            tracing::warn!("restart {account_id}: {e}");
            let _ = self.db.update_account_status(account_id, AccountStatus::Error);
            self.emit(
                "account.error",
                serde_json::json!({
                    "account_id": account_id,
                    "message": e,
                }),
            );
        }
    }
}

fn normalize_remote_id(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.contains('@') {
        return trimmed.to_string();
    }
    let digits: String = trimmed.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        trimmed.to_string()
    } else {
        format!("{digits}@s.whatsapp.net")
    }
}

fn value_to_string(value: Option<&serde_json::Value>) -> Option<String> {
    match value {
        Some(serde_json::Value::String(s)) => {
            let t = s.trim();
            if t.is_empty() {
                None
            } else {
                Some(t.to_string())
            }
        }
        Some(serde_json::Value::Number(n)) => Some(n.to_string()),
        _ => None,
    }
}

fn parse_ts(value: Option<&str>) -> DateTime<Utc> {
    value
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|d| d.with_timezone(&Utc))
        .unwrap_or_else(Utc::now)
}

fn parse_ts_value(value: Option<&serde_json::Value>) -> DateTime<Utc> {
    match value {
        Some(serde_json::Value::String(s)) => {
            if let Ok(n) = s.parse::<i64>() {
                unix_to_utc(n)
            } else {
                parse_ts(Some(s))
            }
        }
        Some(serde_json::Value::Number(n)) => unix_to_utc(n.as_i64().unwrap_or(0)),
        _ => Utc::now(),
    }
}

fn unix_to_utc(n: i64) -> DateTime<Utc> {
    let secs = if n.abs() > 10_000_000_000 { n / 1000 } else { n };
    DateTime::from_timestamp(secs, 0).unwrap_or_else(Utc::now)
}

fn ts_str(value: Option<&serde_json::Value>) -> Option<String> {
    match value {
        None | Some(serde_json::Value::Null) => None,
        other => {
            let s = parse_ts_value(other).to_rfc3339();
            if s.starts_with("0001-") || s.starts_with("0000-") {
                None
            } else {
                Some(s)
            }
        }
    }
}

async fn handle_connector_event(
    db: &Database,
    config: &ConfigStore,
    event_tx: &broadcast::Sender<AppEvent>,
    event: &str,
    account_id: &str,
    payload: serde_json::Value,
) {
    match event {
        "conversation.updated" => {
            let remote_id = value_to_string(payload.get("remote_id")).unwrap_or_default();
            if remote_id.is_empty() {
                return;
            }
            let mut title = payload
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or(remote_id.as_str())
                .to_string();
            if let Ok(account) = db.get_account(account_id) {
                if let Some(identity) = account.identity.as_deref() {
                    if crate::media_store::jids_same(&remote_id, identity) {
                        if let Ok(contacts) = db.list_contacts(account_id) {
                            for contact in contacts {
                                if crate::media_store::jids_same(&contact.remote_id, &remote_id) {
                                    title = format!("{} (You)", contact.display_name);
                                    break;
                                }
                            }
                        }
                        if !title.ends_with("(You)") {
                            if let Some(name) = payload
                                .get("title")
                                .and_then(|v| v.as_str())
                                .filter(|t| !t.chars().all(|c| c.is_ascii_digit() || "+- ()".contains(c)))
                            {
                                if !name.is_empty() && !crate::media_store::jids_same(name, &remote_id) {
                                    title = format!("{name} (You)");
                                }
                            }
                        }
                    }
                }
            }
            let ctype = match payload.get("conversation_type").and_then(|v| v.as_str()) {
                Some("group") => ConversationType::Group,
                Some("channel") => ConversationType::Channel,
                _ => ConversationType::Direct,
            };
            let last_at = ts_str(payload.get("last_message_at"));
            let last_at = last_at.as_deref();
            let preview = payload.get("preview").and_then(|v| v.as_str());
            let archived = payload.get("archived").and_then(|v| v.as_bool());
            let pinned = payload.get("pinned").and_then(|v| v.as_bool());
            let force_recency = payload
                .get("force_recency")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if let Ok(conv) = db.upsert_conversation(
                account_id,
                &remote_id,
                &title,
                ctype,
                last_at,
                preview,
                archived,
                pinned,
                force_recency,
            ) {
                if let Some(rank) = payload.get("list_rank").and_then(|v| v.as_i64()) {
                    let _ = db.merge_conversation_metadata(
                        account_id,
                        &remote_id,
                        &serde_json::json!({ "list_rank": rank }),
                    );
                }
                if let Some(unread) = payload.get("unread_count").and_then(|v| v.as_i64()) {
                    let _ = db.set_unread_count(&conv.id, unread.max(0));
                }
                let conv = db.get_conversation(&conv.id).unwrap_or(conv);
                let history = payload.get("history").and_then(|v| v.as_bool()).unwrap_or(false);
                if !history {
                    let _ = event_tx.send(AppEvent {
                        kind: "conversation.updated".into(),
                        payload: serde_json::json!({ "account_id": account_id, "conversation": conv }),
                    });
                }
            }
        }
        "message.received" | "message.sent" => {
            let remote_jid = value_to_string(payload.get("conversation_id"))
                .or_else(|| value_to_string(payload.get("remote_id")))
                .unwrap_or_default();
            if remote_jid.is_empty() {
                return;
            }
            let msg_obj = payload.get("message").cloned().unwrap_or(payload.clone());
            let from_me = msg_obj
                .get("from_me")
                .and_then(|v| v.as_bool())
                .unwrap_or(event == "message.sent");
            let body = msg_obj
                .get("text")
                .and_then(|t| t.as_str())
                .or_else(|| msg_obj.get("body").and_then(|t| t.as_str()))
                .unwrap_or("")
                .to_string();
            let title = msg_obj
                .get("sender_name")
                .and_then(|v| v.as_str())
                .unwrap_or(remote_jid.as_str());
            let ctype = if remote_jid.ends_with("@g.us") {
                ConversationType::Group
            } else {
                ConversationType::Direct
            };
            let ts = parse_ts_value(msg_obj.get("timestamp"));
            let title = db
                .get_conversation_by_remote(account_id, &remote_jid)
                .ok()
                .flatten()
                .map(|c| c.title)
                .unwrap_or_else(|| title.to_string());
            let preview = msg_obj
                .get("preview")
                .and_then(|v| v.as_str())
                .map(String::from)
                .unwrap_or_else(|| {
                    let mut meta = msg_obj.clone();
                    if meta.get("media_type").is_none() {
                        if let Some(obj) = meta.as_object_mut() {
                            if let Some(t) = obj.get("text").and_then(|v| v.as_str()) {
                                if t.starts_with('[') && t.ends_with(']') {
                                    obj.insert(
                                        "media_type".into(),
                                        serde_json::Value::String(t[1..t.len() - 1].to_string()),
                                    );
                                }
                            }
                        }
                    }
                    crate::db::message_preview(&body, &meta)
                });
            let conv = match db.upsert_conversation(
                account_id,
                &remote_jid,
                &title,
                ctype,
                Some(&ts.to_rfc3339()),
                Some(preview.as_str()),
                None,
                None,
                false,
            ) {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!("upsert conversation: {e}");
                    return;
                }
            };
            let history = payload.get("history").and_then(|v| v.as_bool()).unwrap_or(false);
            let msg = Message {
                id: Uuid::new_v4().to_string(),
                conversation_id: conv.id.clone(),
                remote_id: value_to_string(msg_obj.get("id")),
                sender_id: value_to_string(msg_obj.get("sender_id")),
                sender_name: msg_obj
                    .get("sender_name")
                    .and_then(|v| v.as_str())
                    .map(String::from),
                direction: if from_me {
                    MessageDirection::Outbound
                } else {
                    MessageDirection::Inbound
                },
                body: body.clone(),
                timestamp: ts,
                status: if from_me {
                    MessageStatus::Sent
                } else {
                    MessageStatus::Delivered
                },
                metadata: msg_obj.clone(),
                starred: false,
                pinned: false,
            };
            let inserted = db.insert_message(&msg).unwrap_or(false);
            if inserted && !from_me && !history {
                let _ = db.increment_unread(&conv.id);
                let cfg = config.get();
                if notifications::should_notify_ids(db, &cfg, account_id, &conv.id) {
                    notifications::notify_message(
                        msg.sender_name.as_deref().unwrap_or("New message"),
                        &msg.body,
                    );
                }
                apply_forward_rules(db, &conv, &msg);
            }
            if !history {
                let conv = db.get_conversation(&conv.id).unwrap_or(conv);
                let unread_total = db.total_unread().unwrap_or(0);
                let _ = event_tx.send(AppEvent {
                    kind: event.to_string(),
                    payload: serde_json::json!({
                        "account_id": account_id,
                        "conversation_id": conv.id,
                        "conversation": conv,
                        "message": msg,
                        "unread_total": unread_total,
                    }),
                });
            }
        }
        "inbox.catchup" => {
            let _ = event_tx.send(AppEvent {
                kind: "inbox.catchup".into(),
                payload: serde_json::json!({ "account_id": account_id }),
            });
        }
        "account.connected" => {
            let _ = db.update_account_status(account_id, AccountStatus::Connected);
            if let Some(identity) = payload.get("identity").and_then(|v| v.as_str()) {
                let _ = db.update_account_identity(account_id, identity);
            }
            let _ = event_tx.send(AppEvent {
                kind: "account.connected".into(),
                payload: serde_json::json!({ "account_id": account_id }),
            });
            let _ = event_tx.send(AppEvent {
                kind: "history.sync.started".into(),
                payload: serde_json::json!({ "account_id": account_id }),
            });
        }
        "history.sync.completed" => {
            let _ = db.refresh_conversation_previews(account_id);
            if let Ok(account) = db.get_account(account_id) {
                if let Some(identity) = account.identity.as_deref() {
                    let _ = db.fix_self_conversation_titles(account_id, identity);
                }
            }
            let _ = event_tx.send(AppEvent {
                kind: "history.sync.completed".into(),
                payload: serde_json::json!({ "account_id": account_id }),
            });
        }
        "avatar.updated" => {
            let remote_id = value_to_string(payload.get("remote_id")).unwrap_or_default();
            if remote_id.is_empty() {
                return;
            }
            let mut patch = serde_json::Map::new();
            if let Some(data) = payload.get("avatar_data").and_then(|v| v.as_str()) {
                if !data.is_empty() {
                    patch.insert("avatar_data".into(), serde_json::Value::String(data.to_string()));
                }
            }
            if patch.is_empty() {
                return;
            }
            if let Ok(Some(conv)) =
                db.merge_conversation_metadata(account_id, &remote_id, &serde_json::Value::Object(patch))
            {
                let _ = event_tx.send(AppEvent {
                    kind: "avatar.updated".into(),
                    payload: serde_json::json!({
                        "account_id": account_id,
                        "conversation_id": conv.id,
                        "remote_id": remote_id,
                        "avatar_data": conv.metadata.get("avatar_data"),
                    }),
                });
            }
        }
        "media.downloaded" => {
            let jid = value_to_string(payload.get("conversation_id")).unwrap_or_default();
            let message_id = value_to_string(payload.get("message_id")).unwrap_or_default();
            if message_id.is_empty() {
                return;
            }
            let mut patch = serde_json::Map::new();
            if let Some(mt) = payload.get("media_type").cloned() {
                patch.insert("media_type".into(), mt);
            }
            if let Some(path) = payload.get("media_path").and_then(|v| v.as_str()) {
                if !path.is_empty() {
                    patch.insert("media_path".into(), serde_json::Value::String(path.to_string()));
                }
            }
            if let Some(data) = payload.get("media_data").cloned() {
                patch.insert("media_data".into(), data);
            }
            if let Some(err) = payload.get("error").cloned() {
                patch.insert("media_error".into(), err);
            }
            let merged = serde_json::Value::Object(patch);
            let msg = if !jid.is_empty() {
                db.merge_message_metadata_by_remote(account_id, &jid, &message_id, &merged)
                    .ok()
                    .flatten()
            } else {
                None
            };
            let msg = if msg.is_some() {
                msg
            } else {
                db.merge_message_metadata_by_remote_id(account_id, &message_id, &merged)
                    .ok()
                    .flatten()
            };
            if let Some(msg) = msg {
                let _ = event_tx.send(AppEvent {
                    kind: "media.downloaded".into(),
                    payload: serde_json::json!({
                        "account_id": account_id,
                        "conversation_id": msg.conversation_id,
                        "message": msg,
                    }),
                });
            }
        }
        "contacts.synced" => {
            if let Some(rows) = payload.get("contacts").and_then(|v| v.as_array()) {
                for row in rows {
                    let remote_id = value_to_string(row.get("remote_id")).unwrap_or_default();
                    let name = row
                        .get("display_name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    if remote_id.is_empty() || name.is_empty() {
                        continue;
                    }
                    let _ = db.upsert_contact(account_id, &remote_id, name);
                }
            }
            let _ = event_tx.send(AppEvent {
                kind: "contacts.synced".into(),
                payload: serde_json::json!({ "account_id": account_id }),
            });
        }
        e if e.starts_with("call.") => {
            let _ = event_tx.send(AppEvent {
                kind: e.to_string(),
                payload: payload,
            });
        }
        _ => {
            let _ = event_tx.send(AppEvent {
                kind: event.to_string(),
                payload: serde_json::json!({ "account_id": account_id, "data": payload }),
            });
        }
    }
}

fn apply_forward_rules(db: &Database, conv: &Conversation, msg: &Message) {
    let Ok(rules) = db.list_forward_rules() else {
        return;
    };
    let account = db.get_account(&conv.account_id).ok();
    let workspace = conv
        .workspace_id
        .as_deref()
        .or_else(|| account.as_ref().and_then(|a| a.workspace_id.as_deref()))
        .unwrap_or("others");
    let already_forwarded = msg
        .metadata
        .get("forwarded_from")
        .is_some();

    for rule in rules {
        if !rule.enabled {
            continue;
        }
        if rule.inbound_only && msg.direction != MessageDirection::Inbound {
            continue;
        }
        if !rule.include_self && rule.dest_account_id == conv.account_id && rule.dest_conversation_id == conv.id {
            continue;
        }
        if let Some(source_account_id) = &rule.source_account_id {
            if source_account_id != &conv.account_id {
                continue;
            }
        }
        if let Some(source_conversation_id) = &rule.source_conversation_id {
            if source_conversation_id != &conv.id {
                continue;
            }
        }
        if let Some(source_workspace_id) = &rule.source_workspace_id {
            if source_workspace_id != workspace {
                continue;
            }
        }
        if let Some(keyword) = &rule.keyword {
            if !msg.body.to_lowercase().contains(&keyword.to_lowercase()) {
                continue;
            }
        }
        if rule.skip_if_forwarded && already_forwarded {
            continue;
        }

        let sender_prefix = if rule.strip_sender {
            String::new()
        } else {
            msg.sender_name
                .as_deref()
                .map(|name| format!("{name}: "))
                .unwrap_or_default()
        };
        let mut body = String::new();
        if let Some(prefix) = &rule.prefix {
            if !prefix.is_empty() {
                body.push_str(prefix);
                body.push('\n');
            }
        }
        body.push_str(&sender_prefix);
        body.push_str(&msg.body);
        if let Some(suffix) = &rule.suffix {
            if !suffix.is_empty() {
                body.push('\n');
                body.push_str(suffix);
            }
        }

        let send_at = (Utc::now() + chrono::TimeDelta::seconds(rule.delay_seconds.max(0))).to_rfc3339();
        let _ = db.schedule_message(&ScheduleMessageDraft {
            source_account_id: Some(conv.account_id.clone()),
            source_conversation_id: Some(conv.id.clone()),
            source_message_id: Some(msg.id.clone()),
            dest_account_id: rule.dest_account_id.clone(),
            dest_conversation_id: rule.dest_conversation_id.clone(),
            body,
            send_at,
        });
    }
}

