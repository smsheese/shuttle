//! Per-chat todos and note items.
//!
//! A note item is free text for one chat, optionally quoting a message, optionally carrying
//! ONE schedule: a countdown timer or an absolute alarm. Both resolve to a single `fire_at`
//! instant, so the firing loop (`due_notes` → notify → `mark_note_fired`) is one code path.
//!
//! This replaces the single `conversations.notes` text and the separate `reminders` table.
//! `migrate_catalog` and `Database::migrate_legacy_notes` carry the old data over; the old
//! `reminders` table and `conversations.notes` column are left in place as a backup.

use super::*;
use chrono::{Duration, SecondsFormat};

pub const KIND_TIMER: &str = "timer";
pub const KIND_ALARM: &str = "alarm";

/// Longest accepted countdown (a year); anything above is almost certainly a typo.
const MAX_TIMER_SECS: i64 = 366 * 24 * 3600;
/// An alarm picked "now" at minute precision is a few seconds in the past by the time it
/// arrives; accept that much slack and let it fire on the next tick.
const ALARM_PAST_GRACE_SECS: i64 = 90;
const SNIPPET_MAX_CHARS: usize = 200;

const NOTE_COLS: &str = "id, conversation_id, account_id, body, message_id, message_snippet,
                         fire_at, kind, fired, created_at";
const TODO_COLS: &str =
    "id, conversation_id, account_id, body, due_at, done, created_at, message_id, message_snippet";

/// Store instants in one fixed UTC format so SQL string comparison orders them correctly.
fn fmt_instant(t: DateTime<Utc>) -> String {
    t.to_rfc3339_opts(SecondsFormat::Millis, true)
}

/// Turn a schedule request into `(kind, fire_at)`, validated against `now`.
pub(super) fn resolve_schedule(
    schedule: &NoteSchedule,
    now: DateTime<Utc>,
) -> Result<(&'static str, String), DbError> {
    match (schedule.timer_secs, schedule.alarm_at.as_deref()) {
        (Some(_), Some(_)) => Err(DbError::InvalidInput(
            "a note takes a timer or an alarm, not both".into(),
        )),
        (Some(secs), None) => {
            if !(1..=MAX_TIMER_SECS).contains(&secs) {
                return Err(DbError::InvalidInput("timer length out of range".into()));
            }
            Ok((KIND_TIMER, fmt_instant(now + Duration::seconds(secs))))
        }
        (None, Some(at)) => {
            let at = DateTime::parse_from_rfc3339(at)
                .map_err(|_| DbError::InvalidInput("alarm time is not a valid date-time".into()))?
                .with_timezone(&Utc);
            if at < now - Duration::seconds(ALARM_PAST_GRACE_SECS) {
                return Err(DbError::InvalidInput("alarm time is in the past".into()));
            }
            Ok((KIND_ALARM, fmt_instant(at)))
        }
        (None, None) => Err(DbError::InvalidInput("no timer or alarm given".into())),
    }
}

fn clip_snippet(s: &str) -> String {
    s.chars().take(SNIPPET_MAX_CHARS).collect()
}

fn clean_opt(s: Option<&str>) -> Option<String> {
    s.map(str::trim).filter(|s| !s.is_empty()).map(str::to_string)
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
        message_id: row.get(7)?,
        message_snippet: row.get(8)?,
    })
}

fn map_note_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ChatNote> {
    Ok(ChatNote {
        id: row.get(0)?,
        conversation_id: row.get(1)?,
        account_id: row.get(2)?,
        body: row.get(3)?,
        message_id: row.get(4)?,
        message_snippet: row.get(5)?,
        fire_at: row.get(6)?,
        kind: row.get(7)?,
        fired: row.get::<_, i64>(8)? != 0,
        created_at: row.get(9)?,
    })
}

/// Catalog tables for note items, plus the one-time reminders → alarm notes copy.
pub(super) fn migrate_catalog(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS chat_notes (
            id TEXT PRIMARY KEY,
            conversation_id TEXT NOT NULL,
            account_id TEXT NOT NULL,
            body TEXT NOT NULL DEFAULT '',
            message_id TEXT,
            message_snippet TEXT,
            fire_at TEXT,
            kind TEXT,
            fired INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_chat_notes_conversation
            ON chat_notes(conversation_id, created_at);
        CREATE INDEX IF NOT EXISTS idx_chat_notes_due
            ON chat_notes(fire_at) WHERE fire_at IS NOT NULL AND fired = 0;
        ",
    )?;
    migrate_reminders(conn)
}

