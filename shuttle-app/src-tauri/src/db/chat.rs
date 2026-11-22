//! Chat-state rules (R1-R8 of the sync contract).
//!
//! Connectors report plain facts (conversation events, message events, read events, send
//! replies). Everything that decides what is stored lives here, so the same facts applied in
//! any order give the same titles, chat times, previews and unread counts:
//!
//! * chat time only moves forward and previews follow it (R2),
//! * titles come from one function (R3),
//! * unread is derived from a per-chat read marker, never from a counter (R4),
//! * one chat known under several JIDs is renamed or merged (R7),
//! * sends are reconciled with their replies and echoes (R8).
//!
//! Nothing in here depends on Tauri, so it is unit-tested against a temporary database.

use super::{
    direction_str, get_conversation_locked, get_message_locked, is_placeholder_title,
    map_conversation_row, map_message_row, merge_json_objects, message_preview, message_status_str,
    parse_stored_datetime, pick_conversation_title, Database, DbError, CONV_SELECT,
};
use crate::models::*;
use chrono::{DateTime, Duration, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{Map, Value};
use uuid::Uuid;

/// Echo adoption window (R8).
const ECHO_WINDOW_SECS: i64 = 180;
/// A message older than this never raises a desktop notification (R5).
const NOTIFY_MAX_AGE_SECS: i64 = 600;

// ---------------------------------------------------------------------------------------------
// Pure helpers
// ---------------------------------------------------------------------------------------------

pub fn is_wa_jid(jid: &str) -> bool {
    jid.ends_with("@s.whatsapp.net")
        || jid.ends_with("@lid")
        || jid.ends_with("@g.us")
        || jid.ends_with("@newsletter")
}

pub fn jid_conversation_type(jid: &str) -> ConversationType {
    if jid.ends_with("@g.us") {
        ConversationType::Group
    } else if jid.ends_with("@newsletter") {
        ConversationType::Channel
    } else {
        ConversationType::Direct
    }
}

/// Title for a chat we only know by JID: `+<digits>` for people, `Group` for groups/channels.
pub fn placeholder_title(jid: &str) -> String {
    if jid.ends_with("@g.us") || jid.ends_with("@newsletter") {
        return "Group".into();
    }
    let user = jid.split('@').next().unwrap_or(jid);
    let user = user.split(':').next().unwrap_or(user);
    let digits: String = user.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        user.to_string()
    } else {
        format!("+{digits}")
    }
}

pub struct TitleInputs<'a> {
    pub remote_id: &'a str,
    pub conversation_type: &'a ConversationType,
    pub is_self: bool,
    pub names: &'a Map<String, Value>,
    /// Saved address-book name from the `contacts` table, when there is one.
    pub contact_name: Option<&'a str>,
    pub stored_title: &'a str,
}

fn name_of<'a>(names: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    names
        .get(key)
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

/// R3: the one function that decides a WhatsApp chat title.
pub fn compute_wa_title(i: &TitleInputs<'_>) -> String {
    let contact = i
        .contact_name
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .or_else(|| name_of(i.names, "contact"));
    if i.is_self {
        return match contact {
            Some(name) => format!("{name} (You)"),
            None => "Message yourself".into(),
        };
    }
    let stored = i.stored_title.trim();
    let stored_ok = !stored.is_empty() && !is_placeholder_title(stored, i.remote_id);
    match i.conversation_type {
        ConversationType::Group | ConversationType::Channel => name_of(i.names, "subject")
            .map(str::to_string)
            .or_else(|| stored_ok.then(|| stored.to_string()))
            .unwrap_or_else(|| "Group".into()),
        ConversationType::Direct => contact
            .or_else(|| name_of(i.names, "verified"))
            .or_else(|| name_of(i.names, "push"))
            .map(str::to_string)
            .or_else(|| stored_ok.then(|| stored.to_string()))
            .unwrap_or_else(|| placeholder_title(i.remote_id)),
    }
}

/// Sort order of chat lists everywhere (R2): pinned, newest first (missing last), title
/// (ASCII case-insensitive, like SQLite `COLLATE NOCASE`), id.
pub fn compare_conversations(a: &Conversation, b: &Conversation) -> std::cmp::Ordering {
    b.pinned
        .cmp(&a.pinned)
        .then_with(|| {
            let ta = a.last_message_at.map(|t| t.timestamp());
            let tb = b.last_message_at.map(|t| t.timestamp());
            tb.cmp(&ta)
        })
        .then_with(|| {
            a.title
                .to_ascii_lowercase()
                .cmp(&b.title.to_ascii_lowercase())
        })
        .then_with(|| a.id.cmp(&b.id))
}

fn bodies_match(local: &Message, echo: &Message) -> bool {
    let a = local.body.trim();
    let b = echo.body.trim();
    if a == b {
        return true;
    }
    let media = |m: &Message| {
        m.metadata
            .get("media_type")
            .and_then(|v| v.as_str())
            .is_some_and(|s| !s.is_empty() && s != "text")
    };
    let placeholder = |s: &str| s.is_empty() || (s.starts_with('[') && s.ends_with(']'));
    media(local) && media(echo) && (placeholder(a) || placeholder(b))
}

/// Milliseconds since the epoch, rounded to the nearest millisecond. SQLite's
/// `unixepoch(ts, 'subsec')` rounds the same way; truncating here would make a message at
/// exactly the marker time count as newer than the marker.
fn millis(t: DateTime<Utc>) -> i64 {
    t.timestamp() * 1000 + (i64::from(t.timestamp_subsec_nanos()) + 500_000) / 1_000_000
}

fn later(a: Option<DateTime<Utc>>, b: Option<DateTime<Utc>>) -> Option<DateTime<Utc>> {
    match (a, b) {
        (Some(x), Some(y)) => Some(x.max(y)),
        (x, None) => x,
        (None, y) => y,
    }
}

fn ts_string(t: DateTime<Utc>) -> String {
    t.to_rfc3339()
}

// ---------------------------------------------------------------------------------------------
// Event inputs and outcomes
// ---------------------------------------------------------------------------------------------

/// A `conversation.updated` fact. `names: None` marks a legacy event (other connectors).
#[derive(Debug, Clone, Default)]
pub struct ConversationEvent {
    pub remote_id: String,
    pub alt_remote_ids: Vec<String>,
    pub names: Option<Map<String, Value>>,
    pub is_self: bool,
    /// Legacy `title` field (used only when `names` is `None`).
    pub legacy_title: Option<String>,
    pub conversation_type: Option<ConversationType>,
    pub last_message_at: Option<DateTime<Utc>>,
    pub preview: Option<String>,
    pub archived: Option<bool>,
    pub pinned: Option<bool>,
    /// A group only admins may post to, and whether this account is one of them (absent = unknown).
    pub announce_only: Option<bool>,
    pub self_is_admin: Option<bool>,
    /// Legacy title rule only: bulk-sync titles may replace a stored one.
    pub history: bool,
}

#[derive(Debug, Clone)]
pub struct MergedAway {
    pub removed_id: String,
    pub into_id: String,
}

#[derive(Debug)]
pub struct ConversationOutcome {
    /// Present when the stored row changed, was created, or absorbed another chat.
    pub conversation: Option<Conversation>,
    pub changed: bool,
    pub merged: Vec<MergedAway>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IngestKind {
    /// A new row was stored.
    Inserted,
    /// An outbound echo was matched to an existing local row (`message.updated`).
    Adopted,
    /// Row already known; nothing user-visible changed.
    Duplicate,
}

#[derive(Debug)]
pub struct IngestOutcome {
    pub kind: IngestKind,
    pub conversation: Option<Conversation>,
    pub message: Option<Message>,
    pub merged: Vec<MergedAway>,
    /// R5: inbound, live, newer than the read marker and recent. Mute/quiet checks come later.
    pub notify_eligible: bool,
    /// R6: emit `message.received` / `message.sent` to the UI.
    pub emit_new: bool,
}

#[derive(Debug)]
pub enum SendReconcile {
    Updated {
        conversation_id: String,
        message: Message,
    },
    Removed {
        conversation_id: String,
        message_id: String,
    },
    Nothing,
}

#[derive(Debug)]
pub struct ReadOutcome {
    pub conversation: Option<Conversation>,
    pub changed: bool,
    pub merged: Vec<MergedAway>,
}

const REVOKED_AT: &str = "revoked_at";
const EDITED_AT: &str = "edited_at";

/// A message deleted for everyone stays in place as a tombstone marked by `revoked_at`.
pub fn is_revoked(metadata: &Value) -> bool {
    metadata.get(REVOKED_AT).is_some_and(|v| !v.is_null())
}

/// A body that is a placeholder rather than the sender's text: the media kind in brackets, which
/// is what the connector sends for a media message without a caption, or the bare `unknown`
/// older builds stored (the sender's WhatsApp verification level, not the message).
fn is_placeholder_body(body: &str) -> bool {
    matches!(
        body.trim().to_ascii_lowercase().as_str(),
        "unknown"
            | "[image]"
            | "[photo]"
            | "[video]"
            | "[audio]"
            | "[document]"
            | "[sticker]"
            | "[location]"
            | "[poll]"
            | "[contact]"
    )
}

/// A change to a stored message. The same fact arrives for a change made on the phone and for
/// one made from this app, so both take this one path.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MessageChange {
    Edited { text: String, at: DateTime<Utc> },
    /// Deleted for everyone: the body and media go, the row keeps its place.
    Revoked { at: DateTime<Utc> },
    /// Deleted on this account only: the row goes, and a later sync of it is ignored.
    DeletedForMe,
}

#[derive(Debug)]
pub struct ChangeOutcome {
    pub message: SendReconcile,
    /// Present when the chat's preview changed with it.
    pub conversation: Option<Conversation>,
}

// ---------------------------------------------------------------------------------------------
// Light conversation row (no metadata blob, so no avatars are loaded on hot paths)
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct Lite {
    id: String,
    remote_id: String,
    title: String,
    ctype: ConversationType,
    last_at: Option<DateTime<Utc>>,
    preview: Option<String>,
    read_through: Option<DateTime<Utc>>,
    pinned: bool,
    archived: bool,
    alts: Vec<String>,
    /// `None` when the chat has never received a `names`-carrying event.
    names: Option<Map<String, Value>>,
    is_self: bool,
    /// Upgrade flag: the stored marker wins over the inbox baseline until the chat is read.
    unread_keep: bool,
    /// Who wrote the message the stored preview shows (see `PreviewSource`).
    source: Option<PreviewSource>,
    /// Admin-only group facts; `None` until GOWA reports them.
    announce_only: Option<bool>,
    self_is_admin: Option<bool>,
}

/// Direction and author of the message a chat's preview comes from. Stored next to the
/// preview so the list can draw the up/down arrow and the group "Name:" prefix.
#[derive(Debug, Clone, PartialEq, Eq)]
struct PreviewSource {
    from_me: bool,
    /// Inbound only; always `None` for outbound.
    sender_id: Option<String>,
    sender_name: Option<String>,
}

