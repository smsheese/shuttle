export type AccountStatus = 'disconnected' | 'connecting' | 'connected' | 'error' | 'awaiting_auth' | 'sleeping';

export interface Account {
  id: string;
  connector_id: string;
  name: string;
  identity: string | null;
  status: AccountStatus;
  metadata: Record<string, unknown>;
  created_at: string;
  updated_at: string;
  disabled?: boolean;
  muted?: boolean;
  workspace_id?: string | null;
  notify_enabled?: boolean | null;
  send_receipts?: boolean;
  sleep_enabled?: boolean | null;
  sleep_after_minutes?: number | null;
  sleep_check_minutes?: number | null;
  /** Account-wide auto-download choices; unset groups use the built-in default. */
  auto_download?: AutoDownloadPrefs;
}

/**
 * Which media downloads without the user asking. `undefined`/`null` = not decided here: a chat
 * inherits from its account, an account falls back to the built-in default (images on, videos
 * and files off). Stickers always load; voice notes and audio follow files.
 */
export interface AutoDownloadPrefs {
  images?: boolean | null;
  videos?: boolean | null;
  files?: boolean | null;
}

export type AutoDownloadGroup = keyof AutoDownloadPrefs;

/**
 * What a link status points at, when the connector could read a preview for it.
 * Helper 1 (connector) builds this; the status viewer draws it.
 */
export interface StatusLinkPreview {
  url: string;
  title?: string | null;
  /** Small cached preview image path, when the connector could fetch one. */
  thumbnail_path?: string | null;
}

export interface StatusPost {
  id: string;
  media_type: string;
  text?: string;
  timestamp: string | null;
  /** Small cached preview image path (when WhatsApp's own preview is known). */
  thumbnail_path?: string;
  /** Set for a status whose body is a link (an Instagram reel, a Facebook video). */
  link_preview?: StatusLinkPreview | null;
}

export interface StatusItem {
  account_id?: string;
  sender_id: string;
  sender_name: string;
  timestamp: string | null;
  media_type?: string;
  preview?: string;
  from_me?: boolean;
  count?: number;
  posts?: StatusPost[];
}

export interface Conversation {
  id: string;
  account_id: string;
  remote_id: string;
  contact_id: string | null;
  title: string;
  conversation_type: 'direct' | 'group' | 'channel';
  unread_count: number;
  last_message_at: string | null;
  last_message_preview: string | null;
  /** Direction of the message the preview shows: true = sent by me, false = received, absent = unknown. */
  last_message_from_me?: boolean | null;
  /** Author of an inbound preview (for the group "Name: text" prefix). Never set for outbound. */
  last_message_sender_id?: string | null;
  last_message_sender_name?: string | null;
  pinned: boolean;
  archived: boolean;
  muted: boolean;
  metadata: Record<string, unknown>;
  workspace_id?: string | null;
  priority_group?: string | null;
  notes?: string;
  notify_enabled?: boolean | null;
  send_receipts?: boolean | null;
  /** Per-chat auto-download overrides; unset groups use the account's choice. */
  auto_download?: AutoDownloadPrefs;
}

export interface Contact {
  id: string;
  account_id: string;
  remote_id: string;
  display_name: string;
  avatar_url: string | null;
  metadata: Record<string, unknown>;
}

export interface ContactProfile {
  username?: string | null;
  phone?: string | null;
  about?: string | null;
  business_name?: string | null;
  /** People of a group (not this account), for the @mention picker. */
  members?: { user: string; ids: string[] }[];
}

export interface ContactProfileBundle {
  profile: ContactProfile;
  media: Message[];
  docs: Message[];
  links: Message[];
  starred: Message[];
}

export type SearchScope = 'global' | 'account' | 'conversation';

export interface SearchMessageHit {
  message: Message;
  conversation_title: string;
  account_id: string;
}

export interface SearchResults {
  conversations: Conversation[];
  messages: SearchMessageHit[];
}

