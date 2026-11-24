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
    Sleeping,
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
    /// `None` inherits the app-wide hibernation default.
    #[serde(default)]
    pub sleep_enabled: Option<bool>,
    #[serde(default)]
    pub sleep_after_minutes: Option<u32>,
    /// Periodic wake while hibernated. `0` = only wake on user action. `None` inherits default.
    #[serde(default)]
    pub sleep_check_minutes: Option<u32>,
    /// Which media this account downloads automatically (unset = built-in default).
    #[serde(default)]
    pub auto_download: AutoDownloadPrefs,
}

/// Automatic-download choices per media group. `None` = not decided here: a chat inherits
/// from its account, and an account falls back to the built-in default.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct AutoDownloadPrefs {
    #[serde(default)]
    pub images: Option<bool>,
    #[serde(default)]
    pub videos: Option<bool>,
    #[serde(default)]
    pub files: Option<bool>,
}

/// Media groups the auto-download setting is expressed in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaGroup {
    Images,
    Videos,
    Files,
}

impl MediaGroup {
    /// Stickers follow Images; voice notes and audio follow Files (they always download on
    /// demand when played). Unknown/non-downloadable kinds have no group.
    pub fn of_kind(kind: &str) -> Option<Self> {
        match kind.to_ascii_lowercase().as_str() {
            "image" | "photo" | "sticker" => Some(Self::Images),
            "video" => Some(Self::Videos),
            "document" | "audio" | "ptt" => Some(Self::Files),
            _ => None,
        }
    }

    /// Built-in default when neither the chat nor the account decided.
    pub fn default_on(self) -> bool {
        matches!(self, Self::Images)
    }
}

impl AutoDownloadPrefs {
    pub fn get(&self, group: MediaGroup) -> Option<bool> {
        match group {
            MediaGroup::Images => self.images,
            MediaGroup::Videos => self.videos,
            MediaGroup::Files => self.files,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.images.is_none() && self.videos.is_none() && self.files.is_none()
    }

    /// Effective choice: chat override, else account default, else the built-in default.
    pub fn resolve(group: MediaGroup, chat: &Self, account: &Self) -> bool {
        chat.get(group)
            .or_else(|| account.get(group))
            .unwrap_or_else(|| group.default_on())
    }

    /// Whether `kind` should download without the user asking.
    pub fn allows_auto(kind: &str, chat: &Self, account: &Self) -> bool {
        MediaGroup::of_kind(kind).is_some_and(|g| Self::resolve(g, chat, account))
    }
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
    /// Who wrote the message the preview shows (decided in `db/chat.rs` with the preview).
    /// `None` = unknown (no stored message backs the preview), so the UI shows no arrow.
    #[serde(default)]
    pub last_message_from_me: Option<bool>,
    /// Inbound preview author, for the group "Name: text" prefix. Never set for outbound.
    #[serde(default)]
    pub last_message_sender_id: Option<String>,
    #[serde(default)]
    pub last_message_sender_name: Option<String>,
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
    /// Per-chat auto-download overrides; unset groups use the account's choice.
    #[serde(default)]
    pub auto_download: AutoDownloadPrefs,
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
    /// Reactions to this message, grouped per emoji. Filled by the message-list query only.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reactions: Vec<ReactionGroup>,
    /// The message this one replies to, resolved for display. Never stored: it is built from
    /// `metadata.reply_to` and the stored original when a message is listed or emitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply: Option<ReplyPreview>,
}

/// Where a message page starts. Every anchor but `Newest` names a message the caller already
/// holds, so the page picks up at the same total order the database pages on.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MessageAnchor {
    /// The newest messages of the chat.
    Newest,
    /// Strictly older than the named message.
    Before { message_id: String },
    /// The named message and older ones (re-reading a window the reader already holds).
    From { message_id: String },
    /// Strictly newer than the named message, oldest first.
    After { message_id: String },
    /// Half older, half newer around the named message (a jump to a search hit).
    Around { message_id: String },
}

/// One contiguous page of a chat, oldest first, and whether the chat continues on each side.
#[derive(Debug, Clone, Serialize)]
pub struct MessagePage {
    pub messages: Vec<Message>,
    pub has_older: bool,
    pub has_newer: bool,
}

/// What a reply's quote block shows. Built from the stored original when it exists, else from
/// the snippet the connector reported (`available` is then false and the quote is not a link).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplyPreview {
    /// Remote id of the replied-to message.
    pub remote_id: String,
    /// Local id of the stored original (scroll target); None when it is not stored.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
    /// Author of the original ("You" for this account's own messages).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sender_name: Option<String>,
    pub from_me: bool,
    /// Text or caption of the original, clipped. Empty for captionless media: the UI words
    /// those from `media_type` (and `filename`).
    pub snippet: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media_type: Option<String>,
    /// File name of a document, for its label.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
    /// Small image of the original (its thumbnail, or the image itself) when one is cached.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thumbnail_path: Option<String>,
    pub available: bool,
}

