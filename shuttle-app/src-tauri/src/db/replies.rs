//! Replies: a message that answers an older one.
//!
//! A connector reports the link as a fact in the message's metadata, `reply_to`:
//! `{ remote_id, sender?, snippet?, media_type?, from_me? }`. The same shape is stored on a
//! reply sent from this app (plus `sender_name`). Nothing here writes chat rows. When a
//! message is listed or emitted, the link is resolved against the stored original into a
//! [`ReplyPreview`]: the quote is built from the original when it is stored, else from the
//! snippet the connector gave (and is then not a link).

use super::*;
use serde_json::Value;

const SNIPPET_CHARS: usize = 160;

/// Remote id of the message `meta` replies to.
pub fn reply_remote_id(meta: &Value) -> Option<String> {
    let id = meta.get("reply_to")?.get("remote_id")?;
    let s = match id {
        Value::String(s) => s.trim().to_string(),
        Value::Number(n) => n.to_string(),
        _ => return None,
    };
    (!s.is_empty()).then_some(s)
}

fn clip(text: &str) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= SNIPPET_CHARS {
        return flat;
    }
    let mut out: String = flat.chars().take(SNIPPET_CHARS - 1).collect();
    out.push('…');
    out
}

fn media_kind(meta: &Value) -> Option<String> {
    meta.get("media_type")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty() && s != "text")
}

fn str_field<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key)
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

/// Caption or text of a message, without the placeholders stored for captionless media
/// (`[image]`, or the file name standing in for a document's caption).
fn quote_text(body: &str, kind: Option<&str>, filename: Option<&str>) -> String {
    let t = body.trim();
    if t.is_empty() {
        return String::new();
    }
    if kind.is_some() {
        if t.starts_with('[') && t.ends_with(']') {
            return String::new();
        }
        if filename.is_some_and(|f| f == t) {
            return String::new();
        }
    }
    clip(t)
}

/// The quote block for a reply. `original` is the stored message it answers, if any. The
/// preview carries the text and the media kind; the UI words the media label ("📷 Photo").
pub fn build_preview(
    remote_id: &str,
    fact: &Value,
    original: Option<&Message>,
    sender_label: Option<&str>,
) -> ReplyPreview {
    if let Some(m) = original {
        let from_me = m.direction == MessageDirection::Outbound;
        let kind = media_kind(&m.metadata);
        let image_like = matches!(
            kind.as_deref(),
            Some("image") | Some("photo") | Some("sticker") | Some("gif")
        );
        let thumb = str_field(&m.metadata, "thumbnail_path")
            .or_else(|| {
                if image_like {
                    str_field(&m.metadata, "media_path")
                } else {
                    None
                }
            })
            .map(str::to_string);
        let filename = str_field(&m.metadata, "filename").map(str::to_string);
        let name = if from_me {
            Some("You".to_string())
        } else {
            m.sender_name
                .clone()
                .filter(|n| !n.trim().is_empty())
                .or_else(|| sender_label.map(str::to_string))
        };
        return ReplyPreview {
            remote_id: remote_id.to_string(),
            message_id: Some(m.id.clone()),
            sender_name: name,
            from_me,
            snippet: quote_text(&m.body, kind.as_deref(), filename.as_deref()),
            media_type: kind,
            filename,
            thumbnail_path: thumb,
            available: true,
        };
    }
    let from_me = fact
        .get("from_me")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let kind = media_kind(fact);
    let filename = str_field(fact, "filename").map(str::to_string);
    ReplyPreview {
        remote_id: remote_id.to_string(),
        message_id: None,
        sender_name: if from_me {
            Some("You".to_string())
        } else {
            str_field(fact, "sender_name").map(str::to_string)
        },
        from_me,
        snippet: quote_text(
            str_field(fact, "snippet").unwrap_or(""),
            kind.as_deref(),
            filename.as_deref(),
        ),
        media_type: kind,
        filename,
        thumbnail_path: None,
        available: false,
    }
}

const SELECT: &str = "SELECT id, conversation_id, remote_id, sender_id, sender_name, direction, body, timestamp, status, metadata, starred, pinned
             FROM messages WHERE conversation_id = ?1 AND remote_id = ?2";

