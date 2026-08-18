const CONV_SELECT: &str = "SELECT c.id, c.account_id, c.remote_id, c.contact_id, c.title, c.conversation_type,
        c.unread_count, c.last_message_at, c.last_message_preview, c.pinned, c.archived, c.muted, c.metadata,
        c.workspace_id, c.priority_group, c.notes, c.notify_enabled, c.send_receipts
 FROM conversations c";

use crate::models::*;
use chrono::Utc;
use parking_lot::Mutex;
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use thiserror::Error;
use uuid::Uuid;

mod schema;

#[derive(Error, Debug)]
pub enum DbError {
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("Not found: {0}")]
    NotFound(String),
    #[error("Cannot modify a built-in item")]
    Builtin,
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

struct Conn {
    conn: Mutex<Connection>,
}

impl Conn {
    fn open(path: &Path, inbox: bool) -> Result<Self, DbError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700));
            }
        }
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        if inbox {
            schema::migrate_inbox(&conn)?;
        } else {
            schema::migrate_catalog(&conn)?;
        }
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }
}

/// Catalog of accounts plus one SQLite inbox file per account.
pub struct Database {
    data_dir: PathBuf,
    catalog: Conn,
    inboxes: Mutex<HashMap<String, Arc<Conn>>>,
}

impl Database {
    pub fn open(data_dir: &Path) -> Result<Self, DbError> {
        std::fs::create_dir_all(data_dir)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(data_dir, std::fs::Permissions::from_mode(0o700));
        }
        let catalog = Conn::open(&app_db_path(data_dir), false)?;
        migrate_catalog_to_app(data_dir)?;
        let db = Self {
            data_dir: data_dir.to_path_buf(),
            catalog,
            inboxes: Mutex::new(HashMap::new()),
        };
        migrate_legacy(data_dir, &db.catalog)?;
        if let Ok(accounts) = db.list_accounts() {
            for account in accounts {
                let _ = db.inbox(&account.id);
            }
        }
        Ok(db)
    }

    fn inbox_path(&self, account_id: &str) -> PathBuf {
        self.data_dir
            .join("accounts")
            .join(account_id)
            .join("inbox.sqlite")
    }

    fn inbox(&self, account_id: &str) -> Result<Arc<Conn>, DbError> {
        {
            let map = self.inboxes.lock();
            if let Some(existing) = map.get(account_id) {
                return Ok(existing.clone());
            }
        }
        let conn = Arc::new(Conn::open(&self.inbox_path(account_id), true)?);
        self.inboxes
            .lock()
            .insert(account_id.to_string(), conn.clone());
        Ok(conn)
    }

    fn for_each_inbox<T>(
        &self,
        mut f: impl FnMut(&str, &Conn) -> Result<Vec<T>, DbError>,
    ) -> Result<Vec<T>, DbError> {
        let accounts = self.list_accounts()?;
        let mut out = Vec::new();
        for account in accounts {
            let inbox = self.inbox(&account.id)?;
            out.extend(f(&account.id, inbox.as_ref())?);
        }
        Ok(out)
    }

    pub fn seed_demo_if_empty(&self) -> Result<(), DbError> {
        let count: i64 = {
            let conn = self.catalog.conn.lock();
            conn.query_row("SELECT COUNT(*) FROM accounts", [], |r| r.get(0))?
        };
        if count > 0 {
            return Ok(());
        }
        self.seed_demo_data()
    }

    fn seed_demo_data(&self) -> Result<(), DbError> {
        let wa = self.create_account("whatsapp", "WhatsApp Work")?;
        let tg = self.create_account("telegram", "Telegram Personal")?;
        self.update_account_identity(&wa.id, "+1 555-0100")?;
        self.update_account_identity(&tg.id, "@alexdev")?;
        self.update_account_status(&wa.id, AccountStatus::Connected)?;
        self.update_account_status(&tg.id, AccountStatus::Connected)?;
        let now = Utc::now().to_rfc3339();
        let seed = [
            (&wa.id, "Sarah Chen", "Hey, are we still on for lunch tomorrow?", 2_i64, "direct"),
            (&wa.id, "Design Team", "Alex: Updated the mockups in Figma", 5_i64, "group"),
            (&tg.id, "Mom", "Don't forget to call grandma this weekend ❤️", 1_i64, "direct"),
            (&tg.id, "Rust Developers", "New async patterns discussion thread", 12_i64, "group"),
        ];
        for (account_id, title, preview, unread, ctype) in seed {
            let inbox = self.inbox(account_id)?;
            let conn = inbox.conn.lock();
            let conv_id = Uuid::new_v4().to_string();
            conn.execute(
                "INSERT INTO conversations (id, account_id, remote_id, title, conversation_type, unread_count,
                 last_message_at, last_message_preview, pinned, archived, muted, metadata)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 0, 0, 0, '{}')",
                params![conv_id, account_id, Uuid::new_v4().to_string(), title, ctype, unread, now, preview],
            )?;
            conn.execute(
                "INSERT INTO messages (id, conversation_id, remote_id, sender_name, direction, body, timestamp, status, metadata)
                 VALUES (?1, ?2, ?3, ?4, 'inbound', ?5, ?6, 'delivered', '{}')",
                params![Uuid::new_v4().to_string(), conv_id, Uuid::new_v4().to_string(), title, preview, now],
            )?;
        }
        Ok(())
    }

    pub fn list_accounts(&self) -> Result<Vec<Account>, DbError> {
        let conn = self.catalog.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT id, connector_id, name, identity, status, metadata, created_at, updated_at,
                    disabled, muted, workspace_id, notify_enabled, send_receipts
             FROM accounts ORDER BY name",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(Account {
                id: row.get(0)?,
                connector_id: row.get(1)?,
                name: row.get(2)?,
                identity: row.get(3)?,
                status: parse_account_status(&row.get::<_, String>(4)?),
                metadata: serde_json::from_str(&row.get::<_, String>(5)?).unwrap_or_default(),
                created_at: row.get::<_, String>(6)?.parse().unwrap_or_else(|_| Utc::now()),
                updated_at: row.get::<_, String>(7)?.parse().unwrap_or_else(|_| Utc::now()),
                disabled: row.get::<_, i64>(8)? != 0,
                muted: row.get::<_, i64>(9)? != 0,
                workspace_id: row.get(10)?,
                notify_enabled: opt_bool(row.get(11)?),
                send_receipts: row.get::<_, i64>(12)? != 0,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(DbError::from)
    }

    pub fn create_account(&self, connector_id: &str, name: &str) -> Result<Account, DbError> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        {
            let conn = self.catalog.conn.lock();
            conn.execute(
                "INSERT INTO accounts (id, connector_id, name, identity, status, metadata, created_at, updated_at)
                 VALUES (?1, ?2, ?3, NULL, 'awaiting_auth', '{}', ?4, ?4)",
                params![id, connector_id, name, now],
            )?;
        }
        self.inbox(&id)?;
        Ok(Account {
            id,
            connector_id: connector_id.to_string(),
            name: name.to_string(),
            identity: None,
            status: AccountStatus::AwaitingAuth,
            metadata: serde_json::json!({}),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            disabled: false,
            muted: false,
            workspace_id: None,
            notify_enabled: None,
            send_receipts: false,
        })
    }

    pub fn update_account_status(&self, id: &str, status: AccountStatus) -> Result<(), DbError> {
        let conn = self.catalog.conn.lock();
        let now = Utc::now().to_rfc3339();
        let n = conn.execute(
            "UPDATE accounts SET status = ?1, updated_at = ?2 WHERE id = ?3",
            params![account_status_str(&status), now, id],
        )?;
        if n == 0 {
            return Err(DbError::NotFound(id.to_string()));
        }
        Ok(())
    }

    pub fn update_account_identity(&self, id: &str, identity: &str) -> Result<(), DbError> {
        let conn = self.catalog.conn.lock();
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE accounts SET identity = ?1, updated_at = ?2 WHERE id = ?3",
            params![identity, now, id],
        )?;
        Ok(())
    }

    pub fn delete_account(&self, id: &str) -> Result<(), DbError> {
        {
            let conn = self.catalog.conn.lock();
            conn.execute("DELETE FROM chat_todos WHERE account_id = ?1", params![id])?;
            conn.execute("DELETE FROM reminders WHERE account_id = ?1", params![id])?;
            conn.execute(
                "DELETE FROM scheduled_messages WHERE dest_account_id = ?1 OR source_account_id = ?1",
                params![id],
            )?;
            conn.execute(
                "DELETE FROM forwarding_rules
                 WHERE dest_account_id = ?1 OR source_account_id = ?1",
                params![id],
            )?;
            let n = conn.execute("DELETE FROM accounts WHERE id = ?1", params![id])?;
            if n == 0 {
                return Err(DbError::NotFound(id.to_string()));
            }
        }
        self.inboxes.lock().remove(id);
        let dir = self.data_dir.join("accounts").join(id);
        if dir.exists() {
            std::fs::remove_dir_all(dir)?;
        }
        Ok(())
    }

    pub fn list_conversations(
        &self,
        account_id: Option<&str>,
        workspace_id: Option<&str>,
        priority_group: Option<&str>,
        include_archived: bool,
    ) -> Result<Vec<Conversation>, DbError> {
        let accounts = self.list_accounts()?;
        let mut all = if let Some(aid) = account_id {
            let inbox = self.inbox(aid)?;
            let conn = inbox.conn.lock();
            list_conversations_in(&conn, Some(aid), include_archived)?
        } else {
            self.for_each_inbox(|aid, inbox| {
                let conn = inbox.conn.lock();
                list_conversations_in(&conn, Some(aid), include_archived)
            })?
        };
        if let Some(ws) = workspace_id {
            all.retain(|c| {
                let acct = accounts.iter().find(|a| a.id == c.account_id);
                effective_workspace(c, acct) == ws
            });
        }
        if let Some(pg) = priority_group {
            all.retain(|c| c.priority_group.as_deref() == Some(pg));
        }
        all.sort_by(|a, b| match (a.pinned, b.pinned) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => b.last_message_at.cmp(&a.last_message_at),
        });
        Ok(all)
    }

    pub fn get_conversation(&self, id: &str) -> Result<Conversation, DbError> {
        let accounts = self.list_accounts()?;
        for account in accounts {
            let inbox = self.inbox(&account.id)?;
            let conn = inbox.conn.lock();
            if let Ok(conv) = get_conversation_locked(&conn, id) {
                return Ok(conv);
            }
        }
        Err(DbError::NotFound(id.to_string()))
    }

    pub fn list_messages(&self, conversation_id: &str, limit: i64) -> Result<Vec<Message>, DbError> {
        let conv = self.get_conversation(conversation_id)?;
        let inbox = self.inbox(&conv.account_id)?;
        let conn = inbox.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT id, conversation_id, remote_id, sender_id, sender_name, direction, body, timestamp, status, metadata
             FROM messages WHERE conversation_id = ?1
             ORDER BY timestamp DESC LIMIT ?2",
        )?;
        let mut rows = stmt
            .query_map(params![conversation_id, limit], map_message_row)?
            .collect::<Result<Vec<_>, _>>()?;
        rows.reverse();
        Ok(rows)
    }

    pub fn insert_message(&self, msg: &Message) -> Result<bool, DbError> {
        let conv = self.get_conversation(&msg.conversation_id)?;
        let inbox = self.inbox(&conv.account_id)?;
        let conn = inbox.conn.lock();
        conn.execute(
            "INSERT OR IGNORE INTO messages (id, conversation_id, remote_id, sender_id, sender_name, direction, body, timestamp, status, metadata)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                msg.id,
                msg.conversation_id,
                msg.remote_id,
                msg.sender_id,
                msg.sender_name,
                direction_str(&msg.direction),
                msg.body,
                msg.timestamp.to_rfc3339(),
                message_status_str(&msg.status),
                msg.metadata.to_string(),
            ],
        )?;
        let inserted = conn.changes() > 0;
        if inserted {
            let ts = msg.timestamp.to_rfc3339();
            conn.execute(
                "UPDATE conversations SET
                    last_message_preview = CASE WHEN last_message_at IS NULL OR last_message_at < ?1 THEN ?2 ELSE last_message_preview END,
                    last_message_at = CASE WHEN last_message_at IS NULL OR last_message_at < ?1 THEN ?1 ELSE last_message_at END,
                    updated_at = ?1
                 WHERE id = ?3",
                params![ts, msg.body, msg.conversation_id],
            )?;
        }
        Ok(inserted)
    }

    pub fn upsert_conversation(
        &self,
        account_id: &str,
        remote_id: &str,
        title: &str,
        conversation_type: ConversationType,
        last_message_at: Option<&str>,
        preview: Option<&str>,
        archived: bool,
    ) -> Result<Conversation, DbError> {
        let inbox = self.inbox(account_id)?;
        let conn = inbox.conn.lock();
        if let Some(existing) = get_conversation_by_remote_locked(&conn, account_id, remote_id)? {
            let now = Utc::now().to_rfc3339();
            conn.execute(
                "UPDATE conversations SET title = ?1,
                 last_message_preview = CASE
                    WHEN ?3 IS NULL THEN last_message_preview
                    WHEN last_message_preview IS NULL THEN ?3
                    WHEN last_message_at IS NULL OR (?2 IS NOT NULL AND last_message_at < ?2) THEN ?3
                    ELSE last_message_preview
                 END,
                 last_message_at = CASE
                    WHEN ?2 IS NULL THEN last_message_at
                    WHEN last_message_at IS NULL OR last_message_at < ?2 THEN ?2
                    ELSE last_message_at
                 END,
                 archived = ?4, updated_at = ?5
                 WHERE id = ?6",
                params![
                    title,
                    last_message_at,
                    preview,
                    if archived { 1 } else { 0 },
                    now,
                    existing.id
                ],
            )?;
            return get_conversation_locked(&conn, &existing.id);
        }
        let id = Uuid::new_v4().to_string();
        let ctype = match conversation_type {
            ConversationType::Group => "group",
            ConversationType::Channel => "channel",
            ConversationType::Direct => "direct",
        };
        conn.execute(
            "INSERT INTO conversations (id, account_id, remote_id, title, conversation_type, unread_count,
             last_message_at, last_message_preview, pinned, archived, muted, metadata)
             VALUES (?1, ?2, ?3, ?4, ?5, 0, ?6, ?7, 0, ?8, 0, '{}')",
            params![
                id,
                account_id,
                remote_id,
                title,
                ctype,
                last_message_at,
                preview,
                if archived { 1 } else { 0 }
            ],
        )?;
        get_conversation_locked(&conn, &id)
    }

    pub fn get_conversation_by_remote(
        &self,
        account_id: &str,
        remote_id: &str,
    ) -> Result<Option<Conversation>, DbError> {
        let inbox = self.inbox(account_id)?;
        let conn = inbox.conn.lock();
        get_conversation_by_remote_locked(&conn, account_id, remote_id)
    }

    pub fn increment_unread(&self, conversation_id: &str) -> Result<(), DbError> {
        let conv = self.get_conversation(conversation_id)?;
        let inbox = self.inbox(&conv.account_id)?;
        inbox.conn.lock().execute(
            "UPDATE conversations SET unread_count = unread_count + 1 WHERE id = ?1",
            params![conversation_id],
        )?;
        Ok(())
    }

    pub fn last_inbound_remote_id(&self, conversation_id: &str) -> Result<Option<String>, DbError> {
        let conv = self.get_conversation(conversation_id)?;
        let inbox = self.inbox(&conv.account_id)?;
        let conn = inbox.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT remote_id FROM messages WHERE conversation_id = ?1 AND direction = 'inbound' AND remote_id IS NOT NULL
             ORDER BY timestamp DESC LIMIT 1",
        )?;
        stmt.query_row(params![conversation_id], |row| row.get(0))
            .optional()
            .map_err(DbError::from)
    }

    pub fn mark_conversation_read(&self, conversation_id: &str) -> Result<(), DbError> {
        let conv = self.get_conversation(conversation_id)?;
        let inbox = self.inbox(&conv.account_id)?;
        inbox.conn.lock().execute(
            "UPDATE conversations SET unread_count = 0 WHERE id = ?1",
            params![conversation_id],
        )?;
        Ok(())
    }

    pub fn search(&self, query: &str) -> Result<Vec<Conversation>, DbError> {
        let pattern = format!("%{query}%");
        let mut all = self.for_each_inbox(|_aid, inbox| {
            let conn = inbox.conn.lock();
            let mut stmt = conn.prepare(
                "SELECT DISTINCT c.id, c.account_id, c.remote_id, c.contact_id, c.title, c.conversation_type,
                        c.unread_count, c.last_message_at, c.last_message_preview, c.pinned, c.archived, c.muted, c.metadata,
                        c.workspace_id, c.priority_group, c.notes, c.notify_enabled, c.send_receipts
                 FROM conversations c
                 LEFT JOIN messages m ON m.conversation_id = c.id
                 WHERE c.title LIKE ?1 OR m.body LIKE ?1
                 ORDER BY c.last_message_at DESC LIMIT 50",
            )?;
            let rows = stmt.query_map(params![&pattern], map_conversation_row)?;
            rows.collect::<Result<Vec<_>, _>>().map_err(DbError::from)
        })?;
        all.sort_by(|a, b| b.last_message_at.cmp(&a.last_message_at));
        all.truncate(50);
        Ok(all)
    }

    pub fn total_unread(&self) -> Result<i64, DbError> {
        let mut total = 0_i64;
        for account in self.list_accounts()? {
            let inbox = self.inbox(&account.id)?;
            let count: i64 = inbox.conn.lock().query_row(
                "SELECT COALESCE(SUM(unread_count), 0) FROM conversations WHERE archived = 0",
                [],
                |r| r.get(0),
            )?;
            total += count;
        }
        Ok(total)
    }

    pub fn get_account(&self, id: &str) -> Result<Account, DbError> {
        self.list_accounts()?
            .into_iter()
            .find(|a| a.id == id)
            .ok_or_else(|| DbError::NotFound(id.to_string()))
    }

    pub fn patch_account(&self, id: &str, patch: &AccountPatch) -> Result<Account, DbError> {
        let now = Utc::now().to_rfc3339();
        let conn = self.catalog.conn.lock();
        if patch.name.is_some() {
            conn.execute(
                "UPDATE accounts SET name = COALESCE(?1, name), updated_at = ?2 WHERE id = ?3",
                params![patch.name, now, id],
            )?;
        }
        if let Some(muted) = patch.muted {
            conn.execute(
                "UPDATE accounts SET muted = ?1, updated_at = ?2 WHERE id = ?3",
                params![if muted { 1 } else { 0 }, now, id],
            )?;
        }
        if let Some(disabled) = patch.disabled {
            conn.execute(
                "UPDATE accounts SET disabled = ?1, updated_at = ?2 WHERE id = ?3",
                params![if disabled { 1 } else { 0 }, now, id],
            )?;
        }
        if patch.clear_workspace.unwrap_or(false) {
            conn.execute(
                "UPDATE accounts SET workspace_id = NULL, updated_at = ?1 WHERE id = ?2",
                params![now, id],
            )?;
        } else if patch.workspace_id.is_some() {
            conn.execute(
                "UPDATE accounts SET workspace_id = ?1, updated_at = ?2 WHERE id = ?3",
                params![patch.workspace_id, now, id],
            )?;
        }
        if patch.clear_notify.unwrap_or(false) {
            conn.execute(
                "UPDATE accounts SET notify_enabled = NULL, updated_at = ?1 WHERE id = ?2",
                params![now, id],
            )?;
        } else if let Some(n) = patch.notify_enabled {
            conn.execute(
                "UPDATE accounts SET notify_enabled = ?1, updated_at = ?2 WHERE id = ?3",
                params![if n { 1 } else { 0 }, now, id],
            )?;
        }
        if let Some(r) = patch.send_receipts {
            conn.execute(
                "UPDATE accounts SET send_receipts = ?1, updated_at = ?2 WHERE id = ?3",
                params![if r { 1 } else { 0 }, now, id],
            )?;
        }
        drop(conn);
        self.get_account(id)
    }

    pub fn patch_conversation(
        &self,
        id: &str,
        patch: &ConversationPatch,
    ) -> Result<Conversation, DbError> {
        let conv = self.get_conversation(id)?;
        let inbox = self.inbox(&conv.account_id)?;
        let conn = inbox.conn.lock();
        if let Some(v) = patch.pinned {
            conn.execute(
                "UPDATE conversations SET pinned = ?1 WHERE id = ?2",
                params![if v { 1 } else { 0 }, id],
            )?;
        }
        if let Some(v) = patch.archived {
            conn.execute(
                "UPDATE conversations SET archived = ?1 WHERE id = ?2",
                params![if v { 1 } else { 0 }, id],
            )?;
        }
        if let Some(v) = patch.muted {
            conn.execute(
                "UPDATE conversations SET muted = ?1 WHERE id = ?2",
                params![if v { 1 } else { 0 }, id],
            )?;
        }
        if patch.clear_workspace.unwrap_or(false) {
            conn.execute(
                "UPDATE conversations SET workspace_id = NULL WHERE id = ?1",
                params![id],
            )?;
        } else if patch.workspace_id.is_some() {
            conn.execute(
                "UPDATE conversations SET workspace_id = ?1 WHERE id = ?2",
                params![patch.workspace_id, id],
            )?;
        }
        if patch.clear_priority.unwrap_or(false) {
            conn.execute(
                "UPDATE conversations SET priority_group = NULL WHERE id = ?1",
                params![id],
            )?;
        } else if patch.priority_group.is_some() {
            conn.execute(
                "UPDATE conversations SET priority_group = ?1 WHERE id = ?2",
                params![patch.priority_group, id],
            )?;
        }
        if let Some(notes) = &patch.notes {
            conn.execute(
                "UPDATE conversations SET notes = ?1 WHERE id = ?2",
                params![notes, id],
            )?;
        }
        if patch.clear_notify.unwrap_or(false) {
            conn.execute(
                "UPDATE conversations SET notify_enabled = NULL WHERE id = ?1",
                params![id],
            )?;
        } else if let Some(n) = patch.notify_enabled {
            conn.execute(
                "UPDATE conversations SET notify_enabled = ?1 WHERE id = ?2",
                params![if n { 1 } else { 0 }, id],
            )?;
        }
        if patch.clear_receipts.unwrap_or(false) {
            conn.execute(
                "UPDATE conversations SET send_receipts = NULL WHERE id = ?1",
                params![id],
            )?;
        } else if let Some(r) = patch.send_receipts {
            conn.execute(
                "UPDATE conversations SET send_receipts = ?1 WHERE id = ?2",
                params![if r { 1 } else { 0 }, id],
            )?;
        }
        drop(conn);
        self.get_conversation(id)
    }

    pub fn mark_unread(&self, conversation_id: &str) -> Result<(), DbError> {
        let conv = self.get_conversation(conversation_id)?;
        let inbox = self.inbox(&conv.account_id)?;
        inbox.conn.lock().execute(
            "UPDATE conversations SET unread_count = CASE WHEN unread_count < 1 THEN 1 ELSE unread_count END WHERE id = ?1",
            params![conversation_id],
        )?;
        Ok(())
    }

    pub fn list_workspaces(&self) -> Result<Vec<Workspace>, DbError> {
        let conn = self.catalog.conn.lock();
        let mut stmt =
            conn.prepare("SELECT id, name, builtin, sort_order FROM workspaces ORDER BY sort_order, name")?;
        let rows = stmt.query_map([], |row| {
            Ok(Workspace {
                id: row.get(0)?,
                name: row.get(1)?,
                builtin: row.get::<_, i64>(2)? != 0,
                sort_order: row.get(3)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(DbError::from)
    }

    pub fn create_workspace(&self, name: &str) -> Result<Workspace, DbError> {
        let id = Uuid::new_v4().to_string();
        let conn = self.catalog.conn.lock();
        let max: i64 = conn
            .query_row("SELECT COALESCE(MAX(sort_order), 0) FROM workspaces", [], |r| r.get(0))?;
        conn.execute(
            "INSERT INTO workspaces (id, name, builtin, sort_order) VALUES (?1, ?2, 0, ?3)",
            params![id, name, max + 1],
        )?;
        Ok(Workspace {
            id,
            name: name.to_string(),
            builtin: false,
            sort_order: max + 1,
        })
    }

    pub fn rename_workspace(&self, id: &str, name: &str) -> Result<(), DbError> {
        let n = self.catalog.conn.lock().execute(
            "UPDATE workspaces SET name = ?1 WHERE id = ?2",
            params![name, id],
        )?;
        if n == 0 {
            return Err(DbError::NotFound(id.to_string()));
        }
        Ok(())
    }

    pub fn delete_workspace(&self, id: &str) -> Result<(), DbError> {
        {
            let conn = self.catalog.conn.lock();
            let builtin: i64 = conn
                .query_row("SELECT builtin FROM workspaces WHERE id = ?1", params![id], |r| r.get(0))
                .map_err(|_| DbError::NotFound(id.to_string()))?;
            if builtin != 0 {
                return Err(DbError::Builtin);
            }
            conn.execute("DELETE FROM workspaces WHERE id = ?1", params![id])?;
            conn.execute(
                "UPDATE accounts SET workspace_id = NULL WHERE workspace_id = ?1",
                params![id],
            )?;
        }
        for account in self.list_accounts()? {
            let inbox = self.inbox(&account.id)?;
            inbox.conn.lock().execute(
                "UPDATE conversations SET workspace_id = NULL WHERE workspace_id = ?1",
                params![id],
            )?;
        }
        Ok(())
    }

    pub fn list_priority_groups(&self) -> Result<Vec<PriorityGroup>, DbError> {
        let conn = self.catalog.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT id, name, color, builtin, sort_order FROM priority_groups ORDER BY sort_order, name",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(PriorityGroup {
                id: row.get(0)?,
                name: row.get(1)?,
                color: row.get(2)?,
                builtin: row.get::<_, i64>(3)? != 0,
                sort_order: row.get(4)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(DbError::from)
    }

    pub fn create_priority_group(&self, name: &str, color: Option<&str>) -> Result<PriorityGroup, DbError> {
        let id = Uuid::new_v4().to_string();
        let conn = self.catalog.conn.lock();
        let max: i64 = conn.query_row(
            "SELECT COALESCE(MAX(sort_order), 0) FROM priority_groups",
            [],
            |r| r.get(0),
        )?;
        conn.execute(
            "INSERT INTO priority_groups (id, name, color, builtin, sort_order) VALUES (?1, ?2, ?3, 0, ?4)",
            params![id, name, color, max + 1],
        )?;
        Ok(PriorityGroup {
            id,
            name: name.to_string(),
            color: color.map(str::to_string),
            builtin: false,
            sort_order: max + 1,
        })
    }

    pub fn rename_priority_group(&self, id: &str, name: &str) -> Result<(), DbError> {
        let n = self.catalog.conn.lock().execute(
            "UPDATE priority_groups SET name = ?1 WHERE id = ?2",
            params![name, id],
        )?;
        if n == 0 {
            return Err(DbError::NotFound(id.to_string()));
        }
        Ok(())
    }

    pub fn delete_priority_group(&self, id: &str) -> Result<(), DbError> {
        {
            let conn = self.catalog.conn.lock();
            let builtin: i64 = conn
                .query_row(
                    "SELECT builtin FROM priority_groups WHERE id = ?1",
                    params![id],
                    |r| r.get(0),
                )
                .map_err(|_| DbError::NotFound(id.to_string()))?;
            if builtin != 0 {
                return Err(DbError::Builtin);
            }
            conn.execute("DELETE FROM priority_groups WHERE id = ?1", params![id])?;
        }
        for account in self.list_accounts()? {
            let inbox = self.inbox(&account.id)?;
            inbox.conn.lock().execute(
                "UPDATE conversations SET priority_group = NULL WHERE priority_group = ?1",
                params![id],
            )?;
        }
        Ok(())
    }

    pub fn list_todos(&self, conversation_id: &str) -> Result<Vec<ChatTodo>, DbError> {
        let conn = self.catalog.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT id, conversation_id, account_id, body, due_at, done, created_at
             FROM chat_todos WHERE conversation_id = ?1 ORDER BY done, created_at",
        )?;
        let rows = stmt.query_map(params![conversation_id], map_todo_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(DbError::from)
    }

    pub fn add_todo(
        &self,
        conversation_id: &str,
        account_id: &str,
        body: &str,
        due_at: Option<&str>,
    ) -> Result<ChatTodo, DbError> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        self.catalog.conn.lock().execute(
            "INSERT INTO chat_todos (id, conversation_id, account_id, body, due_at, done, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 0, ?6)",
            params![id, conversation_id, account_id, body, due_at, now],
        )?;
        Ok(ChatTodo {
            id,
            conversation_id: conversation_id.to_string(),
            account_id: account_id.to_string(),
            body: body.to_string(),
            due_at: due_at.map(str::to_string),
            done: false,
            created_at: now,
        })
    }

    pub fn set_todo_done(&self, id: &str, done: bool) -> Result<(), DbError> {
        let n = self.catalog.conn.lock().execute(
            "UPDATE chat_todos SET done = ?1 WHERE id = ?2",
            params![if done { 1 } else { 0 }, id],
        )?;
        if n == 0 {
            return Err(DbError::NotFound(id.to_string()));
        }
        Ok(())
    }

    pub fn delete_todo(&self, id: &str) -> Result<(), DbError> {
        self.catalog
            .conn
            .lock()
            .execute("DELETE FROM chat_todos WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn list_reminders(&self, conversation_id: Option<&str>) -> Result<Vec<Reminder>, DbError> {
        let conn = self.catalog.conn.lock();
        if let Some(cid) = conversation_id {
            let mut stmt = conn.prepare(
                "SELECT id, conversation_id, account_id, fire_at, kind, note, fired, created_at
                 FROM reminders WHERE conversation_id = ?1 ORDER BY fire_at",
            )?;
            let rows = stmt.query_map(params![cid], map_reminder_row)?;
            rows.collect::<Result<Vec<_>, _>>().map_err(DbError::from)
        } else {
            let mut stmt = conn.prepare(
                "SELECT id, conversation_id, account_id, fire_at, kind, note, fired, created_at
                 FROM reminders ORDER BY fire_at",
            )?;
            let rows = stmt.query_map([], map_reminder_row)?;
            rows.collect::<Result<Vec<_>, _>>().map_err(DbError::from)
        }
    }

    pub fn create_reminder(
        &self,
        conversation_id: &str,
        account_id: &str,
        fire_at: &str,
        kind: &str,
        note: Option<&str>,
    ) -> Result<Reminder, DbError> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        self.catalog.conn.lock().execute(
            "INSERT INTO reminders (id, conversation_id, account_id, fire_at, kind, note, fired, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, ?7)",
            params![id, conversation_id, account_id, fire_at, kind, note, now],
        )?;
        Ok(Reminder {
            id,
            conversation_id: conversation_id.to_string(),
            account_id: account_id.to_string(),
            fire_at: fire_at.to_string(),
            kind: kind.to_string(),
            note: note.map(str::to_string),
            fired: false,
            created_at: now,
        })
    }

    pub fn delete_reminder(&self, id: &str) -> Result<(), DbError> {
        self.catalog
            .conn
            .lock()
            .execute("DELETE FROM reminders WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn due_reminders(&self) -> Result<Vec<Reminder>, DbError> {
        let now = Utc::now().to_rfc3339();
        let conn = self.catalog.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT id, conversation_id, account_id, fire_at, kind, note, fired, created_at
             FROM reminders WHERE fired = 0 AND fire_at <= ?1 ORDER BY fire_at",
        )?;
        let rows = stmt.query_map(params![now], map_reminder_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(DbError::from)
    }

    pub fn mark_reminder_fired(&self, id: &str) -> Result<(), DbError> {
        self.catalog.conn.lock().execute(
            "UPDATE reminders SET fired = 1 WHERE id = ?1",
            params![id],
        )?;
        Ok(())
    }

    pub fn list_forward_rules(&self) -> Result<Vec<ForwardRule>, DbError> {
        let conn = self.catalog.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT id, enabled, source_account_id, source_conversation_id, source_workspace_id,
                    dest_account_id, dest_conversation_id, inbound_only, include_self, keyword,
                    prefix, suffix, strip_sender, skip_if_forwarded, delay_seconds, created_at
             FROM forwarding_rules ORDER BY created_at DESC",
        )?;
        let rows = stmt.query_map([], map_forward_rule_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(DbError::from)
    }

    pub fn create_forward_rule(&self, draft: &ForwardRuleDraft) -> Result<ForwardRule, DbError> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        self.catalog.conn.lock().execute(
            "INSERT INTO forwarding_rules (
                id, enabled, source_account_id, source_conversation_id, source_workspace_id,
                dest_account_id, dest_conversation_id, inbound_only, include_self, keyword,
                prefix, suffix, strip_sender, skip_if_forwarded, delay_seconds, created_at
             ) VALUES (?1, 1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
            params![
                id,
                draft.source_account_id,
                draft.source_conversation_id,
                draft.source_workspace_id,
                draft.dest_account_id,
                draft.dest_conversation_id,
                if draft.inbound_only.unwrap_or(true) { 1 } else { 0 },
                if draft.include_self.unwrap_or(false) { 1 } else { 0 },
                draft.keyword,
                draft.prefix,
                draft.suffix,
                if draft.strip_sender.unwrap_or(false) { 1 } else { 0 },
                if draft.skip_if_forwarded.unwrap_or(true) { 1 } else { 0 },
                draft.delay_seconds.unwrap_or(0),
                now,
            ],
        )?;
        self.get_forward_rule(&id)
    }

    pub fn get_forward_rule(&self, id: &str) -> Result<ForwardRule, DbError> {
        let conn = self.catalog.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT id, enabled, source_account_id, source_conversation_id, source_workspace_id,
                    dest_account_id, dest_conversation_id, inbound_only, include_self, keyword,
                    prefix, suffix, strip_sender, skip_if_forwarded, delay_seconds, created_at
             FROM forwarding_rules WHERE id = ?1",
        )?;
        stmt.query_row(params![id], map_forward_rule_row)
            .optional()?
            .ok_or_else(|| DbError::NotFound(id.to_string()))
    }

    pub fn patch_forward_rule(&self, id: &str, patch: &ForwardRulePatch) -> Result<ForwardRule, DbError> {
        let conn = self.catalog.conn.lock();
        if let Some(enabled) = patch.enabled {
            conn.execute(
                "UPDATE forwarding_rules SET enabled = ?1 WHERE id = ?2",
                params![if enabled { 1 } else { 0 }, id],
            )?;
        }
        if patch.clear_source_account.unwrap_or(false) {
            conn.execute("UPDATE forwarding_rules SET source_account_id = NULL WHERE id = ?1", params![id])?;
        } else if patch.source_account_id.is_some() {
            conn.execute(
                "UPDATE forwarding_rules SET source_account_id = ?1 WHERE id = ?2",
                params![patch.source_account_id, id],
            )?;
        }
        if patch.clear_source_conversation.unwrap_or(false) {
            conn.execute(
                "UPDATE forwarding_rules SET source_conversation_id = NULL WHERE id = ?1",
                params![id],
            )?;
        } else if patch.source_conversation_id.is_some() {
            conn.execute(
                "UPDATE forwarding_rules SET source_conversation_id = ?1 WHERE id = ?2",
                params![patch.source_conversation_id, id],
            )?;
        }
        if patch.clear_source_workspace.unwrap_or(false) {
            conn.execute(
                "UPDATE forwarding_rules SET source_workspace_id = NULL WHERE id = ?1",
                params![id],
            )?;
        } else if patch.source_workspace_id.is_some() {
            conn.execute(
                "UPDATE forwarding_rules SET source_workspace_id = ?1 WHERE id = ?2",
                params![patch.source_workspace_id, id],
            )?;
        }
        if let Some(value) = &patch.dest_account_id {
            conn.execute(
                "UPDATE forwarding_rules SET dest_account_id = ?1 WHERE id = ?2",
                params![value, id],
            )?;
        }
        if let Some(value) = &patch.dest_conversation_id {
            conn.execute(
                "UPDATE forwarding_rules SET dest_conversation_id = ?1 WHERE id = ?2",
                params![value, id],
            )?;
        }
        if let Some(v) = patch.inbound_only {
            conn.execute(
                "UPDATE forwarding_rules SET inbound_only = ?1 WHERE id = ?2",
                params![if v { 1 } else { 0 }, id],
            )?;
        }
        if let Some(v) = patch.include_self {
            conn.execute(
                "UPDATE forwarding_rules SET include_self = ?1 WHERE id = ?2",
                params![if v { 1 } else { 0 }, id],
            )?;
        }
        if patch.clear_keyword.unwrap_or(false) {
            conn.execute("UPDATE forwarding_rules SET keyword = NULL WHERE id = ?1", params![id])?;
        } else if patch.keyword.is_some() {
            conn.execute(
                "UPDATE forwarding_rules SET keyword = ?1 WHERE id = ?2",
                params![patch.keyword, id],
            )?;
        }
        if patch.clear_prefix.unwrap_or(false) {
            conn.execute("UPDATE forwarding_rules SET prefix = NULL WHERE id = ?1", params![id])?;
        } else if patch.prefix.is_some() {
            conn.execute(
                "UPDATE forwarding_rules SET prefix = ?1 WHERE id = ?2",
                params![patch.prefix, id],
            )?;
        }
        if patch.clear_suffix.unwrap_or(false) {
            conn.execute("UPDATE forwarding_rules SET suffix = NULL WHERE id = ?1", params![id])?;
        } else if patch.suffix.is_some() {
            conn.execute(
                "UPDATE forwarding_rules SET suffix = ?1 WHERE id = ?2",
                params![patch.suffix, id],
            )?;
        }
        if let Some(v) = patch.strip_sender {
            conn.execute(
                "UPDATE forwarding_rules SET strip_sender = ?1 WHERE id = ?2",
                params![if v { 1 } else { 0 }, id],
            )?;
        }
        if let Some(v) = patch.skip_if_forwarded {
            conn.execute(
                "UPDATE forwarding_rules SET skip_if_forwarded = ?1 WHERE id = ?2",
                params![if v { 1 } else { 0 }, id],
            )?;
        }
        if let Some(v) = patch.delay_seconds {
            conn.execute(
                "UPDATE forwarding_rules SET delay_seconds = ?1 WHERE id = ?2",
                params![v, id],
            )?;
        }
        drop(conn);
        self.get_forward_rule(id)
    }

    pub fn delete_forward_rule(&self, id: &str) -> Result<(), DbError> {
        self.catalog
            .conn
            .lock()
            .execute("DELETE FROM forwarding_rules WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn list_scheduled_messages(&self, include_sent: bool) -> Result<Vec<ScheduledMessage>, DbError> {
        let conn = self.catalog.conn.lock();
        let sql = if include_sent {
            "SELECT id, source_account_id, source_conversation_id, source_message_id, dest_account_id,
                    dest_conversation_id, body, send_at, sent, created_at
             FROM scheduled_messages ORDER BY send_at"
        } else {
            "SELECT id, source_account_id, source_conversation_id, source_message_id, dest_account_id,
                    dest_conversation_id, body, send_at, sent, created_at
             FROM scheduled_messages WHERE sent = 0 ORDER BY send_at"
        };
        let mut stmt = conn.prepare(sql)?;
        let rows = stmt.query_map([], map_scheduled_message_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(DbError::from)
    }

    pub fn schedule_message(&self, draft: &ScheduleMessageDraft) -> Result<ScheduledMessage, DbError> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        self.catalog.conn.lock().execute(
            "INSERT INTO scheduled_messages (
                id, source_account_id, source_conversation_id, source_message_id,
                dest_account_id, dest_conversation_id, body, send_at, sent, created_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 0, ?9)",
            params![
                id,
                draft.source_account_id,
                draft.source_conversation_id,
                draft.source_message_id,
                draft.dest_account_id,
                draft.dest_conversation_id,
                draft.body,
                draft.send_at,
                now,
            ],
        )?;
        self.get_scheduled_message(&id)
    }

    pub fn get_scheduled_message(&self, id: &str) -> Result<ScheduledMessage, DbError> {
        let conn = self.catalog.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT id, source_account_id, source_conversation_id, source_message_id, dest_account_id,
                    dest_conversation_id, body, send_at, sent, created_at
             FROM scheduled_messages WHERE id = ?1",
        )?;
        stmt.query_row(params![id], map_scheduled_message_row)
            .optional()?
            .ok_or_else(|| DbError::NotFound(id.to_string()))
    }

    pub fn due_scheduled_messages(&self) -> Result<Vec<ScheduledMessage>, DbError> {
        let now = Utc::now().to_rfc3339();
        let conn = self.catalog.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT id, source_account_id, source_conversation_id, source_message_id, dest_account_id,
                    dest_conversation_id, body, send_at, sent, created_at
             FROM scheduled_messages WHERE sent = 0 AND send_at <= ?1 ORDER BY send_at",
        )?;
        let rows = stmt.query_map(params![now], map_scheduled_message_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(DbError::from)
    }

    pub fn mark_scheduled_message_sent(&self, id: &str) -> Result<(), DbError> {
        self.catalog
            .conn
            .lock()
            .execute("UPDATE scheduled_messages SET sent = 1 WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn delete_scheduled_message(&self, id: &str) -> Result<(), DbError> {
        self.catalog
            .conn
            .lock()
            .execute("DELETE FROM scheduled_messages WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn get_app_meta(&self, key: &str) -> Result<Option<String>, DbError> {
        let conn = self.catalog.conn.lock();
        conn.query_row(
            "SELECT value FROM app_meta WHERE key = ?1",
            params![key],
            |row| row.get(0),
        )
        .optional()
        .map_err(DbError::from)
    }

    pub fn set_app_meta(&self, key: &str, value: &str) -> Result<(), DbError> {
        self.catalog.conn.lock().execute(
            "INSERT INTO app_meta (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn get_app_setting(&self, key: &str) -> Result<Option<String>, DbError> {
        let conn = self.catalog.conn.lock();
        conn.query_row(
            "SELECT value FROM app_settings WHERE key = ?1",
            params![key],
            |row| row.get(0),
        )
        .optional()
        .map_err(DbError::from)
    }

    pub fn set_app_setting(&self, key: &str, value: &str) -> Result<(), DbError> {
        self.catalog.conn.lock().execute(
            "INSERT INTO app_settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn ensure_installation_id(&self) -> Result<String, DbError> {
        if let Some(id) = self.get_app_meta("installation_id")? {
            return Ok(id);
        }
        let id = Uuid::new_v4().to_string();
        self.set_app_meta("installation_id", &id)?;
        self.set_app_meta("installation_created_at", &Utc::now().to_rfc3339())?;
        Ok(id)
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }
}

fn list_conversations_in(
    conn: &Connection,
    account_id: Option<&str>,
    include_archived: bool,
) -> Result<Vec<Conversation>, DbError> {
    if let Some(aid) = account_id {
        let sql = if include_archived {
            format!("{CONV_SELECT} WHERE c.account_id = ?1 ORDER BY c.pinned DESC, c.last_message_at DESC")
        } else {
            format!("{CONV_SELECT} WHERE c.account_id = ?1 AND c.archived = 0 ORDER BY c.pinned DESC, c.last_message_at DESC")
        };
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params![aid], map_conversation_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(DbError::from)
    } else {
        let sql = if include_archived {
            format!("{CONV_SELECT} ORDER BY c.pinned DESC, c.last_message_at DESC")
        } else {
            format!("{CONV_SELECT} WHERE c.archived = 0 ORDER BY c.pinned DESC, c.last_message_at DESC")
        };
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map([], map_conversation_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(DbError::from)
    }
}

fn app_db_path(data_dir: &Path) -> PathBuf {
    data_dir.join("app.sqlite")
}

fn migrate_catalog_to_app(data_dir: &Path) -> Result<(), DbError> {
    let app = app_db_path(data_dir);
    if app.exists() {
        return Ok(());
    }
    let catalog = data_dir.join("catalog.sqlite");
    if !catalog.exists() {
        return Ok(());
    }
    tracing::info!("renaming catalog.sqlite to app.sqlite");
    std::fs::rename(&catalog, &app)?;
    for suffix in ["-wal", "-shm"] {
        let src = data_dir.join(format!("catalog.sqlite{suffix}"));
        if src.exists() {
            let _ = std::fs::rename(&src, data_dir.join(format!("app.sqlite{suffix}")));
        }
    }
    Ok(())
}

fn migrate_legacy(data_dir: &Path, catalog: &Conn) -> Result<(), DbError> {
    let old = data_dir.join("database.sqlite");
    if !old.exists() {
        return Ok(());
    }
    tracing::info!("splitting legacy database.sqlite into catalog + per-account inboxes");
    let legacy = Connection::open(&old)?;
    let accounts: Vec<(String, String, String, Option<String>, String, String, String, String)> = {
        let mut stmt = legacy.prepare(
            "SELECT id, connector_id, name, identity, status, metadata, created_at, updated_at FROM accounts",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
                row.get(6)?,
                row.get(7)?,
            ))
        })?;
        rows.collect::<Result<Vec<_>, _>>()?
    };

    {
        let cat = catalog.conn.lock();
        for row in &accounts {
            cat.execute(
                "INSERT OR IGNORE INTO accounts (id, connector_id, name, identity, status, metadata, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![row.0, row.1, row.2, row.3, row.4, row.5, row.6, row.7],
            )?;
        }
    }

    let account_ids: Vec<String> = accounts.iter().map(|a| a.0.clone()).collect();
    for account_id in account_ids {
        let path = data_dir.join("accounts").join(&account_id).join("inbox.sqlite");
        let inbox = Conn::open(&path, true)?;
        let dest = inbox.conn.lock();
        copy_account_rows(&legacy, &dest, &account_id)?;
    }

    drop(legacy);
    let bak = data_dir.join("database.sqlite.legacy");
    let _ = std::fs::rename(&old, &bak);
    for suffix in ["-wal", "-shm"] {
        let src = data_dir.join(format!("database.sqlite{suffix}"));
        if src.exists() {
            let _ = std::fs::rename(&src, data_dir.join(format!("database.sqlite.legacy{suffix}")));
        }
    }
    Ok(())
}

fn copy_account_rows(src: &Connection, dest: &Connection, account_id: &str) -> Result<(), DbError> {
    let mut convs = src.prepare(
        "SELECT id, account_id, remote_id, contact_id, title, conversation_type, unread_count,
                last_message_at, last_message_preview, pinned, archived, muted, metadata, updated_at
         FROM conversations WHERE account_id = ?1",
    )?;
    let rows = convs.query_map(params![account_id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, Option<String>>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, String>(5)?,
            row.get::<_, i64>(6)?,
            row.get::<_, Option<String>>(7)?,
            row.get::<_, Option<String>>(8)?,
            row.get::<_, i64>(9)?,
            row.get::<_, i64>(10)?,
            row.get::<_, i64>(11)?,
            row.get::<_, String>(12)?,
            row.get::<_, Option<String>>(13)?,
        ))
    })?;
    let mut conv_ids = Vec::new();
    for row in rows {
        let r = row?;
        conv_ids.push(r.0.clone());
        dest.execute(
            "INSERT OR IGNORE INTO conversations (id, account_id, remote_id, contact_id, title, conversation_type,
             unread_count, last_message_at, last_message_preview, pinned, archived, muted, metadata, updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)",
            params![r.0, r.1, r.2, r.3, r.4, r.5, r.6, r.7, r.8, r.9, r.10, r.11, r.12, r.13],
        )?;
    }
    for conv_id in conv_ids {
        let mut msgs = src.prepare(
            "SELECT id, conversation_id, remote_id, sender_id, sender_name, direction, body, timestamp, status, metadata
             FROM messages WHERE conversation_id = ?1",
        )?;
        let rows = msgs.query_map(params![conv_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, String>(7)?,
                row.get::<_, String>(8)?,
                row.get::<_, String>(9)?,
            ))
        })?;
        for row in rows {
            let r = row?;
            dest.execute(
                "INSERT OR IGNORE INTO messages (id, conversation_id, remote_id, sender_id, sender_name, direction, body, timestamp, status, metadata)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
                params![r.0, r.1, r.2, r.3, r.4, r.5, r.6, r.7, r.8, r.9],
            )?;
        }
    }
    Ok(())
}