export interface CallState {
  call_id: string;
  conversation_id: string;
  account_id: string;
  direction: string;
  mode: string;
  status: string;
  remote_name?: string | null;
}

export interface Message {
  id: string;
  conversation_id: string;
  remote_id: string | null;
  sender_id: string | null;
  sender_name: string | null;
  direction: 'inbound' | 'outbound';
  body: string;
  timestamp: string;
  status: 'pending' | 'sent' | 'delivered' | 'read' | 'failed';
  metadata: Record<string, unknown>;
  starred?: boolean;
  pinned?: boolean;
  /** Reactions to this message, grouped per emoji (absent when there are none). */
  reactions?: ReactionGroup[];
  /** The message this one replies to, resolved for display (absent when it is not a reply). */
  reply?: ReplyPreview;
}

/**
 * The quote block of a reply. Built from the stored original when it exists; otherwise from
 * the snippet the connector reported (`available` false: shown, but not a link).
 */
export interface ReplyPreview {
  /** Remote id of the replied-to message. */
  remote_id: string;
  /** Local id of the stored original: the scroll target. Absent when it is not stored. */
  message_id?: string;
  /** Author of the original ("You" for this account's own messages). */
  sender_name?: string;
  from_me: boolean;
  /** Text or caption of the original. Empty for captionless media: word it with `quoteLabel`. */
  snippet: string;
  media_type?: string;
  /** File name of a document. */
  filename?: string;
  /** Cached small image of the original, when there is one. */
  thumbnail_path?: string;
  available: boolean;
}

/** Everyone who reacted to one message with the same emoji. */
export interface ReactionGroup {
  emoji: string;
  count: number;
  /** Reactor display names, oldest reaction first ("You" for this account). */
  names: string[];
  /** True when this account is one of the reactors. */
  mine: boolean;
}

/** One person's pick in a poll, as the core folded it. */
export interface PollVoter {
  /** Stable identity of the voter: their JID, or this account's own identity. */
  id: string;
  /** Display name, absent when the core knows none. */
  name?: string;
}

/** One option of a poll, with the votes the core has folded into it. */
export interface PollOption {
  /** Option id as the network reports it (WhatsApp's option hash). Votes are keyed by it. */
  id: string;
  text: string;
  votes: number;
  voters: PollVoter[];
  /** This account voted for this option. */
  mine: boolean;
}

/** A poll message's structured data (`message.metadata.poll`). */
export interface PollData {
  question: string;
  /** True when a voter may pick more than one option. */
  multiple: boolean;
  options: PollOption[];
}

export type AttachmentKind =
  | 'image'
  | 'video'
  | 'audio'
  | 'ptt'
  | 'document'
  | 'location'
  | 'poll'
  | 'sticker'
  | 'gif';

export interface AttachmentPayload {
  kind: AttachmentKind;
  caption?: string;
  filename?: string;
  mime?: string;
  data_base64?: string;
  latitude?: number;
  longitude?: number;
  question?: string;
  options?: string[];
  max_answer?: number;
  /** Local id of the message this one replies to (set by the page, not the composer). */
  reply_to_message_id?: string;
}

export interface ConnectorInfo {
  id: string;
  name: string;
  description: string;
  auth_type: string;
  capabilities: string[];
  /** What can be done to a message that is already on the network. */
  actions: MessageActions;
}

/** Message actions one network supports. The core enforces the same rules. */
export interface MessageActions {
  react: boolean;
  /** Own messages only, text or a media caption. */
  edit: boolean;
  /** Delete for me. */
  delete: boolean;
  /** Own messages only. */
  delete_everyone: boolean;
  /** Seconds after sending an own message can be edited; null = no limit. */
  edit_window_secs: number | null;
  /** Seconds after sending an own message can be deleted for everyone; null = no limit. */
  delete_everyone_window_secs: number | null;
}