/// Fill `reply` on every message of `msgs` that answers another one (one conversation).
pub(super) fn resolve_in_conversation(
    conn: &Connection,
    conversation_id: &str,
    msgs: &mut [Message],
) -> Result<(), DbError> {
    let wanted: Vec<usize> = (0..msgs.len())
        .filter(|&i| reply_remote_id(&msgs[i].metadata).is_some())
        .collect();
    if wanted.is_empty() {
        return Ok(());
    }
    // Originals already in the list need no query; the rest are fetched once each.
    let mut known: HashMap<String, Message> = HashMap::new();
    for m in msgs.iter() {
        if let Some(rid) = &m.remote_id {
            known.entry(rid.clone()).or_insert_with(|| {
                let mut slim = m.clone();
                slim.reactions = Vec::new();
                slim
            });
        }
    }
    let mut stmt = conn.prepare_cached(SELECT)?;
    for i in wanted {
        let rid = reply_remote_id(&msgs[i].metadata).unwrap_or_default();
        if !known.contains_key(&rid) {
            if let Some(found) = stmt
                .query_row(params![conversation_id, rid], map_message_row)
                .optional()?
            {
                known.insert(rid.clone(), found);
            }
        }
        let fact = msgs[i].metadata.get("reply_to").cloned().unwrap_or_default();
        let preview = build_preview(&rid, &fact, known.get(&rid), None);
        msgs[i].reply = Some(preview);
    }
    Ok(())
}

impl Database {
    /// Fill `reply` on a message that answers another one (live events, sends).
    pub fn resolve_reply(&self, msg: &mut Message) {
        if reply_remote_id(&msg.metadata).is_none() {
            return;
        }
        let Ok(account_id) = self.conversation_account(&msg.conversation_id) else {
            return;
        };
        let Ok(inbox) = self.inbox(&account_id) else {
            return;
        };
        let conn = inbox.conn.lock();
        let _ = resolve_in_conversation(&conn, &msg.conversation_id.clone(), std::slice::from_mut(msg));
    }

    /// The `reply_to` metadata for a reply to the stored message `message_id` (its local id),
    /// plus the target's remote id and author JID for the connector.
    pub fn reply_target(&self, message_id: &str) -> Result<ReplyTarget, DbError> {
        let (account_id, conversation_id) = self.locate_message(message_id)?;
        let inbox = self.inbox(&account_id)?;
        let conn = inbox.conn.lock();
        let m = get_message_locked(&conn, &conversation_id, message_id)?;
        let Some(remote_id) = m.remote_id.clone().filter(|r| !r.is_empty()) else {
            return Err(DbError::InvalidInput(
                "That message is still sending: reply once it has been sent".into(),
            ));
        };
        let from_me = m.direction == MessageDirection::Outbound;
        let sender = if from_me {
            None
        } else {
            m.sender_id.clone().filter(|s| !s.is_empty())
        };
        let preview = build_preview(&remote_id, &Value::Null, Some(&m), None);
        let mut meta = serde_json::json!({
            "remote_id": remote_id,
            "from_me": from_me,
            "snippet": preview.snippet,
        });
        if let Some(s) = &sender {
            meta["sender"] = Value::String(s.clone());
        }
        if let Some(n) = preview.sender_name.as_ref().filter(|_| !from_me) {
            meta["sender_name"] = Value::String(n.clone());
        }
        if let Some(k) = &preview.media_type {
            meta["media_type"] = Value::String(k.clone());
        }
        if let Some(f) = &preview.filename {
            meta["filename"] = Value::String(f.clone());
        }
        Ok(ReplyTarget {
            conversation_id,
            remote_id,
            sender,
            meta,
        })
    }
}

/// What a send needs to know about the message it answers.
#[derive(Debug, Clone)]
pub struct ReplyTarget {
    pub conversation_id: String,
    pub remote_id: String,
    /// Author JID of the target when it is not this account.
    pub sender: Option<String>,
    /// Value stored as `metadata.reply_to` on the outgoing message.
    pub meta: Value,
}