/// Copy every unfired reminder into a note with an alarm. Fired ones already did their job.
/// Guarded by an `app_meta` key; the `reminders` table itself is kept untouched.
fn migrate_reminders(conn: &Connection) -> rusqlite::Result<()> {
    let done: i64 = conn.query_row(
        "SELECT COUNT(*) FROM app_meta WHERE key = 'chat_notes_from_reminders_v1'",
        [],
        |r| r.get(0),
    )?;
    if done > 0 {
        return Ok(());
    }
    let tx = conn.unchecked_transaction()?;
    let rows: Vec<(String, String, String, String, Option<String>, String)> = {
        let mut stmt = tx.prepare(
            "SELECT id, conversation_id, account_id, fire_at, note, created_at
             FROM reminders WHERE fired = 0",
        )?;
        let mapped = stmt.query_map([], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?))
        })?;
        mapped.collect::<Result<Vec<_>, _>>()?
    };
    for (id, conversation_id, account_id, fire_at, note, created_at) in rows {
        let fire_at = DateTime::parse_from_rfc3339(&fire_at)
            .map(|t| fmt_instant(t.with_timezone(&Utc)))
            .unwrap_or(fire_at);
        let body = clean_opt(note.as_deref()).unwrap_or_else(|| "Reminder".to_string());
        tx.execute(
            "INSERT OR IGNORE INTO chat_notes
                (id, conversation_id, account_id, body, fire_at, kind, fired, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 'alarm', 0, ?6)",
            params![id, conversation_id, account_id, body, fire_at, created_at],
        )?;
    }
    tx.execute(
        "INSERT OR IGNORE INTO app_meta (key, value) VALUES ('chat_notes_from_reminders_v1', ?1)",
        params![Utc::now().to_rfc3339()],
    )?;
    tx.commit()
}

impl Database {
    /// Turn each chat's legacy `conversations.notes` text into one note item. Idempotent:
    /// guarded per inbox by `notes_items_v1`, and the item id is derived from the chat id.
    /// The old column is left as it was.
    pub(super) fn migrate_legacy_notes(&self) -> Result<(), DbError> {
        for account in self.list_accounts()? {
            let inbox = self.inbox(&account.id)?;
            let rows: Vec<(String, String)> = {
                let conn = inbox.conn.lock();
                let done: i64 = conn.query_row(
                    "SELECT COUNT(*) FROM inbox_meta WHERE key = 'notes_items_v1'",
                    [],
                    |r| r.get(0),
                )?;
                if done > 0 {
                    continue;
                }
                let mut stmt = conn.prepare(
                    "SELECT id, notes FROM conversations WHERE trim(notes) != ''",
                )?;
                let mapped = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
                mapped.collect::<Result<Vec<_>, _>>()?
            };
            let now = Utc::now().to_rfc3339();
            {
                let conn = self.catalog.conn.lock();
                for (conversation_id, notes) in &rows {
                    conn.execute(
                        "INSERT OR IGNORE INTO chat_notes
                            (id, conversation_id, account_id, body, fired, created_at)
                         VALUES (?1, ?2, ?3, ?4, 0, ?5)",
                        params![
                            format!("legacy-notes-{conversation_id}"),
                            conversation_id,
                            account.id,
                            notes.trim(),
                            now
                        ],
                    )?;
                }
            }
            inbox.conn.lock().execute(
                "INSERT OR IGNORE INTO inbox_meta (key, value) VALUES ('notes_items_v1', ?1)",
                params![now],
            )?;
        }
        Ok(())
    }

    // ----- todos -----

