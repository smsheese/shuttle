const CONV_SELECT: &str = "SELECT c.id, c.account_id, c.remote_id, c.contact_id, c.title, c.conversation_type,
        c.unread_count, c.last_message_at, c.last_message_preview, c.pinned, c.archived, c.muted, c.metadata,
        c.workspace_id, c.priority_group, c.notes, c.notify_enabled, c.send_receipts,
        c.auto_download, c.last_message_from_me, c.last_message_sender_id, c.last_message_sender_name
 FROM conversations c";

const ACCOUNT_SELECT: &str =
    "SELECT id, connector_id, name, identity, status, metadata, created_at, updated_at,
        disabled, muted, workspace_id, notify_enabled, send_receipts,
        sleep_enabled, sleep_after_minutes, sleep_check_minutes, auto_download
 FROM accounts";

use crate::models::*;
use chrono::{DateTime, Utc};
use parking_lot::Mutex;
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use thiserror::Error;
use uuid::Uuid;

mod chat;
mod notes;
mod reactions;
mod replies;
mod schema;
#[cfg(test)]
mod tests;

pub use chat::{
    is_revoked, ConversationEvent, IngestKind, MergedAway, MessageChange, PollVoteFact,
    SendReconcile,
};
pub use reactions::ReactionFact;
pub use replies::ReplyTarget;

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
    #[error("Invalid input: {0}")]
    InvalidInput(String),
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
        conn.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA foreign_keys=ON;",
        )?;
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
/// Channels and status updates are feeds, not conversations needing a reply: the sidebar badges skip them.
const BADGE_COUNTS: &str = "conversation_type != 'channel' AND remote_id NOT LIKE '%@newsletter' AND remote_id != 'status@broadcast'";

