//! Forwarding stored messages to other chats, on any account. A forward names source messages,
//! never text: each one is resolved here into what can be sent again (its text, or its media
//! file with the original caption), and the result of every (target, source) pair is reported.

use crate::db::{is_revoked, Database};
use crate::models::{Message, ScheduleMessageDraft};
use std::future::Future;
use std::path::Path;

/// A chat to forward into. Targets may belong to any account.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub struct ForwardTarget {
    pub account_id: String,
    pub conversation_id: String,
}

/// One source message as a forward sends it.
#[derive(Debug, Clone, PartialEq)]
pub enum ForwardItem {
    Text(String),
    Media(ForwardMedia),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ForwardMedia {
    pub kind: String,
    pub caption: Option<String>,
    pub filename: Option<String>,
    pub mime: Option<String>,
    /// The cached file, when there is one. A missing file is downloaded before sending.
    pub path: Option<String>,
}

/// What happened to one source message for one target.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ForwardResult {
    pub account_id: String,
    pub conversation_id: String,
    pub message_id: String,
    #[serde(flatten)]
    pub outcome: ForwardOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ForwardOutcome {
    Sent,
    Scheduled,
    /// Permanent: retrying will not help.
    NotForwardable { reason: String },
    Failed { error: String },
}

const MEDIA_KINDS: [&str; 9] = [
    "image", "photo", "gif", "video", "audio", "ptt", "document", "sticker", "file",
];

fn str_meta<'a>(msg: &'a Message, key: &str) -> Option<&'a str> {
    msg.metadata
        .get(key)
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

/// The bracketed stand-ins Shuttle stores for media without a caption. A caption the sender
/// wrote in brackets, such as `[Please review]`, is not one of them.
fn is_placeholder(body: &str) -> bool {
    const PLACEHOLDERS: &[&str] = &[
        "image", "photo", "gif", "video", "audio", "voice message", "document", "file", "sticker",
        "location", "poll", "contact", "media",
    ];
    body.strip_prefix('[')
        .and_then(|b| b.strip_suffix(']'))
        .is_some_and(|inner| PLACEHOLDERS.contains(&inner.trim().to_lowercase().as_str()))
}

/// A long run of base64 characters with no spaces: the file hash WhatsApp stores as the body
/// of a media message without a caption. Mirrors `looksLikeGarbage` in `messageMedia.ts`.
fn is_base64_blob(body: &str) -> bool {
    let data = body.trim_end_matches('=');
    body.len() > 32
        && body.len() - data.len() <= 2
        && data.chars().all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '/')
}

/// Body text of a media message when it is a real caption, not a placeholder, the file name,
/// the sender's name, a mime type or a media key.
fn caption_of(msg: &Message) -> Option<String> {
    let body = msg.body.trim();
    let is_noise = body.is_empty()
        || is_placeholder(body)
        || Some(body) == str_meta(msg, "filename")
        || Some(body) == msg.sender_name.as_deref().map(str::trim)
        || (body.len() >= 16 && body.chars().all(|c| c.is_ascii_hexdigit()))
        || is_base64_blob(body)
        || (!body.contains(char::is_whitespace)
            && body.split_once('/').is_some_and(|(a, b)| {
                !a.is_empty()
                    && !b.is_empty()
                    && a.chars().all(|c| c.is_ascii_alphabetic())
                    && b.chars().all(|c| c.is_ascii_alphanumeric() || ".+-".contains(c))
            }));
    (!is_noise).then(|| body.to_string())
}

/// How a stored message can be forwarded, or why it cannot.
pub fn forward_item(msg: &Message) -> Result<ForwardItem, String> {
    if is_revoked(&msg.metadata) {
        return Err("The message was deleted".into());
    }
    match str_meta(msg, "media_type").map(str::to_lowercase).as_deref() {
        None | Some("text") => {
            let text = msg.body.trim();
            if text.is_empty() {
                Err("The message has no text".into())
            } else {
                Ok(ForwardItem::Text(msg.body.clone()))
            }
        }
        Some(kind) if MEDIA_KINDS.contains(&kind) => {
            let kind = match kind {
                "photo" => "image",
                "file" => "document",
                k => k,
            };
            Ok(ForwardItem::Media(ForwardMedia {
                kind: kind.to_string(),
                caption: caption_of(msg),
                filename: str_meta(msg, "filename").map(str::to_string),
                mime: str_meta(msg, "mime")
                    .or_else(|| str_meta(msg, "mimetype"))
                    .map(str::to_string),
                path: str_meta(msg, "media_path").map(str::to_string),
            }))
        }
        Some("location") => Err("Locations cannot be forwarded".into()),
        Some("poll") => Err("Polls cannot be forwarded".into()),
        Some(kind) => Err(format!("A {kind} message cannot be forwarded")),
    }
}

