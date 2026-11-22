//! Which message actions each network supports, and the rule that decides whether one action
//! may be applied to one stored message. The UI reads the same table to decide what to offer;
//! this check is the guard.

use super::MessageOp;
use crate::db::is_revoked;
use crate::models::{Message, MessageActions, MessageDirection};
use chrono::{DateTime, Utc};

/// The action table, keyed by connector id. Networks without a real connector support nothing.
pub fn message_actions(connector_id: &str) -> MessageActions {
    match connector_id {
        "whatsapp" => MessageActions {
            react: true,
            edit: true,
            delete: true,
            delete_everyone: true,
            edit_window_secs: Some(15 * 60),
            // WhatsApp documents "about two days"; staying inside it keeps the revoke valid.
            delete_everyone_window_secs: Some(2 * 24 * 60 * 60),
        },
        _ => MessageActions::default(),
    }
}

/// What happened to one message of a `apply_message_op` call. `error` is absent on success.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct MessageOpResult {
    pub message_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpRejection {
    Unsupported,
    /// Still sending (or failed): the network has no id for it.
    NotSent,
    NotOwn,
    /// Not text and not a media caption.
    NotEditable,
    EmptyText,
    WindowClosed,
    AlreadyDeleted,
}

impl std::fmt::Display for OpRejection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            OpRejection::Unsupported => "This network does not support that action",
            OpRejection::NotSent => "The message has not been sent yet",
            OpRejection::NotOwn => "Only your own messages can be changed that way",
            OpRejection::NotEditable => "Only text and captions can be edited",
            OpRejection::EmptyText => "The edited message is empty",
            OpRejection::WindowClosed => "It is too late to do that to this message",
            OpRejection::AlreadyDeleted => "The message was deleted",
        })
    }
}

fn within(window_secs: Option<u32>, sent: DateTime<Utc>, now: DateTime<Utc>) -> bool {
    window_secs.is_none_or(|w| {
        (now - sent).num_milliseconds() <= i64::from(w) * 1000
    })
}

const CAPTIONED: [&str; 4] = ["image", "video", "document", "gif"];

fn has_editable_text(msg: &Message) -> bool {
    let body = msg.body.trim();
    if body.is_empty() {
        return false;
    }
    match msg.metadata.get("media_type").and_then(|v| v.as_str()) {
        None | Some("") | Some("text") => true,
        Some(kind) => {
            CAPTIONED.contains(&kind.to_lowercase().as_str())
                && !(body.starts_with('[') && body.ends_with(']'))
        }
    }
}

