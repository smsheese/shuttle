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
    MarkRead {
        account_id: String,
        conversation_id: String,
    },
    GetStatus { account_id: String },
    SyncHistory { account_id: String },
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