fn get_conversation_locked(conn: &Connection, id: &str) -> Result<Conversation, DbError> {
    let sql = format!("{CONV_SELECT} WHERE c.id = ?1");
    let mut stmt = conn.prepare(&sql)?;
    stmt.query_row(params![id], map_conversation_row)
        .optional()?
        .ok_or_else(|| DbError::NotFound(id.to_string()))
}

fn get_conversation_by_remote_locked(
    conn: &Connection,
    account_id: &str,
    remote_id: &str,
) -> Result<Option<Conversation>, DbError> {
    let sql = format!("{CONV_SELECT} WHERE c.account_id = ?1 AND c.remote_id = ?2");
    let mut stmt = conn.prepare(&sql)?;
    stmt.query_row(params![account_id, remote_id], map_conversation_row)
        .optional()
        .map_err(DbError::from)
}

fn map_conversation_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Conversation> {
    Ok(Conversation {
        id: row.get(0)?,
        account_id: row.get(1)?,
        remote_id: row.get(2)?,
        contact_id: row.get(3)?,
        title: row.get(4)?,
        conversation_type: parse_conversation_type(&row.get::<_, String>(5)?),
        unread_count: row.get(6)?,
        last_message_at: row
            .get::<_, Option<String>>(7)?
            .and_then(|s| s.parse().ok()),
        last_message_preview: row.get(8)?,
        pinned: row.get::<_, i64>(9)? != 0,
        archived: row.get::<_, i64>(10)? != 0,
        muted: row.get::<_, i64>(11)? != 0,
        metadata: serde_json::from_str(&row.get::<_, String>(12)?).unwrap_or_default(),
        workspace_id: row.get(13)?,
        priority_group: row.get(14)?,
        notes: row.get::<_, Option<String>>(15)?.unwrap_or_default(),
        notify_enabled: opt_bool(row.get(16)?),
        send_receipts: opt_bool(row.get(17)?),
    })
}

