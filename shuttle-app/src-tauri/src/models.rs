use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AccountStatus {
    Disconnected,
    Connecting,
    Connected,
    Error,
    AwaitingAuth,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub id: String,
    pub connector_id: String,
    pub name: String,
    pub identity: Option<String>,
    pub status: AccountStatus,
    pub metadata: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(default)]
    pub disabled: bool,
    #[serde(default)]
    pub muted: bool,
    #[serde(default)]
    pub workspace_id: Option<String>,
    #[serde(default)]
    pub notify_enabled: Option<bool>,
    #[serde(default)]
    pub send_receipts: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConversationType {
    Direct,
    Group,
    Channel,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conversation {
    pub id: String,
    pub account_id: String,
    pub remote_id: String,
    pub contact_id: Option<String>,
    pub title: String,
    pub conversation_type: ConversationType,
    pub unread_count: i64,
    pub last_message_at: Option<DateTime<Utc>>,
    pub last_message_preview: Option<String>,
    pub pinned: bool,
    pub archived: bool,
    pub muted: bool,
    pub metadata: serde_json::Value,
    #[serde(default)]
    pub workspace_id: Option<String>,
    #[serde(default)]
    pub priority_group: Option<String>,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub notify_enabled: Option<bool>,
    #[serde(default)]
    pub send_receipts: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MessageDirection {
    Inbound,
    Outbound,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MessageStatus {
    Pending,
    Sent,
    Delivered,
    Read,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub conversation_id: String,
    pub remote_id: Option<String>,
    pub sender_id: Option<String>,
    pub sender_name: Option<String>,
    pub direction: MessageDirection,
    pub body: String,
    pub timestamp: DateTime<Utc>,
    pub status: MessageStatus,
    pub metadata: serde_json::Value,
    #[serde(default)]
    pub starred: bool,
    #[serde(default)]
    pub pinned: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Contact {
    pub id: String,
    pub account_id: String,
    pub remote_id: String,
    pub display_name: String,
    pub avatar_url: Option<String>,
    pub metadata: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ContactProfile {
    pub username: Option<String>,
    pub phone: Option<String>,
    pub about: Option<String>,
    pub business_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContactProfileBundle {
    pub profile: ContactProfile,
    pub media: Vec<Message>,
    pub docs: Vec<Message>,
    pub links: Vec<Message>,
    pub starred: Vec<Message>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchMessageHit {
    pub message: Message,
    pub conversation_title: String,
    pub account_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchScope {
    Global,
    Account,
    Conversation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResults {
    pub conversations: Vec<Conversation>,
    pub messages: Vec<SearchMessageHit>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallState {
    pub call_id: String,
    pub conversation_id: String,
    pub account_id: String,
    pub direction: String,
    pub mode: String,
    pub status: String,
    pub remote_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectorInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub auth_type: String,
    pub capabilities: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workspace {
    pub id: String,
    pub name: String,
    pub builtin: bool,
    pub sort_order: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriorityGroup {
    pub id: String,
    pub name: String,
    pub color: Option<String>,
    pub builtin: bool,
    pub sort_order: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatTodo {
    pub id: String,
    pub conversation_id: String,
    pub account_id: String,
    pub body: String,
    pub due_at: Option<String>,
    pub done: bool,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reminder {
    pub id: String,
    pub conversation_id: String,
    pub account_id: String,
    pub fire_at: String,
    pub kind: String,
    pub note: Option<String>,
    pub fired: bool,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForwardRule {
    pub id: String,
    pub enabled: bool,
    pub source_account_id: Option<String>,
    pub source_conversation_id: Option<String>,
    pub source_workspace_id: Option<String>,
    pub dest_account_id: String,
    pub dest_conversation_id: String,
    pub inbound_only: bool,
    pub include_self: bool,
    pub keyword: Option<String>,
    pub prefix: Option<String>,
    pub suffix: Option<String>,
    pub strip_sender: bool,
    pub skip_if_forwarded: bool,
    pub delay_seconds: i64,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduledMessage {
    pub id: String,
    pub source_account_id: Option<String>,
    pub source_conversation_id: Option<String>,
    pub source_message_id: Option<String>,
    pub dest_account_id: String,
    pub dest_conversation_id: String,
    pub body: String,
    pub send_at: String,
    pub sent: bool,
    pub created_at: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ForwardRuleDraft {
    pub source_account_id: Option<String>,
    pub source_conversation_id: Option<String>,
    pub source_workspace_id: Option<String>,
    pub dest_account_id: String,
    pub dest_conversation_id: String,
    pub inbound_only: Option<bool>,
    pub include_self: Option<bool>,
    pub keyword: Option<String>,
    pub prefix: Option<String>,
    pub suffix: Option<String>,
    pub strip_sender: Option<bool>,
    pub skip_if_forwarded: Option<bool>,
    pub delay_seconds: Option<i64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ForwardRulePatch {
    pub enabled: Option<bool>,
    pub source_account_id: Option<String>,
    pub clear_source_account: Option<bool>,
    pub source_conversation_id: Option<String>,
    pub clear_source_conversation: Option<bool>,
    pub source_workspace_id: Option<String>,
    pub clear_source_workspace: Option<bool>,
    pub dest_account_id: Option<String>,
    pub dest_conversation_id: Option<String>,
    pub inbound_only: Option<bool>,
    pub include_self: Option<bool>,
    pub keyword: Option<String>,
    pub clear_keyword: Option<bool>,
    pub prefix: Option<String>,
    pub clear_prefix: Option<bool>,
    pub suffix: Option<String>,
    pub clear_suffix: Option<bool>,
    pub strip_sender: Option<bool>,
    pub skip_if_forwarded: Option<bool>,
    pub delay_seconds: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ScheduleMessageDraft {
    pub source_account_id: Option<String>,
    pub source_conversation_id: Option<String>,
    pub source_message_id: Option<String>,
    pub dest_account_id: String,
    pub dest_conversation_id: String,
    pub body: String,
    pub send_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupManifest {
    pub exported_at: String,
    pub includes_messages: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct AccountPatch {
    pub name: Option<String>,
    pub muted: Option<bool>,
    pub disabled: Option<bool>,
    pub workspace_id: Option<String>,
    pub clear_workspace: Option<bool>,
    pub notify_enabled: Option<bool>,
    pub clear_notify: Option<bool>,
    pub send_receipts: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ConversationPatch {
    pub pinned: Option<bool>,
    pub archived: Option<bool>,
    pub muted: Option<bool>,
    pub workspace_id: Option<String>,
    pub clear_workspace: Option<bool>,
    pub priority_group: Option<String>,
    pub clear_priority: Option<bool>,
    pub notes: Option<String>,
    pub notify_enabled: Option<bool>,
    pub clear_notify: Option<bool>,
    pub send_receipts: Option<bool>,
    pub clear_receipts: Option<bool>,
}