    pub fn list_todos(&self, conversation_id: &str) -> Result<Vec<ChatTodo>, DbError> {
        let conn = self.catalog.conn.lock();
        let mut stmt = conn.prepare(&format!(
            "SELECT {TODO_COLS} FROM chat_todos WHERE conversation_id = ?1 ORDER BY done, created_at"
        ))?;
        let rows = stmt.query_map(params![conversation_id], map_todo_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(DbError::from)
    }

    #[cfg(test)]
    pub fn add_todo(
        &self,
        conversation_id: &str,
        account_id: &str,
        body: &str,
        due_at: Option<&str>,
    ) -> Result<ChatTodo, DbError> {
        self.add_todo_linked(conversation_id, account_id, body, due_at, None, None)
    }

    /// Add a todo, optionally quoting the message it came from.
    pub fn add_todo_linked(
        &self,
        conversation_id: &str,
        account_id: &str,
        body: &str,
        due_at: Option<&str>,
        message_id: Option<&str>,
        message_snippet: Option<&str>,
    ) -> Result<ChatTodo, DbError> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        let message_id = clean_opt(message_id);
        let snippet = message_id
            .as_ref()
            .and(clean_opt(message_snippet))
            .map(|s| clip_snippet(&s));
        self.catalog.conn.lock().execute(
            "INSERT INTO chat_todos
                (id, conversation_id, account_id, body, due_at, done, created_at, message_id, message_snippet)
             VALUES (?1, ?2, ?3, ?4, ?5, 0, ?6, ?7, ?8)",
            params![id, conversation_id, account_id, body, due_at, now, message_id, snippet],
        )?;
        Ok(ChatTodo {
            id,
            conversation_id: conversation_id.to_string(),
            account_id: account_id.to_string(),
            body: body.to_string(),
            due_at: due_at.map(str::to_string),
            done: false,
            created_at: now,
            message_id,
            message_snippet: snippet,
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

    // ----- note items -----

    pub fn list_notes(&self, conversation_id: &str) -> Result<Vec<ChatNote>, DbError> {
        let conn = self.catalog.conn.lock();
        let mut stmt = conn.prepare(&format!(
            "SELECT {NOTE_COLS} FROM chat_notes WHERE conversation_id = ?1 ORDER BY created_at"
        ))?;
        let rows = stmt.query_map(params![conversation_id], map_note_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(DbError::from)
    }

    fn get_note(&self, id: &str) -> Result<ChatNote, DbError> {
        let conn = self.catalog.conn.lock();
        conn.query_row(
            &format!("SELECT {NOTE_COLS} FROM chat_notes WHERE id = ?1"),
            params![id],
            map_note_row,
        )
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => DbError::NotFound(id.to_string()),
            other => DbError::from(other),
        })
    }

    pub fn create_note(
        &self,
        conversation_id: &str,
        account_id: &str,
        body: &str,
        message_id: Option<&str>,
        message_snippet: Option<&str>,
        schedule: Option<&NoteSchedule>,
    ) -> Result<ChatNote, DbError> {
        self.create_note_at(
            conversation_id,
            account_id,
            body,
            message_id,
            message_snippet,
            schedule,
            Utc::now(),
        )
    }

    /// `create_note` with an explicit clock, so timer/alarm scheduling is testable.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn create_note_at(
        &self,
        conversation_id: &str,
        account_id: &str,
        body: &str,
        message_id: Option<&str>,
        message_snippet: Option<&str>,
        schedule: Option<&NoteSchedule>,
        now: DateTime<Utc>,
    ) -> Result<ChatNote, DbError> {
        let body = body.trim();
        let message_id = clean_opt(message_id);
        if body.is_empty() && message_id.is_none() {
            return Err(DbError::InvalidInput("note is empty".into()));
        }
        let snippet = message_id
            .as_ref()
            .and(clean_opt(message_snippet))
            .map(|s| clip_snippet(&s));
        let (kind, fire_at) = match schedule {
            Some(s) => {
                let (k, f) = resolve_schedule(s, now)?;
                (Some(k.to_string()), Some(f))
            }
            None => (None, None),
        };
        let id = Uuid::new_v4().to_string();
        let created_at = now.to_rfc3339();
        self.catalog.conn.lock().execute(
            "INSERT INTO chat_notes
                (id, conversation_id, account_id, body, message_id, message_snippet, fire_at, kind, fired, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 0, ?9)",
            params![id, conversation_id, account_id, body, message_id, snippet, fire_at, kind, created_at],
        )?;
        self.get_note(&id)
    }

    /// Edit a note's text and/or schedule. `schedule` replaces any existing timer/alarm (and
    /// re-arms it); `clear_schedule` removes it. Both at once is rejected.
    pub fn update_note(
        &self,
        id: &str,
        body: Option<&str>,
        schedule: Option<&NoteSchedule>,
        clear_schedule: bool,
    ) -> Result<ChatNote, DbError> {
        self.update_note_at(id, body, schedule, clear_schedule, Utc::now())
    }

    pub(super) fn update_note_at(
        &self,
        id: &str,
        body: Option<&str>,
        schedule: Option<&NoteSchedule>,
        clear_schedule: bool,
        now: DateTime<Utc>,
    ) -> Result<ChatNote, DbError> {
        if schedule.is_some() && clear_schedule {
            return Err(DbError::InvalidInput(
                "cannot set and clear the schedule together".into(),
            ));
        }
        let current = self.get_note(id)?;
        if let Some(body) = body {
            let body = body.trim();
            if body.is_empty() && current.message_id.is_none() {
                return Err(DbError::InvalidInput("note is empty".into()));
            }
            self.catalog.conn.lock().execute(
                "UPDATE chat_notes SET body = ?1 WHERE id = ?2",
                params![body, id],
            )?;
        }
        if let Some(s) = schedule {
            let (kind, fire_at) = resolve_schedule(s, now)?;
            self.catalog.conn.lock().execute(
                "UPDATE chat_notes SET kind = ?1, fire_at = ?2, fired = 0 WHERE id = ?3",
                params![kind, fire_at, id],
            )?;
        } else if clear_schedule {
            self.catalog.conn.lock().execute(
                "UPDATE chat_notes SET kind = NULL, fire_at = NULL, fired = 0 WHERE id = ?1",
                params![id],
            )?;
        }
        self.get_note(id)
    }

    pub fn delete_note(&self, id: &str) -> Result<(), DbError> {
        self.catalog
            .conn
            .lock()
            .execute("DELETE FROM chat_notes WHERE id = ?1", params![id])?;
        Ok(())
    }

    /// Notes whose timer/alarm has come due and not fired yet, oldest first.
    pub fn due_notes(&self) -> Result<Vec<ChatNote>, DbError> {
        self.due_notes_at(Utc::now())
    }

    pub(super) fn due_notes_at(&self, now: DateTime<Utc>) -> Result<Vec<ChatNote>, DbError> {
        let conn = self.catalog.conn.lock();
        let mut stmt = conn.prepare(&format!(
            "SELECT {NOTE_COLS} FROM chat_notes
             WHERE fired = 0 AND fire_at IS NOT NULL AND fire_at <= ?1 ORDER BY fire_at"
        ))?;
        let rows = stmt.query_map(params![fmt_instant(now)], map_note_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(DbError::from)
    }

    pub fn mark_note_fired(&self, id: &str) -> Result<(), DbError> {
        self.catalog
            .conn
            .lock()
            .execute("UPDATE chat_notes SET fired = 1 WHERE id = ?1", params![id])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fx {
        _dir: tempfile::TempDir,
        db: Database,
        account: String,
    }

    fn fx() -> Fx {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::open(dir.path()).unwrap();
        let account = db.create_account("whatsapp", "WA").unwrap().id;
        Fx {
            _dir: dir,
            db,
            account,
        }
    }

    fn timer(secs: i64) -> NoteSchedule {
        NoteSchedule {
            timer_secs: Some(secs),
            alarm_at: None,
        }
    }

    fn alarm(at: DateTime<Utc>) -> NoteSchedule {
        NoteSchedule {
            timer_secs: None,
            alarm_at: Some(at.to_rfc3339()),
        }
    }

    #[test]
    fn timer_fires_after_its_duration_and_only_once() {
        let f = fx();
        let t0 = Utc::now();
        let n = f
            .db
            .create_note_at("c1", &f.account, "tea", None, None, Some(&timer(600)), t0)
            .unwrap();
        assert_eq!(n.kind.as_deref(), Some("timer"));
        assert!(!n.fired);
        assert!(f.db.due_notes_at(t0 + Duration::seconds(599)).unwrap().is_empty());
        let due = f.db.due_notes_at(t0 + Duration::seconds(601)).unwrap();
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].id, n.id);
        f.db.mark_note_fired(&n.id).unwrap();
        assert!(f.db.due_notes_at(t0 + Duration::hours(2)).unwrap().is_empty());
        assert!(f.db.list_notes("c1").unwrap()[0].fired);
    }

    #[test]
    fn alarm_fires_at_its_instant_and_rejects_the_past() {
        let f = fx();
        let t0 = Utc::now();
        let at = t0 + Duration::hours(3);
        let n = f
            .db
            .create_note_at("c1", &f.account, "call", None, None, Some(&alarm(at)), t0)
            .unwrap();
        assert_eq!(n.kind.as_deref(), Some("alarm"));
        assert!(f.db.due_notes_at(at - Duration::seconds(5)).unwrap().is_empty());
        assert_eq!(f.db.due_notes_at(at + Duration::seconds(1)).unwrap().len(), 1);

        let past = f.db.create_note_at(
            "c1",
            &f.account,
            "late",
            None,
            None,
            Some(&alarm(t0 - Duration::hours(1))),
            t0,
        );
        assert!(matches!(past, Err(DbError::InvalidInput(_))));
        // A minute-granularity pick of "now" is slightly in the past and still accepted.
        assert!(f
            .db
            .create_note_at(
                "c1",
                &f.account,
                "now",
                None,
                None,
                Some(&alarm(t0 - Duration::seconds(30))),
                t0
            )
            .is_ok());
    }

    #[test]
    fn schedule_input_is_validated() {
        let t0 = Utc::now();
        let both = NoteSchedule {
            timer_secs: Some(60),
            alarm_at: Some(t0.to_rfc3339()),
        };
        assert!(resolve_schedule(&both, t0).is_err());
        assert!(resolve_schedule(&NoteSchedule::default(), t0).is_err());
        assert!(resolve_schedule(&timer(0), t0).is_err());
        assert!(resolve_schedule(&timer(MAX_TIMER_SECS + 1), t0).is_err());
        let bad = NoteSchedule {
            timer_secs: None,
            alarm_at: Some("tomorrow".into()),
        };
        assert!(resolve_schedule(&bad, t0).is_err());
        // Offsets are normalised to UTC so string comparison in SQL stays valid.
        let off = NoteSchedule {
            timer_secs: None,
            alarm_at: Some("2099-01-01T12:00:00+02:00".into()),
        };
        assert_eq!(
            resolve_schedule(&off, t0).unwrap().1,
            "2099-01-01T10:00:00.000Z"
        );
    }

    #[test]
    fn update_rearms_and_clears_schedule() {
        let f = fx();
        let t0 = Utc::now();
        let n = f
            .db
            .create_note_at("c1", &f.account, "x", None, None, Some(&timer(60)), t0)
            .unwrap();
        f.db.mark_note_fired(&n.id).unwrap();
        let again = f
            .db
            .update_note_at(&n.id, None, Some(&timer(3600)), false, t0)
            .unwrap();
        assert!(!again.fired);
        assert!(f.db.due_notes_at(t0 + Duration::seconds(120)).unwrap().is_empty());
        let cleared = f.db.update_note_at(&n.id, Some("renamed"), None, true, t0).unwrap();
        assert_eq!(cleared.body, "renamed");
        assert!(cleared.fire_at.is_none() && cleared.kind.is_none());
        assert!(f.db.due_notes_at(t0 + Duration::days(2)).unwrap().is_empty());
    }

    #[test]
    fn note_links_a_message_with_a_clipped_snippet() {
        let f = fx();
        let long = "x".repeat(500);
        let n = f
            .db
            .create_note("c1", &f.account, "", Some("m1"), Some(&long), None)
            .unwrap();
        assert_eq!(n.message_id.as_deref(), Some("m1"));
        assert_eq!(n.message_snippet.as_ref().unwrap().chars().count(), SNIPPET_MAX_CHARS);
        assert!(f.db.create_note("c1", &f.account, "  ", None, None, None).is_err());
    }

    #[test]
    fn todo_keeps_message_reference() {
        let f = fx();
        let t = f
            .db
            .add_todo_linked("c1", &f.account, "call back", None, Some("m9"), Some("📷 Photo"))
            .unwrap();
        let listed = f.db.list_todos("c1").unwrap();
        assert_eq!(listed[0].id, t.id);
        assert_eq!(listed[0].message_id.as_deref(), Some("m9"));
        assert_eq!(listed[0].message_snippet.as_deref(), Some("📷 Photo"));
        // Plain todos stay unlinked, and a snippet without a message id is dropped.
        let plain = f
            .db
            .add_todo_linked("c1", &f.account, "plain", None, None, Some("orphan"))
            .unwrap();
        assert!(plain.message_id.is_none() && plain.message_snippet.is_none());
    }

    #[test]
    fn migration_keeps_old_notes_text_and_unfired_reminders() {
        let f = fx();
        // Legacy chat with notes text, written the way the old UI stored it.
        let inbox = f.db.inbox(&f.account).unwrap();
        {
            let conn = inbox.conn.lock();
            conn.execute(
                "INSERT INTO conversations (id, account_id, remote_id, title, notes)
                 VALUES ('chat-a', ?1, 'a@s.whatsapp.net', 'A', '  remember the keys  ')",
                params![f.account],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO conversations (id, account_id, remote_id, title, notes)
                 VALUES ('chat-b', ?1, 'b@s.whatsapp.net', 'B', '')",
                params![f.account],
            )
            .unwrap();
            conn.execute("DELETE FROM inbox_meta WHERE key = 'notes_items_v1'", [])
                .unwrap();
        }
        // Legacy reminders: one pending with a note, one pending without, one already fired.
        {
            let conn = f.db.catalog.conn.lock();
            for (id, at, note, fired) in [
                ("r1", "2099-05-01T09:00:00.000Z", Some("dentist"), 0),
                ("r2", "2099-06-01T09:00:00+01:00", None, 0),
                ("r3", "2020-01-01T00:00:00.000Z", Some("old"), 1),
            ] {
                conn.execute(
                    "INSERT INTO reminders (id, conversation_id, account_id, fire_at, kind, note, fired, created_at)
                     VALUES (?1, 'chat-a', ?2, ?3, 'nudge', ?4, ?5, '2026-01-01T00:00:00Z')",
                    params![id, f.account, at, note, fired],
                )
                .unwrap();
            }
            conn.execute(
                "DELETE FROM app_meta WHERE key = 'chat_notes_from_reminders_v1'",
                [],
            )
            .unwrap();
            migrate_catalog(&conn).unwrap();
        }
        f.db.migrate_legacy_notes().unwrap();
        // Running both again must not duplicate anything.
        f.db.migrate_legacy_notes().unwrap();
        migrate_catalog(&f.db.catalog.conn.lock()).unwrap();

        let notes = f.db.list_notes("chat-a").unwrap();
        assert_eq!(notes.len(), 3, "{notes:?}");
        let text = notes.iter().find(|n| n.id == "legacy-notes-chat-a").unwrap();
        assert_eq!(text.body, "remember the keys");
        assert!(text.kind.is_none() && text.fire_at.is_none());
        let r1 = notes.iter().find(|n| n.id == "r1").unwrap();
        assert_eq!(r1.body, "dentist");
        assert_eq!(r1.kind.as_deref(), Some("alarm"));
        assert_eq!(r1.fire_at.as_deref(), Some("2099-05-01T09:00:00.000Z"));
        let r2 = notes.iter().find(|n| n.id == "r2").unwrap();
        assert_eq!(r2.body, "Reminder");
        assert_eq!(r2.fire_at.as_deref(), Some("2099-06-01T08:00:00.000Z"));
        assert!(notes.iter().all(|n| n.id != "r3"), "fired reminders are not carried over");
        assert!(f.db.list_notes("chat-b").unwrap().is_empty());

        // A legacy reminder that is already due fires on the next tick, as it would have before.
        f.db.catalog
            .conn
            .lock()
            .execute(
                "UPDATE chat_notes SET fire_at = '2000-01-01T00:00:00.000Z' WHERE id = 'r1'",
                [],
            )
            .unwrap();
        assert_eq!(f.db.due_notes().unwrap().len(), 1);
        // The old column is kept as a backup.
        let kept: String = inbox
            .conn
            .lock()
            .query_row("SELECT notes FROM conversations WHERE id = 'chat-a'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(kept.trim(), "remember the keys");
    }

    #[test]
    fn deleting_an_account_removes_its_notes() {
        let f = fx();
        f.db.create_note("c1", &f.account, "bye", None, None, None).unwrap();
        f.db.delete_account(&f.account).unwrap();
        assert!(f.db.list_notes("c1").unwrap().is_empty());
    }
}