fn opt_bool(v: Option<i64>) -> Option<bool> {
    v.map(|n| n != 0)
}

fn effective_workspace<'a>(conv: &'a Conversation, account: Option<&'a Account>) -> &'a str {
    conv.workspace_id
        .as_deref()
        .or_else(|| account.and_then(|a| a.workspace_id.as_deref()))
        .unwrap_or("others")
}

fn map_todo_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ChatTodo> {
    Ok(ChatTodo {
        id: row.get(0)?,
        conversation_id: row.get(1)?,
        account_id: row.get(2)?,
        body: row.get(3)?,
        due_at: row.get(4)?,
        done: row.get::<_, i64>(5)? != 0,
        created_at: row.get(6)?,
    })
}

fn map_reminder_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Reminder> {
    Ok(Reminder {
        id: row.get(0)?,
        conversation_id: row.get(1)?,
        account_id: row.get(2)?,
        fire_at: row.get(3)?,
        kind: row.get(4)?,
        note: row.get(5)?,
        fired: row.get::<_, i64>(6)? != 0,
        created_at: row.get(7)?,
    })
}

fn map_forward_rule_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ForwardRule> {
    Ok(ForwardRule {
        id: row.get(0)?,
        enabled: row.get::<_, i64>(1)? != 0,
        source_account_id: row.get(2)?,
        source_conversation_id: row.get(3)?,
        source_workspace_id: row.get(4)?,
        dest_account_id: row.get(5)?,
        dest_conversation_id: row.get(6)?,
        inbound_only: row.get::<_, i64>(7)? != 0,
        include_self: row.get::<_, i64>(8)? != 0,
        keyword: row.get(9)?,
        prefix: row.get(10)?,
        suffix: row.get(11)?,
        strip_sender: row.get::<_, i64>(12)? != 0,
        skip_if_forwarded: row.get::<_, i64>(13)? != 0,
        delay_seconds: row.get(14)?,
        created_at: row.get(15)?,
    })
}