impl PreviewSource {
    fn of_message(msg: &Message) -> Self {
        let from_me = msg.direction == MessageDirection::Outbound;
        let clean = |v: &Option<String>| v.clone().filter(|s| !from_me && !s.trim().is_empty());
        PreviewSource {
            from_me,
            sender_id: clean(&msg.sender_id),
            sender_name: clean(&msg.sender_name),
        }
    }
}

const LITE_SELECT: &str = "SELECT id, remote_id, title, conversation_type, last_message_at,
        last_message_preview, read_through, unread_count, pinned, archived,
        json_extract(metadata, '$.alt_remote_ids'), json_extract(metadata, '$.names'),
        json_extract(metadata, '$.is_self'), json_extract(metadata, '$.marked_unread'),
        json_extract(metadata, '$.unread_keep'),
        last_message_from_me, last_message_sender_id, last_message_sender_name,
        json_extract(metadata, '$.announce_only'), json_extract(metadata, '$.self_is_admin')
    FROM conversations";

fn map_lite(row: &rusqlite::Row<'_>) -> rusqlite::Result<Lite> {
    let alts: Vec<String> = row
        .get::<_, Option<String>>(10)?
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    let names = row
        .get::<_, Option<String>>(11)?
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .and_then(|v| match v {
            Value::Object(m) => Some(m),
            _ => None,
        });
    Ok(Lite {
        id: row.get(0)?,
        remote_id: row.get(1)?,
        title: row.get(2)?,
        ctype: match row.get::<_, String>(3)?.as_str() {
            "group" => ConversationType::Group,
            "channel" => ConversationType::Channel,
            _ => ConversationType::Direct,
        },
        last_at: row
            .get::<_, Option<String>>(4)?
            .and_then(|s| parse_stored_datetime(&s)),
        preview: row.get(5)?,
        read_through: row
            .get::<_, Option<String>>(6)?
            .and_then(|s| parse_stored_datetime(&s)),
        pinned: row.get::<_, i64>(8)? != 0,
        archived: row.get::<_, i64>(9)? != 0,
        alts,
        names,
        is_self: row.get::<_, Option<i64>>(12)?.unwrap_or(0) != 0,
        unread_keep: row.get::<_, Option<i64>>(14)?.unwrap_or(0) != 0,
        source: row.get::<_, Option<i64>>(15)?.map(|n| PreviewSource {
            from_me: n != 0,
            sender_id: row.get(16).ok().flatten(),
            sender_name: row.get(17).ok().flatten(),
        }),
        announce_only: row.get::<_, Option<i64>>(18)?.map(|n| n != 0),
        self_is_admin: row.get::<_, Option<i64>>(19)?.map(|n| n != 0),
    })
}

fn lite_by_remote(
    conn: &Connection,
    account_id: &str,
    remote: &str,
) -> Result<Option<Lite>, DbError> {
    let sql = format!("{LITE_SELECT} WHERE account_id = ?1 AND remote_id = ?2");
    conn.query_row(&sql, params![account_id, remote], map_lite)
        .optional()
        .map_err(DbError::from)
}

fn lite_by_id(conn: &Connection, id: &str) -> Result<Option<Lite>, DbError> {
    let sql = format!("{LITE_SELECT} WHERE id = ?1");
    conn.query_row(&sql, params![id], map_lite)
        .optional()
        .map_err(DbError::from)
}

/// A chat that earlier events recorded as another name for `jid`.
fn lite_by_stored_alias(
    conn: &Connection,
    account_id: &str,
    jid: &str,
) -> Result<Option<Lite>, DbError> {
    let sql = format!(
        "{LITE_SELECT} WHERE account_id = ?1 AND EXISTS (
            SELECT 1 FROM json_each(conversations.metadata, '$.alt_remote_ids') WHERE value = ?2
         ) LIMIT 1"
    );
    conn.query_row(&sql, params![account_id, jid], map_lite)
        .optional()
        .map_err(DbError::from)
}

// ---------------------------------------------------------------------------------------------
// Read marker and unread cache (R4)
// ---------------------------------------------------------------------------------------------

fn inbox_baseline(conn: &Connection) -> Result<Option<DateTime<Utc>>, DbError> {
    let v: Option<String> = conn
        .query_row(
            "SELECT value FROM inbox_meta WHERE key = 'unread_baseline'",
            [],
            |r| r.get(0),
        )
        .optional()?;
    Ok(v.and_then(|s| parse_stored_datetime(&s)))
}

/// Effective marker (R4): the later of the chat marker and the inbox baseline. Chats upgraded
/// from the counter keep their own marker until they are read (see `migrate_read_markers`).
fn effective_marker(
    read_through: Option<DateTime<Utc>>,
    baseline: Option<DateTime<Utc>>,
    keep: bool,
) -> Option<DateTime<Utc>> {
    if keep {
        read_through
    } else {
        later(read_through, baseline)
    }
}

/// Recompute the cached `unread_count` from the marker. Writes only when the value changes.
/// Returns the new count when it changed.
fn recompute_unread(
    conn: &Connection,
    conv_id: &str,
    baseline: Option<DateTime<Utc>>,
) -> Result<Option<i64>, DbError> {
    let (rt, cached, flag, keep): (Option<String>, i64, Option<i64>, Option<i64>) = conn
        .query_row(
            "SELECT read_through, unread_count, json_extract(metadata, '$.marked_unread'),
                json_extract(metadata, '$.unread_keep')
         FROM conversations WHERE id = ?1",
            params![conv_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )?;
    let marker = effective_marker(
        rt.and_then(|s| parse_stored_datetime(&s)),
        baseline,
        keep.unwrap_or(0) != 0,
    );
    let mut count: i64 = match marker {
        Some(m) => conn.query_row(
            "SELECT COUNT(*) FROM messages WHERE conversation_id = ?1 AND direction = 'inbound'
             AND unixepoch(timestamp, 'subsec') >= ?3
             AND CAST(ROUND(unixepoch(timestamp, 'subsec') * 1000) AS INTEGER) > ?2",
            params![conv_id, millis(m), (millis(m) - 1) as f64 / 1000.0],
            |r| r.get(0),
        )?,
        None => conn.query_row(
            "SELECT COUNT(*) FROM messages WHERE conversation_id = ?1 AND direction = 'inbound'",
            params![conv_id],
            |r| r.get(0),
        )?,
    };
    if flag.unwrap_or(0) != 0 {
        count = count.max(1);
    }
    if count == cached {
        return Ok(None);
    }
    conn.execute(
        "UPDATE conversations SET unread_count = ?1 WHERE id = ?2",
        params![count, conv_id],
    )?;
    Ok(Some(count))
}

/// Move the marker forward (max); when it actually moves, clear the manual-unread flag (and the
/// upgrade flag), then recompute. Returns true when anything was written.
fn advance_marker(
    conn: &Connection,
    lite: &Lite,
    to: DateTime<Utc>,
    baseline: Option<DateTime<Utc>>,
) -> Result<bool, DbError> {
    let mut wrote = false;
    let current = effective_marker(lite.read_through, baseline, lite.unread_keep);
    if current.is_none_or(|m| to > m) {
        let target = later(lite.read_through, Some(to)).unwrap_or(to);
        conn.execute(
            "UPDATE conversations SET read_through = ?1,
                metadata = json_remove(metadata, '$.marked_unread', '$.unread_keep')
             WHERE id = ?2",
            params![ts_string(target), lite.id],
        )?;
        wrote = true;
    }
    if recompute_unread(conn, &lite.id, baseline)?.is_some() {
        wrote = true;
    }
    Ok(wrote)
}

// ---------------------------------------------------------------------------------------------
// Contacts and titles
// ---------------------------------------------------------------------------------------------

fn contact_name_for(conn: &Connection, account_id: &str, jids: &[&str]) -> Option<String> {
    for jid in jids {
        let name: Option<String> = conn
            .query_row(
                "SELECT display_name FROM contacts WHERE account_id = ?1 AND remote_id = ?2",
                params![account_id, jid],
                |r| r.get(0),
            )
            .optional()
            .ok()
            .flatten();
        if let Some(name) = name {
            let t = name.trim();
            if !t.is_empty() && !is_placeholder_title(t, jid) {
                return Some(t.to_string());
            }
        }
    }
    None
}

fn jids_of<'a>(remote: &'a str, alts: &'a [String]) -> Vec<&'a str> {
    let mut v = vec![remote];
    v.extend(alts.iter().map(String::as_str).filter(|a| *a != remote));
    v
}

fn matches_identity(identity: Option<&str>, remote: &str, alts: &[String]) -> bool {
    let Some(identity) = identity else {
        return false;
    };
    jids_of(remote, alts)
        .iter()
        .any(|j| crate::media_store::jids_same(j, identity))
}

/// Today's title logic for events without `names` (other connectors), unchanged.
fn legacy_event_title(
    conn: &Connection,
    account_id: &str,
    ev: &ConversationEvent,
    identity: Option<&str>,
) -> String {
    let remote = ev.remote_id.as_str();
    let mut title = ev
        .legacy_title
        .clone()
        .unwrap_or_else(|| remote.to_string());
    if let Some(identity) = identity {
        if crate::media_store::jids_same(remote, identity) {
            let mut stmt = conn
                .prepare("SELECT remote_id, display_name FROM contacts WHERE account_id = ?1")
                .ok();
            if let Some(stmt) = stmt.as_mut() {
                if let Ok(rows) = stmt.query_map(params![account_id], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
                }) {
                    for row in rows.flatten() {
                        if crate::media_store::jids_same(&row.0, remote) {
                            title = format!("{} (You)", row.1);
                            break;
                        }
                    }
                }
            }
            if !title.ends_with("(You)") {
                if let Some(name) = ev
                    .legacy_title
                    .as_deref()
                    .filter(|t| !t.chars().all(|c| c.is_ascii_digit() || "+- ()".contains(c)))
                {
                    if !name.is_empty() && !crate::media_store::jids_same(name, remote) {
                        title = format!("{name} (You)");
                    }
                }
            }
        }
    }
    title
}

