use rusqlite::Connection;

pub fn migrate_catalog(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS connectors (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            version TEXT NOT NULL,
            metadata TEXT NOT NULL DEFAULT '{}'
        );

        CREATE TABLE IF NOT EXISTS accounts (
            id TEXT PRIMARY KEY,
            connector_id TEXT NOT NULL REFERENCES connectors(id),
            name TEXT NOT NULL,
            identity TEXT,
            status TEXT NOT NULL DEFAULT 'disconnected',
            metadata TEXT NOT NULL DEFAULT '{}',
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS workspaces (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            builtin INTEGER NOT NULL DEFAULT 0,
            sort_order INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS priority_groups (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            color TEXT,
            builtin INTEGER NOT NULL DEFAULT 0,
            sort_order INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS reminders (
            id TEXT PRIMARY KEY,
            conversation_id TEXT NOT NULL,
            account_id TEXT NOT NULL,
            fire_at TEXT NOT NULL,
            kind TEXT NOT NULL DEFAULT 'nudge',
            note TEXT,
            fired INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS chat_todos (
            id TEXT PRIMARY KEY,
            conversation_id TEXT NOT NULL,
            account_id TEXT NOT NULL,
            body TEXT NOT NULL,
            due_at TEXT,
            done INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS forwarding_rules (
            id TEXT PRIMARY KEY,
            enabled INTEGER NOT NULL DEFAULT 1,
            source_account_id TEXT,
            source_conversation_id TEXT,
            source_workspace_id TEXT,
            dest_account_id TEXT NOT NULL,
            dest_conversation_id TEXT NOT NULL,
            inbound_only INTEGER NOT NULL DEFAULT 1,
            include_self INTEGER NOT NULL DEFAULT 0,
            keyword TEXT,
            prefix TEXT,
            suffix TEXT,
            strip_sender INTEGER NOT NULL DEFAULT 0,
            skip_if_forwarded INTEGER NOT NULL DEFAULT 1,
            delay_seconds INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS scheduled_messages (
            id TEXT PRIMARY KEY,
            source_account_id TEXT,
            source_conversation_id TEXT,
            source_message_id TEXT,
            dest_account_id TEXT NOT NULL,
            dest_conversation_id TEXT NOT NULL,
            body TEXT NOT NULL,
            send_at TEXT NOT NULL,
            sent INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL
        );

        INSERT OR IGNORE INTO connectors (id, name, version) VALUES
            ('whatsapp', 'WhatsApp', '1.0.0'),
            ('telegram', 'Telegram', '1.0.0'),
            ('signal', 'Signal', '1.0.0'),
            ('messenger', 'Messenger', '1.0.0'),
            ('instagram', 'Instagram', '1.0.0'),
            ('email', 'Email', '1.0.0'),
            ('matrix', 'Matrix', '1.0.0');

        INSERT OR IGNORE INTO workspaces (id, name, builtin, sort_order) VALUES
            ('default', 'Default', 1, 0);

        CREATE TABLE IF NOT EXISTS app_meta (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS app_settings (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );
        ",
    )?;
    add_column(conn, "accounts", "disabled INTEGER NOT NULL DEFAULT 0")?;
    add_column(conn, "accounts", "muted INTEGER NOT NULL DEFAULT 0")?;
    add_column(conn, "accounts", "workspace_id TEXT")?;
    add_column(conn, "accounts", "notify_enabled INTEGER")?;
    add_column(conn, "accounts", "send_receipts INTEGER NOT NULL DEFAULT 0")?;
    add_column(conn, "accounts", "sleep_enabled INTEGER")?;
    add_column(conn, "accounts", "sleep_after_minutes INTEGER")?;
    add_column(conn, "accounts", "sleep_check_minutes INTEGER")?;
    // JSON `{"images":bool,"videos":bool,"files":bool}`; a missing key = built-in default.
    add_column(conn, "accounts", "auto_download TEXT")?;
    add_column(
        conn,
        "scheduled_messages",
        "attempts INTEGER NOT NULL DEFAULT 0",
    )?;
    add_column(conn, "scheduled_messages", "last_error TEXT")?;
    add_column(
        conn,
        "scheduled_messages",
        "failed INTEGER NOT NULL DEFAULT 0",
    )?;
    add_column(
        conn,
        "scheduled_messages",
        "forward INTEGER NOT NULL DEFAULT 0",
    )?;
    add_column(conn, "chat_todos", "message_id TEXT")?;
    add_column(conn, "chat_todos", "message_snippet TEXT")?;
    migrate_org_defaults(conn)?;
    super::notes::migrate_catalog(conn)?;
    Ok(())
}

fn migrate_org_defaults(conn: &Connection) -> rusqlite::Result<()> {
    let done: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM app_meta WHERE key = 'org_defaults_v2'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    if done > 0 {
        return Ok(());
    }
    conn.execute(
        "INSERT OR IGNORE INTO workspaces (id, name, builtin, sort_order) VALUES ('default', 'Default', 1, 0)",
        [],
    )?;
    conn.execute(
        "UPDATE accounts SET workspace_id = 'default' WHERE workspace_id IN ('personal', 'work', 'others')",
        [],
    )?;
    conn.execute(
        "DELETE FROM workspaces WHERE id IN ('personal', 'work', 'others')",
        [],
    )?;
    conn.execute("DELETE FROM priority_groups WHERE builtin = 1", [])?;
    conn.execute(
        "INSERT OR IGNORE INTO app_meta (key, value) VALUES ('org_defaults_v2', '1')",
        [],
    )?;
    Ok(())
}

pub fn migrate_inbox(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS contacts (
            id TEXT PRIMARY KEY,
            account_id TEXT NOT NULL,
            remote_id TEXT NOT NULL,
            display_name TEXT NOT NULL,
            avatar_url TEXT,
            metadata TEXT NOT NULL DEFAULT '{}',
            UNIQUE(account_id, remote_id)
        );

        CREATE TABLE IF NOT EXISTS conversations (
            id TEXT PRIMARY KEY,
            account_id TEXT NOT NULL,
            remote_id TEXT NOT NULL,
            contact_id TEXT,
            title TEXT NOT NULL,
            conversation_type TEXT NOT NULL DEFAULT 'direct',
            unread_count INTEGER NOT NULL DEFAULT 0,
            last_message_at TEXT,
            last_message_preview TEXT,
            pinned INTEGER NOT NULL DEFAULT 0,
            archived INTEGER NOT NULL DEFAULT 0,
            muted INTEGER NOT NULL DEFAULT 0,
            metadata TEXT NOT NULL DEFAULT '{}',
            updated_at TEXT,
            UNIQUE(account_id, remote_id)
        );

        CREATE TABLE IF NOT EXISTS messages (
            id TEXT PRIMARY KEY,
            conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
            remote_id TEXT,
            sender_id TEXT,
            sender_name TEXT,
            direction TEXT NOT NULL,
            body TEXT NOT NULL,
            timestamp TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'pending',
            metadata TEXT NOT NULL DEFAULT '{}'
        );

        CREATE INDEX IF NOT EXISTS idx_messages_conversation ON messages(conversation_id, timestamp);
        -- The chat rules order and compare by unixepoch(timestamp, 'subsec'). Without this index
        -- every such lookup sorted the whole chat (128 ms at 200k messages, 0.02 ms with it).
        CREATE INDEX IF NOT EXISTS idx_messages_conv_epoch ON messages(conversation_id, unixepoch(timestamp, 'subsec') DESC, id);
        CREATE INDEX IF NOT EXISTS idx_conversations_account ON conversations(account_id, last_message_at DESC);
        CREATE INDEX IF NOT EXISTS idx_conversations_unread ON conversations(unread_count) WHERE unread_count > 0;
        CREATE UNIQUE INDEX IF NOT EXISTS idx_messages_remote ON messages(conversation_id, remote_id) WHERE remote_id IS NOT NULL;

        -- One row per (message, reactor): a new reaction replaces the old one, an empty emoji
        -- deletes it. Keyed by the target's REMOTE id (not messages.id) so a reaction that
        -- arrives before its message still attaches when the message shows up. The inbox file
        -- is per account, so the conversation id also fixes the account.
        CREATE TABLE IF NOT EXISTS message_reactions (
            conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
            target_remote_id TEXT NOT NULL,
            reactor_id TEXT NOT NULL,
            reactor_name TEXT,
            emoji TEXT NOT NULL,
            from_me INTEGER NOT NULL DEFAULT 0,
            reacted_at TEXT NOT NULL,
            PRIMARY KEY (conversation_id, target_remote_id, reactor_id)
        ) WITHOUT ROWID;

        -- Messages deleted on this account only ('delete for me'): a later sync of the same
        -- remote id is ignored instead of bringing the message back.
        CREATE TABLE IF NOT EXISTS deleted_messages (
            conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
            remote_id TEXT NOT NULL,
            PRIMARY KEY (conversation_id, remote_id)
        ) WITHOUT ROWID;
        ",
    )?;
    add_column(conn, "conversations", "workspace_id TEXT")?;
    add_column(conn, "conversations", "priority_group TEXT")?;
    add_column(conn, "conversations", "notes TEXT NOT NULL DEFAULT ''")?;
    add_column(conn, "conversations", "notify_enabled INTEGER")?;
    add_column(conn, "conversations", "send_receipts INTEGER")?;
    // Per-chat auto-download overrides, same JSON shape; a missing key = inherit the account.
    add_column(conn, "conversations", "auto_download TEXT")?;
    add_column(conn, "messages", "starred INTEGER NOT NULL DEFAULT 0")?;
    add_column(conn, "messages", "pinned INTEGER NOT NULL DEFAULT 0")?;
    add_column(conn, "conversations", "read_through TEXT")?;
    // Who wrote the message the stored preview shows. Written only by `db/chat.rs`, together
    // with the preview. NULL = unknown (the UI shows no direction arrow).
    add_column(conn, "conversations", "last_message_from_me INTEGER")?;
    add_column(conn, "conversations", "last_message_sender_id TEXT")?;
    add_column(conn, "conversations", "last_message_sender_name TEXT")?;
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS inbox_meta (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );
        ",
    )?;
    migrate_jid_body_cleanup(conn)?;
    super::chat::repair_placeholder_bodies(conn)?;
    migrate_read_markers(conn)?;
    migrate_preview_source(conn)?;
    Ok(())
}

/// Upgrade: fill the preview's direction and author for chats stored before those columns
/// existed, from each chat's newest stored message. Chats without stored messages stay
/// unknown. Idempotent: guarded by the `preview_source_v1` key.
fn migrate_preview_source(conn: &Connection) -> rusqlite::Result<()> {
    let done: i64 = conn.query_row(
        "SELECT COUNT(*) FROM inbox_meta WHERE key = 'preview_source_v1'",
        [],
        |r| r.get(0),
    )?;
    if done > 0 {
        return Ok(());
    }
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "UPDATE conversations SET
            last_message_from_me = (
                SELECT m.direction = 'outbound' FROM messages m WHERE m.conversation_id = conversations.id
                ORDER BY unixepoch(m.timestamp, 'subsec') DESC, m.id LIMIT 1),
            last_message_sender_id = (
                SELECT CASE WHEN m.direction = 'inbound' THEN m.sender_id END FROM messages m
                WHERE m.conversation_id = conversations.id
                ORDER BY unixepoch(m.timestamp, 'subsec') DESC, m.id LIMIT 1),
            last_message_sender_name = (
                SELECT CASE WHEN m.direction = 'inbound' THEN m.sender_name END FROM messages m
                WHERE m.conversation_id = conversations.id
                ORDER BY unixepoch(m.timestamp, 'subsec') DESC, m.id LIMIT 1)
         WHERE last_message_preview IS NOT NULL AND last_message_from_me IS NULL",
        [],
    )?;
    tx.execute(
        "INSERT OR IGNORE INTO inbox_meta (key, value) VALUES ('preview_source_v1', ?1)",
        [chrono::Utc::now().to_rfc3339()],
    )?;
    tx.commit()
}

/// R4 upgrade: replace the unread counter by a read marker, keeping current badges.
///
/// For every chat the marker is chosen so exactly `unread_count` inbound messages are newer
/// than it (or it is the newest message time when the count is 0). The per-inbox baseline is
/// the migration time. Idempotent: guarded by the `read_marker_v1` key.
fn migrate_read_markers(conn: &Connection) -> rusqlite::Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    // Baseline is set once, whatever else happens.
    conn.execute(
        "INSERT OR IGNORE INTO inbox_meta (key, value) VALUES ('unread_baseline', ?1)",
        [&now],
    )?;
    let done: i64 = conn.query_row(
        "SELECT COUNT(*) FROM inbox_meta WHERE key = 'read_marker_v1'",
        [],
        |r| r.get(0),
    )?;
    if done > 0 {
        return Ok(());
    }
    let tx = conn.unchecked_transaction()?;
    // Junk chat times would break ordering; treat them as missing.
    tx.execute(
        "UPDATE conversations SET last_message_at = NULL
         WHERE last_message_at = '' OR last_message_at LIKE '0001-%' OR last_message_at LIKE '0000-%'",
        [],
    )?;
    let rows: Vec<(String, i64)> = {
        let mut stmt =
            tx.prepare("SELECT id, unread_count FROM conversations WHERE read_through IS NULL")?;
        let mapped = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
        mapped.collect::<Result<Vec<_>, _>>()?
    };
    for (id, unread) in rows {
        if unread <= 0 {
            let newest: Option<String> = tx
                .query_row(
                    "SELECT timestamp FROM messages WHERE conversation_id = ?1
                     ORDER BY unixepoch(timestamp, 'subsec') DESC LIMIT 1",
                    [&id],
                    |r| r.get(0),
                )
                .ok();
            if let Some(ts) = newest {
                tx.execute(
                    "UPDATE conversations SET read_through = ?1 WHERE id = ?2",
                    rusqlite::params![ts, id],
                )?;
            }
            continue;
        }
        let times: Vec<String> = {
            let mut stmt = tx.prepare(
                "SELECT timestamp FROM messages WHERE conversation_id = ?1 AND direction = 'inbound'
                 ORDER BY unixepoch(timestamp, 'subsec') DESC LIMIT ?2",
            )?;
            let mapped =
                stmt.query_map(rusqlite::params![id, unread + 1], |r| r.get::<_, String>(0))?;
            mapped.collect::<Result<Vec<_>, _>>()?
        };
        // `unread_keep`: until the chat is read, its own marker wins over the inbox baseline
        // (which is the migration time), so the badge survives the next recompute.
        if times.len() as i64 > unread {
            // The (unread+1)-th newest inbound message is the newest one that is already read.
            tx.execute(
                "UPDATE conversations SET read_through = ?1,
                    metadata = json_set(metadata, '$.unread_keep', json('true')) WHERE id = ?2",
                rusqlite::params![times[unread as usize], id],
            )?;
        } else if times.is_empty() {
            // Badge without stored messages: keep a badge through the manual-unread flag.
            tx.execute(
                "UPDATE conversations SET metadata = json_set(metadata, '$.marked_unread', json('true')) WHERE id = ?1",
                [&id],
            )?;
        } else {
            // Fewer inbound messages than the badge: no marker, every stored message counts.
            tx.execute(
                "UPDATE conversations SET metadata = json_set(metadata, '$.unread_keep', json('true')) WHERE id = ?1",
                [&id],
            )?;
        }
    }
    tx.execute(
        "INSERT OR IGNORE INTO inbox_meta (key, value) VALUES ('read_marker_v1', ?1)",
        [&now],
    )?;
    tx.commit()
}

fn migrate_jid_body_cleanup(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE messages SET body = ''
         WHERE length(body) < 40
           AND (body LIKE '%@lid' OR body LIKE '%@s.whatsapp.net')
           AND (metadata IS NULL OR metadata = '{}' OR json_extract(metadata, '$.media_type') IS NULL)",
        [],
    )?;
    Ok(())
}

fn add_column(conn: &Connection, table: &str, def: &str) -> rusqlite::Result<()> {
    let sql = format!("ALTER TABLE {table} ADD COLUMN {def}");
    match conn.execute(&sql, []) {
        Ok(_) => Ok(()),
        Err(e) if e.to_string().contains("duplicate column") => Ok(()),
        Err(e) => Err(e),
    }
}