/// May `op` be applied to `msg` at `now`? `now` is a parameter so the time rules are testable.
pub fn check_op(
    actions: &MessageActions,
    msg: &Message,
    op: &MessageOp,
    now: DateTime<Utc>,
) -> Result<(), OpRejection> {
    let supported = match op {
        MessageOp::React { .. } => actions.react,
        MessageOp::Edit { .. } => actions.edit,
        MessageOp::DeleteForEveryone => actions.delete_everyone,
        MessageOp::DeleteForMe => actions.delete,
    };
    if !supported {
        return Err(OpRejection::Unsupported);
    }
    if msg.remote_id.as_deref().is_none_or(str::is_empty) {
        return Err(OpRejection::NotSent);
    }
    let revoked = is_revoked(&msg.metadata);
    let own = msg.direction == MessageDirection::Outbound;
    match op {
        MessageOp::DeleteForMe => Ok(()),
        _ if revoked => Err(OpRejection::AlreadyDeleted),
        MessageOp::React { .. } => Ok(()),
        MessageOp::Edit { text } => {
            if !own {
                Err(OpRejection::NotOwn)
            } else if !has_editable_text(msg) {
                Err(OpRejection::NotEditable)
            } else if text.trim().is_empty() {
                Err(OpRejection::EmptyText)
            } else if !within(actions.edit_window_secs, msg.timestamp, now) {
                Err(OpRejection::WindowClosed)
            } else {
                Ok(())
            }
        }
        MessageOp::DeleteForEveryone => {
            if !own {
                Err(OpRejection::NotOwn)
            } else if !within(actions.delete_everyone_window_secs, msg.timestamp, now) {
                Err(OpRejection::WindowClosed)
            } else {
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::MessageStatus;
    use chrono::Duration;

    fn sent_at(t: DateTime<Utc>) -> Message {
        Message {
            id: "local".into(),
            conversation_id: "c".into(),
            remote_id: Some("WAMID".into()),
            sender_id: None,
            sender_name: Some("You".into()),
            direction: MessageDirection::Outbound,
            body: "hello".into(),
            timestamp: t,
            status: MessageStatus::Sent,
            metadata: serde_json::json!({}),
            starred: false,
            pinned: false,
            reactions: Vec::new(),
            reply: None,
        }
    }

    fn edit() -> MessageOp {
        MessageOp::Edit {
            text: "hello again".into(),
        }
    }

    fn t0() -> DateTime<Utc> {
        "2026-01-01T12:00:00Z".parse().unwrap()
    }

    #[test]
    fn whatsapp_edit_window_is_fifteen_minutes_inclusive() {
        let wa = message_actions("whatsapp");
        let msg = sent_at(t0());
        let at = |secs: i64| check_op(&wa, &msg, &edit(), t0() + Duration::seconds(secs));
        assert_eq!(at(0), Ok(()));
        assert_eq!(at(14 * 60 + 59), Ok(()));
        assert_eq!(at(15 * 60), Ok(()), "exactly at the edge");
        assert_eq!(at(15 * 60 + 1), Err(OpRejection::WindowClosed));
        assert_eq!(
            check_op(&wa, &msg, &edit(), t0() + Duration::milliseconds(900_001)),
            Err(OpRejection::WindowClosed)
        );
    }

    #[test]
    fn only_own_text_or_captions_can_be_edited() {
        let wa = message_actions("whatsapp");
        let now = t0() + Duration::minutes(1);
        let mut theirs = sent_at(t0());
        theirs.direction = MessageDirection::Inbound;
        assert_eq!(
            check_op(&wa, &theirs, &edit(), now),
            Err(OpRejection::NotOwn)
        );

        let mut photo = sent_at(t0());
        photo.body = "[Image]".into();
        photo.metadata = serde_json::json!({"media_type": "image"});
        assert_eq!(
            check_op(&wa, &photo, &edit(), now),
            Err(OpRejection::NotEditable)
        );
        photo.body = "sunset".into();
        assert_eq!(check_op(&wa, &photo, &edit(), now), Ok(()));

        let mut voice = sent_at(t0());
        voice.body = "x".into();
        voice.metadata = serde_json::json!({"media_type": "ptt"});
        assert_eq!(
            check_op(&wa, &voice, &edit(), now),
            Err(OpRejection::NotEditable)
        );

        let blank = MessageOp::Edit { text: "  ".into() };
        assert_eq!(
            check_op(&wa, &sent_at(t0()), &blank, now),
            Err(OpRejection::EmptyText)
        );
    }

    #[test]
    fn delete_for_everyone_is_own_messages_inside_the_window() {
        let wa = message_actions("whatsapp");
        let msg = sent_at(t0());
        let op = MessageOp::DeleteForEveryone;
        assert_eq!(check_op(&wa, &msg, &op, t0() + Duration::hours(47)), Ok(()));
        assert_eq!(
            check_op(&wa, &msg, &op, t0() + Duration::hours(49)),
            Err(OpRejection::WindowClosed)
        );
        let mut theirs = sent_at(t0());
        theirs.direction = MessageDirection::Inbound;
        assert_eq!(check_op(&wa, &theirs, &op, t0()), Err(OpRejection::NotOwn));
        assert_eq!(
            check_op(
                &wa,
                &theirs,
                &MessageOp::DeleteForMe,
                t0() + Duration::days(400)
            ),
            Ok(())
        );
    }

    #[test]
    fn unsent_deleted_and_unsupported_messages_are_refused() {
        let wa = message_actions("whatsapp");
        let react = MessageOp::React {
            emoji: Some("👍".into()),
        };
        let mut pending = sent_at(t0());
        pending.remote_id = None;
        assert_eq!(
            check_op(&wa, &pending, &react, t0()),
            Err(OpRejection::NotSent)
        );

        let mut gone = sent_at(t0());
        gone.body = String::new();
        gone.metadata = serde_json::json!({"revoked_at": "2026-01-01T12:01:00Z"});
        assert_eq!(
            check_op(&wa, &gone, &react, t0()),
            Err(OpRejection::AlreadyDeleted)
        );
        assert_eq!(
            check_op(&wa, &gone, &edit(), t0()),
            Err(OpRejection::AlreadyDeleted)
        );
        assert_eq!(check_op(&wa, &gone, &MessageOp::DeleteForMe, t0()), Ok(()));

        let stub = message_actions("telegram");
        assert_eq!(stub, MessageActions::default());
        assert_eq!(
            check_op(&stub, &sent_at(t0()), &react, t0()),
            Err(OpRejection::Unsupported)
        );
    }

    #[test]
    fn no_window_means_no_time_limit() {
        let unlimited = MessageActions {
            edit: true,
            ..Default::default()
        };
        let msg = sent_at(t0());
        assert_eq!(
            check_op(&unlimited, &msg, &edit(), t0() + Duration::days(3650)),
            Ok(())
        );
    }
}