pub struct Database {
    data_dir: PathBuf,
    catalog: Conn,
    inboxes: Mutex<HashMap<String, Arc<Conn>>>,
    /// Conversation id -> account id, so hot paths need not scan every inbox.
    conv_index: Mutex<HashMap<String, String>>,
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
            conv_index: Mutex::new(HashMap::new()),
        };
        migrate_legacy(data_dir, &db.catalog)?;
        if let Ok(accounts) = db.list_accounts() {
            for account in accounts {
                let _ = db.inbox(&account.id);
            }
        }
        // Old per-chat notes text becomes note items (retried on the next start on failure).
        if let Err(e) = db.migrate_legacy_notes() {
            tracing::warn!("migrate legacy chat notes: {e}");
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
            (
                &wa.id,
                "Sarah Chen",
                "Hey, are we still on for lunch tomorrow?",
                2_i64,
                "direct",
            ),
            (
                &wa.id,
                "Design Team",
                "Alex: Updated the mockups in Figma",
                5_i64,
                "group",
            ),
            (
                &tg.id,
                "Mom",
                "Don't forget to call grandma this weekend ❤️",
                1_i64,
                "direct",
            ),
            (
                &tg.id,
                "Rust Developers",
                "New async patterns discussion thread",
                12_i64,
                "group",
            ),
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
        let mut stmt = conn.prepare(&format!("{ACCOUNT_SELECT} ORDER BY name"))?;
        let rows = stmt.query_map([], map_account_row)?;
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
            sleep_enabled: None,
            sleep_after_minutes: None,
            sleep_check_minutes: None,
            auto_download: AutoDownloadPrefs::default(),
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

    pub fn merge_account_metadata(
        &self,
        id: &str,
        patch: &serde_json::Value,
    ) -> Result<Account, DbError> {
        let conn = self.catalog.conn.lock();
        let existing: String = conn
            .query_row(
                "SELECT metadata FROM accounts WHERE id = ?1",
                params![id],
                |row| row.get(0),
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => DbError::NotFound(id.to_string()),
                other => DbError::from(other),
            })?;
        let base: serde_json::Value =
            serde_json::from_str(&existing).unwrap_or_else(|_| serde_json::json!({}));
        let merged = merge_json_objects(base, patch.clone());
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE accounts SET metadata = ?1, updated_at = ?2 WHERE id = ?3",
            params![merged.to_string(), now, id],
        )?;
        drop(conn);
        self.get_account(id)
    }

    pub fn delete_account(&self, id: &str) -> Result<(), DbError> {
        {
            let conn = self.catalog.conn.lock();
            conn.execute("DELETE FROM chat_todos WHERE account_id = ?1", params![id])?;
            conn.execute("DELETE FROM reminders WHERE account_id = ?1", params![id])?;
            conn.execute("DELETE FROM chat_notes WHERE account_id = ?1", params![id])?;
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
        self.conv_index.lock().retain(|_, account| account != id);
        let dir = self.data_dir.join("accounts").join(id);
        if dir.exists() {
            std::fs::remove_dir_all(dir)?;
        }
        Ok(())
    }

    pub fn get_conversation(&self, id: &str) -> Result<Conversation, DbError> {
        let cached = self.conv_index.lock().get(id).cloned();
        if let Some(account_id) = cached {
            if let Ok(inbox) = self.inbox(&account_id) {
                let found = get_conversation_locked(&inbox.conn.lock(), id);
                if let Ok(conv) = found {
                    return Ok(conv);
                }
            }
            self.conv_index.lock().remove(id);
        }
        for account in self.list_accounts()? {
            let inbox = self.inbox(&account.id)?;
            let found = get_conversation_locked(&inbox.conn.lock(), id);
            if let Ok(conv) = found {
                self.remember_conversation(id, &account.id);
                return Ok(conv);
            }
        }
        Err(DbError::NotFound(id.to_string()))
    }

    /// Account that owns a conversation, without loading the row when it is already known.
    pub fn conversation_account(&self, id: &str) -> Result<String, DbError> {
        if let Some(account_id) = self.conv_index.lock().get(id).cloned() {
            return Ok(account_id);
        }
        Ok(self.get_conversation(id)?.account_id)
    }

    fn remember_conversation(&self, id: &str, account_id: &str) {
        self.conv_index
            .lock()
            .insert(id.to_string(), account_id.to_string());
    }

    fn forget_conversation(&self, id: &str) {
        self.conv_index.lock().remove(id);
    }
    /// One page of a chat's messages, oldest first, plus whether the chat has more on each side.
    ///
    /// The page is a keyset window over `(unixepoch(timestamp, 'subsec') DESC, id ASC)`, the total
    /// order the chat rules use and the one `idx_messages_conv_epoch` stores, so equal timestamps
    /// neither repeat nor skip a row and a page is an index range scan, not a sort of the chat.
    pub fn message_page(
        &self,
        conversation_id: &str,
        anchor: &MessageAnchor,
        limit: i64,
    ) -> Result<MessagePage, DbError> {
        let limit = limit.clamp(1, 5000);
        let account_id = self.conversation_account(conversation_id)?;
        let inbox = self.inbox(&account_id)?;
        let conn = inbox.conn.lock();

        let newest_first = |predicate: &str, values: &[&dyn rusqlite::ToSql]| {
            let sql = format!(
                "{PAGE_SELECT} {predicate} ORDER BY unixepoch(timestamp, 'subsec') DESC, id ASC LIMIT {limit}"
            );
            rows_in(&conn, &sql, values)
        };
        let oldest_first = |predicate: &str, values: &[&dyn rusqlite::ToSql]| {
            let sql = format!(
                "{PAGE_SELECT} {predicate} ORDER BY unixepoch(timestamp, 'subsec') ASC, id DESC LIMIT {limit}"
            );
            rows_in(&conn, &sql, values)
        };

        let mut rows: Vec<Message> = match anchor {
            MessageAnchor::Newest => {
                let mut page = newest_first("", params![conversation_id])?;
                page.reverse();
                page
            }
            MessageAnchor::Before { message_id } => {
                let (epoch, id) = page_key(&conn, conversation_id, message_id)?;
                let mut page = newest_first(
                    "AND (unixepoch(timestamp, 'subsec') < ?2 OR (unixepoch(timestamp, 'subsec') = ?2 AND id > ?3))",
                    params![conversation_id, epoch, id],
                )?;
                page.reverse();
                page
            }
            MessageAnchor::From { message_id } => {
                let (epoch, id) = page_key(&conn, conversation_id, message_id)?;
                let mut page = newest_first(
                    "AND (unixepoch(timestamp, 'subsec') < ?2 OR (unixepoch(timestamp, 'subsec') = ?2 AND id >= ?3))",
                    params![conversation_id, epoch, id],
                )?;
                page.reverse();
                page
            }
            MessageAnchor::After { message_id } => {
                let (epoch, id) = page_key(&conn, conversation_id, message_id)?;
                oldest_first(
                    "AND (unixepoch(timestamp, 'subsec') > ?2 OR (unixepoch(timestamp, 'subsec') = ?2 AND id < ?3))",
                    params![conversation_id, epoch, id],
                )?
            }
            MessageAnchor::Around { message_id } => {
                let (epoch, id) = page_key(&conn, conversation_id, message_id)?;
                let older_count = (limit / 2).max(1);
                let newer_count = (limit - older_count).max(0);
                let mut older: Vec<Message> = {
                    let sql = format!(
                        "{PAGE_SELECT} AND (unixepoch(timestamp, 'subsec') < ?2 OR (unixepoch(timestamp, 'subsec') = ?2 AND id >= ?3)) ORDER BY unixepoch(timestamp, 'subsec') DESC, id ASC LIMIT {older_count}"
                    );
                    rows_in(&conn, &sql, params![conversation_id, epoch, id])?
                };
                older.reverse();
                let newer = {
                    let sql = format!(
                        "{PAGE_SELECT} AND (unixepoch(timestamp, 'subsec') > ?2 OR (unixepoch(timestamp, 'subsec') = ?2 AND id < ?3)) ORDER BY unixepoch(timestamp, 'subsec') ASC, id DESC LIMIT {newer_count}"
                    );
                    rows_in(&conn, &sql, params![conversation_id, epoch, id])?
                };
                older.extend(newer);
                older
            }
        };

        if rows.is_empty() {
            return Ok(MessagePage {
                messages: Vec::new(),
                has_older: false,
                has_newer: false,
            });
        }
        let mut reactions = reactions::reactions_by_target(&conn, conversation_id)?;
        if !reactions.is_empty() {
            for m in &mut rows {
                if let Some(groups) = m.remote_id.as_ref().and_then(|rid| reactions.remove(rid)) {
                    m.reactions = groups;
                }
            }
        }
        // An unloaded original of a reply is read here, so a quote resolves on every page.
        replies::resolve_in_conversation(&conn, conversation_id, &mut rows)?;

        let oldest = page_key(&conn, conversation_id, &rows[0].id)?;
        let newest = page_key(&conn, conversation_id, &rows[rows.len() - 1].id)?;
        Ok(MessagePage {
            messages: rows,
            has_older: has_row_beyond(&conn, conversation_id, &oldest, PageSide::Older)?,
            has_newer: has_row_beyond(&conn, conversation_id, &newest, PageSide::Newer)?,
        })
    }

    pub fn set_message_starred(&self, message_id: &str, starred: bool) -> Result<Message, DbError> {
        let (account_id, conversation_id) = self.locate_message(message_id)?;
        let inbox = self.inbox(&account_id)?;
        let conn = inbox.conn.lock();
        conn.execute(
            "UPDATE messages SET starred = ?1 WHERE id = ?2",
            params![if starred { 1 } else { 0 }, message_id],
        )?;
        get_message_locked(&conn, &conversation_id, message_id)
    }

    pub fn set_message_pinned(&self, message_id: &str, pinned: bool) -> Result<Message, DbError> {
        let (account_id, conversation_id) = self.locate_message(message_id)?;
        let inbox = self.inbox(&account_id)?;
        let conn = inbox.conn.lock();
        conn.execute(
            "UPDATE messages SET pinned = ?1 WHERE id = ?2",
            params![if pinned { 1 } else { 0 }, message_id],
        )?;
        get_message_locked(&conn, &conversation_id, message_id)
    }

    pub fn list_messages_by_kind(
        &self,
        conversation_id: &str,
        kind: &str,
        limit: i64,
    ) -> Result<Vec<Message>, DbError> {
        // The media, documents, links and starred tabs read the whole chat, newest first, and
        // never page: their rows are their own list, not a window of the conversation.
        let account_id = self.conversation_account(conversation_id)?;
        let inbox = self.inbox(&account_id)?;
        let conn = inbox.conn.lock();
        let sql = match kind {
            "media" => "SELECT id, conversation_id, remote_id, sender_id, sender_name, direction, body, timestamp, status, metadata, starred, pinned
                 FROM messages WHERE conversation_id = ?1
                   AND (
                     json_extract(metadata, '$.media_type') IN ('image','photo','video','audio','ptt','sticker')
                   )
                 ORDER BY timestamp DESC LIMIT ?2",
            "docs" => "SELECT id, conversation_id, remote_id, sender_id, sender_name, direction, body, timestamp, status, metadata, starred, pinned
                 FROM messages WHERE conversation_id = ?1
                   AND json_extract(metadata, '$.media_type') = 'document'
                 ORDER BY timestamp DESC LIMIT ?2",
            "links" => "SELECT id, conversation_id, remote_id, sender_id, sender_name, direction, body, timestamp, status, metadata, starred, pinned
                 FROM messages WHERE conversation_id = ?1
                   AND (body LIKE '%http://%' OR body LIKE '%https://%')
                 ORDER BY timestamp DESC LIMIT ?2",
            "starred" => "SELECT id, conversation_id, remote_id, sender_id, sender_name, direction, body, timestamp, status, metadata, starred, pinned
                 FROM messages WHERE conversation_id = ?1 AND starred = 1
                 ORDER BY timestamp DESC LIMIT ?2",
            _ => return Ok(vec![]),
        };
        let mut stmt = conn.prepare(sql)?;
        let rows = stmt
            .query_map(params![conversation_id, limit], map_message_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    fn locate_message(&self, message_id: &str) -> Result<(String, String), DbError> {
        for account in self.list_accounts()? {
            let inbox = self.inbox(&account.id)?;
            let conn = inbox.conn.lock();
            if let Ok(row) = conn.query_row(
                "SELECT c.account_id, m.conversation_id FROM messages m
                 JOIN conversations c ON c.id = m.conversation_id
                 WHERE m.id = ?1",
                params![message_id],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
            ) {
                return Ok(row);
            }
        }
        Err(DbError::NotFound(message_id.to_string()))
    }

    pub fn search_messages(
        &self,
        query: &str,
        scope: SearchScope,
        account_id: Option<&str>,
        conversation_id: Option<&str>,
    ) -> Result<SearchResults, DbError> {
        let pattern = format!("%{query}%");
        let mut conversations = Vec::new();
        let mut messages = Vec::new();

        match scope {
            SearchScope::Conversation => {
                let cid = conversation_id
                    .ok_or_else(|| DbError::InvalidInput("conversation_id required".into()))?;
                let conv = self.get_conversation(cid)?;
                let inbox = self.inbox(&conv.account_id)?;
                let conn = inbox.conn.lock();
                if conv.title.to_lowercase().contains(&query.to_lowercase()) {
                    conversations.push(conv.clone());
                }
                let mut stmt = conn.prepare(
                    "SELECT m.id, m.conversation_id, m.remote_id, m.sender_id, m.sender_name, m.direction, m.body, m.timestamp, m.status, m.metadata, m.starred, m.pinned
                     FROM messages m
                     WHERE m.conversation_id = ?1 AND (m.body LIKE ?2 OR m.sender_name LIKE ?2)
                     ORDER BY m.timestamp DESC LIMIT 50",
                )?;
                let rows = stmt.query_map(params![cid, &pattern], map_message_row)?;
                for msg in rows {
                    messages.push(SearchMessageHit {
                        message: msg?,
                        conversation_title: conv.title.clone(),
                        account_id: conv.account_id.clone(),
                    });
                }
            }
            SearchScope::Account => {
                let aid = account_id
                    .ok_or_else(|| DbError::InvalidInput("account_id required".into()))?;
                let inbox = self.inbox(aid)?;
                let conn = inbox.conn.lock();
                let mut stmt = conn.prepare(
                    "SELECT DISTINCT c.id, c.account_id, c.remote_id, c.contact_id, c.title, c.conversation_type,
                            c.unread_count, c.last_message_at, c.last_message_preview, c.pinned, c.archived, c.muted, c.metadata,
                            c.workspace_id, c.priority_group, c.notes, c.notify_enabled, c.send_receipts,
                            c.auto_download, c.last_message_from_me, c.last_message_sender_id, c.last_message_sender_name
                     FROM conversations c
                     LEFT JOIN messages m ON m.conversation_id = c.id
                     WHERE c.account_id = ?1 AND (c.title LIKE ?2 OR m.body LIKE ?2 OR m.sender_name LIKE ?2)
                     ORDER BY datetime(COALESCE(NULLIF(c.last_message_at, ''), c.updated_at, '1970-01-01')) DESC
                     LIMIT 50",
                )?;
                conversations = stmt
                    .query_map(params![aid, &pattern], map_conversation_row)?
                    .collect::<Result<Vec<_>, _>>()?;
                let mut stmt = conn.prepare(
                    "SELECT m.id, m.conversation_id, m.remote_id, m.sender_id, m.sender_name, m.direction, m.body, m.timestamp, m.status, m.metadata, m.starred, m.pinned,
                            c.title, c.account_id
                     FROM messages m
                     JOIN conversations c ON c.id = m.conversation_id
                     WHERE c.account_id = ?1 AND (m.body LIKE ?2 OR m.sender_name LIKE ?2 OR c.title LIKE ?2)
                     ORDER BY m.timestamp DESC LIMIT 50",
                )?;
                let rows = stmt.query_map(params![aid, &pattern], |row| {
                    Ok(SearchMessageHit {
                        message: Message {
                            id: row.get(0)?,
                            conversation_id: row.get(1)?,
                            remote_id: row.get(2)?,
                            sender_id: row.get(3)?,
                            sender_name: row.get(4)?,
                            direction: parse_direction(&row.get::<_, String>(5)?),
                            body: row.get(6)?,
                            timestamp: parse_stored_datetime(&row.get::<_, String>(7)?)
                                .unwrap_or_else(Utc::now),
                            status: parse_message_status(&row.get::<_, String>(8)?),
                            metadata: serde_json::from_str(&row.get::<_, String>(9)?)
                                .unwrap_or_default(),
                            starred: row.get::<_, i64>(10)? != 0,
                            pinned: row.get::<_, i64>(11)? != 0,
                            reactions: Vec::new(),
                            reply: None,
                        },
                        conversation_title: row.get(12)?,
                        account_id: row.get(13)?,
                    })
                })?;
                messages = rows.collect::<Result<Vec<_>, _>>()?;
            }
            SearchScope::Global => {
                conversations = self.search(query)?;
                messages = self.for_each_inbox(|aid, inbox| {
                    let conn = inbox.conn.lock();
                    let mut stmt = conn.prepare(
                        "SELECT m.id, m.conversation_id, m.remote_id, m.sender_id, m.sender_name, m.direction, m.body, m.timestamp, m.status, m.metadata, m.starred, m.pinned,
                                c.title
                         FROM messages m
                         JOIN conversations c ON c.id = m.conversation_id
                         WHERE m.body LIKE ?1 OR m.sender_name LIKE ?1 OR c.title LIKE ?1
                         ORDER BY m.timestamp DESC LIMIT 50",
                    )?;
                    let rows = stmt.query_map(params![&pattern], |row| {
                        Ok(SearchMessageHit {
                            message: Message {
                                id: row.get(0)?,
                                conversation_id: row.get(1)?,
                                remote_id: row.get(2)?,
                                sender_id: row.get(3)?,
                                sender_name: row.get(4)?,
                                direction: parse_direction(&row.get::<_, String>(5)?),
                                body: row.get(6)?,
                                timestamp: parse_stored_datetime(&row.get::<_, String>(7)?).unwrap_or_else(Utc::now),
                                status: parse_message_status(&row.get::<_, String>(8)?),
                                metadata: serde_json::from_str(&row.get::<_, String>(9)?).unwrap_or_default(),
                                starred: row.get::<_, i64>(10)? != 0,
                                pinned: row.get::<_, i64>(11)? != 0,
                                reactions: Vec::new(),
                                reply: None,
                            },
                            conversation_title: row.get(12)?,
                            account_id: aid.to_string(),
                        })
                    })?;
                    rows.collect::<Result<Vec<_>, _>>().map_err(DbError::from)
                })?;
                messages.sort_by(|a, b| b.message.timestamp.cmp(&a.message.timestamp));
                messages.truncate(50);
            }
        }

        Ok(SearchResults {
            conversations,
            messages,
        })
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

    /// A group member as a contact: their direct chat on this account (by JID or a recorded
    /// alias) and their address-book entry. Reads only; it never creates a chat.
    pub fn sender_contact(
        &self,
        account_id: &str,
        sender_id: &str,
    ) -> Result<SenderContact, DbError> {
        let inbox = self.inbox(account_id)?;
        let conn = inbox.conn.lock();
        let conversation_id: Option<String> = conn
            .query_row(
                "SELECT id FROM conversations
                 WHERE account_id = ?1 AND conversation_type = 'direct' AND (remote_id = ?2
                   OR EXISTS (SELECT 1 FROM json_each(conversations.metadata, '$.alt_remote_ids')
                              WHERE value = ?2))
                 ORDER BY remote_id = ?2 DESC LIMIT 1",
                params![account_id, sender_id],
                |r| r.get(0),
            )
            .optional()?;
        let conversation = conversation_id
            .map(|id| get_conversation_locked(&conn, &id))
            .transpose()?;
        let contact = conn
            .query_row(
                "SELECT id, account_id, remote_id, display_name, avatar_url, metadata
                 FROM contacts WHERE account_id = ?1 AND remote_id = ?2",
                params![account_id, sender_id],
                map_contact_row,
            )
            .optional()?;
        Ok(SenderContact {
            conversation,
            contact,
        })
    }

    pub fn get_message(&self, conversation_id: &str, message_id: &str) -> Result<Message, DbError> {
        let account_id = self.conversation_account(conversation_id)?;
        let inbox = self.inbox(&account_id)?;
        let conn = inbox.conn.lock();
        get_message_locked(&conn, conversation_id, message_id)
    }

    pub fn merge_conversation_metadata(
        &self,
        account_id: &str,
        remote_id: &str,
        patch: &serde_json::Value,
    ) -> Result<Option<Conversation>, DbError> {
        let inbox = self.inbox(account_id)?;
        let conn = inbox.conn.lock();
        let Some(existing) = get_conversation_by_remote_locked(&conn, account_id, remote_id)?
        else {
            return Ok(None);
        };
        let merged = merge_json_objects(existing.metadata.clone(), patch.clone());
        conn.execute(
            "UPDATE conversations SET metadata = ?1 WHERE id = ?2",
            params![merged.to_string(), existing.id],
        )?;
        Ok(Some(get_conversation_locked(&conn, &existing.id)?))
    }

    pub fn merge_message_metadata_by_remote(
        &self,
        account_id: &str,
        conversation_remote_id: &str,
        message_remote_id: &str,
        patch: &serde_json::Value,
    ) -> Result<Option<Message>, DbError> {
        let inbox = self.inbox(account_id)?;
        let conn = inbox.conn.lock();
        let Some(conv) =
            get_conversation_by_remote_locked(&conn, account_id, conversation_remote_id)?
        else {
            return Ok(None);
        };
        merge_message_metadata_locked(&conn, &conv.id, message_remote_id, patch)?;
        let mut stmt = conn.prepare(
            "SELECT id, conversation_id, remote_id, sender_id, sender_name, direction, body, timestamp, status, metadata
             FROM messages WHERE conversation_id = ?1 AND remote_id = ?2 LIMIT 1",
        )?;
        let mut msg = stmt
            .query_row(params![conv.id, message_remote_id], map_message_row)
            .optional()?;
        if let Some(m) = msg.as_mut() {
            replies::resolve_in_conversation(&conn, &conv.id, std::slice::from_mut(m))?;
        }
        Ok(msg)
    }

    pub fn merge_message_metadata_by_remote_id(
        &self,
        account_id: &str,
        message_remote_id: &str,
        patch: &serde_json::Value,
    ) -> Result<Option<Message>, DbError> {
        let inbox = self.inbox(account_id)?;
        let conn = inbox.conn.lock();
        let conversation_id: Option<String> = conn
            .query_row(
                "SELECT m.conversation_id FROM messages m
                 INNER JOIN conversations c ON c.id = m.conversation_id
                 WHERE c.account_id = ?1 AND m.remote_id = ?2 LIMIT 1",
                params![account_id, message_remote_id],
                |row| row.get(0),
            )
            .optional()?;
        let Some(conversation_id) = conversation_id else {
            return Ok(None);
        };
        merge_message_metadata_locked(&conn, &conversation_id, message_remote_id, patch)?;
        let mut stmt = conn.prepare(
            "SELECT id, conversation_id, remote_id, sender_id, sender_name, direction, body, timestamp, status, metadata
             FROM messages WHERE conversation_id = ?1 AND remote_id = ?2 LIMIT 1",
        )?;
        let mut msg = stmt
            .query_row(params![conversation_id, message_remote_id], map_message_row)
            .optional()?;
        if let Some(m) = msg.as_mut() {
            replies::resolve_in_conversation(&conn, &conversation_id, std::slice::from_mut(m))?;
        }
        Ok(msg)
    }

    pub fn mark_message_media_error(
        &self,
        conversation_id: &str,
        message_id: &str,
        error: &str,
    ) -> Result<Option<Message>, DbError> {
        let account_id = self.conversation_account(conversation_id)?;
        let inbox = self.inbox(&account_id)?;
        let conn = inbox.conn.lock();
        let existing: Option<String> = conn
            .query_row(
                "SELECT metadata FROM messages WHERE id = ?1 AND conversation_id = ?2",
                params![message_id, conversation_id],
                |row| row.get(0),
            )
            .optional()?;
        let Some(raw) = existing else {
            return Ok(None);
        };
        let base: serde_json::Value =
            serde_json::from_str(&raw).unwrap_or_else(|_| serde_json::json!({}));
        let merged = merge_json_objects(base, serde_json::json!({ "media_error": error }));
        conn.execute(
            "UPDATE messages SET metadata = ?1 WHERE id = ?2",
            params![merged.to_string(), message_id],
        )?;
        drop(conn);
        match self.get_message(conversation_id, message_id) {
            Ok(msg) => Ok(Some(msg)),
            Err(_) => Ok(None),
        }
    }

    /// Forget cached files that retention just deleted: every message whose `media_path` is one
    /// of `purged` loses it, so the UI shows the thumbnail and a download button again (the
    /// message itself and its network copy are untouched). Returns, per account, how many
    /// messages were reset.
    pub fn clear_media_paths(
        &self,
        purged: &std::collections::HashSet<String>,
    ) -> Result<Vec<(String, u64)>, DbError> {
        if purged.is_empty() {
            return Ok(Vec::new());
        }
        self.for_each_inbox(|account_id, inbox| {
            let conn = inbox.conn.lock();
            let ids: Vec<String> = {
                let mut stmt = conn.prepare(
                    "SELECT id, json_extract(metadata, '$.media_path') FROM messages
                     WHERE json_extract(metadata, '$.media_path') IS NOT NULL",
                )?;
                let rows = stmt.query_map([], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
                })?;
                let mut ids = Vec::new();
                for row in rows {
                    let (id, path) = row?;
                    if path.is_some_and(|p| purged.contains(&p)) {
                        ids.push(id);
                    }
                }
                ids
            };
            if ids.is_empty() {
                return Ok(Vec::new());
            }
            let tx = conn.unchecked_transaction()?;
            for id in &ids {
                tx.execute(
                    "UPDATE messages SET metadata = json_remove(metadata, '$.media_path') WHERE id = ?1",
                    params![id],
                )?;
            }
            tx.commit()?;
            Ok(vec![(account_id.to_string(), ids.len() as u64)])
        })
    }

    pub fn update_message_status(
        &self,
        conversation_id: &str,
        message_id: &str,
        status: MessageStatus,
    ) -> Result<(), DbError> {
        let account_id = self.conversation_account(conversation_id)?;
        let inbox = self.inbox(&account_id)?;
        let conn = inbox.conn.lock();
        conn.execute(
            "UPDATE messages SET status = ?1 WHERE id = ?2 AND conversation_id = ?3",
            params![message_status_str(&status), message_id, conversation_id],
        )?;
        Ok(())
    }

    #[cfg(test)]
    pub fn upsert_contact(
        &self,
        account_id: &str,
        remote_id: &str,
        display_name: &str,
    ) -> Result<Contact, DbError> {
        self.upsert_contact_flagged(account_id, remote_id, display_name, None)
    }

    /// `saved` is the connector's fact: the name comes from the address book (true) or only from
    /// a push name seen in a group (false). `None` leaves the stored flag untouched.
    pub fn upsert_contact_flagged(
        &self,
        account_id: &str,
        remote_id: &str,
        display_name: &str,
        saved: Option<bool>,
    ) -> Result<Contact, DbError> {
        let inbox = self.inbox(account_id)?;
        let conn = inbox.conn.lock();
        let id = Uuid::new_v4().to_string();
        let meta = match saved {
            Some(v) => serde_json::json!({ "saved": v }).to_string(),
            None => "{}".to_string(),
        };
        conn.execute(
            "INSERT INTO contacts (id, account_id, remote_id, display_name, avatar_url, metadata)
             VALUES (?1, ?2, ?3, ?4, NULL, ?5)
             ON CONFLICT(account_id, remote_id) DO UPDATE SET display_name = excluded.display_name,
               metadata = CASE WHEN ?6 THEN json_set(contacts.metadata, '$.saved', json(CASE WHEN ?7 THEN 'true' ELSE 'false' END)) ELSE contacts.metadata END
             WHERE contacts.display_name != excluded.display_name
                OR (?6 AND json_extract(contacts.metadata, '$.saved') IS NOT ?7)",
            params![id, account_id, remote_id, display_name, meta, saved.is_some(), saved.unwrap_or(true) as i64],
        )?;
        let mut stmt = conn.prepare(
            "SELECT id, account_id, remote_id, display_name, avatar_url, metadata
             FROM contacts WHERE account_id = ?1 AND remote_id = ?2",
        )?;
        stmt.query_row(params![account_id, remote_id], map_contact_row)
            .map_err(DbError::from)
    }

    /// Address-book rows for the New chat picker. Each row also carries two derived facts in
    /// `metadata`: `has_chat` (a direct chat with it exists) and, from that chat, `avatar_url`.
    pub fn list_contacts(&self, account_id: &str) -> Result<Vec<Contact>, DbError> {
        let inbox = self.inbox(account_id)?;
        let conn = inbox.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT k.id, k.account_id, k.remote_id, k.display_name,
                    COALESCE(k.avatar_url, json_extract(c.metadata, '$.avatar_data')), k.metadata,
                    c.id IS NOT NULL
             FROM contacts k
             LEFT JOIN conversations c ON c.account_id = k.account_id AND c.remote_id = k.remote_id
                  AND c.conversation_type = 'direct'
             WHERE k.account_id = ?1 AND k.remote_id NOT LIKE 'status@%'
             ORDER BY k.display_name COLLATE NOCASE ASC",
        )?;
        let rows = stmt.query_map(params![account_id], |row| {
            let mut contact = map_contact_row(row)?;
            let has_chat: bool = row.get(6)?;
            if let Some(map) = contact.metadata.as_object_mut() {
                map.insert("has_chat".into(), serde_json::Value::Bool(has_chat));
            } else {
                contact.metadata = serde_json::json!({ "has_chat": has_chat });
            }
            Ok(contact)
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(DbError::from)
    }

    pub fn last_inbound_remote_id(&self, conversation_id: &str) -> Result<Option<String>, DbError> {
        let account_id = self.conversation_account(conversation_id)?;
        let inbox = self.inbox(&account_id)?;
        let conn = inbox.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT remote_id FROM messages WHERE conversation_id = ?1 AND direction = 'inbound' AND remote_id IS NOT NULL
             ORDER BY timestamp DESC LIMIT 1",
        )?;
        stmt.query_row(params![conversation_id], |row| row.get(0))
            .optional()
            .map_err(DbError::from)
    }

    pub fn search(&self, query: &str) -> Result<Vec<Conversation>, DbError> {
        let pattern = format!("%{query}%");
        let mut all = self.for_each_inbox(|_aid, inbox| {
            let conn = inbox.conn.lock();
            let mut stmt = conn.prepare(
                "SELECT DISTINCT c.id, c.account_id, c.remote_id, c.contact_id, c.title, c.conversation_type,
                        c.unread_count, c.last_message_at, c.last_message_preview, c.pinned, c.archived, c.muted, c.metadata,
                        c.workspace_id, c.priority_group, c.notes, c.notify_enabled, c.send_receipts,
                        c.auto_download, c.last_message_from_me, c.last_message_sender_id, c.last_message_sender_name
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
            if account.muted {
                continue;
            }
            let inbox = self.inbox(&account.id)?;
            let count: i64 = inbox.conn.lock().query_row(
                &format!("SELECT COALESCE(SUM(unread_count), 0) FROM conversations WHERE archived = 0 AND muted = 0 AND {BADGE_COUNTS}"),
                [],
                |r| r.get(0),
            )?;
            total += count;
        }
        Ok(total)
    }

    pub fn self_avatar(&self, account: &Account) -> Result<Option<String>, DbError> {
        let inbox = self.inbox(&account.id)?;
        let conn = inbox.conn.lock();
        let identity = account.identity.as_deref().unwrap_or("").split('@').next().unwrap_or("");
        let avatar = conn.query_row(
            "SELECT json_extract(metadata, '$.avatar_data') FROM conversations WHERE (json_extract(metadata, '$.is_self') = 1 OR (?1 != '' AND remote_id = ?1 || '@s.whatsapp.net')) AND json_extract(metadata, '$.avatar_data') IS NOT NULL LIMIT 1",
            params![identity], |row| row.get::<_, String>(0),
        ).optional()?;
        Ok(avatar)
    }

    pub fn unread_by_account(&self) -> Result<std::collections::HashMap<String, i64>, DbError> {
        let mut totals = std::collections::HashMap::new();
        for account in self.list_accounts()? {
            let inbox = self.inbox(&account.id)?;
            let count = inbox.conn.lock().query_row(
                &format!("SELECT COALESCE(SUM(unread_count), 0) FROM conversations WHERE archived = 0 AND {BADGE_COUNTS}"),
                [],
                |r| r.get(0),
            )?;
            totals.insert(account.id, count);
        }
        Ok(totals)
    }

    pub fn get_account(&self, id: &str) -> Result<Account, DbError> {
        let conn = self.catalog.conn.lock();
        conn.query_row(
            &format!("{ACCOUNT_SELECT} WHERE id = ?1"),
            params![id],
            map_account_row,
        )
        .optional()?
        .ok_or_else(|| DbError::NotFound(id.to_string()))
    }

    pub fn patch_account(&self, id: &str, patch: &AccountPatch) -> Result<Account, DbError> {
        let now = Utc::now().to_rfc3339();
        let conn = self.catalog.conn.lock();
        if let Some(color) = &patch.accent_color {
            if color.len() != 7
                || !color.starts_with('#')
                || !color[1..].bytes().all(|b| b.is_ascii_hexdigit())
            {
                return Err(DbError::InvalidInput("Invalid account accent color".into()));
            }
            conn.execute("UPDATE accounts SET metadata = json_set(metadata, '$.accent_color', ?1), updated_at = ?2 WHERE id = ?3", params![color, now, id])?;
        }
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
        if patch.clear_sleep_enabled.unwrap_or(false) {
            conn.execute(
                "UPDATE accounts SET sleep_enabled = NULL, updated_at = ?1 WHERE id = ?2",
                params![now, id],
            )?;
        } else if let Some(v) = patch.sleep_enabled {
            conn.execute(
                "UPDATE accounts SET sleep_enabled = ?1, updated_at = ?2 WHERE id = ?3",
                params![if v { 1 } else { 0 }, now, id],
            )?;
        }
        if patch.clear_sleep_after.unwrap_or(false) {
            conn.execute(
                "UPDATE accounts SET sleep_after_minutes = NULL, updated_at = ?1 WHERE id = ?2",
                params![now, id],
            )?;
        } else if let Some(v) = patch.sleep_after_minutes {
            conn.execute(
                "UPDATE accounts SET sleep_after_minutes = ?1, updated_at = ?2 WHERE id = ?3",
                params![v as i64, now, id],
            )?;
        }
        if patch.clear_sleep_check.unwrap_or(false) {
            conn.execute(
                "UPDATE accounts SET sleep_check_minutes = NULL, updated_at = ?1 WHERE id = ?2",
                params![now, id],
            )?;
        } else if let Some(v) = patch.sleep_check_minutes {
            conn.execute(
                "UPDATE accounts SET sleep_check_minutes = ?1, updated_at = ?2 WHERE id = ?3",
                params![v as i64, now, id],
            )?;
        }
        if let Some(prefs) = &patch.auto_download {
            conn.execute(
                "UPDATE accounts SET auto_download = ?1, updated_at = ?2 WHERE id = ?3",
                params![auto_download_column(prefs), now, id],
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
        let account_id = self.conversation_account(id)?;
        let inbox = self.inbox(&account_id)?;
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
        if let Some(prefs) = &patch.auto_download {
            conn.execute(
                "UPDATE conversations SET auto_download = ?1 WHERE id = ?2",
                params![auto_download_column(prefs), id],
            )?;
        }
        drop(conn);
        self.get_conversation(id)
    }

    pub fn list_workspaces(&self) -> Result<Vec<Workspace>, DbError> {
        let conn = self.catalog.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT id, name, builtin, sort_order FROM workspaces ORDER BY sort_order, name",
        )?;
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
        let max: i64 = conn.query_row(
            "SELECT COALESCE(MAX(sort_order), 0) FROM workspaces",
            [],
            |r| r.get(0),
        )?;
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
                .query_row(
                    "SELECT builtin FROM workspaces WHERE id = ?1",
                    params![id],
                    |r| r.get(0),
                )
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

    pub fn create_priority_group(
        &self,
        name: &str,
        color: Option<&str>,
    ) -> Result<PriorityGroup, DbError> {
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
                if draft.inbound_only.unwrap_or(true) {
                    1
                } else {
                    0
                },
                if draft.include_self.unwrap_or(false) {
                    1
                } else {
                    0
                },
                draft.keyword,
                draft.prefix,
                draft.suffix,
                if draft.strip_sender.unwrap_or(false) {
                    1
                } else {
                    0
                },
                if draft.skip_if_forwarded.unwrap_or(true) {
                    1
                } else {
                    0
                },
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

    pub fn patch_forward_rule(
        &self,
        id: &str,
        patch: &ForwardRulePatch,
    ) -> Result<ForwardRule, DbError> {
        let conn = self.catalog.conn.lock();
        if let Some(enabled) = patch.enabled {
            conn.execute(
                "UPDATE forwarding_rules SET enabled = ?1 WHERE id = ?2",
                params![if enabled { 1 } else { 0 }, id],
            )?;
        }
        if patch.clear_source_account.unwrap_or(false) {
            conn.execute(
                "UPDATE forwarding_rules SET source_account_id = NULL WHERE id = ?1",
                params![id],
            )?;
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
            conn.execute(
                "UPDATE forwarding_rules SET keyword = NULL WHERE id = ?1",
                params![id],
            )?;
        } else if patch.keyword.is_some() {
            conn.execute(
                "UPDATE forwarding_rules SET keyword = ?1 WHERE id = ?2",
                params![patch.keyword, id],
            )?;
        }
        if patch.clear_prefix.unwrap_or(false) {
            conn.execute(
                "UPDATE forwarding_rules SET prefix = NULL WHERE id = ?1",
                params![id],
            )?;
        } else if patch.prefix.is_some() {
            conn.execute(
                "UPDATE forwarding_rules SET prefix = ?1 WHERE id = ?2",
                params![patch.prefix, id],
            )?;
        }
        if patch.clear_suffix.unwrap_or(false) {
            conn.execute(
                "UPDATE forwarding_rules SET suffix = NULL WHERE id = ?1",
                params![id],
            )?;
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

    pub fn list_scheduled_messages(
        &self,
        include_sent: bool,
    ) -> Result<Vec<ScheduledMessage>, DbError> {
        let conn = self.catalog.conn.lock();
        let sql = if include_sent {
            "SELECT id, source_account_id, source_conversation_id, source_message_id, dest_account_id,
                    dest_conversation_id, body, send_at, sent, created_at, attempts, last_error, failed, forward
             FROM scheduled_messages ORDER BY send_at"
        } else {
            "SELECT id, source_account_id, source_conversation_id, source_message_id, dest_account_id,
                    dest_conversation_id, body, send_at, sent, created_at, attempts, last_error, failed, forward
             FROM scheduled_messages WHERE sent = 0 ORDER BY send_at"
        };
        let mut stmt = conn.prepare(sql)?;
        let rows = stmt.query_map([], map_scheduled_message_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(DbError::from)
    }

    pub fn schedule_message(
        &self,
        draft: &ScheduleMessageDraft,
    ) -> Result<ScheduledMessage, DbError> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        self.catalog.conn.lock().execute(
            "INSERT INTO scheduled_messages (
                id, source_account_id, source_conversation_id, source_message_id,
                dest_account_id, dest_conversation_id, body, send_at, sent, created_at, forward
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 0, ?9, ?10)",
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
                draft.forward,
            ],
        )?;
        self.get_scheduled_message(&id)
    }

    pub fn get_scheduled_message(&self, id: &str) -> Result<ScheduledMessage, DbError> {
        let conn = self.catalog.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT id, source_account_id, source_conversation_id, source_message_id, dest_account_id,
                    dest_conversation_id, body, send_at, sent, created_at, attempts, last_error, failed, forward
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
                    dest_conversation_id, body, send_at, sent, created_at, attempts, last_error, failed, forward
             FROM scheduled_messages WHERE sent = 0 AND failed = 0 AND send_at <= ?1 ORDER BY send_at, created_at",
        )?;
        let rows = stmt.query_map(params![now], map_scheduled_message_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(DbError::from)
    }

    pub fn mark_scheduled_message_sent(&self, id: &str) -> Result<(), DbError> {
        self.catalog.conn.lock().execute(
            "UPDATE scheduled_messages SET sent = 1 WHERE id = ?1",
            params![id],
        )?;
        Ok(())
    }

    pub fn mark_scheduled_message_attempt(
        &self,
        id: &str,
        error: &str,
        give_up: bool,
    ) -> Result<(), DbError> {
        let conn = self.catalog.conn.lock();
        let attempts: i64 = conn.query_row(
            "SELECT attempts FROM scheduled_messages WHERE id = ?1",
            params![id],
            |r| r.get(0),
        )?;
        let attempts = attempts + 1;
        let failed = give_up || attempts >= 8;
        conn.execute(
            "UPDATE scheduled_messages SET attempts = ?1, last_error = ?2, failed = ?3 WHERE id = ?4",
            params![attempts, error, if failed { 1 } else { 0 }, id],
        )?;
        Ok(())
    }

    pub fn delete_scheduled_message(&self, id: &str) -> Result<(), DbError> {
        self.catalog
            .conn
            .lock()
            .execute("DELETE FROM scheduled_messages WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn update_scheduled_message(
        &self,
        id: &str,
        body: Option<&str>,
        send_at: Option<&str>,
    ) -> Result<ScheduledMessage, DbError> {
        let existing = self.get_scheduled_message(id)?;
        if existing.sent {
            return Err(DbError::NotFound(id.to_string()));
        }
        self.catalog.conn.lock().execute(
            "UPDATE scheduled_messages SET body = COALESCE(?1, body), send_at = COALESCE(?2, send_at) WHERE id = ?3 AND sent = 0",
            params![body, send_at, id],
        )?;
        self.get_scheduled_message(id)
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
    let accounts: Vec<(
        String,
        String,
        String,
        Option<String>,
        String,
        String,
        String,
        String,
    )> = {
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
        let path = data_dir
            .join("accounts")
            .join(&account_id)
            .join("inbox.sqlite");
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
            let _ = std::fs::rename(
                &src,
                data_dir.join(format!("database.sqlite.legacy{suffix}")),
            );
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

fn map_account_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Account> {
    Ok(Account {
        id: row.get(0)?,
        connector_id: row.get(1)?,
        name: row.get(2)?,
        identity: row.get(3)?,
        status: parse_account_status(&row.get::<_, String>(4)?),
        metadata: serde_json::from_str(&row.get::<_, String>(5)?).unwrap_or_default(),
        created_at: row
            .get::<_, String>(6)?
            .parse()
            .unwrap_or_else(|_| Utc::now()),
        updated_at: row
            .get::<_, String>(7)?
            .parse()
            .unwrap_or_else(|_| Utc::now()),
        disabled: row.get::<_, i64>(8)? != 0,
        muted: row.get::<_, i64>(9)? != 0,
        workspace_id: row.get(10)?,
        notify_enabled: opt_bool(row.get(11)?),
        send_receipts: row.get::<_, i64>(12)? != 0,
        sleep_enabled: opt_bool(row.get(13)?),
        sleep_after_minutes: opt_u32(row.get(14)?),
        sleep_check_minutes: opt_u32(row.get(15)?),
        auto_download: parse_auto_download(row.get(16)?),
    })
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
            .and_then(|s| parse_stored_datetime(&s)),
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
        auto_download: parse_auto_download(row.get(18)?),
        last_message_from_me: opt_bool(row.get(19)?),
        last_message_sender_id: row.get(20)?,
        last_message_sender_name: row.get(21)?,
    })
}

fn parse_auto_download(raw: Option<String>) -> AutoDownloadPrefs {
    raw.and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// Stored form: NULL when nothing is decided, else the JSON object.
fn auto_download_column(prefs: &AutoDownloadPrefs) -> Option<String> {
    (!prefs.is_empty()).then(|| serde_json::to_string(prefs).unwrap_or_default())
}

fn opt_bool(v: Option<i64>) -> Option<bool> {
    v.map(|n| n != 0)
}

fn opt_u32(v: Option<i64>) -> Option<u32> {
    v.and_then(|n| u32::try_from(n).ok())
}

pub fn parse_stored_datetime(value: &str) -> Option<DateTime<Utc>> {
    let s = value.trim();
    if s.is_empty() || s.starts_with("0001-") || s.starts_with("0000-") {
        return None;
    }
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Some(dt.with_timezone(&Utc));
    }
    let mut normalized = if s.contains('T') {
        s.to_string()
    } else {
        s.replacen(' ', "T", 1)
    };
    let has_offset = normalized.ends_with('Z')
        || normalized.contains('+')
        || normalized.rfind('-').map(|i| i > 10).unwrap_or(false);
    if !has_offset {
        normalized.push('Z');
    }
    if let Ok(dt) = DateTime::parse_from_rfc3339(&normalized) {
        return Some(dt.with_timezone(&Utc));
    }
    if let Ok(n) = s.parse::<i64>() {
        let secs = if n.abs() > 10_000_000_000 {
            n / 1000
        } else {
            n
        };
        return DateTime::from_timestamp(secs, 0);
    }
    None
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
        attempts: row.get(10)?,
        last_error: row.get(11)?,
        failed: row.get::<_, i64>(12)? != 0,
        forward: row.get::<_, i64>(13)? != 0,
    })
}

/// The columns every message query selects, in the order [`map_message_row`] reads them.
const PAGE_SELECT: &str = "SELECT id, conversation_id, remote_id, sender_id, sender_name, direction, body, timestamp, status, metadata, starred, pinned
             FROM messages WHERE conversation_id = ?1";

/// Which side of a page to look for more rows on.
#[derive(Clone, Copy)]
enum PageSide {
    Older,
    Newer,
}

fn rows_in(
    conn: &Connection,
    sql: &str,
    values: &[&dyn rusqlite::ToSql],
) -> Result<Vec<Message>, DbError> {
    let mut stmt = conn.prepare_cached(sql)?;
    let rows = stmt
        .query_map(values, map_message_row)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// The paging key of a stored message: its sub-second epoch and its id, the total order the
/// page queries sort on. Keyset paging on the id keeps equal timestamps from repeating a row.
fn page_key(
    conn: &Connection,
    conversation_id: &str,
    message_id: &str,
) -> Result<(f64, String), DbError> {
    conn.query_row(
        "SELECT unixepoch(timestamp, 'subsec'), id FROM messages WHERE conversation_id = ?1 AND id = ?2",
        params![conversation_id, message_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )
    .optional()?
    .ok_or_else(|| DbError::NotFound(message_id.to_string()))
}

/// Whether the chat has a row past `key` on `side`, which is what tells the reader that
/// scrolling that way loads more.
fn has_row_beyond(
    conn: &Connection,
    conversation_id: &str,
    key: &(f64, String),
    side: PageSide,
) -> Result<bool, DbError> {
    let comparison = match side {
        PageSide::Older => "< ?2 OR (unixepoch(timestamp, 'subsec') = ?2 AND id > ?3)",
        PageSide::Newer => "> ?2 OR (unixepoch(timestamp, 'subsec') = ?2 AND id < ?3)",
    };
    let sql = format!(
        "SELECT 1 FROM messages WHERE conversation_id = ?1
           AND (unixepoch(timestamp, 'subsec') {comparison}) LIMIT 1"
    );
    let found: Option<i64> = conn
        .prepare_cached(&sql)?
        .query_row(params![conversation_id, key.0, key.1], |row| row.get(0))
        .optional()?;
    Ok(found.is_some())
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
        timestamp: parse_stored_datetime(&row.get::<_, String>(7)?).unwrap_or_else(Utc::now),
        status: parse_message_status(&row.get::<_, String>(8)?),
        metadata: serde_json::from_str(&row.get::<_, String>(9)?).unwrap_or_default(),
        starred: row.get::<_, i64>(10).unwrap_or(0) != 0,
        pinned: row.get::<_, i64>(11).unwrap_or(0) != 0,
        reactions: Vec::new(),
        reply: None,
    })
}

fn get_message_locked(
    conn: &Connection,
    conversation_id: &str,
    message_id: &str,
) -> Result<Message, DbError> {
    let mut stmt = conn.prepare(
        "SELECT id, conversation_id, remote_id, sender_id, sender_name, direction, body, timestamp, status, metadata, starred, pinned
         FROM messages WHERE id = ?1 AND conversation_id = ?2",
    )?;
    let mut msg = stmt
        .query_row(params![message_id, conversation_id], map_message_row)
        .optional()?
        .ok_or_else(|| DbError::NotFound(message_id.to_string()))?;
    // Every message the UI receives carries its resolved reply quote, so an update never
    // makes a quote disappear.
    replies::resolve_in_conversation(conn, conversation_id, std::slice::from_mut(&mut msg))?;
    Ok(msg)
}

fn map_contact_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Contact> {
    Ok(Contact {
        id: row.get(0)?,
        account_id: row.get(1)?,
        remote_id: row.get(2)?,
        display_name: row.get(3)?,
        avatar_url: row.get(4)?,
        metadata: serde_json::from_str(&row.get::<_, String>(5)?).unwrap_or_default(),
    })
}

pub fn message_preview(body: &str, metadata: &serde_json::Value) -> String {
    if is_revoked(metadata) {
        return "This message was deleted".into();
    }
    let media_type = metadata
        .get("media_type")
        .and_then(|v| v.as_str())
        .map(|s| s.to_lowercase());
    let text = body.trim();
    if let Some(media) = media_type {
        let label = match media.as_str() {
            "image" | "photo" => "📷 Photo",
            "sticker" => "Sticker",
            "video" => "🎬 Video",
            "audio" | "ptt" => "🎵 Audio",
            "document" => "📎 Document",
            "contact" => "👤 Contact",
            "poll" => "📊 Poll",
            "event" => "📅 Event",
            "location" => "📍 Location",
            other => other,
        };
        let placeholder = format!("[{}]", media);
        if media == "poll" && !text.is_empty() && text.to_lowercase() != placeholder {
            // The question, not the answer: a vote never touches this preview.
            return format!("Poll: {text}");
        }
        if !text.is_empty() && text.to_lowercase() != placeholder {
            return text.to_string();
        }
        if media == "document" {
            if let Some(name) = metadata.get("filename").and_then(|v| v.as_str()) {
                if !name.trim().is_empty() {
                    return format!("📎 {}", name.trim());
                }
            }
        }
        return label.to_string();
    }
    text.to_string()
}

fn merge_json_objects(base: serde_json::Value, patch: serde_json::Value) -> serde_json::Value {
    match (base, patch) {
        (serde_json::Value::Object(mut a), serde_json::Value::Object(b)) => {
            for (k, v) in b {
                if v.is_null() {
                    // A successful download reports `media_error: null` to clear the old failure.
                    if k == "media_error" {
                        a.remove(&k);
                    }
                    continue;
                }
                if k == "media_data" {
                    if let Some(existing) = a.get("media_data").and_then(|x| x.as_str()) {
                        if !existing.is_empty() {
                            continue;
                        }
                    }
                }
                // A later report of the same reply (the echo of a send, a history pass) may
                // carry fewer facts than the one stored: keep what it does not mention.
                if k == "reply_to" {
                    if let (Some(serde_json::Value::Object(old)), serde_json::Value::Object(new)) =
                        (a.get("reply_to"), &v)
                    {
                        let mut merged = old.clone();
                        for (fk, fv) in new {
                            if !fv.is_null() {
                                merged.insert(fk.clone(), fv.clone());
                            }
                        }
                        a.insert(k, serde_json::Value::Object(merged));
                        continue;
                    }
                }
                a.insert(k, v);
            }
            serde_json::Value::Object(a)
        }
        (_, patch) => patch,
    }
}

fn merge_message_metadata_locked(
    conn: &Connection,
    conversation_id: &str,
    remote_id: &str,
    patch: &serde_json::Value,
) -> Result<(), DbError> {
    let existing: Option<String> = conn
        .query_row(
            "SELECT metadata FROM messages WHERE conversation_id = ?1 AND remote_id = ?2 LIMIT 1",
            params![conversation_id, remote_id],
            |row| row.get(0),
        )
        .optional()?;
    let Some(raw) = existing else {
        return Ok(());
    };
    let base: serde_json::Value =
        serde_json::from_str(&raw).unwrap_or_else(|_| serde_json::json!({}));
    let merged = merge_json_objects(base, patch.clone());
    conn.execute(
        "UPDATE messages SET metadata = ?1 WHERE conversation_id = ?2 AND remote_id = ?3",
        params![merged.to_string(), conversation_id, remote_id],
    )?;
    Ok(())
}

fn parse_account_status(s: &str) -> AccountStatus {
    match s {
        "connected" => AccountStatus::Connected,
        "connecting" => AccountStatus::Connecting,
        "error" => AccountStatus::Error,
        "awaiting_auth" => AccountStatus::AwaitingAuth,
        "sleeping" => AccountStatus::Sleeping,
        _ => AccountStatus::Disconnected,
    }
}

fn account_status_str(s: &AccountStatus) -> &'static str {
    match s {
        AccountStatus::Connected => "connected",
        AccountStatus::Connecting => "connecting",
        AccountStatus::Error => "error",
        AccountStatus::AwaitingAuth => "awaiting_auth",
        AccountStatus::Sleeping => "sleeping",
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

fn jid_user_part(jid: &str) -> &str {
    jid.split('@').next().unwrap_or(jid)
}

fn is_placeholder_title(name: &str, remote_id: &str) -> bool {
    let n = name.trim();
    if n.is_empty() {
        return true;
    }
    if n.contains('∙') || n.contains('•') || n.contains('·') {
        return true;
    }
    let user = jid_user_part(remote_id);
    if n == user || n == remote_id {
        return true;
    }
    let letters = n.chars().any(|c| c.is_alphabetic());
    let digits = n.chars().any(|c| c.is_ascii_digit());
    digits && !letters
}

fn pick_conversation_title(
    existing: &str,
    incoming: &str,
    remote_id: &str,
    replace_title: bool,
) -> String {
    let ex_bad = is_placeholder_title(existing, remote_id);
    let in_bad = is_placeholder_title(incoming, remote_id);
    // History/contact refresh may replace a leaked business self-name with the
    // real contact title. Live websocket events should not clobber a good title.
    if replace_title && !in_bad {
        return incoming.to_string();
    }
    if !ex_bad {
        return existing.to_string();
    }
    if !in_bad {
        return incoming.to_string();
    }
    incoming.to_string()
}