/// Everyone who reacted to one message with the same emoji.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReactionGroup {
    pub emoji: String,
    pub count: usize,
    /// Display names of the reactors, oldest reaction first ("You" for this account).
    pub names: Vec<String>,
    /// True when this account is one of the reactors.
    pub mine: bool,
}

/// One person's pick in a poll. `id` is `me` for this account, else a JID.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PollVoter {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// One option of a poll with the votes folded into it. `id` is the network's option hash: votes
/// name an option by it, so a renamed option keeps its votes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PollOptionData {
    pub id: String,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub votes: u32,
    #[serde(default)]
    pub voters: Vec<PollVoter>,
    /// True when this account voted for this option.
    #[serde(default)]
    pub mine: bool,
}

/// A poll message's structured data, stored at `metadata.poll`. Mirrors `PollData` in the UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PollData {
    #[serde(default)]
    pub question: String,
    /// True when a voter may pick more than one option.
    #[serde(default)]
    pub multiple: bool,
    #[serde(default)]
    pub options: Vec<PollOptionData>,
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

/// A group member seen as a contact. `conversation` is their direct chat on the same account,
/// when there is one.
#[derive(Debug, Clone, Serialize)]
pub struct SenderContact {
    pub conversation: Option<Conversation>,
    pub contact: Option<Contact>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ContactProfile {
    pub username: Option<String>,
    pub phone: Option<String>,
    pub about: Option<String>,
    pub business_name: Option<String>,
    /// People of a group chat (not this account), for the @mention picker.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub members: Vec<GroupMember>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupMember {
    /// Digits an @mention of this person is written with.
    pub user: String,
    /// Every digit run (phone, LID) that names the person.
    #[serde(default)]
    pub ids: Vec<String>,
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
    /// What can be done to a message that is already on the network.
    pub actions: MessageActions,
}

/// Message actions one network supports. The UI only offers what is listed here; the core
/// enforces the same rules before anything reaches a connector.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageActions {
    pub react: bool,
    /// Own messages only, text or a media caption.
    pub edit: bool,
    /// Delete for me.
    pub delete: bool,
    /// Own messages only.
    pub delete_everyone: bool,
    /// How long after sending an own message can be edited. `None` = no limit.
    pub edit_window_secs: Option<u32>,
    /// How long after sending an own message can be deleted for everyone. `None` = no limit.
    pub delete_everyone_window_secs: Option<u32>,
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
    /// Local id of the message this todo was created from (right-click → Add to todo).
    #[serde(default)]
    pub message_id: Option<String>,
    /// Short quote of that message, so the todo stays readable if the message is not loaded.
    #[serde(default)]
    pub message_snippet: Option<String>,
}

/// A per-chat note item. It may quote a message and may carry one countdown timer or one
/// absolute alarm; `fire_at` is the instant it fires either way (RFC 3339, UTC).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatNote {
    pub id: String,
    pub conversation_id: String,
    pub account_id: String,
    pub body: String,
    pub message_id: Option<String>,
    pub message_snippet: Option<String>,
    pub fire_at: Option<String>,
    /// `timer` (countdown from creation) or `alarm` (absolute date-time); `None` without schedule.
    pub kind: Option<String>,
    pub fired: bool,
    pub created_at: String,
}

/// Either a countdown (`timer_secs`) or an absolute `alarm_at` (RFC 3339), never both.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NoteSchedule {
    #[serde(default)]
    pub timer_secs: Option<i64>,
    #[serde(default)]
    pub alarm_at: Option<String>,
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
    #[serde(default)]
    pub attempts: i64,
    #[serde(default)]
    pub last_error: Option<String>,
    #[serde(default)]
    pub failed: bool,
    /// Sends the source message itself (text or media) when due, not `body`, which is only
    /// its preview.
    #[serde(default)]
    pub forward: bool,
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
    #[serde(default)]
    pub forward: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupManifest {
    pub exported_at: String,
    pub includes_messages: bool,
    #[serde(default)]
    pub includes_media: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct AccountPatch {
    pub accent_color: Option<String>,
    pub name: Option<String>,
    pub muted: Option<bool>,
    pub disabled: Option<bool>,
    pub workspace_id: Option<String>,
    pub clear_workspace: Option<bool>,
    pub notify_enabled: Option<bool>,
    pub clear_notify: Option<bool>,
    pub send_receipts: Option<bool>,
    pub sleep_enabled: Option<bool>,
    pub clear_sleep_enabled: Option<bool>,
    pub sleep_after_minutes: Option<u32>,
    pub clear_sleep_after: Option<bool>,
    pub sleep_check_minutes: Option<u32>,
    pub clear_sleep_check: Option<bool>,
    /// Replaces the account's auto-download choices (unset groups use the built-in default).
    pub auto_download: Option<AutoDownloadPrefs>,
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
    /// Replaces the chat's overrides (unset groups inherit the account's choice).
    pub auto_download: Option<AutoDownloadPrefs>,
}