fn sanitize_names(names: &Map<String, Value>) -> Map<String, Value> {
    let mut out = Map::new();
    for (k, v) in names {
        if let Some(s) = v.as_str() {
            let t = s.trim();
            if !t.is_empty() {
                out.insert(k.clone(), Value::String(t.to_string()));
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------
// One chat, several JIDs (R7)
// ---------------------------------------------------------------------------------------------

struct Resolved {
    conv: Option<Lite>,
    merged: Vec<MergedAway>,
    /// Former JIDs of renamed/merged rows; recorded as aliases of the survivor.
    retired: Vec<String>,
}

fn resolve_conversation(
    conn: &Connection,
    account_id: &str,
    remote: &str,
    alts: &[String],
) -> Result<Resolved, DbError> {
    let primary = lite_by_remote(conn, account_id, remote)?;
    let mut aliases: Vec<Lite> = Vec::new();
    for alt in alts {
        if alt.is_empty() || alt == remote {
            continue;
        }
        if let Some(l) = lite_by_remote(conn, account_id, alt)? {
            if !aliases.iter().any(|a| a.id == l.id) {
                aliases.push(l);
            }
        }
    }
    let mut merged = Vec::new();
    let mut retired = Vec::new();
    let survivor_id: Option<String> = match (primary, aliases.len()) {
        (Some(p), 0) => {
            return Ok(Resolved {
                conv: Some(p),
                merged,
                retired,
            })
        }
        (None, 0) => {
            // Not stored under any JID the event names; maybe under one an earlier event listed.
            for jid in jids_of(remote, alts) {
                if let Some(l) = lite_by_stored_alias(conn, account_id, jid)? {
                    return Ok(Resolved {
                        conv: Some(l),
                        merged,
                        retired,
                    });
                }
            }
            return Ok(Resolved {
                conv: None,
                merged,
                retired,
            });
        }
        (Some(p), _) => {
            for a in &aliases {
                merge_conversations(conn, &p.id, &a.id)?;
                merged.push(MergedAway {
                    removed_id: a.id.clone(),
                    into_id: p.id.clone(),
                });
                retired.push(a.remote_id.clone());
            }
            Some(p.id)
        }
        (None, _) => {
            aliases.sort_by(|a, b| b.last_at.cmp(&a.last_at).then_with(|| a.id.cmp(&b.id)));
            let survivor = aliases.remove(0);
            conn.execute(
                "UPDATE conversations SET remote_id = ?1, updated_at = ?2 WHERE id = ?3",
                params![remote, ts_string(Utc::now()), survivor.id],
            )?;
            retired.push(survivor.remote_id.clone());
            for a in &aliases {
                merge_conversations(conn, &survivor.id, &a.id)?;
                merged.push(MergedAway {
                    removed_id: a.id.clone(),
                    into_id: survivor.id.clone(),
                });
                retired.push(a.remote_id.clone());
            }
            Some(survivor.id)
        }
    };
    let conv = match survivor_id {
        Some(id) => lite_by_id(conn, &id)?,
        None => None,
    };
    Ok(Resolved {
        conv,
        merged,
        retired,
    })
}

/// Read-only lookup of the stored chat that an event's JIDs name (its own JID, any listed
/// alternative, or a stored alias). Unlike `resolve_conversation` it never merges or renames.
pub(super) fn find_conversation_id(
    conn: &Connection,
    account_id: &str,
    remote: &str,
    alts: &[String],
) -> Result<Option<String>, DbError> {
    for jid in jids_of(remote, alts) {
        if let Some(l) = lite_by_remote(conn, account_id, jid)? {
            return Ok(Some(l.id));
        }
    }
    for jid in jids_of(remote, alts) {
        if let Some(l) = lite_by_stored_alias(conn, account_id, jid)? {
            return Ok(Some(l.id));
        }
    }
    Ok(None)
}

/// Record alias JIDs on the chat. Writes only when the set grows.
fn sync_aliases(
    conn: &Connection,
    lite: &Lite,
    remote: &str,
    extra: &[String],
) -> Result<bool, DbError> {
    let mut desired = lite.alts.clone();
    for a in extra {
        if !a.is_empty() && a != remote && !desired.contains(a) {
            desired.push(a.clone());
        }
    }
    desired.retain(|a| a != remote);
    if desired == lite.alts {
        return Ok(false);
    }
    conn.execute(
        "UPDATE conversations SET metadata = json_set(metadata, '$.alt_remote_ids', json(?1)) WHERE id = ?2",
        params![serde_json::to_string(&desired).unwrap_or_else(|_| "[]".into()), lite.id],
    )?;
    Ok(true)
}

fn merge_conversations(
    conn: &Connection,
    survivor_id: &str,
    other_id: &str,
) -> Result<(), DbError> {
    let s = get_conversation_locked(conn, survivor_id)?;
    let o = get_conversation_locked(conn, other_id)?;
    let rt = |id: &str| -> Option<DateTime<Utc>> {
        conn.query_row(
            "SELECT read_through FROM conversations WHERE id = ?1",
            params![id],
            |r| r.get::<_, Option<String>>(0),
        )
        .ok()
        .flatten()
        .and_then(|v| parse_stored_datetime(&v))
    };
    let (s_rt, o_rt) = (rt(survivor_id), rt(other_id));

    // Messages: skip rows whose remote id already exists in the survivor (they die with `other`).
    conn.execute(
        "UPDATE OR IGNORE messages SET conversation_id = ?1 WHERE conversation_id = ?2",
        params![survivor_id, other_id],
    )?;

    // Chat time/preview: newest of survivor, other and the newest stored message.
    let newest: Option<(DateTime<Utc>, String, PreviewSource)> = conn
        .query_row(
            "SELECT body, timestamp, metadata, direction, sender_id, sender_name FROM messages
             WHERE conversation_id = ?1
             ORDER BY unixepoch(timestamp, 'subsec') DESC LIMIT 1",
            params![survivor_id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)? == "outbound",
                    r.get::<_, Option<String>>(4)?,
                    r.get::<_, Option<String>>(5)?,
                ))
            },
        )
        .optional()?
        .and_then(|(body, ts, meta, from_me, sid, sname)| {
            let t = parse_stored_datetime(&ts)?;
            let meta: Value = serde_json::from_str(&meta).unwrap_or_else(|_| serde_json::json!({}));
            let keep = |v: Option<String>| v.filter(|x| !from_me && !x.trim().is_empty());
            Some((
                t,
                message_preview(&body, &meta),
                PreviewSource {
                    from_me,
                    sender_id: keep(sid),
                    sender_name: keep(sname),
                },
            ))
        });
    let conv_source = |c: &Conversation| {
        c.last_message_from_me.map(|from_me| PreviewSource {
            from_me,
            sender_id: c.last_message_sender_id.clone(),
            sender_name: c.last_message_sender_name.clone(),
        })
    };
    let mut best: (Option<DateTime<Utc>>, Option<String>) =
        (s.last_message_at, s.last_message_preview.clone());
    let mut best_src = conv_source(&s);
    if o.last_message_at > best.0 || (best.1.is_none() && o.last_message_at.is_some()) {
        if o.last_message_preview.is_some() {
            best_src = conv_source(&o);
        }
        best = (
            later(best.0, o.last_message_at),
            o.last_message_preview.clone().or(best.1),
        );
    }
    if let Some((t, p, src)) = newest {
        if best.0.is_none_or(|b| t > b) {
            best = (Some(t), Some(p));
            best_src = Some(src);
        } else if best.1.is_none() {
            best.1 = Some(p);
            best_src = Some(src);
        }
    }

    // Settings: survivor wins, other fills gaps.
    let mut meta = merge_json_objects(o.metadata.clone(), s.metadata.clone());
    if let Value::Object(m) = &mut meta {
        let mut names = o
            .metadata
            .get("names")
            .and_then(|v| v.as_object())
            .cloned()
            .unwrap_or_default();
        if let Some(sn) = s.metadata.get("names").and_then(|v| v.as_object()) {
            for (k, v) in sn {
                names.insert(k.clone(), v.clone());
            }
        }
        if o.metadata.get("names").is_some() || s.metadata.get("names").is_some() {
            m.insert("names".into(), Value::Object(names));
        }
        let mut alts: Vec<Value> = s
            .metadata
            .get("alt_remote_ids")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        for a in o
            .metadata
            .get("alt_remote_ids")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default()
        {
            if !alts.contains(&a) {
                alts.push(a);
            }
        }
        let other_remote = Value::String(o.remote_id.clone());
        if !alts.contains(&other_remote) {
            alts.push(other_remote);
        }
        alts.retain(|a| a.as_str() != Some(s.remote_id.as_str()));
        m.insert("alt_remote_ids".into(), Value::Array(alts));
        let flag = |v: &Value, k: &str| v.get(k).and_then(|x| x.as_bool()).unwrap_or(false);
        if flag(&s.metadata, "is_self") || flag(&o.metadata, "is_self") {
            m.insert("is_self".into(), Value::Bool(true));
        }
        if flag(&s.metadata, "marked_unread") || flag(&o.metadata, "marked_unread") {
            m.insert("marked_unread".into(), Value::Bool(true));
        }
    }
    let notes = if s.notes.trim().is_empty() {
        o.notes.clone()
    } else {
        s.notes.clone()
    };
    conn.execute(
        "UPDATE conversations SET pinned = ?1, archived = ?2, muted = ?3, notes = ?4,
            workspace_id = ?5, priority_group = ?6, notify_enabled = ?7, send_receipts = ?8,
            metadata = ?9, last_message_at = ?10, last_message_preview = ?11, read_through = ?12,
            updated_at = ?13
         WHERE id = ?14",
        params![
            (s.pinned || o.pinned) as i64,
            (s.archived || o.archived) as i64,
            (s.muted || o.muted) as i64,
            notes,
            s.workspace_id.clone().or(o.workspace_id.clone()),
            s.priority_group.clone().or(o.priority_group.clone()),
            s.notify_enabled.or(o.notify_enabled).map(|b| b as i64),
            s.send_receipts.or(o.send_receipts).map(|b| b as i64),
            meta.to_string(),
            best.0.map(ts_string),
            best.1,
            later(s_rt, o_rt).map(ts_string),
            ts_string(Utc::now()),
            survivor_id,
        ],
    )?;
    write_preview_source(conn, survivor_id, best_src.as_ref())?;
    // Reactions follow their messages; a (message, reactor) the survivor already has wins.
    conn.execute(
        "UPDATE OR IGNORE message_reactions SET conversation_id = ?1 WHERE conversation_id = ?2",
        params![survivor_id, other_id],
    )?;
    conn.execute(
        "UPDATE OR IGNORE deleted_messages SET conversation_id = ?1 WHERE conversation_id = ?2",
        params![survivor_id, other_id],
    )?;
    conn.execute("DELETE FROM conversations WHERE id = ?1", params![other_id])?;
    let baseline = inbox_baseline(conn)?;
    recompute_unread(conn, survivor_id, baseline)?;
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Polls
//
// A poll's options live in its own message metadata (`metadata.poll`). A vote is a fact about
// that message, never a message of its own. Votes are folded by voter, so replaying the same
// vote, or a later vote from the same person, converges to the same counts.
// ---------------------------------------------------------------------------------------------

/// The voter id this account's own votes use, matching the reaction facts.
pub const ME_VOTER_ID: &str = "me";

/// One vote as a connector reports it: who, and which option ids they picked.
#[derive(Debug, Clone)]
pub struct PollVoteFact {
    /// `me` for this account, else the voter's JID (the connector's convention, as for reactions).
    pub voter_id: String,
    pub voter_name: Option<String>,
    /// Empty clears this voter's vote.
    pub selected: Vec<String>,
}

#[derive(Debug, Default)]
pub struct PollOutcome {
    pub conversation_id: Option<String>,
    /// The updated poll message, for the UI to redraw in place.
    pub message: Option<Message>,
    /// Set when the poll message is the chat's preview source and its text changed.
    pub conversation: Option<Conversation>,
    pub changed: bool,
}

/// Recompute what each option displays from the voters it holds.
fn settle_poll(poll: &mut PollData) {
    for option in &mut poll.options {
        option.votes = option.voters.len() as u32;
        option.mine = option.voters.iter().any(|v| v.id == ME_VOTER_ID);
    }
}

/// Bring question and options up to date without losing folded votes. An option keeps its votes
/// across a rename or a reorder, because votes name an option by id.
pub fn merge_poll_definition(poll: &mut PollData, incoming: &PollData) -> bool {
    let mut changed = false;
    if !incoming.question.trim().is_empty() && incoming.question != poll.question {
        poll.question = incoming.question.clone();
        changed = true;
    }
    if incoming.options.is_empty() {
        return changed;
    }
    if poll.multiple != incoming.multiple {
        poll.multiple = incoming.multiple;
        changed = true;
    }
    let mut options: Vec<PollOptionData> = Vec::with_capacity(incoming.options.len());
    for wanted in &incoming.options {
        let mut option = poll
            .options
            .iter()
            .find(|o| o.id == wanted.id)
            .cloned()
            .unwrap_or_else(|| PollOptionData {
                id: wanted.id.clone(),
                ..Default::default()
            });
        if option.text != wanted.text {
            option.text = wanted.text.clone();
            changed = true;
        }
        options.push(option);
    }
    if options != poll.options {
        poll.options = options;
        changed = true;
    }
    settle_poll(poll);
    changed
}

/// Fold one voter's pick into the poll. Returns whether anything changed.
///
/// The voter is removed from every option first, so a change of mind moves the vote instead of
/// adding a second one, and an empty `selected` withdraws it.
pub fn fold_poll_vote(poll: &mut PollData, vote: &PollVoteFact) -> bool {
    if vote.voter_id.is_empty() {
        return false;
    }
    let mut changed = false;
    for option in &mut poll.options {
        let before = option.voters.len();
        option.voters.retain(|v| v.id != vote.voter_id);
        changed |= option.voters.len() != before;
    }
    for id in &vote.selected {
        let Some(option) = poll.options.iter_mut().find(|o| &o.id == id) else {
            continue;
        };
        if option.voters.iter().any(|v| v.id == vote.voter_id) {
            continue;
        }
        option.voters.push(PollVoter {
            id: vote.voter_id.clone(),
            name: vote.voter_name.clone(),
        });
        changed = true;
    }
    if changed {
        settle_poll(poll);
    }
    changed
}

// ---------------------------------------------------------------------------------------------
// Database API
// ---------------------------------------------------------------------------------------------

impl Database {
    /// Fold one poll fact into the poll message whose remote id is `poll_id`.
    ///
    /// A vote that arrives before the poll message is dropped: there is nothing to fold it into,
    /// and the connector reports the full state of a vote again only if it changes.
    pub fn apply_poll_change(
        &self,
        account_id: &str,
        remote: &str,
        alts: &[String],
        poll_id: &str,
        definition: Option<&PollData>,
        vote: Option<&PollVoteFact>,
    ) -> Result<PollOutcome, DbError> {
        let inbox = self.inbox(account_id)?;
        let conn = inbox.conn.lock();
        let Some(conversation_id) = find_conversation_id(&conn, account_id, remote, alts)? else {
            return Ok(PollOutcome::default());
        };
        let nothing = || PollOutcome {
            conversation_id: Some(conversation_id.clone()),
            ..Default::default()
        };
        if poll_id.is_empty() {
            return Ok(nothing());
        }
        let tx = conn.unchecked_transaction()?;
        let sql = format!("{MESSAGE_SELECT} WHERE conversation_id = ?1 AND remote_id = ?2");
        let Some(msg) = tx
            .query_row(&sql, params![conversation_id, poll_id], map_message_row)
            .optional()?
        else {
            return Ok(nothing());
        };
        let mut poll: PollData = msg
            .metadata
            .get("poll")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();
        let before = poll.clone();
        if let Some(def) = definition {
            merge_poll_definition(&mut poll, def);
        }
        if let Some(v) = vote {
            fold_poll_vote(&mut poll, v);
        }
        if poll == before {
            tx.commit()?;
            return Ok(nothing());
        }
        let meta = merge_json_objects(
            msg.metadata.clone(),
            serde_json::json!({ "poll": serde_json::to_value(&poll)
                .map_err(|e| DbError::InvalidInput(e.to_string()))? }),
        );
        tx.execute(
            "UPDATE messages SET metadata = ?1 WHERE id = ?2",
            params![meta.to_string(), msg.id],
        )?;
        let lite = lite_by_id(&tx, &conversation_id)?
            .ok_or_else(|| DbError::NotFound(conversation_id.clone()))?;
        let conversation = if preview_comes_from(&tx, &lite, &msg)? {
            Some(rederive_preview(&tx, &lite)?)
        } else {
            None
        };
        let message = get_message_locked(&tx, &conversation_id, &msg.id)?;
        tx.commit()?;
        Ok(PollOutcome {
            conversation_id: Some(conversation_id),
            message: Some(message),
            conversation,
            changed: true,
        })
    }

    /// Apply a `conversation.updated` fact (R2, R3, R7).
    pub fn apply_conversation_event(
        &self,
        account_id: &str,
        ev: &ConversationEvent,
    ) -> Result<ConversationOutcome, DbError> {
        let identity = self.get_account(account_id).ok().and_then(|a| a.identity);
        let inbox = self.inbox(account_id)?;
        let out = {
            let conn = inbox.conn.lock();
            let tx = conn.unchecked_transaction()?;
            let out = apply_conversation_locked(&tx, account_id, ev, identity.as_deref())?;
            tx.commit()?;
            out
        };
        for m in &out.merged {
            self.forget_conversation(&m.removed_id);
            self.repoint_conversation_refs(&m.removed_id, &m.into_id);
        }
        if let Some(c) = &out.conversation {
            self.remember_conversation(&c.id, account_id);
        }
        Ok(out)
    }

    /// Apply a message fact: dedupe, echo adoption, chat time/preview, read marker, unread.
    pub fn ingest_message(
        &self,
        account_id: &str,
        conv_remote: &str,
        alts: &[String],
        history: bool,
        msg: Message,
        preview: &str,
        fallback_title: &str,
    ) -> Result<IngestOutcome, DbError> {
        let inbox = self.inbox(account_id)?;
        let out = {
            let conn = inbox.conn.lock();
            let tx = conn.unchecked_transaction()?;
            let out = ingest_locked(
                &tx,
                account_id,
                conv_remote,
                alts,
                history,
                msg,
                preview,
                fallback_title,
                Utc::now(),
            )?;
            tx.commit()?;
            out
        };
        for m in &out.merged {
            self.forget_conversation(&m.removed_id);
            self.repoint_conversation_refs(&m.removed_id, &m.into_id);
        }
        if let Some(c) = &out.conversation {
            self.remember_conversation(&c.id, account_id);
        }
        Ok(out)
    }

    /// Apply a `message.changed` fact to the message `target` (remote id) of one chat.
    /// Re-applying a fact changes nothing.
    pub fn apply_message_change(
        &self,
        account_id: &str,
        remote: &str,
        alts: &[String],
        target: &str,
        change: &MessageChange,
    ) -> Result<ChangeOutcome, DbError> {
        let inbox = self.inbox(account_id)?;
        let conn = inbox.conn.lock();
        let Some(conversation_id) = find_conversation_id(&conn, account_id, remote, alts)? else {
            return Ok(ChangeOutcome {
                message: SendReconcile::Nothing,
                conversation: None,
            });
        };
        let tx = conn.unchecked_transaction()?;
        let out = apply_change_locked(&tx, &conversation_id, target, change)?;
        tx.commit()?;
        Ok(out)
    }

    /// Store a message the user sent from this app (status as given, no remote id yet).
    pub fn insert_message(&self, msg: &Message) -> Result<bool, DbError> {
        let account_id = self.conversation_account(&msg.conversation_id)?;
        let inbox = self.inbox(&account_id)?;
        let conn = inbox.conn.lock();
        let tx = conn.unchecked_transaction()?;
        let remote: String = tx
            .query_row(
                "SELECT remote_id FROM conversations WHERE id = ?1",
                params![msg.conversation_id],
                |r| r.get(0),
            )
            .optional()?
            .ok_or_else(|| DbError::NotFound(msg.conversation_id.clone()))?;
        let preview = message_preview(&msg.body, &msg.metadata);
        let out = ingest_locked(
            &tx,
            &account_id,
            &remote,
            &[],
            false,
            msg.clone(),
            &preview,
            "",
            Utc::now(),
        )?;
        tx.commit()?;
        Ok(out.kind == IngestKind::Inserted)
    }

    /// `chat.read`: the account read this chat elsewhere up to `read_through` (W6).
    pub fn apply_chat_read(
        &self,
        account_id: &str,
        remote: &str,
        alts: &[String],
        read_through: DateTime<Utc>,
    ) -> Result<ReadOutcome, DbError> {
        let inbox = self.inbox(account_id)?;
        let out = {
            let conn = inbox.conn.lock();
            let tx = conn.unchecked_transaction()?;
            let resolved = resolve_conversation(&tx, account_id, remote, alts)?;
            let mut changed = !resolved.merged.is_empty();
            let mut conversation = None;
            if let Some(lite) = resolved.conv {
                if sync_aliases(&tx, &lite, remote, &resolved.retired)? {
                    changed = true;
                }
                let baseline = inbox_baseline(&tx)?;
                if advance_marker(&tx, &lite, read_through, baseline)? {
                    changed = true;
                }
                if changed {
                    conversation = Some(get_conversation_locked(&tx, &lite.id)?);
                }
            }
            tx.commit()?;
            ReadOutcome {
                conversation,
                changed,
                merged: resolved.merged,
            }
        };
        for m in &out.merged {
            self.forget_conversation(&m.removed_id);
            self.repoint_conversation_refs(&m.removed_id, &m.into_id);
        }
        Ok(out)
    }

    /// The user opened/read the chat in Shuttle: move the marker to the newest message time or
    /// now, whichever is later, and clear the manual-unread flag.
    pub fn mark_conversation_read(&self, conversation_id: &str) -> Result<(), DbError> {
        let account_id = self.conversation_account(conversation_id)?;
        let inbox = self.inbox(&account_id)?;
        let conn = inbox.conn.lock();
        let tx = conn.unchecked_transaction()?;
        let lite = lite_by_id(&tx, conversation_id)?
            .ok_or_else(|| DbError::NotFound(conversation_id.to_string()))?;
        let newest_ms: Option<i64> = tx
            .query_row(
                "SELECT CAST(ROUND(MAX(unixepoch(timestamp, 'subsec')) * 1000) AS INTEGER)
                 FROM messages WHERE conversation_id = ?1",
                params![conversation_id],
                |r| r.get(0),
            )
            .optional()?
            .flatten();
        let now = Utc::now();
        let target = newest_ms
            .and_then(DateTime::<Utc>::from_timestamp_millis)
            .map_or(now, |t| t.max(now));
        let baseline = inbox_baseline(&tx)?;
        advance_marker(&tx, &lite, target, baseline)?;
        tx.commit()?;
        Ok(())
    }

    /// Manual "mark as unread": keep at least one unread until the marker moves again.
    pub fn mark_unread(&self, conversation_id: &str) -> Result<(), DbError> {
        let account_id = self.conversation_account(conversation_id)?;
        let inbox = self.inbox(&account_id)?;
        let conn = inbox.conn.lock();
        let tx = conn.unchecked_transaction()?;
        tx.execute(
            "UPDATE conversations SET metadata = json_set(metadata, '$.marked_unread', json('true')) WHERE id = ?1",
            params![conversation_id],
        )?;
        let baseline = inbox_baseline(&tx)?;
        recompute_unread(&tx, conversation_id, baseline)?;
        tx.commit()?;
        Ok(())
    }

    /// R8: the connector's `ok` reply to a send carried the remote message id.
    pub fn reconcile_send_ok(
        &self,
        account_hint: Option<&str>,
        request_id: &str,
        remote_message_id: &str,
    ) -> Result<SendReconcile, DbError> {
        let Some(account_id) = self.account_of_message(account_hint, request_id)? else {
            return Ok(SendReconcile::Nothing);
        };
        let inbox = self.inbox(&account_id)?;
        let conn = inbox.conn.lock();
        let tx = conn.unchecked_transaction()?;
        let row: Option<(String, Option<String>, String)> = tx
            .query_row(
                "SELECT conversation_id, remote_id, status FROM messages WHERE id = ?1",
                params![request_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        let Some((conversation_id, current_remote, status)) = row else {
            return Ok(SendReconcile::Nothing);
        };
        let result = match current_remote.as_deref() {
            Some(r) if r == remote_message_id => {
                if status == "pending" || status == "failed" {
                    tx.execute(
                        "UPDATE messages SET status = 'sent' WHERE id = ?1",
                        params![request_id],
                    )?;
                    SendReconcile::Updated {
                        message: get_message_locked(&tx, &conversation_id, request_id)?,
                        conversation_id,
                    }
                } else {
                    SendReconcile::Nothing
                }
            }
            Some(_) => SendReconcile::Nothing,
            None => {
                let dup: Option<String> = tx
                    .query_row(
                        "SELECT id FROM messages WHERE conversation_id = ?1 AND remote_id = ?2",
                        params![conversation_id, remote_message_id],
                        |r| r.get(0),
                    )
                    .optional()?;
                if let Some(echo_id) = dup {
                    // The echo may report a thinner reply link than the one stored when the
                    // reply was sent; keep the fuller facts on the surviving row.
                    let local: Option<String> = tx
                        .query_row(
                            "SELECT metadata FROM messages WHERE id = ?1",
                            params![request_id],
                            |r| r.get(0),
                        )
                        .optional()?;
                    let reply = local
                        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
                        .and_then(|m| m.get("reply_to").cloned());
                    if let Some(reply) = reply {
                        let raw: String = tx.query_row(
                            "SELECT metadata FROM messages WHERE id = ?1",
                            params![echo_id],
                            |r| r.get(0),
                        )?;
                        let base: Value =
                            serde_json::from_str(&raw).unwrap_or_else(|_| serde_json::json!({}));
                        let merged =
                            merge_json_objects(base.clone(), serde_json::json!({ "reply_to": reply }));
                        if merged != base {
                            tx.execute(
                                "UPDATE messages SET metadata = ?1 WHERE id = ?2",
                                params![merged.to_string(), echo_id],
                            )?;
                        }
                    }
                    tx.execute("DELETE FROM messages WHERE id = ?1", params![request_id])?;
                    SendReconcile::Removed {
                        conversation_id,
                        message_id: request_id.to_string(),
                    }
                } else {
                    tx.execute(
                        "UPDATE messages SET remote_id = ?1,
                            status = CASE WHEN status IN ('pending', 'failed') THEN 'sent' ELSE status END
                         WHERE id = ?2",
                        params![remote_message_id, request_id],
                    )?;
                    SendReconcile::Updated {
                        message: get_message_locked(&tx, &conversation_id, request_id)?,
                        conversation_id,
                    }
                }
            }
        };
        tx.commit()?;
        Ok(result)
    }

    /// The connector accepted a send but the network returned no message id: the row is sent,
    /// and its remote id arrives later with the echo (echo adoption).
    pub fn mark_send_accepted(
        &self,
        account_hint: Option<&str>,
        request_id: &str,
    ) -> Result<SendReconcile, DbError> {
        let Some(account_id) = self.account_of_message(account_hint, request_id)? else {
            return Ok(SendReconcile::Nothing);
        };
        let inbox = self.inbox(&account_id)?;
        let conn = inbox.conn.lock();
        let conversation_id: Option<String> = conn
            .query_row(
                "SELECT conversation_id FROM messages WHERE id = ?1 AND status = 'pending'",
                params![request_id],
                |r| r.get(0),
            )
            .optional()?;
        let Some(conversation_id) = conversation_id else {
            return Ok(SendReconcile::Nothing);
        };
        conn.execute(
            "UPDATE messages SET status = 'sent' WHERE id = ?1",
            params![request_id],
        )?;
        Ok(SendReconcile::Updated {
            message: get_message_locked(&conn, &conversation_id, request_id)?,
            conversation_id,
        })
    }

    /// R8: the connector answered a send with an error. Only a still-pending row fails.
    pub fn reconcile_send_error(
        &self,
        account_hint: Option<&str>,
        request_id: &str,
    ) -> Result<SendReconcile, DbError> {
        let Some(account_id) = self.account_of_message(account_hint, request_id)? else {
            return Ok(SendReconcile::Nothing);
        };
        let inbox = self.inbox(&account_id)?;
        let conn = inbox.conn.lock();
        let conversation_id: Option<String> = conn
            .query_row(
                "SELECT conversation_id FROM messages WHERE id = ?1 AND status = 'pending'",
                params![request_id],
                |r| r.get(0),
            )
            .optional()?;
        let Some(conversation_id) = conversation_id else {
            return Ok(SendReconcile::Nothing);
        };
        conn.execute(
            "UPDATE messages SET status = 'failed' WHERE id = ?1",
            params![request_id],
        )?;
        Ok(SendReconcile::Updated {
            message: get_message_locked(&conn, &conversation_id, request_id)?,
            conversation_id,
        })
    }

    fn account_of_message(
        &self,
        hint: Option<&str>,
        message_id: &str,
    ) -> Result<Option<String>, DbError> {
        if let Some(account_id) = hint {
            if let Ok(inbox) = self.inbox(account_id) {
                let found: Option<i64> = inbox
                    .conn
                    .lock()
                    .query_row(
                        "SELECT 1 FROM messages WHERE id = ?1",
                        params![message_id],
                        |r| r.get(0),
                    )
                    .optional()?;
                if found.is_some() {
                    return Ok(Some(account_id.to_string()));
                }
            }
        }
        match self.locate_message(message_id) {
            Ok((account_id, _)) => Ok(Some(account_id)),
            Err(DbError::NotFound(_)) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Repoint rows in `app.sqlite` that reference a conversation that was merged away (R7).
    fn repoint_conversation_refs(&self, from: &str, to: &str) {
        let conn = self.catalog.conn.lock();
        for sql in [
            "UPDATE chat_todos SET conversation_id = ?1 WHERE conversation_id = ?2",
            "UPDATE reminders SET conversation_id = ?1 WHERE conversation_id = ?2",
            "UPDATE chat_notes SET conversation_id = ?1 WHERE conversation_id = ?2",
            "UPDATE forwarding_rules SET source_conversation_id = ?1 WHERE source_conversation_id = ?2",
            "UPDATE forwarding_rules SET dest_conversation_id = ?1 WHERE dest_conversation_id = ?2",
            "UPDATE scheduled_messages SET source_conversation_id = ?1 WHERE source_conversation_id = ?2",
            "UPDATE scheduled_messages SET dest_conversation_id = ?1 WHERE dest_conversation_id = ?2",
        ] {
            if let Err(e) = conn.execute(sql, params![to, from]) {
                tracing::warn!("repoint conversation refs: {e}");
            }
        }
    }

    /// Recompute WhatsApp titles (chats that carry `names`) with the one title function, and
    /// keep today's contact/self rules for chats of other connectors.
    pub fn refresh_titles(&self, account_id: &str) -> Result<(), DbError> {
        let identity = self.get_account(account_id).ok().and_then(|a| a.identity);
        let contacts: std::collections::HashMap<String, String> = self
            .list_contacts(account_id)?
            .into_iter()
            .map(|c| (c.remote_id, c.display_name))
            .collect();
        let real_contact = |jid: &str| -> Option<&str> {
            contacts
                .get(jid)
                .map(|s| s.trim())
                .filter(|s| !s.is_empty() && !is_placeholder_title(s, jid))
        };
        let inbox = self.inbox(account_id)?;
        let conn = inbox.conn.lock();
        let tx = conn.unchecked_transaction()?;
        let rows: Vec<Lite> = {
            let sql = format!("{LITE_SELECT} WHERE account_id = ?1");
            let mut stmt = tx.prepare(&sql)?;
            let mapped = stmt.query_map(params![account_id], map_lite)?;
            mapped.collect::<Result<Vec<_>, _>>()?
        };
        for l in rows {
            let jids = jids_of(&l.remote_id, &l.alts);
            let contact = jids.iter().find_map(|j| real_contact(j));
            let self_chat =
                l.is_self || matches_identity(identity.as_deref(), &l.remote_id, &l.alts);
            let new_title = if let Some(names) = &l.names {
                compute_wa_title(&TitleInputs {
                    remote_id: &l.remote_id,
                    conversation_type: &l.ctype,
                    is_self: self_chat,
                    names,
                    contact_name: contact,
                    stored_title: &l.title,
                })
            } else {
                // Legacy chats: the address book wins for 1:1 chats; own chat gets "(You)".
                let mut t = contact
                    .map(str::to_string)
                    .unwrap_or_else(|| l.title.clone());
                if matches_identity(identity.as_deref(), &l.remote_id, &[]) && !t.ends_with("(You)")
                {
                    t = match real_contact(&l.remote_id) {
                        Some(c) => format!("{c} (You)"),
                        None => contacts
                            .iter()
                            .find(|(k, _)| crate::media_store::jids_same(k, &l.remote_id))
                            .map(|(_, v)| format!("{v} (You)"))
                            .unwrap_or_else(|| "Message yourself".into()),
                    };
                }
                t
            };
            if new_title != l.title {
                tx.execute(
                    "UPDATE conversations SET title = ?1, updated_at = ?2 WHERE id = ?3",
                    params![new_title, ts_string(Utc::now()), l.id],
                )?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// After a bulk sync: make previews follow the newest stored message (never moving chat time
    /// backwards, R2). Writes only rows that change.
    pub fn refresh_conversation_previews(&self, account_id: &str) -> Result<(), DbError> {
        let inbox = self.inbox(account_id)?;
        let conn = inbox.conn.lock();
        let tx = conn.unchecked_transaction()?;
        let rows: Vec<(
            String,
            Option<String>,
            Option<String>,
            String,
            String,
            String,
            PreviewSource,
            Option<PreviewSource>,
        )> = {
            let mut stmt = tx.prepare(
                "SELECT c.id, c.last_message_at, c.last_message_preview, m.body, m.timestamp, m.metadata,
                        m.direction, m.sender_id, m.sender_name,
                        c.last_message_from_me, c.last_message_sender_id, c.last_message_sender_name
                 FROM conversations c
                 JOIN messages m ON m.id = (
                    SELECT m2.id FROM messages m2 WHERE m2.conversation_id = c.id
                    ORDER BY unixepoch(m2.timestamp, 'subsec') DESC, m2.id LIMIT 1)
                 WHERE c.account_id = ?1",
            )?;
            let mapped = stmt.query_map(params![account_id], |r| {
                let from_me = r.get::<_, String>(6)? == "outbound";
                let keep = |v: Option<String>| v.filter(|x| !from_me && !x.trim().is_empty());
                let fresh = PreviewSource {
                    from_me,
                    sender_id: keep(r.get(7)?),
                    sender_name: keep(r.get(8)?),
                };
                let stored = r.get::<_, Option<i64>>(9)?.map(|n| PreviewSource {
                    from_me: n != 0,
                    sender_id: r.get(10).ok().flatten(),
                    sender_name: r.get(11).ok().flatten(),
                });
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    fresh,
                    stored,
                ))
            })?;
            mapped.collect::<Result<Vec<_>, _>>()?
        };
        for (id, last, stored_preview, body, ts, meta_raw, fresh_src, stored_src) in rows {
            let Some(msg_at) = parse_stored_datetime(&ts) else {
                continue;
            };
            let stored_at = last.as_deref().and_then(parse_stored_datetime);
            let meta: Value =
                serde_json::from_str(&meta_raw).unwrap_or_else(|_| serde_json::json!({}));
            let preview = message_preview(&body, &meta);
            let advance = stored_at.is_none_or(|s| msg_at >= s);
            let new_at = later(stored_at, Some(msg_at));
            if advance
                && (new_at != stored_at || stored_preview.as_deref() != Some(preview.as_str()))
            {
                tx.execute(
                    "UPDATE conversations SET last_message_at = ?1, last_message_preview = ?2 WHERE id = ?3",
                    params![new_at.map(ts_string), preview, id],
                )?;
                write_preview_source(&tx, &id, Some(&fresh_src))?;
            } else if stored_preview.is_none() {
                tx.execute(
                    "UPDATE conversations SET last_message_preview = ?1 WHERE id = ?2",
                    params![preview, id],
                )?;
                write_preview_source(&tx, &id, Some(&fresh_src))?;
            } else if stored_preview.as_deref() == Some(preview.as_str())
                && stored_src.as_ref() != Some(&fresh_src)
            {
                // Same text, but the author/direction was unknown or stale.
                write_preview_source(&tx, &id, Some(&fresh_src))?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Find or create a chat the user started by hand (legacy title rules).
    pub fn ensure_conversation(
        &self,
        account_id: &str,
        remote_id: &str,
        title: &str,
        conversation_type: ConversationType,
    ) -> Result<Conversation, DbError> {
        let out = self.apply_conversation_event(
            account_id,
            &ConversationEvent {
                remote_id: remote_id.to_string(),
                legacy_title: Some(title.to_string()),
                conversation_type: Some(conversation_type),
                ..Default::default()
            },
        )?;
        match out.conversation {
            Some(c) => Ok(c),
            None => self
                .get_conversation_by_remote(account_id, remote_id)?
                .ok_or_else(|| DbError::NotFound(remote_id.to_string())),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Locked implementations
// ---------------------------------------------------------------------------------------------

fn type_str(t: &ConversationType) -> &'static str {
    match t {
        ConversationType::Group => "group",
        ConversationType::Channel => "channel",
        ConversationType::Direct => "direct",
    }
}

fn insert_conversation(
    conn: &Connection,
    account_id: &str,
    remote: &str,
    title: &str,
    ctype: &ConversationType,
    metadata: &Value,
    pinned: bool,
    archived: bool,
) -> Result<String, DbError> {
    let id = Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO conversations (id, account_id, remote_id, title, conversation_type, unread_count,
         last_message_at, last_message_preview, pinned, archived, muted, metadata, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, 0, NULL, NULL, ?6, ?7, 0, ?8, ?9)",
        params![
            id,
            account_id,
            remote,
            title,
            type_str(ctype),
            pinned as i64,
            archived as i64,
            metadata.to_string(),
            ts_string(Utc::now()),
        ],
    )?;
    Ok(id)
}

fn apply_conversation_locked(
    conn: &Connection,
    account_id: &str,
    ev: &ConversationEvent,
    identity: Option<&str>,
) -> Result<ConversationOutcome, DbError> {
    let remote = ev.remote_id.as_str();
    let resolved = resolve_conversation(conn, account_id, remote, &ev.alt_remote_ids)?;
    let mut changed = !resolved.merged.is_empty();
    let incoming_names = ev.names.as_ref().map(sanitize_names);
    let self_now = ev.is_self || matches_identity(identity, remote, &ev.alt_remote_ids);

    let Some(l) = resolved.conv else {
        let ctype = ev.conversation_type.clone().unwrap_or_else(|| {
            if incoming_names.is_some() {
                jid_conversation_type(remote)
            } else {
                ConversationType::Direct
            }
        });
        let title = match &incoming_names {
            Some(names) => {
                let jids = jids_of(remote, &ev.alt_remote_ids);
                let contact = contact_name_for(conn, account_id, &jids);
                compute_wa_title(&TitleInputs {
                    remote_id: remote,
                    conversation_type: &ctype,
                    is_self: self_now,
                    names,
                    contact_name: contact.as_deref(),
                    stored_title: "",
                })
            }
            None => legacy_event_title(conn, account_id, ev, identity),
        };
        let mut meta = serde_json::json!({});
        if let Some(names) = &incoming_names {
            meta["names"] = Value::Object(names.clone());
        }
        let alts: Vec<&String> = ev
            .alt_remote_ids
            .iter()
            .filter(|a| !a.is_empty() && *a != remote)
            .collect();
        if !alts.is_empty() {
            meta["alt_remote_ids"] = serde_json::json!(alts);
        }
        if self_now && incoming_names.is_some() {
            meta["is_self"] = Value::Bool(true);
        }
        if let Some(admin_only) = ev.announce_only {
            meta["announce_only"] = Value::Bool(admin_only);
        }
        if let Some(admin) = ev.self_is_admin {
            meta["self_is_admin"] = Value::Bool(admin);
        }
        let id = insert_conversation(
            conn,
            account_id,
            remote,
            &title,
            &ctype,
            &meta,
            ev.pinned.unwrap_or(false),
            ev.archived.unwrap_or(false),
        )?;
        // Chat time and preview go through the same forward-only rule as updates.
        let lite = lite_by_id(conn, &id)?.ok_or_else(|| DbError::NotFound(id.clone()))?;
        apply_time_and_preview(conn, &lite, ev.last_message_at, ev.preview.as_deref(), None)?;
        return Ok(ConversationOutcome {
            conversation: Some(get_conversation_locked(conn, &id)?),
            changed: true,
            merged: resolved.merged,
        });
    };

    if sync_aliases(
        conn,
        &l,
        remote,
        &resolved
            .retired
            .iter()
            .chain(ev.alt_remote_ids.iter())
            .cloned()
            .collect::<Vec<_>>(),
    )? {
        changed = true;
    }
    // Names merge: a new value replaces the old one of the same key; a missing key erases nothing.
    let mut names_now = l.names.clone();
    let mut names_changed = false;
    if let Some(incoming) = &incoming_names {
        let merged = names_now.get_or_insert_with(|| {
            names_changed = true;
            Map::new()
        });
        for (k, v) in incoming {
            if merged.get(k) != Some(v) {
                merged.insert(k.clone(), v.clone());
                names_changed = true;
            }
        }
    }
    let self_new = l.is_self || (self_now && incoming_names.is_some());
    let ctype_new = match (&ev.conversation_type, incoming_names.is_some()) {
        (Some(t), true) => t.clone(),
        _ => l.ctype.clone(),
    };
    let title_new = match (&incoming_names, &names_now) {
        (Some(_), Some(names)) => {
            let mut all: Vec<String> = ev.alt_remote_ids.clone();
            all.extend(l.alts.iter().cloned());
            let probe = jids_of(remote, &all);
            let contact = contact_name_for(conn, account_id, &probe);
            compute_wa_title(&TitleInputs {
                remote_id: remote,
                conversation_type: &ctype_new,
                is_self: self_new,
                names,
                contact_name: contact.as_deref(),
                stored_title: &l.title,
            })
        }
        _ => {
            let incoming = legacy_event_title(conn, account_id, ev, identity);
            pick_conversation_title(&l.title, &incoming, remote, ev.history)
        }
    };
    let pinned_new = ev.pinned.unwrap_or(l.pinned);
    let archived_new = ev.archived.unwrap_or(l.archived);
    let announce_new = ev.announce_only.or(l.announce_only);
    let admin_new = ev.self_is_admin.or(l.self_is_admin);
    let send_changed = announce_new != l.announce_only || admin_new != l.self_is_admin;

    if title_new != l.title
        || ctype_new != l.ctype
        || pinned_new != l.pinned
        || archived_new != l.archived
    {
        conn.execute(
            "UPDATE conversations SET title = ?1, conversation_type = ?2, pinned = ?3, archived = ?4,
             updated_at = ?5 WHERE id = ?6",
            params![
                title_new,
                type_str(&ctype_new),
                pinned_new as i64,
                archived_new as i64,
                ts_string(Utc::now()),
                l.id
            ],
        )?;
        changed = true;
    }
    if names_changed || (self_new && !l.is_self) || send_changed {
        let mut sql = String::from("UPDATE conversations SET metadata = json_set(metadata");
        let mut vals: Vec<String> = Vec::new();
        if names_changed {
            sql.push_str(", '$.names', json(?)");
            vals.push(Value::Object(names_now.clone().unwrap_or_default()).to_string());
        }
        if self_new && !l.is_self {
            sql.push_str(", '$.is_self', json('true')");
        }
        if let Some(v) = announce_new.filter(|_| send_changed) {
            sql.push_str(", '$.announce_only', json(?)");
            vals.push(Value::Bool(v).to_string());
        }
        if let Some(v) = admin_new.filter(|_| send_changed) {
            sql.push_str(", '$.self_is_admin', json(?)");
            vals.push(Value::Bool(v).to_string());
        }
        sql.push_str(") WHERE id = ?");
        vals.push(l.id.clone());
        conn.execute(&sql, rusqlite::params_from_iter(vals.iter()))?;
        changed = true;
    }
    if apply_time_and_preview(conn, &l, ev.last_message_at, ev.preview.as_deref(), None)? {
        changed = true;
    }
    let conversation = if changed {
        Some(get_conversation_locked(conn, &l.id)?)
    } else {
        None
    };
    Ok(ConversationOutcome {
        conversation,
        changed,
        merged: resolved.merged,
    })
}

/// R2: chat time = max(stored, incoming); preview replaced only by something at least as new as
/// the stored chat time, or when none is stored. Writes only when something changes.
///
/// `source` is the message the incoming preview describes, when the caller has it (a stored
/// message). It is written with the preview, and also when the preview text is unchanged but
/// the stored author/direction is unknown or different (an equal text from another sender).
/// A preview without a source (conversation event) leaves the source unknown.
fn apply_time_and_preview(
    conn: &Connection,
    l: &Lite,
    incoming_at: Option<DateTime<Utc>>,
    incoming_preview: Option<&str>,
    source: Option<&PreviewSource>,
) -> Result<bool, DbError> {
    let new_at = later(l.last_at, incoming_at);
    let at_changed = new_at != l.last_at;
    let preview_in = incoming_preview.filter(|p| !p.is_empty());
    let adopts = match preview_in {
        Some(_) => {
            let fresh = match (incoming_at, l.last_at) {
                (_, None) => true,
                (Some(t), Some(s)) => t >= s,
                (None, Some(_)) => false,
            };
            l.preview.is_none() || fresh
        }
        None => false,
    };
    let take_preview = adopts && l.preview.as_deref() != preview_in;
    // `Some(x)` = rewrite the source columns to `x` (None = unknown).
    let new_source: Option<Option<&PreviewSource>> = if take_preview {
        Some(source)
    } else if adopts && source.is_some() && l.source.as_ref() != source {
        Some(source)
    } else {
        None
    };
    if !at_changed && !take_preview && new_source.is_none() {
        return Ok(false);
    }
    conn.execute(
        "UPDATE conversations SET
            last_message_at = COALESCE(?1, last_message_at),
            last_message_preview = COALESCE(?2, last_message_preview),
            updated_at = ?3
         WHERE id = ?4",
        params![
            if at_changed {
                new_at.map(ts_string)
            } else {
                None
            },
            if take_preview { incoming_preview } else { None },
            ts_string(Utc::now()),
            l.id
        ],
    )?;
    if let Some(src) = new_source {
        write_preview_source(conn, &l.id, src)?;
    }
    Ok(true)
}

/// Store (or clear, with `None`) the direction and author the chat's preview belongs to.
fn write_preview_source(
    conn: &Connection,
    conv_id: &str,
    src: Option<&PreviewSource>,
) -> Result<(), DbError> {
    conn.execute(
        "UPDATE conversations SET last_message_from_me = ?1, last_message_sender_id = ?2,
            last_message_sender_name = ?3 WHERE id = ?4",
        params![
            src.map(|x| x.from_me as i64),
            src.and_then(|x| x.sender_id.clone()),
            src.and_then(|x| x.sender_name.clone()),
            conv_id
        ],
    )?;
    Ok(())
}

const MESSAGE_SELECT: &str = "SELECT id, conversation_id, remote_id, sender_id, sender_name,
        direction, body, timestamp, status, metadata, starred, pinned FROM messages";

fn newest_message_locked(conn: &Connection, conv_id: &str) -> Result<Option<Message>, DbError> {
    let sql = format!(
        "{MESSAGE_SELECT} WHERE conversation_id = ?1
         ORDER BY unixepoch(timestamp, 'subsec') DESC, id LIMIT 1"
    );
    conn.query_row(&sql, params![conv_id], map_message_row)
        .optional()
        .map_err(DbError::from)
}

/// Is the chat's preview taken from `msg`? Only then does changing `msg` change the preview.
fn preview_comes_from(conn: &Connection, l: &Lite, msg: &Message) -> Result<bool, DbError> {
    let newest = newest_message_locked(conn, &l.id)?;
    Ok(newest.is_some_and(|n| n.id == msg.id)
        && l.last_at.is_none_or(|t| msg.timestamp >= t))
}

/// Re-derive the preview from the newest stored message. Chat time never moves (R2).
fn rederive_preview(conn: &Connection, l: &Lite) -> Result<Conversation, DbError> {
    let newest = newest_message_locked(conn, &l.id)?;
    let preview = newest.as_ref().map(|m| message_preview(&m.body, &m.metadata));
    conn.execute(
        "UPDATE conversations SET last_message_preview = ?1, updated_at = ?2 WHERE id = ?3",
        params![preview, ts_string(Utc::now()), l.id],
    )?;
    write_preview_source(
        conn,
        &l.id,
        newest.as_ref().map(PreviewSource::of_message).as_ref(),
    )?;
    get_conversation_locked(conn, &l.id)
}

/// Replace a placeholder body with the caption a later report of the same message brought.
/// Replace a placeholder body with the caption a later report of the same message brought, and
/// re-derive the preview when that message is the chat's newest.
fn adopt_caption(
    conn: &Connection,
    lite: &Lite,
    message_id: &str,
    body: &str,
) -> Result<(), DbError> {
    conn.execute(
        "UPDATE messages SET body = ?1 WHERE id = ?2",
        params![body, message_id],
    )?;
    if newest_message_locked(conn, &lite.id)?.is_some_and(|n| n.id == message_id) {
        rederive_preview(conn, lite)?;
    }
    Ok(())
}

/// Repair bodies stored as the `unknown` placeholder by an older build: the caption of the same
/// message is in its metadata. A message with no caption loses the placeholder and keeps an
/// empty body, which is how the core stores a message that has no text of its own.
/// Idempotent: nothing matches once repaired.
pub(crate) fn repair_placeholder_bodies(conn: &Connection) -> rusqlite::Result<()> {
    let mut convs: Vec<String> = {
        let mut stmt = conn.prepare(
            "SELECT DISTINCT conversation_id FROM messages WHERE lower(trim(body)) = 'unknown'",
        )?;
        let mapped = stmt.query_map([], |r| r.get::<_, String>(0))?;
        mapped.collect::<Result<Vec<String>, _>>()?
    };
    if convs.is_empty() {
        return Ok(());
    }
    conn.execute(
        "UPDATE messages SET body = CASE
            WHEN json_type(metadata, '$.text') = 'text'
                 AND lower(trim(json_extract(metadata, '$.text'))) NOT IN ('', 'unknown')
            THEN json_extract(metadata, '$.text')
            ELSE '' END
         WHERE lower(trim(body)) = 'unknown'",
        [],
    )?;
    convs.sort();
    convs.dedup();
    for id in convs {
        if let Ok(Some(lite)) = lite_by_id(conn, &id) {
            let _ = rederive_preview(conn, &lite);
        }
    }
    Ok(())
}

fn apply_change_locked(
    conn: &Connection,
    conv_id: &str,
    target: &str,
    change: &MessageChange,
) -> Result<ChangeOutcome, DbError> {
    let nothing = || ChangeOutcome {
        message: SendReconcile::Nothing,
        conversation: None,
    };
    if target.is_empty() {
        return Ok(nothing());
    }
    let lite = lite_by_id(conn, conv_id)?.ok_or_else(|| DbError::NotFound(conv_id.into()))?;
    if *change == MessageChange::DeletedForMe {
        conn.execute(
            "INSERT OR IGNORE INTO deleted_messages (conversation_id, remote_id) VALUES (?1, ?2)",
            params![conv_id, target],
        )?;
    }
    let sql = format!("{MESSAGE_SELECT} WHERE conversation_id = ?1 AND remote_id = ?2");
    let Some(msg) = conn
        .query_row(&sql, params![conv_id, target], map_message_row)
        .optional()?
    else {
        return Ok(nothing());
    };
    let shown_in_preview = preview_comes_from(conn, &lite, &msg)?;
    let clear_reactions = || {
        conn.execute(
            "DELETE FROM message_reactions WHERE conversation_id = ?1 AND target_remote_id = ?2",
            params![conv_id, target],
        )
    };
    match change {
        MessageChange::DeletedForMe => {
            conn.execute("DELETE FROM messages WHERE id = ?1", params![msg.id])?;
            clear_reactions()?;
        }
        MessageChange::Revoked { at } => {
            if is_revoked(&msg.metadata) {
                return Ok(nothing());
            }
            conn.execute(
                "UPDATE messages SET body = '', metadata = ?1 WHERE id = ?2",
                params![serde_json::json!({ REVOKED_AT: ts_string(*at) }).to_string(), msg.id],
            )?;
            clear_reactions()?;
        }
        MessageChange::Edited { text, at } => {
            let stored_at = msg
                .metadata
                .get(EDITED_AT)
                .and_then(|v| v.as_str())
                .and_then(parse_stored_datetime);
            if is_revoked(&msg.metadata) || stored_at.is_some_and(|s| s >= *at) {
                return Ok(nothing());
            }
            let meta = merge_json_objects(
                msg.metadata.clone(),
                serde_json::json!({ EDITED_AT: ts_string(*at) }),
            );
            conn.execute(
                "UPDATE messages SET body = ?1, metadata = ?2 WHERE id = ?3",
                params![text, meta.to_string(), msg.id],
            )?;
        }
    }
    let conversation = if shown_in_preview {
        Some(rederive_preview(conn, &lite)?)
    } else {
        None
    };
    let message = match change {
        MessageChange::DeletedForMe => SendReconcile::Removed {
            conversation_id: conv_id.to_string(),
            message_id: msg.id,
        },
        _ => SendReconcile::Updated {
            message: get_message_locked(conn, conv_id, &msg.id)?,
            conversation_id: conv_id.to_string(),
        },
    };
    Ok(ChangeOutcome {
        message,
        conversation,
    })
}

#[allow(clippy::too_many_arguments)]
fn ingest_locked(
    conn: &Connection,
    account_id: &str,
    conv_remote: &str,
    alts: &[String],
    history: bool,
    mut msg: Message,
    preview: &str,
    fallback_title: &str,
    now: DateTime<Utc>,
) -> Result<IngestOutcome, DbError> {
    let resolved = resolve_conversation(conn, account_id, conv_remote, alts)?;
    let merged = resolved.merged;
    let mut lite = match resolved.conv {
        Some(l) => l,
        None => {
            let wa = is_wa_jid(conv_remote);
            // R3: a message never names a chat after its sender on WhatsApp.
            let title = if wa {
                placeholder_title(conv_remote)
            } else {
                fallback_title.to_string()
            };
            let ctype = if wa {
                jid_conversation_type(conv_remote)
            } else {
                ConversationType::Direct
            };
            let mut meta = serde_json::json!({});
            if wa {
                meta["names"] = serde_json::json!({});
            }
            let alt_list: Vec<&String> = alts
                .iter()
                .filter(|a| !a.is_empty() && *a != conv_remote)
                .collect();
            if !alt_list.is_empty() {
                meta["alt_remote_ids"] = serde_json::json!(alt_list);
            }
            let id = insert_conversation(
                conn,
                account_id,
                conv_remote,
                &title,
                &ctype,
                &meta,
                false,
                false,
            )?;
            lite_by_id(conn, &id)?.ok_or_else(|| DbError::NotFound(id))?
        }
    };
    let mut touched = !merged.is_empty();
    if sync_aliases(
        conn,
        &lite,
        conv_remote,
        &resolved
            .retired
            .iter()
            .chain(alts.iter())
            .cloned()
            .collect::<Vec<_>>(),
    )? {
        touched = true;
        lite = lite_by_id(conn, &lite.id)?.unwrap_or(lite);
    }
    msg.conversation_id = lite.id.clone();
    let duplicate = |touched: bool,
                     conn: &Connection,
                     lite: &Lite,
                     merged: Vec<MergedAway>|
     -> Result<IngestOutcome, DbError> {
        Ok(IngestOutcome {
            kind: IngestKind::Duplicate,
            conversation: if touched {
                Some(get_conversation_locked(conn, &lite.id)?)
            } else {
                None
            },
            message: None,
            merged,
            notify_eligible: false,
            emit_new: false,
        })
    };

    if let Some(rid) = msg.remote_id.clone() {
        let deleted_for_me: Option<i64> = conn
            .query_row(
                "SELECT 1 FROM deleted_messages WHERE conversation_id = ?1 AND remote_id = ?2",
                params![lite.id, rid],
                |r| r.get(0),
            )
            .optional()?;
        if deleted_for_me.is_some() {
            return duplicate(touched, conn, &lite, merged);
        }
        // Known row: only merge genuinely new metadata (no UPDATE otherwise).
        let existing: Option<(String, String, Option<String>, String)> = conn
            .query_row(
                "SELECT id, metadata, sender_id, body FROM messages WHERE conversation_id = ?1 AND remote_id = ?2",
                params![lite.id, rid],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()?;
        if let Some((existing_id, raw, stored_sender, stored_body)) = existing {
            // Older builds stored a group message under the group's own JID, so its author
            // was lost. A later report that names a real person corrects that (and only that).
            if msg.direction == MessageDirection::Inbound
                && lite.ctype == ConversationType::Group
                && sender_is_placeholder(stored_sender.as_deref(), &lite.remote_id)
                && msg.sender_id.as_deref().is_some_and(|s| {
                    !s.is_empty() && !s.ends_with("@g.us") && s != lite.remote_id
                })
            {
                conn.execute(
                    "UPDATE messages SET sender_id = ?1, sender_name = ?2 WHERE id = ?3",
                    params![msg.sender_id, msg.sender_name, existing_id],
                )?;
            }
            let base: Value = serde_json::from_str(&raw).unwrap_or_else(|_| serde_json::json!({}));
            let merged_meta = merge_json_objects(base.clone(), msg.metadata.clone());
            // A tombstone never gets the deleted message's media or text facts back.
            if merged_meta != base && !is_revoked(&base) {
                conn.execute(
                    "UPDATE messages SET metadata = ?1 WHERE id = ?2",
                    params![merged_meta.to_string(), existing_id],
                )?;
            }
            // A media message is often reported before its caption arrives, so the stored body
            // can be the placeholder `[video]` (or `unknown` from an older build) while the real
            // text is already in the metadata. Take the caption from that later report.
            if !is_revoked(&base)
                && is_placeholder_body(&stored_body)
                && !is_placeholder_body(&msg.body)
            {
                adopt_caption(conn, &lite, &existing_id, &msg.body)?;
                return duplicate(true, conn, &lite, merged);
            }
            return duplicate(touched, conn, &lite, merged);
        }
        // Echo adoption (R8): an outbound message we already stored without a remote id.
        if msg.direction == MessageDirection::Outbound {
            let window = Duration::seconds(ECHO_WINDOW_SECS);
            let mut best: Option<(i64, String)> = None;
            let mut stmt = conn.prepare(
                "SELECT id, conversation_id, remote_id, sender_id, sender_name, direction, body, timestamp,
                        status, metadata, starred, pinned
                 FROM messages WHERE conversation_id = ?1 AND direction = 'outbound' AND remote_id IS NULL
                 ORDER BY timestamp DESC LIMIT 25",
            )?;
            let rows = stmt.query_map(params![lite.id], map_message_row)?;
            for row in rows.flatten() {
                let diff = (row.timestamp - msg.timestamp).num_seconds().abs();
                if row.timestamp - msg.timestamp <= window
                    && msg.timestamp - row.timestamp <= window
                    && bodies_match(&row, &msg)
                    && best.as_ref().is_none_or(|(d, _)| diff < *d)
                {
                    best = Some((diff, row.id));
                }
            }
            drop(stmt);
            if let Some((_, local_id)) = best {
                conn.execute(
                    "UPDATE messages SET remote_id = ?1,
                        status = CASE WHEN status = 'pending' THEN 'sent' ELSE status END
                     WHERE id = ?2",
                    params![rid, local_id],
                )?;
                let adopted = get_message_locked(conn, &lite.id, &local_id)?;
                return Ok(IngestOutcome {
                    kind: IngestKind::Adopted,
                    conversation: Some(get_conversation_locked(conn, &lite.id)?),
                    message: Some(adopted),
                    merged,
                    notify_eligible: false,
                    emit_new: false,
                });
            }
        }
    }

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
    if conn.changes() == 0 {
        return duplicate(touched, conn, &lite, merged);
    }

    let baseline = inbox_baseline(conn)?;
    let marker_before = effective_marker(lite.read_through, baseline, lite.unread_keep);
    apply_time_and_preview(
        conn,
        &lite,
        Some(msg.timestamp),
        Some(preview),
        Some(&PreviewSource::of_message(&msg)),
    )?;
    let mut notify_eligible = false;
    match msg.direction {
        MessageDirection::Outbound => {
            // R4: sending (from any device, any path) reads the chat up to that message.
            advance_marker(conn, &lite, msg.timestamp, baseline)?;
        }
        MessageDirection::Inbound => {
            let after_marker = marker_before.is_none_or(|m| msg.timestamp > m);
            if after_marker {
                recompute_unread(conn, &lite.id, baseline)?;
                notify_eligible =
                    !history && now - msg.timestamp <= Duration::seconds(NOTIFY_MAX_AGE_SECS);
            }
        }
    }
    let emit_new = !history || baseline.is_none_or(|b| msg.timestamp > b);
    Ok(IngestOutcome {
        kind: IngestKind::Inserted,
        conversation: Some(get_conversation_locked(conn, &lite.id)?),
        message: Some(msg),
        merged,
        notify_eligible,
        emit_new,
    })
}

/// A stored sender that is not a person: missing, a group JID, or the chat's own JID.
fn sender_is_placeholder(stored: Option<&str>, chat_remote: &str) -> bool {
    match stored {
        None => true,
        Some(s) => s.is_empty() || s.ends_with("@g.us") || (s == chat_remote && chat_remote.ends_with("@g.us")),
    }
}

// ---------------------------------------------------------------------------------------------
// Conversation listing (sorted and paginated in SQL, per inbox)
// ---------------------------------------------------------------------------------------------

const LAST_KEY: &str = "unixepoch(CASE WHEN substr(c.last_message_at, 1, 5) IN ('0001-', '0000-') THEN NULL ELSE c.last_message_at END)";

fn list_filter(
    archived_only: bool,
    workspace_id: Option<&str>,
    account_workspace: Option<&str>,
    priority_group: Option<&str>,
) -> (String, Vec<rusqlite::types::Value>) {
    use rusqlite::types::Value as Sql;
    let mut sql = String::from("c.archived = ? AND c.remote_id NOT LIKE 'status@%'");
    let mut vals = vec![Sql::Integer(archived_only as i64)];
    if let Some(ws) = workspace_id {
        sql.push_str(" AND COALESCE(c.workspace_id, ?, 'default') = ?");
        vals.push(match account_workspace {
            Some(w) => Sql::Text(w.to_string()),
            None => Sql::Null,
        });
        vals.push(Sql::Text(ws.to_string()));
    }
    if let Some(pg) = priority_group {
        sql.push_str(" AND c.priority_group = ?");
        vals.push(Sql::Text(pg.to_string()));
    }
    (sql, vals)
}

impl Database {
    pub fn list_conversations(
        &self,
        account_id: Option<&str>,
        workspace_id: Option<&str>,
        priority_group: Option<&str>,
        archived_only: bool,
        offset: i64,
        limit: i64,
    ) -> Result<Vec<Conversation>, DbError> {
        let off = offset.max(0);
        let lim = limit.max(1);
        let accounts = match account_id {
            Some(aid) => vec![self.get_account(aid)?],
            None => self.list_accounts()?,
        };
        let single = account_id.is_some();
        let mut all: Vec<Conversation> = Vec::new();
        for account in accounts {
            let inbox = self.inbox(&account.id)?;
            let conn = inbox.conn.lock();
            let (cond, mut vals) = list_filter(
                archived_only,
                workspace_id,
                account.workspace_id.as_deref(),
                priority_group,
            );
            let sql = format!(
                "{CONV_SELECT} WHERE {cond}
                 ORDER BY c.pinned DESC, {LAST_KEY} DESC, c.title COLLATE NOCASE ASC, c.id ASC
                 LIMIT ? OFFSET ?"
            );
            // One inbox: page in SQL. Several: the first offset+limit rows of each are enough.
            vals.push(rusqlite::types::Value::Integer(if single {
                lim
            } else {
                off + lim
            }));
            vals.push(rusqlite::types::Value::Integer(if single {
                off
            } else {
                0
            }));
            let mut stmt = conn.prepare(&sql)?;
            let rows = stmt.query_map(
                rusqlite::params_from_iter(vals.iter()),
                map_conversation_row,
            )?;
            for row in rows {
                all.push(row?);
            }
        }
        for c in &all {
            self.remember_conversation(&c.id, &c.account_id);
        }
        if single {
            return Ok(all);
        }
        all.sort_by(compare_conversations);
        Ok(all
            .into_iter()
            .skip(off as usize)
            .take(lim as usize)
            .collect())
    }

    pub fn count_conversations(
        &self,
        account_id: Option<&str>,
        workspace_id: Option<&str>,
        priority_group: Option<&str>,
        archived_only: bool,
    ) -> Result<i64, DbError> {
        let accounts = match account_id {
            Some(aid) => vec![self.get_account(aid)?],
            None => self.list_accounts()?,
        };
        let mut total = 0_i64;
        for account in accounts {
            let inbox = self.inbox(&account.id)?;
            let conn = inbox.conn.lock();
            let (cond, vals) = list_filter(
                archived_only,
                workspace_id,
                account.workspace_id.as_deref(),
                priority_group,
            );
            let sql = format!("SELECT COUNT(*) FROM conversations c WHERE {cond}");
            let n: i64 =
                conn.query_row(&sql, rusqlite::params_from_iter(vals.iter()), |r| r.get(0))?;
            total += n;
        }
        Ok(total)
    }
}

/// Row counts used only to bucket diagnostics; never exposes ids, names or content.
#[derive(Debug, Clone, Copy, Default)]
pub struct DataProfile {
    pub accounts: u64,
    pub conversations: u64,
    pub messages: u64,
    pub largest_account_messages: u64,
}

impl Database {
    pub fn data_profile(&self) -> Result<DataProfile, DbError> {
        let accounts = self.list_accounts()?;
        let mut profile = DataProfile {
            accounts: accounts.len() as u64,
            ..Default::default()
        };
        for account in accounts {
            let inbox = self.inbox(&account.id)?;
            let conn = inbox.conn.lock();
            let convs: i64 = conn.query_row("SELECT COUNT(*) FROM conversations", [], |r| r.get(0))?;
            let msgs: i64 = conn.query_row("SELECT COUNT(*) FROM messages", [], |r| r.get(0))?;
            profile.conversations += convs.max(0) as u64;
            profile.messages += msgs.max(0) as u64;
            profile.largest_account_messages = profile.largest_account_messages.max(msgs.max(0) as u64);
        }
        Ok(profile)
    }
}