/** One action on a sent message. `emoji: null` removes this account's reaction. */
export type MessageOp =
  | { kind: 'react'; emoji: string | null }
  | { kind: 'edit'; text: string }
  | { kind: 'delete_for_everyone' }
  | { kind: 'delete_for_me' };

/** A chat to forward into; any account. */
export interface ForwardTarget {
  account_id: string;
  conversation_id: string;
}

/** What happened to one source message for one target. `not_forwardable` is permanent. */
export type ForwardResult = ForwardTarget & { message_id: string } & (
    | { status: 'sent' }
    | { status: 'scheduled' }
    | { status: 'not_forwardable'; reason: string }
    | { status: 'failed'; error: string }
  );

/** A group member as a contact: their direct chat on the same account, if there is one. */
export interface SenderContact {
  conversation: Conversation | null;
  contact: Contact | null;
}

/** Outcome for one message of an `applyMessageOp` call; `error` is absent on success. */
export interface MessageOpResult {
  message_id: string;
  error?: string;
}

/** One translated message; exactly one of `text` / `error` is set. Never stored. */
export interface Translation {
  message_id: string;
  text?: string;
  error?: string;
}

export type TranslateError = { kind: 'not_configured' } | { kind: 'failed'; message: string };

export interface ComponentRequirement {
  id: string;
  label: string;
  size: number;
  installed: boolean;
  optional: boolean;
}

export interface ConnectorRequirements {
  connector_id: string;
  components: ComponentRequirement[];
  total_download_bytes: number;
}

export interface InstalledComponent {
  id: string;
  version?: string | null;
  sha256?: string | null;
  path: string;
  source: string;
}

export interface ComponentInstallProgress {
  component_id: string;
  bytes_done: number;
  bytes_total: number;
  phase: string;
}

export interface AppInfo {
  version: string;
  os: string;
  platform: string;
}

/** One component row of the About page; `size` stays null when the manifest is out of reach. */
export interface ComponentStatus {
  id: string;
  label: string;
  installed: boolean;
  optional: boolean;
  version?: string | null;
  size?: number | null;
  source?: string | null;
}

export interface ConnectorComponentStatus {
  connector_id: string;
  connector_label: string;
  components: ComponentStatus[];
}

export interface ComponentsOverview {
  platform: string;
  expected_version: string;
  manifest_version?: string | null;
  manifest_available: boolean;
  connectors: ConnectorComponentStatus[];
}

export interface UpdateAsset {
  name: string;
  url: string;
  size: number;
}

export interface UpdateCheck {
  current: string;
  latest: string;
  available: boolean;
  notes_url: string;
  asset?: UpdateAsset | null;
  checksums_url?: string | null;
}

export interface UpdateDownloadProgress {
  bytes_done: number;
  bytes_total: number;
}

/**
 * Event kinds the UI reacts to. Kinds it does not handle still arrive as plain strings.
 * Payload shapes: message.* / conversation.* carry the row, conversation.removed carries
 * {account_id, conversation_id, into_conversation_id}, message.removed carries
 * {account_id, conversation_id, message_id}, message.reactions carries
 * {account_id, conversation_id, updates: [{remote_id, reactions: ReactionGroup[]}]}.
 */
export type ShuttleEventKind =
  | 'message.received'
  | 'message.sent'
  | 'message.updated'
  | 'message.removed'
  | 'message.reactions'
  | 'conversation.updated'
  | 'conversation.removed'
  | 'inbox.catchup'
  | 'history.sync.completed'
  | 'contacts.synced'
  | 'status.feed'
  | 'reminder.fired'
  | 'scheduled_message.sent'
  | (string & {});

export interface ShuttleEvent {
  kind: ShuttleEventKind;
  payload: Record<string, unknown>;
}

export interface Workspace {
  id: string;
  name: string;
  builtin: boolean;
  sort_order: number;
}

export interface PriorityGroup {
  id: string;
  name: string;
  color: string | null;
  builtin: boolean;
  sort_order: number;
}

