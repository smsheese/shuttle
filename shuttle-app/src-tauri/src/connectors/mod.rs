mod protocol;

pub use protocol::*;

use crate::config::ConfigStore;
use crate::db::Database;
use crate::models::*;
use crate::notifications;
use crate::telemetry::TelemetryManager;
use chrono::{DateTime, Utc};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
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
}

pub struct ConnectorManager {
    db: Arc<Database>,
    config: Arc<ConfigStore>,
    telemetry: Arc<TelemetryManager>,
    processes: Mutex<HashMap<String, RunningConnector>>,
    event_tx: broadcast::Sender<AppEvent>,
    connectors_dir: PathBuf,
    data_dir: PathBuf,
}

impl ConnectorManager {
    pub fn new(
        db: Arc<Database>,
        config: Arc<ConfigStore>,
        connectors_dir: PathBuf,
        data_dir: PathBuf,
        telemetry: Arc<TelemetryManager>,
    ) -> Self {
        let (event_tx, _) = broadcast::channel(256);
        Self {
            db,
            config,
            telemetry,
            processes: Mutex::new(HashMap::new()),
            event_tx,
            connectors_dir,
            data_dir,
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<AppEvent> {
        self.event_tx.subscribe()
    }

    pub fn data_dir(&self) -> &PathBuf {
        &self.data_dir
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
                ],
            },
            ConnectorInfo {
                id: "signal".into(),
                name: "Signal".into(),
                description: "Register with your phone number".into(),
                auth_type: "phone".into(),
                capabilities: vec!["text".into(), "media".into(), "read_receipts".into(), "groups".into()],
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

    pub fn connector_binary(&self, connector_id: &str) -> PathBuf {
        self.connectors_dir.join(format!("{connector_id}-connector"))
    }

    fn connector_script(&self, connector_id: &str) -> Result<PathBuf, String> {
        let script_name = format!("{connector_id}-connector.py");
        let bundled = self.connectors_dir.join(&script_name);
        if bundled.exists() {
            return Ok(bundled);
        }
        let dev = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../connectors")
            .join(&script_name);
        if dev.exists() {
            return Ok(dev);
        }
        let legacy = self.connector_binary(connector_id);
        if legacy.exists() {
            return Ok(legacy);
        }
        Err(format!(
            "Connector not found for {connector_id} (expected {} or {})",
            bundled.display(),
            legacy.display()
        ))
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
            let (python, extra_args) = self.python_launcher();
            let mut cmd = Command::new(python);
            for arg in extra_args {
                cmd.arg(arg);
            }
            if let Some(parent) = entry.parent() {
                let existing = std::env::var_os("PYTHONPATH");
                let merged = match existing {
                    Some(path) => {
                        let mut paths: Vec<PathBuf> = vec![parent.to_path_buf()];
                        paths.extend(std::env::split_paths(&path));
                        std::env::join_paths(paths).unwrap_or_else(|_| parent.as_os_str().into())
                    }
                    None => parent.as_os_str().into(),
                };
                cmd.env("PYTHONPATH", merged);
            }
            cmd.arg(&entry);
            cmd
        } else {
            Command::new(&entry)
        };
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env("SHUTTLE_ACCOUNT_ID", account_id)
            .env("SHUTTLE_DATA_DIR", &self.data_dir)
            .env("SHUTTLE_GOWA_BIN", self.gowa_binary())
            .env("SHUTTLE_TDLIB", self.tdlib_path())
            .env("SHUTTLE_SIGNAL_CLI", self.signal_cli())
            .spawn()
            .map_err(|e| format!("Failed to spawn connector: {e}"))
    }

    fn bundled_python_root(&self) -> PathBuf {
        self.connectors_dir
            .parent()
            .unwrap_or(&self.connectors_dir)
            .join("python-runtime")
    }

    fn find_bundled_python(&self) -> Option<PathBuf> {
        let root = self.bundled_python_root();
        let candidates: &[&[&str]] = if cfg!(windows) {
            &[
                &["python", "python.exe"],
                &["python.exe"],
                &["install", "python.exe"],
                &["python", "install", "python.exe"],
            ]
        } else {
            &[
                &["python", "bin", "python3"],
                &["python", "bin", "python"],
                &["bin", "python3"],
                &["bin", "python"],
                &["install", "bin", "python3"],
                &["python", "install", "bin", "python3"],
            ]
        };
        for parts in candidates {
            let mut path = root.clone();
            for part in *parts {
                path.push(part);
            }
            if path.exists() {
                return Some(path);
            }
        }
        None
    }

    fn python_launcher(&self) -> (String, Vec<String>) {
        if let Ok(p) = std::env::var("SHUTTLE_PYTHON") {
            return (p, Vec::new());
        }
        if let Some(bundled) = self.find_bundled_python() {
            return (bundled.to_string_lossy().into_owned(), Vec::new());
        }
        if cfg!(windows) {
            ("py".into(), vec!["-3".into()])
        } else {
            ("python3".into(), Vec::new())
        }
    }

    fn gowa_binary(&self) -> PathBuf {
        self.tool_path("SHUTTLE_GOWA_BIN", &["gowa", "whatsapp"])
    }

    fn tdlib_path(&self) -> PathBuf {
        if let Ok(p) = std::env::var("SHUTTLE_TDLIB") {
            return PathBuf::from(p);
        }
        let names: &[&str] = if cfg!(windows) {
            &["tdjson.dll", "libtdjson.dll"]
        } else if cfg!(target_os = "macos") {
            &["libtdjson.dylib", "tdjson.dylib"]
        } else {
            &["libtdjson.so", "tdjson.so"]
        };
        for name in names {
            let bundled = self.connectors_dir.join("tdlib").join(name);
            if bundled.exists() {
                return bundled;
            }
            let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../connectors/tdlib")
                .join(name);
            if repo.exists() {
                return repo;
            }
        }
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../connectors/tdlib/libtdjson.so")
    }

    fn signal_cli(&self) -> PathBuf {
        if let Ok(p) = std::env::var("SHUTTLE_SIGNAL_CLI") {
            return PathBuf::from(p);
        }
        for rel in [
            &["signal", "signal-cli"][..],
            &["signal", "signal-cli.exe"][..],
            &["signal", "signal-cli.bat"][..],
            &["signal", "runtime", "bin", "signal-cli"][..],
        ] {
            let mut bundled = self.connectors_dir.clone();
            for part in rel {
                bundled.push(part);
            }
            if bundled.exists() {
                return bundled;
            }
        }
        self.tool_path("SHUTTLE_SIGNAL_CLI", &["signal", "signal-cli"])
    }

    fn tool_path(&self, env_key: &str, rel: &[&str]) -> PathBuf {
        if let Ok(p) = std::env::var(env_key) {
            return PathBuf::from(p);
        }
        let mut bundled = self.connectors_dir.clone();
        for part in rel {
            bundled.push(part);
        }
        if bundled.exists() {
            return bundled;
        }
        if cfg!(windows) {
            let with_exe = bundled.with_extension("exe");
            if with_exe.exists() {
                return with_exe;
            }
        }
        let mut repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../connectors");
        for part in rel {
            repo.push(part);
        }
        if repo.exists() {
            return repo;
        }
        if cfg!(windows) {
            let with_exe = repo.with_extension("exe");
            if with_exe.exists() {
                return with_exe;
            }
        }
        repo
    }

    pub async fn start_connector(
        &self,
        connector_id: &str,
        account_id: &str,
        credentials: serde_json::Value,
    ) -> Result<String, String> {
        if let Ok(account) = self.db.get_account(account_id) {
            if account.disabled {
                return Err("Account is disabled".into());
            }
        }
        self.stop_connector(account_id);
        let mut child = self.spawn_connector_process(connector_id, account_id)?;

        let stdout = child.stdout.take().ok_or("No stdout")?;
        let mut stdin = child.stdin.take().ok_or("No stdin")?;
        if let Some(stderr) = child.stderr.take() {
            tokio::spawn(async move {
                let mut reader = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = reader.next_line().await {
                    tracing::debug!(target: "connector", "{line}");
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
                        _ => {}
                    }
                }
            }
        });

        self.processes.lock().insert(
            account_id.to_string(),
            RunningConnector { child, tx: tx.clone() },
        );

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
        };
        self.db.insert_message(&msg).map_err(|e| e.to_string())?;

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