fn map_scheduled_message_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ScheduledMessage> {
    Ok(ScheduledMessage {
        id: row.get(0)?,
        source_account_id: row.get(1)?,
        source_conversation_id: row.get(2)?,
        source_message_id: row.get(3)?,
        dest_account_id: row.get(4)?,
        dest_conversation_id: row.get(5)?,
        body: row.get(6)?,
        send_at: row.get(7)?,
        sent: row.get::<_, i64>(8)? != 0,
        created_at: row.get(9)?,
    })
}

fn map_message_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Message> {
    Ok(Message {
        id: row.get(0)?,
        conversation_id: row.get(1)?,
        remote_id: row.get(2)?,
        sender_id: row.get(3)?,
        sender_name: row.get(4)?,
        direction: parse_direction(&row.get::<_, String>(5)?),
        body: row.get(6)?,
        timestamp: row.get::<_, String>(7)?.parse().unwrap_or_else(|_| Utc::now()),
        status: parse_message_status(&row.get::<_, String>(8)?),
        metadata: serde_json::from_str(&row.get::<_, String>(9)?).unwrap_or_default(),
    })
}

fn parse_account_status(s: &str) -> AccountStatus {
    match s {
        "connected" => AccountStatus::Connected,
        "connecting" => AccountStatus::Connecting,
        "error" => AccountStatus::Error,
        "awaiting_auth" => AccountStatus::AwaitingAuth,
        _ => AccountStatus::Disconnected,
    }
}