/// The source messages of one chat in chat order, each resolved for forwarding. A message id
/// that is not in the chat is reported as not forwardable.
pub fn resolve_sources(
    db: &Database,
    conversation_id: &str,
    message_ids: &[String],
) -> Vec<(String, Result<(Message, ForwardItem), String>)> {
    let mut found: Vec<_> = message_ids
        .iter()
        .map(|id| match db.get_message(conversation_id, id) {
            Err(_) => (None, id.clone(), Err("The message is no longer stored".to_string())),
            Ok(m) => (
                Some(m.timestamp),
                id.clone(),
                forward_item(&m).map(|item| (m, item)),
            ),
        })
        .collect();
    found.sort_by_key(|(at, _, _)| *at);
    found.into_iter().map(|(_, id, r)| (id, r)).collect()
}

fn result(target: &ForwardTarget, message_id: &str, outcome: ForwardOutcome) -> ForwardResult {
    ForwardResult {
        account_id: target.account_id.clone(),
        conversation_id: target.conversation_id.clone(),
        message_id: message_id.to_string(),
        outcome,
    }
}

/// Send every source to every target, in chat order per target. Media without a cached file is
/// downloaded once (`download` returns the cached path); a source that cannot be fetched fails
/// for each target while the others still go out.
pub async fn run_forward<D, DF, S, SF>(
    sources: Vec<(String, Result<(Message, ForwardItem), String>)>,
    targets: &[ForwardTarget],
    download: D,
    send: S,
) -> Vec<ForwardResult>
where
    D: Fn(Message) -> DF,
    DF: Future<Output = Result<String, String>>,
    S: Fn(ForwardTarget, ForwardItem) -> SF,
    SF: Future<Output = Result<(), String>>,
{
    let mut ready: Vec<(String, Result<ForwardItem, ForwardOutcome>)> = Vec::new();
    for (id, resolved) in sources {
        let item = match resolved {
            Err(reason) => Err(ForwardOutcome::NotForwardable { reason }),
            Ok((msg, ForwardItem::Media(mut media)))
                if !media.path.as_deref().is_some_and(|p| Path::new(p).is_file()) =>
            {
                match download(msg).await {
                    Ok(path) => {
                        media.path = Some(path);
                        Ok(ForwardItem::Media(media))
                    }
                    Err(e) => Err(ForwardOutcome::Failed {
                        error: format!("The media could not be downloaded: {e}"),
                    }),
                }
            }
            Ok((_, item)) => Ok(item),
        };
        ready.push((id, item));
    }
    let mut out = Vec::with_capacity(targets.len() * ready.len());
    for target in targets {
        for (id, item) in &ready {
            let outcome = match item {
                Err(outcome) => outcome.clone(),
                Ok(item) => match send(target.clone(), item.clone()).await {
                    Ok(()) => ForwardOutcome::Sent,
                    Err(error) => ForwardOutcome::Failed { error },
                },
            };
            out.push(result(target, id, outcome));
        }
    }
    out
}