    pub fn stop_connector(&self, account_id: &str) {
        if let Some(mut running) = self.processes.lock().remove(account_id) {
            let _ = running.tx.send(
                encode_line(&ConnectorRequest::Shutdown).unwrap_or_else(|_| "{\"type\":\"shutdown\"}\n".into()),
            );
            let _ = running.child.start_kill();
        }
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
        other => Some(parse_ts_value(other).to_rfc3339()),
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
            let title = payload
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or(remote_id.as_str());
            let ctype = match payload.get("conversation_type").and_then(|v| v.as_str()) {
                Some("group") => ConversationType::Group,
                Some("channel") => ConversationType::Channel,
                _ => ConversationType::Direct,
            };
            let last_at = ts_str(payload.get("last_message_at"));
            let last_at = last_at.as_deref();
            let preview = payload.get("preview").and_then(|v| v.as_str());
            let archived = payload.get("archived").and_then(|v| v.as_bool()).unwrap_or(false);
            if let Ok(conv) = db.upsert_conversation(
                account_id,
                &remote_id,
                title,
                ctype,
                last_at,
                preview,
                archived,
            ) {
                let _ = event_tx.send(AppEvent {
                    kind: "conversation.updated".into(),
                    payload: serde_json::json!({ "account_id": account_id, "conversation": conv }),
                });
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
            let conv = match db.upsert_conversation(
                account_id,
                &remote_jid,
                &title,
                ctype,
                Some(&ts.to_rfc3339()),
                Some(body.as_str()),
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
                let _ = event_tx.send(AppEvent {
                    kind: event.to_string(),
                    payload: serde_json::json!({
                        "account_id": account_id,
                        "conversation_id": conv.id,
                        "message": msg,
                    }),
                });
            }
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