export interface ChatTodo {
  id: string;
  conversation_id: string;
  account_id: string;
  body: string;
  due_at: string | null;
  done: boolean;
  created_at: string;
  /** Message this todo was created from, if any. */
  message_id: string | null;
  /** Short quote of that message (first ~80 chars, or "📷 Photo" etc.). */
  message_snippet: string | null;
}

/** A per-chat note item; may quote a message and may carry one timer or alarm. */
export interface ChatNote {
  id: string;
  conversation_id: string;
  account_id: string;
  body: string;
  message_id: string | null;
  message_snippet: string | null;
  /** When the timer/alarm fires (ISO, UTC); null when the note has no schedule. */
  fire_at: string | null;
  kind: 'timer' | 'alarm' | null;
  fired: boolean;
  created_at: string;
}

/** Either a countdown in seconds or an absolute ISO date-time, never both. */
export interface NoteSchedule {
  timer_secs?: number;
  alarm_at?: string;
}

export interface ForwardRule {
  id: string;
  enabled: boolean;
  source_account_id: string | null;
  source_conversation_id: string | null;
  source_workspace_id: string | null;
  dest_account_id: string;
  dest_conversation_id: string;
  inbound_only: boolean;
  include_self: boolean;
  keyword: string | null;
  prefix: string | null;
  suffix: string | null;
  strip_sender: boolean;
  skip_if_forwarded: boolean;
  delay_seconds: number;
  created_at: string;
}

export interface ForwardRuleDraft {
  source_account_id?: string | null;
  source_conversation_id?: string | null;
  source_workspace_id?: string | null;
  dest_account_id: string;
  dest_conversation_id: string;
  inbound_only?: boolean;
  include_self?: boolean;
  keyword?: string | null;
  prefix?: string | null;
  suffix?: string | null;
  strip_sender?: boolean;
  skip_if_forwarded?: boolean;
  delay_seconds?: number;
}

export interface ForwardRulePatch {
  enabled?: boolean;
  source_account_id?: string;
  clear_source_account?: boolean;
  source_conversation_id?: string;
  clear_source_conversation?: boolean;
  source_workspace_id?: string;
  clear_source_workspace?: boolean;
  dest_account_id?: string;
  dest_conversation_id?: string;
  inbound_only?: boolean;
  include_self?: boolean;
  keyword?: string;
  clear_keyword?: boolean;
  prefix?: string;
  clear_prefix?: boolean;
  suffix?: string;
  clear_suffix?: boolean;
  strip_sender?: boolean;
  skip_if_forwarded?: boolean;
  delay_seconds?: number;
}

export interface ScheduledMessage {
  id: string;
  source_account_id: string | null;
  source_conversation_id: string | null;
  source_message_id: string | null;
  dest_account_id: string;
  dest_conversation_id: string;
  body: string;
  send_at: string;
  sent: boolean;
  created_at: string;
  attempts?: number;
  last_error?: string | null;
  failed?: boolean;
  /** Sends the source message itself when due; `body` is only its preview. */
  forward?: boolean;
}

export interface ScheduleMessageDraft {
  source_account_id?: string | null;
  source_conversation_id?: string | null;
  source_message_id?: string | null;
  dest_account_id: string;
  dest_conversation_id: string;
  body: string;
  send_at: string;
}

export interface BackupManifest {
  exported_at: string;
  includes_messages: boolean;
  includes_media?: boolean;
}

export interface ChannelStyle {
  tag?: string | null;
  background?: string | null;
  font?: string | null;
}

/** Where explicit saves go and how long the app-private download cache is kept. */
export interface MediaConfig {
  /** Folder for saved media; null/empty = `<Documents>/shuttle`. */
  save_dir?: string | null;
  /** Show the native save dialog for every save. */
  ask_each_time: boolean;
  /** Delete cached downloads unused for this many days; null = keep forever. */
  keep_days: number | null;
}

