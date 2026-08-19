use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ConnectorRequest {
    Handshake { protocol_version: u32 },
    Connect { account_id: String, credentials: Value },
    Disconnect { account_id: String },
    Authenticate {
        account_id: String,
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
    },
    MarkRead {
        account_id: String,
        conversation_id: String,
    },
    GetStatus { account_id: String },
    SyncHistory { account_id: String },
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
    HandshakeOk { connector_id: String, version: String, capabilities: Vec<String> },
    AuthRequired {
        method: String,
        qr_data: Option<String>,
        url: Option<String>,
        #[serde(default)]
        message: Option<String>,
    },
    Status { account_id: String, status: String, identity: Option<String> },
    Ok { request_id: Option<String> },
    ContactProfile {
        #[serde(default)]
        request_id: Option<String>,
        #[serde(default)]
        conversation_id: Option<String>,
        profile: Value,
    },
    Error { message: String },
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
