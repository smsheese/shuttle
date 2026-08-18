export type AccountStatus = 'disconnected' | 'connecting' | 'connected' | 'error' | 'awaiting_auth';

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
  pinned: boolean;
  archived: boolean;
  muted: boolean;
  metadata: Record<string, unknown>;
  workspace_id?: string | null;
  priority_group?: string | null;
  notes?: string;
  notify_enabled?: boolean | null;
  send_receipts?: boolean | null;
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
}

export interface ConnectorInfo {
  id: string;
  name: string;
  description: string;
  auth_type: string;
  capabilities: string[];
}

export interface ShuttleEvent {
  kind: string;
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
}

export interface Reminder {
  id: string;
  conversation_id: string;
  account_id: string;
  fire_at: string;
  kind: string;
  note: string | null;
  fired: boolean;
  created_at: string;
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
}

export interface ChannelStyle {
  tag?: string | null;
  background?: string | null;
  font?: string | null;
}

export interface AppConfig {
  appearance: {
    color_scheme: 'system' | 'light' | 'dark' | string;
    theme_id: string;
    tweakcn_css?: string | null;
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
  channel_styles: Record<string, ChannelStyle>;
}

export interface AccountPatch {
  name?: string;
  muted?: boolean;
  disabled?: boolean;
  workspace_id?: string;
  clear_workspace?: boolean;
  notify_enabled?: boolean;
  clear_notify?: boolean;
  send_receipts?: boolean;
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
}

export interface MenuItem {
  id: string;
  label: string;
  danger?: boolean;
  disabled?: boolean;
  separator?: boolean;
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
  { id: 'shuttle', label: 'Shuttle' },
  { id: 'zinc', label: 'Zinc' },
  { id: 'ocean', label: 'Ocean' },
  { id: 'twilight', label: 'Twilight' },
] as const;