export interface MediaCacheInfo {
  bytes: number;
  files: number;
  /** The folder explicit saves go to right now. */
  save_dir: string;
}

export interface AppConfig {
  appearance: {
    color_scheme: 'system' | 'light' | 'dark' | string;
    theme_id: string;
    datetime_format?: string;
    font_scale?: number;
    tweakcn_css?: string | null;
    dark_schedule?: 'off' | 'system' | 'always' | 'time' | 'sun' | null;
    dark_from?: string | null;
    dark_to?: string | null;
    dark_location?: string | null;
    dark_latitude?: number | null;
    dark_longitude?: number | null;
  };
  notifications: {
    enabled: boolean;
    quiet_hours_enabled: boolean;
    quiet_hours_start: string;
    quiet_hours_end: string;
  };
  privacy: {
    crash_reports: boolean;
    usage_diagnostics: boolean;
  };
  sleep: {
    enabled: boolean;
    after_minutes: number;
    check_minutes: number;
  };
  channel_styles: Record<string, ChannelStyle>;
  media: MediaConfig;
  /** The user's own OpenRouter account for translation and later AI features. Empty key = off. */
  ai?: { openrouter_api_key: string; model: string };
  integrations?: { giphy_api_key: string };
  /** Linux WebView tuning; read by the Rust core at startup, never edited from the UI. */
  performance?: { low_memory_webview: boolean };
}

export interface AccountPatch {
  accent_color?: string;
  name?: string;
  muted?: boolean;
  disabled?: boolean;
  workspace_id?: string;
  clear_workspace?: boolean;
  notify_enabled?: boolean;
  clear_notify?: boolean;
  send_receipts?: boolean;
  sleep_enabled?: boolean | null;
  clear_sleep_enabled?: boolean;
  sleep_after_minutes?: number | null;
  clear_sleep_after?: boolean;
  sleep_check_minutes?: number | null;
  clear_sleep_check?: boolean;
  /** Replaces the account's auto-download choices. */
  auto_download?: AutoDownloadPrefs;
}

export interface ConversationPatch {
  pinned?: boolean;
  archived?: boolean;
  muted?: boolean;
  workspace_id?: string;
  clear_workspace?: boolean;
  priority_group?: string;
  clear_priority?: boolean;
  notes?: string;
  notify_enabled?: boolean;
  clear_notify?: boolean;
  send_receipts?: boolean;
  clear_receipts?: boolean;
  /** Replaces the chat's overrides (all unset = use the account's choice). */
  auto_download?: AutoDownloadPrefs;
}

export interface MenuItem {
  id: string;
  label: string;
  danger?: boolean;
  disabled?: boolean;
  separator?: boolean;
  /** Draw this entry as a row of compact buttons (the quick reaction bar). */
  row?: MenuItem[];
  /** Highlighted inside a row (the reaction this account already gave). */
  active?: boolean;
  /** Accessible name when `label` is a symbol. */
  title?: string;
}

export const CONNECTOR_COLORS: Record<string, string> = {
  whatsapp: '#25D366',
  telegram: '#2AABEE',
  signal: '#3A76F0',
  messenger: '#0084FF',
  instagram: '#E1306C',
  slack: '#4A154B',
  discord: '#5865F2',
  email: '#EA4335',
  matrix: '#0DBD8B',
};

export const CONNECTOR_ICONS: Record<string, string> = {
  whatsapp: 'W',
  telegram: 'T',
  signal: 'S',
  messenger: 'M',
  instagram: 'I',
  matrix: 'X',
};

export const THEME_PRESETS = [
  { id: 'cmlhfpjhw000004l4f4ax3m7z', label: 'Light Green (tweakcn)' },
  { id: 'shuttle', label: 'Shuttle' },
  { id: 'zinc', label: 'Zinc' },
  { id: 'ocean', label: 'Ocean' },
  { id: 'twilight', label: 'Twilight' },
] as const;