/// Queue a forward to send at `send_at`. The scheduled row keeps the source message, not its
/// content, so the media is fetched when it is due even if the cache was purged meanwhile.
pub fn schedule_forward(
    db: &Database,
    source_account_id: &str,
    source_conversation_id: &str,
    sources: &[(String, Result<(Message, ForwardItem), String>)],
    targets: &[ForwardTarget],
    send_at: &str,
) -> Vec<ForwardResult> {
    let mut out = Vec::with_capacity(targets.len() * sources.len());
    for target in targets {
        for (id, resolved) in sources {
            let outcome = match resolved {
                Err(reason) => ForwardOutcome::NotForwardable {
                    reason: reason.clone(),
                },
                Ok((msg, _)) => {
                    let draft = ScheduleMessageDraft {
                        source_account_id: Some(source_account_id.to_string()),
                        source_conversation_id: Some(source_conversation_id.to_string()),
                        source_message_id: Some(msg.id.clone()),
                        dest_account_id: target.account_id.clone(),
                        dest_conversation_id: target.conversation_id.clone(),
                        body: crate::db::message_preview(&msg.body, &msg.metadata),
                        send_at: send_at.to_string(),
                        forward: true,
                    };
                    match db.schedule_message(&draft) {
                        Ok(_) => ForwardOutcome::Scheduled,
                        Err(e) => ForwardOutcome::Failed {
                            error: e.to_string(),
                        },
                    }
                }
            };
            out.push(result(target, id, outcome));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{MessageDirection, MessageStatus};
    use chrono::{Duration, Utc};
    use parking_lot::Mutex;
    use serde_json::json;
    use std::sync::Arc;

    const CHAT: &str = "15550001111@s.whatsapp.net";

    struct Fx {
        dir: tempfile::TempDir,
        db: Database,
        account: String,
        conversation: String,
    }

    fn message(remote: &str, body: &str, secs: i64, metadata: serde_json::Value) -> Message {
        Message {
            id: uuid::Uuid::new_v4().to_string(),
            conversation_id: String::new(),
            remote_id: Some(remote.into()),
            sender_id: Some(CHAT.into()),
            sender_name: Some("Sam".into()),
            direction: MessageDirection::Inbound,
            body: body.into(),
            timestamp: Utc::now() - Duration::hours(1) + Duration::seconds(secs),
            status: MessageStatus::Delivered,
            metadata,
            starred: false,
            pinned: false,
            reactions: Vec::new(),
            reply: None,
        }
    }

    fn fx() -> Fx {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::open(dir.path()).unwrap();
        let account = db.create_account("whatsapp", "WA").unwrap().id;
        let first = message("SEED", "hello", 0, json!({}));
        let out = db
            .ingest_message(&account, CHAT, &[], true, first, "hello", "Sam")
            .unwrap();
        let conversation = out.conversation.unwrap().id;
        Fx {
            dir,
            db,
            account,
            conversation,
        }
    }

    impl Fx {
        fn add(&self, remote: &str, body: &str, secs: i64, metadata: serde_json::Value) -> String {
            let msg = message(remote, body, secs, metadata);
            let preview = crate::db::message_preview(&msg.body, &msg.metadata);
            self.db
                .ingest_message(&self.account, CHAT, &[], true, msg, &preview, "Sam")
                .unwrap()
                .message
                .unwrap()
                .id
        }

        fn cached_file(&self, name: &str) -> String {
            let path = self.dir.path().join(name);
            std::fs::write(&path, b"bytes").unwrap();
            path.to_string_lossy().into_owned()
        }
    }

    fn target(account: &str, conversation: &str) -> ForwardTarget {
        ForwardTarget {
            account_id: account.into(),
            conversation_id: conversation.into(),
        }
    }

    type Sent = Arc<Mutex<Vec<(ForwardTarget, ForwardItem)>>>;

    /// Runs a forward with a recording sender. `fail_for` names target conversations whose sends
    /// fail; `download` answers for media without a cached file.
    async fn forward(
        fx: &Fx,
        ids: &[String],
        targets: &[ForwardTarget],
        fail_for: &'static str,
        download: Result<String, String>,
    ) -> (Vec<ForwardResult>, Vec<(ForwardTarget, ForwardItem)>, usize) {
        let sent: Sent = Arc::default();
        let downloads = Arc::new(Mutex::new(0usize));
        let sources = resolve_sources(&fx.db, &fx.conversation, ids);
        let results = run_forward(
            sources,
            targets,
            |_msg| {
                let (downloads, download) = (downloads.clone(), download.clone());
                async move {
                    *downloads.lock() += 1;
                    download
                }
            },
            |t, item| {
                let sent = sent.clone();
                async move {
                    if t.conversation_id == fail_for {
                        return Err("Connector is not running for this account".to_string());
                    }
                    sent.lock().push((t, item));
                    Ok(())
                }
            },
        )
        .await;
        let sent = sent.lock().clone();
        let downloads = *downloads.lock();
        (results, sent, downloads)
    }

    fn outcomes(results: &[ForwardResult]) -> Vec<(&str, &str, &ForwardOutcome)> {
        results
            .iter()
            .map(|r| (r.conversation_id.as_str(), r.message_id.as_str(), &r.outcome))
            .collect()
    }

    #[tokio::test]
    async fn text_is_sent_as_its_own_text_without_a_prefix() {
        let fx = fx();
        let id = fx.add("T1", "See you at 5", 10, json!({}));
        let (results, sent, _) =
            forward(&fx, &[id.clone()], &[target("acc-b", "dest")], "", Err("unused".into())).await;
        assert_eq!(sent, vec![(target("acc-b", "dest"), ForwardItem::Text("See you at 5".into()))]);
        assert_eq!(results[0].account_id, "acc-b");
        assert_eq!(outcomes(&results), vec![("dest", id.as_str(), &ForwardOutcome::Sent)]);
    }

    #[tokio::test]
    async fn cached_media_goes_out_as_the_file_with_its_caption() {
        let fx = fx();
        let path = fx.cached_file("sunset.jpg");
        let with_caption = fx.add(
            "M1",
            "sunset at the pier",
            10,
            json!({"media_type": "image", "media_path": path, "mime": "image/jpeg", "filename": "sunset.jpg"}),
        );
        let media_only = fx.add(
            "M2",
            "[Image]",
            20,
            json!({"media_type": "image", "media_path": path, "mime": "image/jpeg"}),
        );
        let (_, sent, downloads) = forward(
            &fx,
            &[media_only, with_caption],
            &[target("acc-a", "dest")],
            "",
            Err("unused".into()),
        )
        .await;
        assert_eq!(downloads, 0, "a cached file is not downloaded again");
        let media: Vec<ForwardMedia> = sent
            .into_iter()
            .map(|(_, item)| match item {
                ForwardItem::Media(m) => m,
                other => panic!("expected media, got {other:?}"),
            })
            .collect();
        assert_eq!(media[0].caption.as_deref(), Some("sunset at the pier"), "chat order");
        assert_eq!(media[0].path.as_deref(), Some(path.as_str()));
        assert_eq!(media[0].mime.as_deref(), Some("image/jpeg"));
        assert_eq!(media[0].kind, "image");
        assert_eq!(media[1].caption, None, "a placeholder body is never sent as a caption");
    }

    #[test]
    fn a_caption_written_in_brackets_is_kept_but_placeholders_are_not() {
        let caption = |body: &str| {
            caption_of(&message("C1", body, 1, json!({"media_type": "image"})))
        };
        assert_eq!(caption("[Please review]").as_deref(), Some("[Please review]"));
        assert_eq!(caption("[Image]"), None);
        assert_eq!(caption("[video]"), None);
        assert_eq!(caption("[Voice message]"), None);
    }

    #[test]
    fn a_file_hash_body_is_not_a_caption() {
        let caption = |body: &str| caption_of(&message("C1", body, 1, json!({"media_type": "image"})));
        assert_eq!(caption("nDXT4hDDZalrGuLwhkDIwABBzwR5YkjTGaJcVPOhQJs="), None);
        assert_eq!(caption("nDXT4hDDZalrGuLwhkDIwABBzwR5YkjTGaJcVPOhQJs"), None);
        assert_eq!(caption("see you at the pier tomorrow").as_deref(), Some("see you at the pier tomorrow"));
        assert_eq!(caption("https://example.com/a/b").as_deref(), Some("https://example.com/a/b"));
    }

    #[tokio::test]
    async fn uncached_media_is_downloaded_once_for_all_targets() {
        let fx = fx();
        let fresh = fx.cached_file("voice.ogg");
        let id = fx.add(
            "V1",
            "[Audio]",
            10,
            json!({"media_type": "ptt", "media_path": "/purged/by/retention.ogg"}),
        );
        let targets = [target("acc-a", "d1"), target("acc-b", "d2")];
        let (results, sent, downloads) =
            forward(&fx, &[id], &targets, "", Ok(fresh.clone())).await;
        assert_eq!(downloads, 1);
        assert_eq!(sent.len(), 2);
        for (_, item) in &sent {
            let ForwardItem::Media(m) = item else { panic!("expected media") };
            assert_eq!(m.path.as_deref(), Some(fresh.as_str()));
            assert_eq!(m.kind, "ptt");
        }
        assert!(results.iter().all(|r| r.outcome == ForwardOutcome::Sent));
    }

    #[tokio::test]
    async fn media_that_cannot_be_fetched_fails_without_stopping_the_rest() {
        let fx = fx();
        let photo = fx.add("P1", "[Image]", 10, json!({"media_type": "image"}));
        let text = fx.add("T1", "and this text", 20, json!({}));
        let (results, sent, _) = forward(
            &fx,
            &[photo.clone(), text.clone()],
            &[target("acc-a", "dest")],
            "",
            Err("expired on the server".into()),
        )
        .await;
        assert_eq!(sent, vec![(target("acc-a", "dest"), ForwardItem::Text("and this text".into()))]);
        assert_eq!(
            outcomes(&results),
            vec![
                (
                    "dest",
                    photo.as_str(),
                    &ForwardOutcome::Failed {
                        error: "The media could not be downloaded: expired on the server".into()
                    }
                ),
                ("dest", text.as_str(), &ForwardOutcome::Sent),
            ]
        );
    }

    #[tokio::test]
    async fn locations_polls_and_deleted_messages_are_reported_not_dropped() {
        let fx = fx();
        let location = fx.add(
            "L1",
            "[Location]",
            10,
            json!({"media_type": "location", "latitude": 1.0, "longitude": 2.0}),
        );
        let poll = fx.add("Q1", "Lunch?", 20, json!({"media_type": "poll"}));
        let deleted = fx.add("D1", "", 30, json!({"revoked_at": "2026-01-01T00:00:00Z"}));
        let ids = [location.clone(), poll.clone(), deleted.clone(), "missing".to_string()];
        let (results, sent, _) =
            forward(&fx, &ids, &[target("acc-a", "dest")], "", Err("unused".into())).await;
        assert!(sent.is_empty());
        let reason = |id: &str| {
            results
                .iter()
                .find(|r| r.message_id == id)
                .map(|r| r.outcome.clone())
                .unwrap()
        };
        let not = |reason: &str| ForwardOutcome::NotForwardable {
            reason: reason.into(),
        };
        assert_eq!(reason(&location), not("Locations cannot be forwarded"));
        assert_eq!(reason(&poll), not("Polls cannot be forwarded"));
        assert_eq!(reason(&deleted), not("The message was deleted"));
        assert_eq!(reason("missing"), not("The message is no longer stored"));
    }

    #[tokio::test]
    async fn every_pair_is_reported_across_accounts_and_one_failing_target_keeps_the_others() {
        let fx = fx();
        let a = fx.add("T1", "first", 10, json!({}));
        let b = fx.add("T2", "second", 20, json!({}));
        let targets = [
            target("acc-whatsapp-2", "wa-chat"),
            target("acc-telegram", "tg-chat"),
            target("acc-down", "down-chat"),
        ];
        let (results, sent, _) =
            forward(&fx, &[b.clone(), a.clone()], &targets, "down-chat", Err("unused".into())).await;
        let order: Vec<(&str, &str)> = sent
            .iter()
            .map(|(t, item)| {
                let ForwardItem::Text(text) = item else { panic!("expected text") };
                (t.account_id.as_str(), text.as_str())
            })
            .collect();
        assert_eq!(
            order,
            vec![
                ("acc-whatsapp-2", "first"),
                ("acc-whatsapp-2", "second"),
                ("acc-telegram", "first"),
                ("acc-telegram", "second"),
            ]
        );
        assert_eq!(results.len(), 6);
        let failed: Vec<(&str, &str)> = results
            .iter()
            .filter(|r| matches!(r.outcome, ForwardOutcome::Failed { .. }))
            .map(|r| (r.account_id.as_str(), r.message_id.as_str()))
            .collect();
        assert_eq!(failed, vec![("acc-down", a.as_str()), ("acc-down", b.as_str())]);
    }

    #[test]
    fn a_scheduled_forward_keeps_the_source_message_and_reports_unforwardable_ones() {
        let fx = fx();
        let photo = fx.add("P1", "pier", 10, json!({"media_type": "image"}));
        let poll = fx.add("Q1", "Lunch?", 20, json!({"media_type": "poll"}));
        let sources = resolve_sources(&fx.db, &fx.conversation, &[photo.clone(), poll.clone()]);
        let send_at = (Utc::now() - Duration::minutes(1)).to_rfc3339();
        let results = schedule_forward(
            &fx.db,
            &fx.account,
            &fx.conversation,
            &sources,
            &[target("acc-b", "dest")],
            &send_at,
        );
        assert_eq!(results[0].outcome, ForwardOutcome::Scheduled);
        assert!(matches!(results[1].outcome, ForwardOutcome::NotForwardable { .. }));
        let due = fx.db.due_scheduled_messages().unwrap();
        assert_eq!(due.len(), 1);
        assert!(due[0].forward);
        assert_eq!(due[0].source_message_id.as_deref(), Some(photo.as_str()));
        assert_eq!(due[0].source_conversation_id.as_deref(), Some(fx.conversation.as_str()));
        assert_eq!(due[0].dest_account_id, "acc-b");
        assert_eq!(due[0].body, "pier");
    }

    #[test]
    fn captions_ignore_file_names_sender_names_mime_types_and_keys() {
        let fx = fx();
        let make = |body: &str| {
            let mut m = message("X", body, 0, json!({"media_type": "document", "filename": "a.pdf"}));
            m.conversation_id = fx.conversation.clone();
            m
        };
        for noise in ["", "[Document]", "a.pdf", "Sam", "application/pdf", "0123456789ABCDEF0123"] {
            let ForwardItem::Media(m) = forward_item(&make(noise)).unwrap() else { panic!() };
            assert_eq!(m.caption, None, "{noise:?}");
        }
        let ForwardItem::Media(m) = forward_item(&make("signed copy / v2")).unwrap() else { panic!() };
        assert_eq!(m.caption.as_deref(), Some("signed copy / v2"));
    }
}
