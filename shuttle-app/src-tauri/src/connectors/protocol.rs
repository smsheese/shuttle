use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const PROTOCOL_VERSION: u32 = 1;

/// One action on a message that already exists on the network.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MessageOp {
    /// `None` removes this account's reaction.
    React {
        #[serde(default)]
        emoji: Option<String>,
    },
    Edit {
        text: String,
    },
    DeleteForEveryone,
    DeleteForMe,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ConnectorRequest {
    Handshake {
        protocol_version: u32,
    },
    Connect {
        account_id: String,
        credentials: Value,
    },
    Disconnect {
        account_id: String,
    },
    Authenticate {
        account_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        files_dir: Option<String>,
        #[serde(default)]
        credentials: Value,
    },
    SubmitAuth {
        account_id: String,
        credentials: Value,
    },
    SendMessage {
        account_id: String,
        conversation_id: String,
        text: String,
        /// Remote id of the message this one answers (a real reply, not quoted text).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reply_to_id: Option<String>,
        /// Author of that message when it is not this account (a group reply needs it).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reply_to_sender: Option<String>,
        /// Local message row id; echoed back in the connector's `ok` / `error` reply.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
        /// Mark the message as forwarded on networks that show it.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        forwarded: bool,
    },
    SendAttachment {
        account_id: String,
        conversation_id: String,
        kind: String,
        #[serde(default)]
        caption: Option<String>,
        #[serde(default)]
        text: Option<String>,
        #[serde(default)]
        filename: Option<String>,
        #[serde(default)]
        mime: Option<String>,
        #[serde(default)]
        data_base64: Option<String>,
        #[serde(default)]
        path: Option<String>,
        #[serde(default)]
        latitude: Option<f64>,
        #[serde(default)]
        longitude: Option<f64>,
        #[serde(default)]
        question: Option<String>,
        #[serde(default)]
        options: Vec<String>,
        #[serde(default)]
        max_answer: Option<i32>,
        /// Remote id of the message this one answers (see `SendMessage`).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reply_to_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reply_to_sender: Option<String>,
        /// Local message row id; echoed back in the connector's `ok` / `error` reply.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
        /// Mark the message as forwarded on networks that show it.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        forwarded: bool,
    },
    /// Apply `op` to the message `message_id` (remote id) in `conversation_id` (remote id).
    /// The connector answers `ok` / `error` with the same `request_id` and, on success, reports
    /// the result as the same fact an incoming change would produce (`reaction` or
    /// `message.changed`).
    MessageOp {
        account_id: String,
        conversation_id: String,
        message_id: String,
        op: MessageOp,
        request_id: String,
    },
    MarkRead {
        account_id: String,
        conversation_id: String,
        /// Remote id of the newest inbound message in the chat, when known.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        message_id: Option<String>,
    },
    GetStatus {
        account_id: String,
    },
    SyncHistory {
        account_id: String,
    },
    SyncChat {
        account_id: String,
        conversation_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        since_message_id: Option<String>,
    },
    DownloadMedia {
        account_id: String,
        conversation_id: String,
        message_id: String,
    },
    DownloadStatusMedia {
        account_id: String,
        message_id: String,
    },
    FetchAvatar {
        account_id: String,
        conversation_id: String,
    },
    CreateGroup {
        account_id: String,
        title: String,
        participants: Vec<String>,
    },
    FetchContactProfile {
        account_id: String,
        conversation_id: String,
    },
    /// Ask whether a typed phone number is on WhatsApp. The answer is a `number.checked` event
    /// carrying the same `request_id`.
    CheckNumber {
        account_id: String,
        phone: String,
        request_id: String,
    },
    StartCall {
        account_id: String,
        conversation_id: String,
        mode: String,
        #[serde(default)]
        share_screen: bool,
    },
    AcceptCall {
        account_id: String,
        call_id: String,
    },
    RejectCall {
        account_id: String,
        call_id: String,
    },
    HangupCall {
        account_id: String,
        call_id: String,
    },
    Shutdown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ConnectorResponse {
    HandshakeOk {
        connector_id: String,
        version: String,
        capabilities: Vec<String>,
    },
    AuthRequired {
        method: String,
        qr_data: Option<String>,
        url: Option<String>,
        #[serde(default)]
        message: Option<String>,
        #[serde(default)]
        account_id: Option<String>,
    },
    Status {
        account_id: String,
        status: String,
        identity: Option<String>,
    },
    Ok {
        #[serde(default)]
        request_id: Option<String>,
        /// Remote (network) id of the message a send produced.
        #[serde(default)]
        message_id: Option<String>,
        #[serde(default)]
        conversation_id: Option<String>,
        #[serde(default)]
        account_id: Option<String>,
    },
    ContactProfile {
        #[serde(default)]
        request_id: Option<String>,
        #[serde(default)]
        conversation_id: Option<String>,
        #[serde(default)]
        account_id: Option<String>,
        profile: Value,
    },
    Error {
        message: String,
        #[serde(default)]
        account_id: Option<String>,
        /// Set when the error answers a specific send request.
        #[serde(default)]
        request_id: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ConnectorEvent {
    Event {
        event: String,
        account_id: String,
        payload: Value,
    },
}

pub fn encode_line<T: Serialize>(msg: &T) -> Result<String, serde_json::Error> {
    let mut line = serde_json::to_string(msg)?;
    line.push('\n');
    Ok(line)
}

pub fn decode_line<T: for<'de> Deserialize<'de>>(line: &str) -> Result<T, serde_json::Error> {
    serde_json::from_str(line.trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_skip_missing_optional_fields() {
        let line = encode_line(&ConnectorRequest::SendMessage {
            account_id: "a".into(),
            conversation_id: "c".into(),
            text: "t".into(),
            reply_to_id: None,
            reply_to_sender: None,
            request_id: None,
            forwarded: false,
        })
        .unwrap();
        assert!(!line.contains("request_id"));
        assert!(!line.contains("reply_to"));
        assert!(!line.contains("forwarded"));
        let line = encode_line(&ConnectorRequest::SendMessage {
            account_id: "a".into(),
            conversation_id: "g@g.us".into(),
            text: "t".into(),
            reply_to_id: Some("ORIG".into()),
            reply_to_sender: Some("p@s.whatsapp.net".into()),
            request_id: Some("r".into()),
            forwarded: true,
        })
        .unwrap();
        assert!(line.contains("\"forwarded\":true"));
        assert!(line.contains("\"reply_to_id\":\"ORIG\""));
        assert!(line.contains("\"reply_to_sender\":\"p@s.whatsapp.net\""));
        let line = encode_line(&ConnectorRequest::MarkRead {
            account_id: "a".into(),
            conversation_id: "c".into(),
            message_id: Some("m".into()),
        })
        .unwrap();
        assert!(line.contains("\"message_id\":\"m\""));
    }

    #[test]
    fn message_ops_cross_the_boundary_as_one_request_kind() {
        let line = |op: MessageOp| {
            let raw = encode_line(&ConnectorRequest::MessageOp {
                account_id: "acc".into(),
                conversation_id: "123@s.whatsapp.net".into(),
                message_id: "WAMID".into(),
                op,
                request_id: "op-1".into(),
            })
            .unwrap();
            serde_json::from_str::<Value>(&raw).unwrap()
        };
        let react = line(MessageOp::React {
            emoji: Some("👍".into()),
        });
        assert_eq!(
            react,
            serde_json::json!({
                "type": "message_op",
                "account_id": "acc",
                "conversation_id": "123@s.whatsapp.net",
                "message_id": "WAMID",
                "op": {"kind": "react", "emoji": "👍"},
                "request_id": "op-1",
            })
        );
        assert_eq!(
            line(MessageOp::React { emoji: None })["op"],
            serde_json::json!({"kind": "react", "emoji": null})
        );
        assert_eq!(
            line(MessageOp::Edit { text: "fixed".into() })["op"],
            serde_json::json!({"kind": "edit", "text": "fixed"})
        );
        assert_eq!(
            line(MessageOp::DeleteForEveryone)["op"],
            serde_json::json!({"kind": "delete_for_everyone"})
        );
        assert_eq!(
            line(MessageOp::DeleteForMe)["op"],
            serde_json::json!({"kind": "delete_for_me"})
        );
        let from_ui: MessageOp = serde_json::from_str(r#"{"kind":"react"}"#).unwrap();
        assert_eq!(from_ui, MessageOp::React { emoji: None });
    }

    #[test]
    fn old_and_new_replies_parse() {
        let ok: ConnectorResponse = decode_line(r#"{"type":"ok"}"#).unwrap();
        assert!(matches!(
            ok,
            ConnectorResponse::Ok {
                message_id: None,
                ..
            }
        ));
        let ok: ConnectorResponse = decode_line(
            r#"{"type":"ok","account_id":"A","request_id":"R","message_id":"X","conversation_id":"J"}"#,
        )
        .unwrap();
        match ok {
            ConnectorResponse::Ok {
                request_id,
                message_id,
                account_id,
                ..
            } => {
                assert_eq!(request_id.as_deref(), Some("R"));
                assert_eq!(message_id.as_deref(), Some("X"));
                assert_eq!(account_id.as_deref(), Some("A"));
            }
            _ => panic!("expected ok"),
        }
        let err: ConnectorResponse = decode_line(r#"{"type":"error","message":"boom"}"#).unwrap();
        assert!(matches!(
            err,
            ConnectorResponse::Error {
                request_id: None,
                ..
            }
        ));
    }
}