fn account_status_str(s: &AccountStatus) -> &'static str {
    match s {
        AccountStatus::Connected => "connected",
        AccountStatus::Connecting => "connecting",
        AccountStatus::Error => "error",
        AccountStatus::AwaitingAuth => "awaiting_auth",
        AccountStatus::Disconnected => "disconnected",
    }
}

fn parse_conversation_type(s: &str) -> ConversationType {
    match s {
        "group" => ConversationType::Group,
        "channel" => ConversationType::Channel,
        _ => ConversationType::Direct,
    }
}

fn parse_direction(s: &str) -> MessageDirection {
    if s == "outbound" {
        MessageDirection::Outbound
    } else {
        MessageDirection::Inbound
    }
}

fn direction_str(d: &MessageDirection) -> &'static str {
    match d {
        MessageDirection::Outbound => "outbound",
        MessageDirection::Inbound => "inbound",
    }
}

fn parse_message_status(s: &str) -> MessageStatus {
    match s {
        "sent" => MessageStatus::Sent,
        "delivered" => MessageStatus::Delivered,
        "read" => MessageStatus::Read,
        "failed" => MessageStatus::Failed,
        _ => MessageStatus::Pending,
    }
}

fn message_status_str(s: &MessageStatus) -> &'static str {
    match s {
        MessageStatus::Sent => "sent",
        MessageStatus::Delivered => "delivered",
        MessageStatus::Read => "read",
        MessageStatus::Failed => "failed",
        MessageStatus::Pending => "pending",
    }
}
