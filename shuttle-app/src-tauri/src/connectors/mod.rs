mod forward;
mod message_ops;
mod process_lock;
mod process_tree;
mod protocol;

pub use forward::{ForwardResult, ForwardTarget};
pub use message_ops::{message_actions, MessageOpResult};
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
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{broadcast, mpsc, oneshot};
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
    /// Shuttle account ids attached to this one sidecar process.
    accounts: HashSet<String>,
}

pub struct ConnectorManager {
    db: Arc<Database>,
    config: Arc<ConfigStore>,
    telemetry: Arc<TelemetryManager>,
    /// Keyed by `connector_id` (whatsapp, telegram, …), not account id.
    processes: Mutex<HashMap<String, RunningConnector>>,
    do_not_restart: Mutex<HashSet<String>>,
    sleeping: Mutex<HashSet<String>>,
    last_activity: Mutex<HashMap<String, Instant>>,
    last_check: Mutex<HashMap<String, Instant>>,
    active_account: Mutex<Option<String>>,
    start_lock: Mutex<()>,
    generation: AtomicU64,
    event_tx: broadcast::Sender<AppEvent>,
    components: Arc<ComponentManager>,
    data_dir: PathBuf,
    /// Message ops waiting for the connector's `ok` / `error`, by request id.
    pending_ops: Mutex<HashMap<String, oneshot::Sender<Result<(), String>>>>,
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
        process_tree::init();
        Self {
            db,
            config,
            telemetry,
            processes: Mutex::new(HashMap::new()),
            do_not_restart: Mutex::new(HashSet::new()),
            sleeping: Mutex::new(HashSet::new()),
            last_activity: Mutex::new(HashMap::new()),
            last_check: Mutex::new(HashMap::new()),
            active_account: Mutex::new(None),
            start_lock: Mutex::new(()),
            generation: AtomicU64::new(0),
            event_tx,
            components,
            data_dir,
            pending_ops: Mutex::new(HashMap::new()),
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
                actions: message_actions("whatsapp"),
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
                actions: message_actions("telegram"),
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
                ],
                actions: message_actions("signal"),
            },
            ConnectorInfo {
                id: "messenger".into(),
                name: "Messenger".into(),
                description: "Log in with Facebook email and password".into(),
                auth_type: "password".into(),
                capabilities: vec!["text".into(), "groups".into()],
                actions: message_actions("messenger"),
            },
            ConnectorInfo {
                id: "instagram".into(),
                name: "Instagram".into(),
                description: "Log in to Instagram DMs".into(),
                auth_type: "password".into(),
                capabilities: vec!["text".into()],
                actions: message_actions("instagram"),
            },
            ConnectorInfo {
                id: "email".into(),
                name: "Email".into(),
                description: "Connect IMAP and SMTP".into(),
                auth_type: "email".into(),
                capabilities: vec!["text".into()],
                actions: message_actions("email"),
            },
            ConnectorInfo {
                id: "matrix".into(),
                name: "Matrix".into(),
                description: "Log in to a Matrix homeserver".into(),
                auth_type: "password".into(),
                capabilities: vec!["text".into(), "groups".into(), "channels".into()],
                actions: message_actions("matrix"),
            },
        ]
    }

    pub fn components(&self) -> Arc<ComponentManager> {
        self.components.clone()
    }

    fn connector_script(&self, connector_id: &str) -> Result<PathBuf, String> {
        self.components().connector_script(connector_id)
    }

    /// An older build kept message previews in the user's Documents folder. Copy them into
    /// the account cache once, so chats show them without waiting for the dump backfill.
    fn migrate_legacy_previews(&self, connector_id: &str, account_id: &str, files_dir: &Path) {
        let Ok(account) = self.db.get_account(account_id) else {
            return;
        };
        let label = self
            .list_connectors()
            .into_iter()
            .find(|c| c.id == connector_id)
            .map(|c| c.name)
            .unwrap_or_default();
        let legacy = crate::media_store::legacy_account_dirs(&label, account_id, &account.name);
        let copied = crate::media_store::migrate_legacy_previews(&legacy, &files_dir.join("thumbs"));
        if copied > 0 {
            tracing::info!("migrated {copied} message previews out of the legacy folder");
        }
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
        // Downloads go to the app-private cache next to the app data, never to Documents.
        let files_dir = crate::media_store::account_cache_root(&self.data_dir, account_id);
        crate::media_store::ensure_account_dirs(&files_dir).map_err(|e| e.to_string())?;
        self.migrate_legacy_previews(connector_id, account_id, &files_dir);
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env("SHUTTLE_ACCOUNT_ID", account_id)
            .env("SHUTTLE_DATA_DIR", &self.data_dir)
            .env("SHUTTLE_FILES_DIR", &files_dir)
            .env(
                "SHUTTLE_FILES_ROOT",
                crate::media_store::media_cache_root(&self.data_dir),
            )
            .env("SHUTTLE_GOWA_BIN", self.components.gowa_binary())
            .env("SHUTTLE_TDLIB", self.components.tdlib_path())
            .env("SHUTTLE_SIGNAL_CLI", self.components.signal_cli());
        process_tree::prepare_connector_command(&mut command);

        let child = command
            .spawn()
            .map_err(|e| format!("Failed to spawn connector: {e}"))?;
        if let Some(pid) = child.id() {
            process_tree::on_connector_spawned(pid);
        }
        Ok(child)
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
        self.sleeping.lock().remove(account_id);
        self.touch_activity(account_id);
        self.components
            .ensure_connector_installed(connector_id)
            .map_err(|e| format!("Connector components not ready: {e}"))?;

        let _start_guard = self.start_lock.lock();
        if self.attach_to_running(connector_id, account_id, credentials.clone())? {
            return Ok("Connector attached".into());
        }

        // Hard cap: one OS process per connector_id. Reclaim orphans from crashed runs.
        process_lock::reclaim(&self.data_dir, connector_id);

        let mut child = self.spawn_connector_process(connector_id, account_id)?;
        if let Some(pid) = child.id() {
            process_lock::write_pid(&self.data_dir, connector_id, pid);
        }
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
            connector_id.to_string(),
            RunningConnector {
                child,
                tx: tx.clone(),
                generation,
                accounts: HashSet::from([account_id.to_string()]),
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
                    if event == "message.received" || event == "message.sent" {
                        this.touch_activity(&account_id);
                    }
                    handle_connector_event(&db, &config, &event_tx, &event, &account_id, payload)
                        .await;
                } else if let Ok(resp) = decode_line::<ConnectorResponse>(&line) {
                    if this.settle_op(&resp) {
                        continue;
                    }
                    match resp {
                        ConnectorResponse::AuthRequired {
                            method,
                            qr_data,
                            url,
                            message,
                            account_id: resp_account,
                        } => {
                            let aid = resp_account.unwrap_or_else(|| account_id_owned.clone());
                            let _ = event_tx.send(AppEvent {
                                kind: "auth.required".into(),
                                payload: serde_json::json!({
                                    "account_id": aid,
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
                                "sleeping" => AccountStatus::Sleeping,
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
                        ConnectorResponse::Ok {
                            request_id: Some(rid),
                            message_id: Some(mid),
                            account_id: resp_account,
                            ..
                        } => match db.reconcile_send_ok(resp_account.as_deref(), &rid, &mid) {
                            Ok(r) => emit_send_reconcile(
                                &db,
                                &event_tx,
                                resp_account.as_deref().unwrap_or(&account_id_owned),
                                r,
                            ),
                            Err(e) => tracing::warn!("send reply {rid}: {e}"),
                        },
                        // A send reply (it names the chat) without a message id: the send
                        // went through, the id comes later with the echo.
                        ConnectorResponse::Ok {
                            request_id: Some(rid),
                            message_id: None,
                            conversation_id: Some(_),
                            account_id: resp_account,
                        } => match db.mark_send_accepted(resp_account.as_deref(), &rid) {
                            Ok(r) => emit_send_reconcile(
                                &db,
                                &event_tx,
                                resp_account.as_deref().unwrap_or(&account_id_owned),
                                r,
                            ),
                            Err(e) => tracing::warn!("send reply {rid}: {e}"),
                        },
                        ConnectorResponse::Error {
                            message,
                            account_id: resp_account,
                            request_id: Some(rid),
                        } => {
                            // Answers one send: fail that row, leave the account status alone.
                            tracing::warn!("send {rid} failed: {message}");
                            match db.reconcile_send_error(resp_account.as_deref(), &rid) {
                                Ok(r) => emit_send_reconcile(
                                    &db,
                                    &event_tx,
                                    resp_account.as_deref().unwrap_or(&account_id_owned),
                                    r,
                                ),
                                Err(e) => tracing::warn!("send error reply {rid}: {e}"),
                            }
                        }
                        ConnectorResponse::Error {
                            message,
                            account_id: resp_account,
                            ..
                        } => {
                            let aid = resp_account.unwrap_or_else(|| account_id_owned.clone());
                            let _ = db.update_account_status(&aid, AccountStatus::Error);
                            let _ = event_tx.send(AppEvent {
                                kind: "account.error".into(),
                                payload: serde_json::json!({
                                    "account_id": aid,
                                    "message": message,
                                }),
                            });
                        }
                        ConnectorResponse::ContactProfile {
                            conversation_id,
                            profile,
                            account_id: resp_account,
                            ..
                        } => {
                            let aid = resp_account.unwrap_or_else(|| account_id_owned.clone());
                            if let Some(remote) = conversation_id.as_deref() {
                                let _ = db.merge_conversation_metadata(
                                    &aid,
                                    remote,
                                    &serde_json::json!({ "contact_profile": profile }),
                                );
                                let _ = event_tx.send(AppEvent {
                                    kind: "contact.profile".into(),
                                    payload: serde_json::json!({
                                        "account_id": aid,
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
            this.on_connector_exit(&connector_id_owned, generation)
                .await;
        });

        let auth_req = ConnectorRequest::Authenticate {
            account_id: account_id.to_string(),
            files_dir: Some(
                crate::media_store::account_cache_root(&self.data_dir, account_id)
                    .to_string_lossy()
                    .into_owned(),
            ),
            credentials,
        };
        let line = encode_line(&auth_req).map_err(|e| e.to_string())?;
        tx.send(line).map_err(|e| e.to_string())?;

        Ok("Connector started".into())
    }

    pub fn submit_auth(
        &self,
        account_id: &str,
        credentials: serde_json::Value,
    ) -> Result<(), String> {
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
        let mut request = serde_json::to_value(req).map_err(|e| e.to_string())?;
        if self
            .db
            .get_account(account_id)
            .map_err(|e| e.to_string())?
            .connector_id
            == "whatsapp"
        {
            // Resolve on each request so account renames and duplicate labels stay isolated
            // even while several accounts share one connector process.
            request["files_dir"] = serde_json::json!(crate::media_store::account_cache_root(
                &self.data_dir,
                account_id
            ));
        }
        let line = encode_line(&request).map_err(|e| e.to_string())?;
        let map = self.processes.lock();
        let proc = map
            .values()
            .find(|p| p.accounts.contains(account_id))
            .ok_or_else(|| "Connector is not running for this account".to_string())?;
        proc.tx.send(line).map_err(|e| e.to_string())
    }

    fn attach_to_running(
        &self,
        connector_id: &str,
        account_id: &str,
        credentials: serde_json::Value,
    ) -> Result<bool, String> {
        let tx = {
            let mut map = self.processes.lock();
            let Some(running) = map.get_mut(connector_id) else {
                return Ok(false);
            };
            running.accounts.insert(account_id.to_string());
            running.tx.clone()
        };
        let auth_req = ConnectorRequest::Authenticate {
            account_id: account_id.to_string(),
            files_dir: Some(
                crate::media_store::account_cache_root(&self.data_dir, account_id)
                    .to_string_lossy()
                    .into_owned(),
            ),
            credentials,
        };
        let line = encode_line(&auth_req).map_err(|e| e.to_string())?;
        tx.send(line).map_err(|e| e.to_string())?;
        Ok(true)
    }

    pub fn touch_activity(&self, account_id: &str) {
        self.last_activity
            .lock()
            .insert(account_id.to_string(), Instant::now());
    }

    pub fn set_active_account(&self, account_id: Option<String>) {
        if let Some(ref id) = account_id {
            self.touch_activity(id);
            self.sleeping.lock().remove(id);
            self.do_not_restart.lock().remove(id);
        }
        *self.active_account.lock() = account_id;
    }

    pub fn wake_account(self: &Arc<Self>, account_id: &str) -> Result<String, String> {
        self.set_active_account(Some(account_id.to_string()));
        self.ensure_running(account_id)
    }

    fn ensure_running(self: &Arc<Self>, account_id: &str) -> Result<String, String> {
        let account = self.db.get_account(account_id).map_err(|e| e.to_string())?;
        if account.disabled {
            return Err("Account is disabled".into());
        }
        self.sleeping.lock().remove(account_id);
        self.do_not_restart.lock().remove(account_id);
        if self.is_running(account_id) {
            return Ok("already running".into());
        }
        let creds = crate::secrets::load(&self.data_dir, account_id);
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
        self.start_connector(&account.connector_id, account_id, creds)
    }

    pub fn sleep_account(&self, account_id: &str) {
        if self.active_account.lock().as_deref() == Some(account_id) {
            return;
        }
        self.sleeping.lock().insert(account_id.to_string());
        self.do_not_restart.lock().insert(account_id.to_string());
        self.detach_account(account_id, false);
        let _ = self
            .db
            .update_account_status(account_id, AccountStatus::Sleeping);
        self.emit(
            "account.status",
            serde_json::json!({
                "account_id": account_id,
                "status": "sleeping",
            }),
        );
    }

    pub fn spawn_sleep_loop(self: &Arc<Self>) {
        let this = Arc::clone(self);
        tauri::async_runtime::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_secs(15));
            loop {
                tick.tick().await;
                this.tick_sleep();
            }
        });
    }

    fn resolved_sleep(&self, account: &Account) -> (bool, u32, u32) {
        let cfg = self.config.get().sleep;
        let enabled = account.sleep_enabled.unwrap_or(cfg.enabled);
        let after = account
            .sleep_after_minutes
            .unwrap_or(cfg.after_minutes)
            .max(1);
        let check = account.sleep_check_minutes.unwrap_or(cfg.check_minutes);
        (enabled, after, check)
    }

    fn is_running(&self, account_id: &str) -> bool {
        self.processes
            .lock()
            .values()
            .any(|p| p.accounts.contains(account_id))
    }

    fn tick_sleep(self: &Arc<Self>) {
        let accounts = match self.db.list_accounts() {
            Ok(a) => a,
            Err(_) => return,
        };
        let active = self.active_account.lock().clone();
        let now = Instant::now();
        for account in accounts {
            if account.disabled {
                continue;
            }
            if matches!(
                account.status,
                AccountStatus::AwaitingAuth | AccountStatus::Connecting | AccountStatus::Error
            ) {
                continue;
            }
            let (enabled, after_min, check_min) = self.resolved_sleep(&account);
            if !enabled {
                continue;
            }
            if active.as_deref() == Some(account.id.as_str()) {
                continue;
            }
            let running = self.is_running(&account.id);
            let last = *self
                .last_activity
                .lock()
                .entry(account.id.clone())
                .or_insert(now);
            if running && last.elapsed() >= Duration::from_secs(u64::from(after_min) * 60) {
                tracing::info!("hibernating account {}", account.name);
                self.sleep_account(&account.id);
                continue;
            }
            if !running && self.sleeping.lock().contains(&account.id) && check_min > 0 {
                let due = self
                    .last_check
                    .lock()
                    .get(&account.id)
                    .map(|t| t.elapsed() >= Duration::from_secs(u64::from(check_min) * 60))
                    .unwrap_or(true);
                if due {
                    self.last_check
                        .lock()
                        .insert(account.id.clone(), Instant::now());
                    tracing::info!("hibernation check for {}", account.name);
                    let this = Arc::clone(self);
                    let id = account.id.clone();
                    tauri::async_runtime::spawn(async move {
                        let _ = this.ensure_running(&id);
                        tokio::time::sleep(Duration::from_secs(45)).await;
                        if this.active_account.lock().as_deref() != Some(id.as_str()) {
                            this.sleep_account(&id);
                        }
                    });
                }
            }
        }
    }

    /// Resolve the message a send answers (`reply_to_message_id`, a local id) into what is
    /// stored on the outgoing message and what the connector needs. It must be in this chat.
    fn reply_for_send(
        &self,
        conversation_id: &str,
        reply_to_message_id: Option<&str>,
    ) -> Result<Option<crate::db::ReplyTarget>, String> {
        let Some(id) = reply_to_message_id.filter(|s| !s.is_empty()) else {
            return Ok(None);
        };
        let target = self.db.reply_target(id).map_err(|e| e.to_string())?;
        if target.conversation_id != conversation_id {
            return Err("The message to reply to is in another chat".into());
        }
        Ok(Some(target))
    }

    pub async fn send_message(
        self: &Arc<Self>,
        account_id: &str,
        conversation_id: &str,
        text: &str,
        reply_to_message_id: Option<&str>,
        forwarded: bool,
    ) -> Result<Message, String> {
        self.ensure_running(account_id)?;
        let conv = self
            .db
            .get_conversation(conversation_id)
            .map_err(|e| e.to_string())?;
        let reply = self.reply_for_send(conversation_id, reply_to_message_id)?;
        let mut msg = Message {
            id: Uuid::new_v4().to_string(),
            conversation_id: conversation_id.to_string(),
            remote_id: None,
            sender_id: None,
            sender_name: Some("You".into()),
            direction: MessageDirection::Outbound,
            body: text.to_string(),
            timestamp: Utc::now(),
            status: MessageStatus::Pending,
            metadata: match &reply {
                Some(r) => serde_json::json!({ "reply_to": r.meta }),
                None if forwarded => serde_json::json!({ "forwarded": true }),
                None => serde_json::json!({}),
            },
            starred: false,
            pinned: false,
            reactions: Vec::new(),
            reply: None,
        };
        self.db.insert_message(&msg).map_err(|e| e.to_string())?;
        self.db.resolve_reply(&mut msg);
        self.touch_activity(account_id);
        let conv = self.db.get_conversation(conversation_id).unwrap_or(conv);

        if let Err(e) = self.send_to_connector(
            account_id,
            &ConnectorRequest::SendMessage {
                account_id: account_id.to_string(),
                conversation_id: conv.remote_id.clone(),
                text: text.to_string(),
                reply_to_id: reply.as_ref().map(|r| r.remote_id.clone()),
                reply_to_sender: reply.as_ref().and_then(|r| r.sender.clone()),
                request_id: Some(msg.id.clone()),
                forwarded,
            },
        ) {
            tracing::error!("[send_message] send_to_connector failed: {e}");
            let _ = self
                .db
                .update_message_status(conversation_id, &msg.id, MessageStatus::Failed);
            msg.status = MessageStatus::Failed;
            self.emit(
                "message.sent",
                serde_json::json!({
                    "account_id": account_id,
                    "conversation_id": conversation_id,
                    "conversation": conv,
                    "message": msg,
                }),
            );
            return Err(e);
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

    pub async fn send_attachment(
        self: &Arc<Self>,
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
        reply_to_message_id: Option<&str>,
        path: Option<&str>,
        forwarded: bool,
    ) -> Result<Message, String> {
        if let Some(p) = path {
            let roots = crate::media_store::media_read_roots(&self.data_dir);
            crate::media_store::validated_media_path(std::path::Path::new(p), &roots)?;
        }
        self.ensure_running(account_id)?;
        let conv = self
            .db
            .get_conversation(conversation_id)
            .map_err(|e| e.to_string())?;
        let reply = self.reply_for_send(conversation_id, reply_to_message_id)?;
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
        let resolved_filename = filename
            .map(str::to_string)
            .or_else(|| mime.map(|m| format!("file{}", crate::media_store::mime_to_ext(m))));
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
            metadata["media_data"] =
                serde_json::Value::String(format!("data:{mime_type};base64,{b64}"));
        }
        if let Some(r) = &reply {
            metadata["reply_to"] = r.meta.clone();
        }
        if let Some(p) = path {
            metadata["media_path"] = serde_json::Value::String(p.to_string());
        }
        if forwarded {
            metadata["forwarded"] = serde_json::Value::Bool(true);
        }
        let mut msg = Message {
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
            reactions: Vec::new(),
            reply: None,
        };
        self.db.insert_message(&msg).map_err(|e| e.to_string())?;
        self.db.resolve_reply(&mut msg);
        self.touch_activity(account_id);
        let conv = self.db.get_conversation(conversation_id).unwrap_or(conv);

        let b64_len = data_base64.map(|s| s.len()).unwrap_or(0);
        tracing::info!(
            "[send_attachment] kind={kind} mime={} b64_len={b64_len} filename={:?} remote_id={}",
            mime.unwrap_or("none"),
            resolved_filename,
            conv.remote_id
        );
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
                path: path.map(str::to_string),
                latitude,
                longitude,
                question: question.map(str::to_string),
                options,
                max_answer,
                reply_to_id: reply.as_ref().map(|r| r.remote_id.clone()),
                reply_to_sender: reply.as_ref().and_then(|r| r.sender.clone()),
                request_id: Some(msg.id.clone()),
                forwarded,
            },
        ) {
            tracing::error!("[send_attachment] send_to_connector failed: {e}");
            let _ = self
                .db
                .update_message_status(conversation_id, &msg.id, MessageStatus::Failed);
            msg.status = MessageStatus::Failed;
            self.emit(
                "message.sent",
                serde_json::json!({
                    "account_id": account_id,
                    "conversation_id": conversation_id,
                    "conversation": conv,
                    "message": msg,
                }),
            );
            return Err(e);
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

    /// Forward messages of one chat to chats on any account, now or (with `send_at`) later.
    /// Returns one result per (target, source message), targets in the given order and sources
    /// in chat order.
    pub async fn forward_messages(
        self: &Arc<Self>,
        source_conversation_id: &str,
        message_ids: &[String],
        targets: &[ForwardTarget],
        send_at: Option<&str>,
    ) -> Result<Vec<ForwardResult>, String> {
        let source = self
            .db
            .get_conversation(source_conversation_id)
            .map_err(|e| e.to_string())?;
        let sources = forward::resolve_sources(&self.db, source_conversation_id, message_ids);
        if let Some(at) = send_at {
            return Ok(forward::schedule_forward(
                &self.db,
                &source.account_id,
                source_conversation_id,
                &sources,
                targets,
                at,
            ));
        }
        Ok(self.run_forward(&source.account_id, sources, targets).await)
    }

    /// Send a scheduled forward that is due: the source message as it is stored now.
    pub async fn send_scheduled_forward(self: &Arc<Self>, msg: &ScheduledMessage) -> Result<(), String> {
        let (Some(account_id), Some(conversation_id), Some(message_id)) = (
            msg.source_account_id.as_deref(),
            msg.source_conversation_id.as_deref(),
            msg.source_message_id.as_deref(),
        ) else {
            return Err("The forwarded message is missing".into());
        };
        let sources =
            forward::resolve_sources(&self.db, conversation_id, &[message_id.to_string()]);
        let target = ForwardTarget {
            account_id: msg.dest_account_id.clone(),
            conversation_id: msg.dest_conversation_id.clone(),
        };
        let results = self.run_forward(account_id, sources, &[target]).await;
        match results.into_iter().next().map(|r| r.outcome) {
            Some(forward::ForwardOutcome::Sent) => Ok(()),
            Some(forward::ForwardOutcome::NotForwardable { reason }) => Err(reason),
            Some(forward::ForwardOutcome::Failed { error }) => Err(error),
            _ => Err("The forwarded message is missing".into()),
        }
    }

    async fn run_forward(
        self: &Arc<Self>,
        source_account_id: &str,
        sources: Vec<(String, Result<(Message, forward::ForwardItem), String>)>,
        targets: &[ForwardTarget],
    ) -> Vec<ForwardResult> {
        forward::run_forward(
            sources,
            targets,
            |msg| {
                let (this, account_id) = (self.clone(), source_account_id.to_string());
                async move { this.cached_media_path(&account_id, &msg).await }
            },
            |target, item| {
                let this = self.clone();
                async move { this.send_forward_item(&target, item).await }
            },
        )
        .await
    }

    async fn send_forward_item(
        self: &Arc<Self>,
        target: &ForwardTarget,
        item: forward::ForwardItem,
    ) -> Result<(), String> {
        let (account_id, conversation_id) = (&target.account_id, &target.conversation_id);
        match item {
            forward::ForwardItem::Text(text) => self
                .send_message(account_id, conversation_id, &text, None, true)
                .await
                .map(drop),
            forward::ForwardItem::Media(m) => self
                .send_attachment(
                    account_id,
                    conversation_id,
                    &m.kind,
                    m.caption.as_deref(),
                    m.filename.as_deref(),
                    m.mime.as_deref(),
                    None,
                    None,
                    None,
                    None,
                    Vec::new(),
                    None,
                    None,
                    m.path.as_deref(),
                    true,
                )
                .await
                .map(drop),
        }
    }

    /// Download a message's media through the normal download path and wait for the file.
    async fn cached_media_path(self: &Arc<Self>, account_id: &str, msg: &Message) -> Result<String, String> {
        const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(120);
        let cached = |m: &Message| {
            m.metadata
                .get("media_path")
                .and_then(|v| v.as_str())
                .filter(|p| std::path::Path::new(p).is_file())
                .map(str::to_string)
        };
        let mut events = self.subscribe();
        self.ensure_running(account_id)?;
        self.download_message_media(account_id, &msg.conversation_id, &msg.id, false)?;
        let wait = async {
            loop {
                match events.recv().await {
                    Ok(ev) if ev.kind == "media.downloaded" => {
                        let Some(done) = ev
                            .payload
                            .get("message")
                            .and_then(|m| serde_json::from_value::<Message>(m.clone()).ok())
                            .filter(|m| m.id == msg.id)
                        else {
                            continue;
                        };
                        return cached(&done).ok_or_else(|| {
                            done.metadata
                                .get("media_error")
                                .and_then(|v| v.as_str())
                                .unwrap_or("no file was saved")
                                .to_string()
                        });
                    }
                    Ok(_) => {}
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        let stored = self.db.get_message(&msg.conversation_id, &msg.id);
                        if let Some(path) = stored.ok().as_ref().and_then(cached) {
                            return Ok(path);
                        }
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        return Err("the app is shutting down".to_string())
                    }
                }
            }
        };
        tokio::time::timeout(DOWNLOAD_TIMEOUT, wait)
            .await
            .map_err(|_| "the download timed out".to_string())?
    }

    pub fn mark_read_remote(&self, account_id: &str, conversation_id: &str) -> Result<(), String> {
        let conv = self
            .db
            .get_conversation(conversation_id)
            .map_err(|e| e.to_string())?;
        let message_id = self
            .db
            .last_inbound_remote_id(conversation_id)
            .ok()
            .flatten();
        self.send_to_connector(
            account_id,
            &ConnectorRequest::MarkRead {
                account_id: account_id.to_string(),
                conversation_id: conv.remote_id,
                message_id,
            },
        )
    }

    /// Apply one action to messages of one chat and report each message's outcome. Every message
    /// is checked against its network's action table first. The stored result arrives through
    /// the connector's facts, the same way a change made on the phone does.
    pub async fn apply_message_op(
        self: &Arc<Self>,
        conversation_id: &str,
        message_ids: &[String],
        op: MessageOp,
    ) -> Result<Vec<MessageOpResult>, String> {
        let conv = self
            .db
            .get_conversation(conversation_id)
            .map_err(|e| e.to_string())?;
        let account = self
            .db
            .get_account(&conv.account_id)
            .map_err(|e| e.to_string())?;
        let actions = message_actions(&account.connector_id);
        self.ensure_running(&account.id)?;
        self.touch_activity(&account.id);
        let now = Utc::now();
        let one = |message_id: String| {
            let (account_id, conv, op) = (&account.id, &conv, &op);
            async move {
                let outcome = async {
                    let msg = self
                        .db
                        .get_message(&conv.id, &message_id)
                        .map_err(|e| e.to_string())?;
                    message_ops::check_op(&actions, &msg, op, now).map_err(|r| r.to_string())?;
                    self.request_op(
                        account_id,
                        &conv.remote_id,
                        msg.remote_id.as_deref().unwrap_or_default(),
                        op,
                    )
                    .await
                }
                .await;
                MessageOpResult {
                    message_id,
                    error: outcome.err(),
                }
            }
        };
        Ok(futures_util::future::join_all(message_ids.iter().cloned().map(one)).await)
    }

    async fn request_op(
        &self,
        account_id: &str,
        conversation_remote: &str,
        message_remote: &str,
        op: &MessageOp,
    ) -> Result<(), String> {
        const OP_TIMEOUT: Duration = Duration::from_secs(30);
        let request_id = Uuid::new_v4().to_string();
        let (tx, rx) = oneshot::channel();
        self.pending_ops.lock().insert(request_id.clone(), tx);
        let sent = self.send_to_connector(
            account_id,
            &ConnectorRequest::MessageOp {
                account_id: account_id.to_string(),
                conversation_id: conversation_remote.to_string(),
                message_id: message_remote.to_string(),
                op: op.clone(),
                request_id: request_id.clone(),
            },
        );
        let reply = match sent {
            Ok(()) => tokio::time::timeout(OP_TIMEOUT, rx).await,
            Err(e) => {
                self.pending_ops.lock().remove(&request_id);
                return Err(e);
            }
        };
        self.pending_ops.lock().remove(&request_id);
        match reply {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err("The connector stopped before answering".into()),
            Err(_) => Err("The network did not answer in time".into()),
        }
    }

    /// Hand a connector reply to the message op waiting for it. False when none is waiting.
    fn settle_op(&self, resp: &ConnectorResponse) -> bool {
        let (rid, result) = match resp {
            ConnectorResponse::Ok {
                request_id: Some(rid),
                ..
            } => (rid, Ok(())),
            ConnectorResponse::Error {
                request_id: Some(rid),
                message,
                ..
            } => (rid, Err(message.clone())),
            _ => return false,
        };
        match self.pending_ops.lock().remove(rid) {
            Some(tx) => {
                let _ = tx.send(result);
                true
            }
            None => false,
        }
    }

    /// Ask the connector to download one message's media into the cache.
    ///
    /// `auto` marks a download nobody asked for (rendering a thread): it only goes ahead when the
    /// effective auto-download setting (chat override, else account default, else built-in)
    /// allows that kind. User-initiated downloads (`auto == false`) always run. Returns whether
    /// a download was requested.
    pub fn download_message_media(
        &self,
        account_id: &str,
        conversation_id: &str,
        message_id: &str,
        auto: bool,
    ) -> Result<bool, String> {
        let conv = self
            .db
            .get_conversation(conversation_id)
            .map_err(|e| e.to_string())?;
        let msg = self
            .db
            .get_message(conversation_id, message_id)
            .map_err(|e| e.to_string())?;
        if auto {
            let account = self.db.get_account(account_id).map_err(|e| e.to_string())?;
            let kind = msg
                .metadata
                .get("media_type")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            if !crate::models::AutoDownloadPrefs::allows_auto(
                kind,
                &conv.auto_download,
                &account.auto_download,
            ) {
                return Ok(false);
            }
        }
        let remote_id = msg
            .remote_id
            .filter(|s| !s.is_empty())
            .ok_or_else(|| "Message has no remote id".to_string())?;
        if self
            .send_to_connector(
                account_id,
                &ConnectorRequest::DownloadMedia {
                    account_id: account_id.to_string(),
                    conversation_id: conv.remote_id.clone(),
                    message_id: remote_id.clone(),
                },
            )
            .is_err()
        {
            let _ =
                self.db
                    .mark_message_media_error(conversation_id, &msg.id, "connector unavailable");
            return Err("Connector is not running for this account".into());
        }
        Ok(true)
    }

    pub fn download_status_media(&self, account_id: &str, message_id: &str) -> Result<(), String> {
        if message_id.trim().is_empty() {
            return Err("Missing status message id".into());
        }
        self.send_to_connector(
            account_id,
            &ConnectorRequest::DownloadStatusMedia {
                account_id: account_id.to_string(),
                message_id: message_id.to_string(),
            },
        )
    }

    pub fn fetch_conversation_avatar(
        &self,
        account_id: &str,
        conversation_id: &str,
    ) -> Result<(), String> {
        let conv = self
            .db
            .get_conversation(conversation_id)
            .map_err(|e| e.to_string())?;
        self.send_to_connector(
            account_id,
            &ConnectorRequest::FetchAvatar {
                account_id: account_id.to_string(),
                conversation_id: conv.remote_id,
            },
        )
    }

    pub fn sync_conversation(&self, account_id: &str, conversation_id: &str) -> Result<(), String> {
        let conv = self
            .db
            .get_conversation(conversation_id)
            .map_err(|e| e.to_string())?;
        let since_message_id = self
            .db
            .last_inbound_remote_id(conversation_id)
            .ok()
            .flatten();
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
        let conv = self
            .db
            .get_conversation(conversation_id)
            .map_err(|e| e.to_string())?;
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
        let conv = self
            .db
            .get_conversation(conversation_id)
            .map_err(|e| e.to_string())?;
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
        self.emit(
            "call.ringing",
            serde_json::to_value(&state).unwrap_or_default(),
        );
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

    /// Fire a `check_number` request; the connector answers with a `number.checked` event.
    pub fn check_number(
        &self,
        account_id: &str,
        phone: &str,
        request_id: &str,
    ) -> Result<(), String> {
        self.send_to_connector(
            account_id,
            &ConnectorRequest::CheckNumber {
                account_id: account_id.to_string(),
                phone: phone.to_string(),
                request_id: request_id.to_string(),
            },
        )
    }

    pub fn start_conversation(
        &self,
        account_id: &str,
        remote_id: &str,
        title: &str,
    ) -> Result<Conversation, String> {
        let remote = normalize_remote_id(remote_id)?;
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
            .ensure_conversation(account_id, &remote, &label, ctype)
            .map_err(|e| e.to_string())
    }

    pub fn stop_connector(&self, account_id: &str) {
        self.do_not_restart.lock().insert(account_id.to_string());
        self.sleeping.lock().remove(account_id);
        self.detach_account(account_id, true);
    }

    fn stop_child(child: &mut Child) {
        if let Some(pid) = child.id() {
            process_lock::kill_process(pid as i32);
        }
    }

    /// Tear down every sidecar (and recorded GOWA) — call on app quit.
    pub fn shutdown_all(&self) {
        let mut map = self.processes.lock();
        for (connector_id, mut running) in map.drain() {
            self.do_not_restart
                .lock()
                .extend(running.accounts.iter().cloned());
            let _ = running.tx.send(
                encode_line(&ConnectorRequest::Shutdown)
                    .unwrap_or_else(|_| "{\"type\":\"shutdown\"}\n".into()),
            );
            Self::stop_child(&mut running.child);
            process_lock::clear_pid(&self.data_dir, &connector_id);
        }
        drop(map);
        process_lock::stop_gowa(&self.data_dir);
    }

    /// Drop one account from a shared sidecar. If it was the last account, kill the process.
    fn detach_account(&self, account_id: &str, send_shutdown_if_last: bool) {
        let mut map = self.processes.lock();
        let Some(key) = map
            .iter()
            .find(|(_, p)| p.accounts.contains(account_id))
            .map(|(k, _)| k.clone())
        else {
            return;
        };
        let empty = {
            let Some(running) = map.get_mut(&key) else {
                return;
            };
            running.accounts.remove(account_id);
            let _ = running.tx.send(
                encode_line(&ConnectorRequest::Disconnect {
                    account_id: account_id.to_string(),
                })
                .unwrap_or_else(|_| {
                    format!("{{\"type\":\"disconnect\",\"account_id\":\"{account_id}\"}}\n")
                }),
            );
            running.accounts.is_empty()
        };
        if empty {
            if let Some(mut running) = map.remove(&key) {
                if send_shutdown_if_last {
                    let _ = running.tx.send(
                        encode_line(&ConnectorRequest::Shutdown)
                            .unwrap_or_else(|_| "{\"type\":\"shutdown\"}\n".into()),
                    );
                }
                Self::stop_child(&mut running.child);
                process_lock::clear_pid(&self.data_dir, &key);
            }
        }
    }

    async fn on_connector_exit(self: &Arc<Self>, connector_id: &str, generation: u64) {
        let accounts: Vec<String> = {
            let mut map = self.processes.lock();
            match map.get(connector_id) {
                Some(running) if running.generation == generation => {
                    let ids: Vec<String> = running.accounts.iter().cloned().collect();
                    map.remove(connector_id);
                    process_lock::clear_pid(&self.data_dir, connector_id);
                    ids
                }
                _ => return,
            }
        };
        let restart: Vec<String> = accounts
            .into_iter()
            .filter(|id| {
                !self.do_not_restart.lock().contains(id)
                    && !self.sleeping.lock().contains(id)
                    && self.db.get_account(id).ok().is_some_and(|a| !a.disabled)
            })
            .collect();
        if restart.is_empty() {
            return;
        }
        tracing::warn!(
            "sidecar {} exited; restarting {} account(s) in 2s",
            connector_id,
            restart.len()
        );
        tokio::time::sleep(Duration::from_secs(2)).await;
        for account_id in restart {
            if self.do_not_restart.lock().contains(&account_id)
                || self.sleeping.lock().contains(&account_id)
            {
                continue;
            }
            if self.is_running(&account_id) {
                continue;
            }
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
            let creds = crate::secrets::load(&self.data_dir, &account_id);
            if let Err(e) = self.start_connector(connector_id, &account_id, creds) {
                tracing::warn!("restart {account_id}: {e}");
                let _ = self
                    .db
                    .update_account_status(&account_id, AccountStatus::Error);
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
}

/// A typed number or a JID, as a JID. A bare number loses spaces, dashes, brackets, a leading `+`
/// and the `00` international prefix, and must then be a plausible E.164 length (7 to 15 digits).
fn normalize_remote_id(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.contains('@') {
        return Ok(trimmed.to_string());
    }
    let digits: String = trimmed.chars().filter(|c| c.is_ascii_digit()).collect();
    let digits = digits.strip_prefix("00").unwrap_or(&digits);
    if !(7..=15).contains(&digits.len()) {
        return Err("Enter the full number with country code, e.g. +974 8899 8899".to_string());
    }
    Ok(format!("{digits}@s.whatsapp.net"))
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

fn parse_ts_opt(value: Option<&serde_json::Value>) -> Option<DateTime<Utc>> {
    let dt = match value {
        Some(serde_json::Value::String(s)) => match s.trim().parse::<i64>() {
            Ok(n) => unix_to_utc(n),
            Err(_) => crate::db::parse_stored_datetime(s),
        },
        Some(serde_json::Value::Number(n)) => n.as_i64().and_then(unix_to_utc),
        _ => None,
    };
    dt.filter(|t| t.timestamp() > 0)
}

fn unix_to_utc(n: i64) -> Option<DateTime<Utc>> {
    if n <= 0 {
        return None;
    }
    let secs = if n > 10_000_000_000 { n / 1000 } else { n };
    DateTime::from_timestamp(secs, 0)
}

fn string_list(value: Option<&serde_json::Value>) -> Vec<String> {
    value
        .and_then(|v| v.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|v| value_to_string(Some(v)))
                .collect()
        })
        .unwrap_or_default()
}

fn emit_merges(
    db: &Database,
    event_tx: &broadcast::Sender<AppEvent>,
    account_id: &str,
    merged: &[crate::db::MergedAway],
) {
    for m in merged {
        let _ = event_tx.send(AppEvent {
            kind: "conversation.removed".into(),
            payload: serde_json::json!({
                "account_id": account_id,
                "conversation_id": m.removed_id,
                "into_conversation_id": m.into_id,
            }),
        });
    }
    let mut seen = HashSet::new();
    for m in merged {
        if !seen.insert(m.into_id.clone()) {
            continue;
        }
        if let Ok(conv) = db.get_conversation(&m.into_id) {
            let _ = event_tx.send(AppEvent {
                kind: "conversation.updated".into(),
                payload: serde_json::json!({ "account_id": account_id, "conversation": conv }),
            });
        }
    }
}

fn emit_send_reconcile(
    db: &Database,
    event_tx: &broadcast::Sender<AppEvent>,
    account_id: &str,
    result: crate::db::SendReconcile,
) {
    use crate::db::SendReconcile;
    let account_of = |conversation_id: &str| {
        db.conversation_account(conversation_id)
            .unwrap_or_else(|_| account_id.to_string())
    };
    match result {
        SendReconcile::Updated {
            conversation_id,
            message,
        } => {
            let _ = event_tx.send(AppEvent {
                kind: "message.updated".into(),
                payload: serde_json::json!({
                    "account_id": account_of(&conversation_id),
                    "conversation_id": conversation_id,
                    "message": message,
                }),
            });
        }
        SendReconcile::Removed {
            conversation_id,
            message_id,
        } => {
            let _ = event_tx.send(AppEvent {
                kind: "message.removed".into(),
                payload: serde_json::json!({
                    "account_id": account_of(&conversation_id),
                    "conversation_id": conversation_id,
                    "message_id": message_id,
                }),
            });
        }
        SendReconcile::Nothing => {}
    }
}

fn handle_conversation_event(
    db: &Database,
    event_tx: &broadcast::Sender<AppEvent>,
    account_id: &str,
    payload: &serde_json::Value,
) {
    let remote_id = value_to_string(payload.get("remote_id")).unwrap_or_default();
    if remote_id.is_empty() || remote_id.starts_with("status@") {
        return;
    }
    let history = payload
        .get("history")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let ev = crate::db::ConversationEvent {
        remote_id,
        alt_remote_ids: string_list(payload.get("alt_remote_ids")),
        names: payload.get("names").and_then(|v| v.as_object()).cloned(),
        is_self: payload
            .get("is_self")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        legacy_title: payload
            .get("title")
            .and_then(|v| v.as_str())
            .map(String::from),
        conversation_type: match payload.get("conversation_type").and_then(|v| v.as_str()) {
            Some("group") => Some(ConversationType::Group),
            Some("channel") => Some(ConversationType::Channel),
            Some("direct") => Some(ConversationType::Direct),
            _ => None,
        },
        last_message_at: parse_ts_opt(payload.get("last_message_at")),
        preview: payload
            .get("preview")
            .and_then(|v| v.as_str())
            .map(String::from),
        archived: payload.get("archived").and_then(|v| v.as_bool()),
        pinned: payload.get("pinned").and_then(|v| v.as_bool()),
        announce_only: payload.get("announce_only").and_then(|v| v.as_bool()),
        self_is_admin: payload.get("self_is_admin").and_then(|v| v.as_bool()),
        history,
    };
    match db.apply_conversation_event(account_id, &ev) {
        Ok(out) => {
            emit_merges(db, event_tx, account_id, &out.merged);
            if out.changed && out.merged.is_empty() && !history {
                if let Some(conv) = out.conversation {
                    let _ = event_tx.send(AppEvent {
                        kind: "conversation.updated".into(),
                        payload: serde_json::json!({ "account_id": account_id, "conversation": conv }),
                    });
                }
            }
        }
        Err(e) => tracing::warn!("conversation event: {e}"),
    }
}

fn handle_message_event(
    db: &Database,
    config: &ConfigStore,
    event_tx: &broadcast::Sender<AppEvent>,
    event: &str,
    account_id: &str,
    payload: &serde_json::Value,
) {
    let remote_jid = value_to_string(payload.get("conversation_id"))
        .or_else(|| value_to_string(payload.get("remote_id")))
        .unwrap_or_default();
    if remote_jid.is_empty() || remote_jid.starts_with("status@") {
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
    let history = payload
        .get("history")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let has_media = msg_obj
        .get("media_type")
        .and_then(|v| v.as_str())
        .map(|s| !s.is_empty() && s != "text")
        .unwrap_or(false);
    // Skip protocol stubs / empty rows: they would create blank threads.
    if body.trim().is_empty() && !has_media {
        return;
    }
    // R1: bulk-sync rows need a real time; live rows without one are stamped now.
    let ts = match parse_ts_opt(msg_obj.get("timestamp")) {
        Some(t) => t,
        None if history => {
            tracing::warn!("dropping history message without a valid timestamp in {remote_jid}");
            return;
        }
        None => Utc::now(),
    };
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
    let fallback_title = msg_obj
        .get("sender_name")
        .and_then(|v| v.as_str())
        .unwrap_or(remote_jid.as_str())
        .to_string();
    let msg = Message {
        id: Uuid::new_v4().to_string(),
        conversation_id: String::new(),
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
        body,
        timestamp: ts,
        status: if from_me {
            MessageStatus::Sent
        } else {
            MessageStatus::Delivered
        },
        metadata: msg_obj.clone(),
        starred: false,
        pinned: false,
        reactions: Vec::new(),
        reply: None,
    };
    let alts = string_list(payload.get("alt_remote_ids"));
    let outcome = match db.ingest_message(
        account_id,
        &remote_jid,
        &alts,
        history,
        msg,
        &preview,
        &fallback_title,
    ) {
        Ok(o) => o,
        Err(e) => {
            tracing::warn!("ingest message: {e}");
            return;
        }
    };
    emit_merges(db, event_tx, account_id, &outcome.merged);
    match outcome.kind {
        crate::db::IngestKind::Inserted => {
            let (Some(conv), Some(mut msg)) = (outcome.conversation, outcome.message) else {
                return;
            };
            db.resolve_reply(&mut msg);
            // A reaction can be stored before its message arrives; show it from the start.
            if let Some(rid) = msg.remote_id.clone() {
                if let Ok(mut map) = db.reactions_for_targets(&conv.id, std::slice::from_ref(&rid)) {
                    if let Some(groups) = map.remove(&rid) {
                        msg.reactions = groups;
                    }
                }
            }
            if !from_me {
                if outcome.notify_eligible {
                    let cfg = config.get();
                    if notifications::should_notify_ids(db, &cfg, account_id, &conv.id) {
                        notifications::notify_message(
                            msg.sender_name.as_deref().unwrap_or("New message"),
                            &msg.body,
                        );
                    }
                }
                if !history {
                    apply_forward_rules(db, &conv, &msg);
                }
            }
            // Old bulk-sync rows stay silent; catch-up duplicates never reach this point.
            if outcome.emit_new {
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
        crate::db::IngestKind::Adopted => {
            if let Some(msg) = outcome.message {
                let _ = event_tx.send(AppEvent {
                    kind: "message.updated".into(),
                    payload: serde_json::json!({
                        "account_id": account_id,
                        "conversation_id": msg.conversation_id,
                        "message": msg,
                    }),
                });
            }
        }
        crate::db::IngestKind::Duplicate => {
            if let (Some(conv), true) = (outcome.conversation, outcome.merged.is_empty()) {
                let _ = event_tx.send(AppEvent {
                    kind: "conversation.updated".into(),
                    payload: serde_json::json!({ "account_id": account_id, "conversation": conv }),
                });
            }
        }
    }
}

/// `reaction`: store the facts. Never touches the chat row (time, preview, unread); the UI is
/// told which messages now carry different reactions so an open thread updates in place.
fn handle_reaction_event(
    db: &Database,
    event_tx: &broadcast::Sender<AppEvent>,
    account_id: &str,
    payload: &serde_json::Value,
) {
    let remote_id = value_to_string(payload.get("remote_id"))
        .or_else(|| value_to_string(payload.get("conversation_id")))
        .unwrap_or_default();
    if remote_id.is_empty() || remote_id.starts_with("status@") {
        return;
    }
    let facts: Vec<crate::db::ReactionFact> = payload
        .get("reactions")
        .and_then(|v| v.as_array())
        .map(|rows| {
            rows.iter()
                .filter_map(|r| {
                    let target = value_to_string(r.get("target_message_id"))?;
                    let reactor = value_to_string(r.get("reactor_id"))?;
                    Some(crate::db::ReactionFact {
                        target_remote_id: target,
                        reactor_id: reactor,
                        reactor_name: value_to_string(r.get("reactor_name")),
                        emoji: r
                            .get("emoji")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                        at: parse_ts_opt(r.get("timestamp")).unwrap_or_else(Utc::now),
                        from_me: r.get("from_me").and_then(|v| v.as_bool()).unwrap_or(false),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    if facts.is_empty() {
        return;
    }
    let alts = string_list(payload.get("alt_remote_ids"));
    let history = payload
        .get("history")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let out = match db.apply_reactions(account_id, &remote_id, &alts, &facts) {
        Ok(o) => o,
        Err(e) => {
            tracing::warn!("reaction: {e}");
            return;
        }
    };
    let Some(conversation_id) = out.conversation_id else {
        return;
    };
    if out.changed_targets.is_empty() || (history && out.changed_targets.len() > 200) {
        // Bulk history imports are picked up by the next message-list load.
        return;
    }
    let updates = match db.reactions_for_targets(&conversation_id, &out.changed_targets) {
        Ok(map) => out
            .changed_targets
            .iter()
            .map(|t| {
                serde_json::json!({
                    "remote_id": t,
                    "reactions": map.get(t).cloned().unwrap_or_default(),
                })
            })
            .collect::<Vec<_>>(),
        Err(e) => {
            tracing::warn!("reaction readback: {e}");
            return;
        }
    };
    let _ = event_tx.send(AppEvent {
        kind: "message.reactions".into(),
        payload: serde_json::json!({
            "account_id": account_id,
            "conversation_id": conversation_id,
            "updates": updates,
        }),
    });
}

/// `poll.changed`: a vote, an edited question or an added option, folded into the poll message
/// that already exists. Never creates a message, so a vote never lands in the thread as text.
fn handle_poll_event(
    db: &Database,
    event_tx: &broadcast::Sender<AppEvent>,
    account_id: &str,
    payload: &serde_json::Value,
) {
    let remote = value_to_string(payload.get("remote_id"))
        .or_else(|| value_to_string(payload.get("conversation_id")))
        .unwrap_or_default();
    let poll_id = value_to_string(payload.get("poll_id")).unwrap_or_default();
    if remote.is_empty() || remote.starts_with("status@") || poll_id.is_empty() {
        return;
    }
    let definition = payload
        .get("poll")
        .and_then(|v| serde_json::from_value::<crate::models::PollData>(v.clone()).ok())
        .filter(|d| !d.question.trim().is_empty() || !d.options.is_empty());
    let vote = payload.get("vote").map(|v| crate::db::PollVoteFact {
        voter_id: value_to_string(v.get("voter_id")).unwrap_or_default(),
        voter_name: value_to_string(v.get("voter_name")),
        selected: string_list(v.get("selected")),
    });
    if definition.is_none() && vote.is_none() {
        return;
    }
    let alts = string_list(payload.get("alt_remote_ids"));
    let out = match db.apply_poll_change(
        account_id,
        &remote,
        &alts,
        &poll_id,
        definition.as_ref(),
        vote.as_ref(),
    ) {
        Ok(o) => o,
        Err(e) => {
            tracing::warn!("poll.changed: {e}");
            return;
        }
    };
    if !out.changed {
        if out.message.is_none() && out.conversation_id.is_some() {
            tracing::debug!("poll.changed: poll message {poll_id} is not stored yet, dropping");
        }
        return;
    }
    let Some(conversation_id) = out.conversation_id else {
        return;
    };
    if let Some(msg) = out.message {
        let _ = event_tx.send(AppEvent {
            kind: "message.updated".into(),
            payload: serde_json::json!({
                "account_id": account_id,
                "conversation_id": conversation_id,
                "message": msg,
            }),
        });
    }
    if let Some(conv) = out.conversation {
        let _ = event_tx.send(AppEvent {
            kind: "conversation.updated".into(),
            payload: serde_json::json!({ "account_id": account_id, "conversation": conv }),
        });
    }
}

/// `message.changed`: a stored message was edited, deleted for everyone, or deleted for me.
fn handle_message_change_event(
    db: &Database,
    event_tx: &broadcast::Sender<AppEvent>,
    account_id: &str,
    payload: &serde_json::Value,
) {
    let remote_id = value_to_string(payload.get("remote_id"))
        .or_else(|| value_to_string(payload.get("conversation_id")))
        .unwrap_or_default();
    let target = value_to_string(payload.get("message_id")).unwrap_or_default();
    if remote_id.is_empty() || remote_id.starts_with("status@") || target.is_empty() {
        return;
    }
    let change: crate::db::MessageChange =
        match serde_json::from_value(payload.get("change").cloned().unwrap_or_default()) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("message.changed: {e}");
                return;
            }
        };
    let alts = string_list(payload.get("alt_remote_ids"));
    match db.apply_message_change(account_id, &remote_id, &alts, &target, &change) {
        Ok(out) => {
            emit_send_reconcile(db, event_tx, account_id, out.message);
            if let Some(conv) = out.conversation {
                let _ = event_tx.send(AppEvent {
                    kind: "conversation.updated".into(),
                    payload: serde_json::json!({ "account_id": account_id, "conversation": conv }),
                });
            }
        }
        Err(e) => tracing::warn!("message.changed: {e}"),
    }
}

fn handle_chat_read(
    db: &Database,
    event_tx: &broadcast::Sender<AppEvent>,
    account_id: &str,
    payload: &serde_json::Value,
) {
    let remote_id = value_to_string(payload.get("remote_id")).unwrap_or_default();
    let Some(read_through) = parse_ts_opt(payload.get("read_through")) else {
        return;
    };
    if remote_id.is_empty() || remote_id.starts_with("status@") {
        return;
    }
    let alts = string_list(payload.get("alt_remote_ids"));
    match db.apply_chat_read(account_id, &remote_id, &alts, read_through) {
        Ok(out) => {
            emit_merges(db, event_tx, account_id, &out.merged);
            if out.changed && out.merged.is_empty() {
                if let Some(conv) = out.conversation {
                    let unread_total = db.total_unread().unwrap_or(0);
                    let _ = event_tx.send(AppEvent {
                        kind: "conversation.updated".into(),
                        payload: serde_json::json!({
                            "account_id": account_id,
                            "conversation": conv,
                            "unread_total": unread_total,
                        }),
                    });
                }
            }
        }
        Err(e) => tracing::warn!("chat.read: {e}"),
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
        "conversation.updated" => handle_conversation_event(db, event_tx, account_id, &payload),
        "message.received" | "message.sent" => {
            handle_message_event(db, config, event_tx, event, account_id, &payload)
        }
        "chat.read" => handle_chat_read(db, event_tx, account_id, &payload),
        "reaction" => handle_reaction_event(db, event_tx, account_id, &payload),
        "poll.changed" => handle_poll_event(db, event_tx, account_id, &payload),
        "message.changed" => handle_message_change_event(db, event_tx, account_id, &payload),
        "inbox.catchup" => {
            let _ = event_tx.send(AppEvent {
                kind: "inbox.catchup".into(),
                payload: serde_json::json!({ "account_id": account_id }),
            });
        }
        "chat.synced" => {
            let _ = event_tx.send(AppEvent {
                kind: "chat.synced".into(),
                payload: serde_json::json!({
                    "account_id": account_id,
                    "remote_id": payload.get("remote_id"),
                }),
            });
        }
        "status.feed" => {
            let _ = event_tx.send(AppEvent {
                kind: "status.feed".into(),
                payload: serde_json::json!({
                    "account_id": account_id,
                    "items": payload.get("items"),
                    "upsert": payload.get("upsert"),
                }),
            });
        }
        "status.media" => {
            let _ = event_tx.send(AppEvent {
                kind: "status.media".into(),
                payload: serde_json::json!({
                    "account_id": account_id,
                    "message_id": payload.get("message_id"),
                    "media_type": payload.get("media_type"),
                    "media_path": payload.get("media_path"),
                    "filename": payload.get("filename"),
                    "error": payload.get("error"),
                }),
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
            let _ = db.refresh_titles(account_id);
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
                    patch.insert(
                        "avatar_data".into(),
                        serde_json::Value::String(data.to_string()),
                    );
                }
            }
            if patch.is_empty() {
                return;
            }
            if let Ok(Some(conv)) = db.merge_conversation_metadata(
                account_id,
                &remote_id,
                &serde_json::Value::Object(patch),
            ) {
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
        "account.avatar" => {
            let mut patch = serde_json::Map::new();
            if let Some(data) = payload.get("avatar_data").and_then(|v| v.as_str()) {
                if !data.is_empty() {
                    patch.insert(
                        "avatar_data".into(),
                        serde_json::Value::String(data.to_string()),
                    );
                }
            }
            if patch.is_empty() {
                return;
            }
            if let Ok(account) =
                db.merge_account_metadata(account_id, &serde_json::Value::Object(patch))
            {
                let _ = event_tx.send(AppEvent {
                    kind: "account.avatar".into(),
                    payload: serde_json::json!({
                        "account_id": account_id,
                        "avatar_data": account.metadata.get("avatar_data"),
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
                    patch.insert(
                        "media_path".into(),
                        serde_json::Value::String(path.to_string()),
                    );
                    patch.insert("media_error".into(), serde_json::Value::Null);
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
        "media.thumbnails" => {
            // Previews the connector extracted for messages it already reported (facts only).
            let mut touched = false;
            for item in payload
                .get("items")
                .and_then(|v| v.as_array())
                .into_iter()
                .flatten()
            {
                let jid = value_to_string(item.get("conversation_id")).unwrap_or_default();
                let message_id = value_to_string(item.get("message_id")).unwrap_or_default();
                let path = item
                    .get("thumbnail_path")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();
                if message_id.is_empty() || path.is_empty() {
                    continue;
                }
                let patch = serde_json::json!({ "thumbnail_path": path });
                let hit = (!jid.is_empty())
                    .then(|| {
                        db.merge_message_metadata_by_remote(account_id, &jid, &message_id, &patch)
                            .ok()
                            .flatten()
                    })
                    .flatten()
                    .or_else(|| {
                        db.merge_message_metadata_by_remote_id(account_id, &message_id, &patch)
                            .ok()
                            .flatten()
                    });
                touched |= hit.is_some();
            }
            if touched {
                let _ = event_tx.send(AppEvent {
                    kind: "media.thumbnails".into(),
                    payload: serde_json::json!({ "account_id": account_id }),
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
                    let saved = row.get("saved").and_then(|v| v.as_bool());
                    let _ = db.upsert_contact_flagged(account_id, &remote_id, name, saved);
                }
            }
            let _ = db.refresh_titles(account_id);
            let _ = event_tx.send(AppEvent {
                kind: "contacts.synced".into(),
                payload: serde_json::json!({ "account_id": account_id }),
            });
            let _ = event_tx.send(AppEvent {
                kind: "inbox.catchup".into(),
                payload: serde_json::json!({ "account_id": account_id }),
            });
        }
        e if e.starts_with("call.") => {
            let _ = event_tx.send(AppEvent {
                kind: e.to_string(),
                payload,
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
        .unwrap_or("default");
    let already_forwarded = msg.metadata.get("forwarded_from").is_some();

    for rule in rules {
        if !rule.enabled {
            continue;
        }
        if rule.inbound_only && msg.direction != MessageDirection::Inbound {
            continue;
        }
        if !rule.include_self
            && rule.dest_account_id == conv.account_id
            && rule.dest_conversation_id == conv.id
        {
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

        let send_at =
            (Utc::now() + chrono::TimeDelta::seconds(rule.delay_seconds.max(0))).to_rfc3339();
        let _ = db.schedule_message(&ScheduleMessageDraft {
            source_account_id: Some(conv.account_id.clone()),
            source_conversation_id: Some(conv.id.clone()),
            source_message_id: Some(msg.id.clone()),
            dest_account_id: rule.dest_account_id.clone(),
            dest_conversation_id: rule.dest_conversation_id.clone(),
            body,
            send_at,
            forward: false,
        });
    }
}

#[cfg(test)]
mod remote_id_tests {
    use super::normalize_remote_id;

    #[test]
    fn typed_numbers_become_phone_jids() {
        for raw in ["+974 8899 8899", "00974-8899-8899", " (974) 88998899 ", "97488998899"] {
            assert_eq!(normalize_remote_id(raw).unwrap(), "97488998899@s.whatsapp.net", "{raw}");
        }
    }

    #[test]
    fn jids_pass_through() {
        assert_eq!(normalize_remote_id("123@lid").unwrap(), "123@lid");
        assert_eq!(normalize_remote_id("120363@g.us").unwrap(), "120363@g.us");
    }

    #[test]
    fn implausible_numbers_are_rejected() {
        for raw in ["", "abc", "12345", "1234567890123456", "+00"] {
            assert!(normalize_remote_id(raw).is_err(), "{raw}");
        }
    }
}
