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
            ('personal', 'Personal', 1, 0),
            ('work', 'Work', 1, 1),
            ('others', 'Others', 1, 2);

        INSERT OR IGNORE INTO priority_groups (id, name, color, builtin, sort_order) VALUES
            ('urgent', 'Urgent', '#ef4444', 1, 0),
            ('waiting', 'Waiting', '#f59e0b', 1, 1),
            ('later', 'Later', '#64748b', 1, 2);

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
        CREATE INDEX IF NOT EXISTS idx_conversations_account ON conversations(account_id, last_message_at DESC);
        CREATE INDEX IF NOT EXISTS idx_conversations_unread ON conversations(unread_count) WHERE unread_count > 0;
        CREATE UNIQUE INDEX IF NOT EXISTS idx_messages_remote ON messages(conversation_id, remote_id) WHERE remote_id IS NOT NULL;
        ",
    )?;
    add_column(conn, "conversations", "workspace_id TEXT")?;
    add_column(conn, "conversations", "priority_group TEXT")?;
    add_column(conn, "conversations", "notes TEXT NOT NULL DEFAULT ''")?;
    add_column(conn, "conversations", "notify_enabled INTEGER")?;
    add_column(conn, "conversations", "send_receipts INTEGER")?;
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
