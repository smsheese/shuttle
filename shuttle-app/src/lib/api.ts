import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { isTauri, mockApi } from './mock';
import type {
  Account,
  AccountPatch,
  AppConfig,
  AttachmentPayload,
  ChatTodo,
  ConnectorInfo,
  ComponentInstallProgress,
  ConnectorRequirements,
  InstalledComponent,
  Conversation,
  Contact,
  ContactProfileBundle,
  CallState,
  ConversationPatch,
  ForwardRule,
  ForwardRuleDraft,
  ForwardRulePatch,
  Message,
  PriorityGroup,
  BackupManifest,
  Reminder,
  ScheduledMessage,
  ScheduleMessageDraft,
  SearchResults,
  SearchScope,
  ShuttleEvent,
  Workspace,
} from './types';
import type { TelemetryErrorContext, TelemetryProps } from './telemetry/types';

export async function listAccounts(): Promise<Account[]> {
  return isTauri() ? invoke('list_accounts') : mockApi.listAccounts();
}

export async function listConnectors(): Promise<ConnectorInfo[]> {
  return isTauri() ? invoke('list_connectors') : mockApi.listConnectors();
}

export async function getConnectorRequirements(
  connectorId: string
): Promise<ConnectorRequirements> {
  return isTauri()
    ? invoke('get_connector_requirements', { connectorId })
    : {
        connector_id: connectorId,
        components: [],
        total_download_bytes: 0,
      };
}

export async function getInstalledComponents(): Promise<InstalledComponent[]> {
  return isTauri() ? invoke('get_installed_components') : [];
}

export async function ensureConnectorComponents(connectorId: string): Promise<void> {
  if (!isTauri()) return;
  await invoke('ensure_connector_components', { connectorId });
}

export async function cancelComponentInstall(): Promise<void> {
  if (!isTauri()) return;
  await invoke('cancel_component_install');
}

export function onComponentInstallProgress(
  handler: (progress: ComponentInstallProgress) => void
): Promise<UnlistenFn> {
  if (!isTauri()) return Promise.resolve(async () => {});
  return listen<ShuttleEvent>('shuttle-event', (e) => {
    if (e.payload.kind === 'component.install.progress') {
      handler(e.payload.payload as unknown as ComponentInstallProgress);
    }
  });
}

export function formatBytes(bytes: number): string {
  if (bytes <= 0) return '0 B';
  const units = ['B', 'KB', 'MB', 'GB'];
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value >= 10 || unit === 0 ? value.toFixed(0) : value.toFixed(1)} ${units[unit]}`;
}

export async function createAccount(connectorId: string, name: string): Promise<Account> {
  return isTauri()
    ? invoke('create_account', { connectorId, name })
    : mockApi.createAccount(connectorId, name);
}

export async function deleteAccount(accountId: string): Promise<void> {
  return isTauri() ? invoke('delete_account', { accountId }) : mockApi.deleteAccount(accountId);
}

export async function updateAccount(accountId: string, patch: AccountPatch): Promise<Account> {
  return isTauri()
    ? invoke('update_account', { accountId, patch })
    : mockApi.updateAccount(accountId, patch);
}

export async function wakeAccount(accountId: string): Promise<string> {
  return isTauri() ? invoke('wake_account', { accountId }) : mockApi.wakeAccount(accountId);
}

export async function setActiveAccount(accountId: string | null): Promise<void> {
  if (isTauri()) {
    await invoke('set_active_account', { accountId });
  } else {
    await mockApi.setActiveAccount(accountId);
  }
}

export async function connectAccount(
  accountId: string,
  credentials?: Record<string, string>
): Promise<string> {
  return isTauri()
    ? invoke('connect_account', { accountId, credentials: credentials ?? {} })
    : mockApi.connectAccount(accountId);
}

export async function submitAuth(
  accountId: string,
  credentials: Record<string, string>
): Promise<void> {
  return isTauri() ? invoke('submit_auth', { accountId, credentials }) : Promise.resolve();
}

export async function listConversations(
  accountId?: string,
  workspaceId?: string,
  priorityGroup?: string,
  archivedOnly?: boolean,
  offset = 0,
  limit = 30
): Promise<Conversation[]> {
  return isTauri()
    ? invoke('list_conversations', {
        accountId: accountId ?? null,
        workspaceId: workspaceId ?? null,
        priorityGroup: priorityGroup ?? null,
        archivedOnly: archivedOnly ?? false,
        offset,
        limit,
      })
    : mockApi.listConversations(accountId, workspaceId, priorityGroup, archivedOnly, offset, limit);
}

export async function countConversations(
  accountId?: string,
  workspaceId?: string,
  priorityGroup?: string,
  archivedOnly?: boolean
): Promise<number> {
  return isTauri()
    ? invoke('count_conversations', {
        accountId: accountId ?? null,
        workspaceId: workspaceId ?? null,
        priorityGroup: priorityGroup ?? null,
        archivedOnly: archivedOnly ?? false,
      })
    : mockApi.countConversations(accountId, workspaceId, priorityGroup, archivedOnly);
}

export async function listContacts(accountId: string): Promise<Contact[]> {
  return isTauri() ? invoke('list_contacts', { accountId }) : mockApi.listContacts(accountId);
}

export async function startConversation(
  accountId: string,
  remoteId: string,
  title: string
): Promise<Conversation> {
  return isTauri()
    ? invoke('start_conversation', { accountId, remoteId, title })
    : mockApi.startConversation(accountId, remoteId, title);
}

export async function createGroup(
  accountId: string,
  title: string,
  participants: string[]
): Promise<void> {
  if (isTauri()) {
    await invoke('create_group', { accountId, title, participants });
  } else {
    await mockApi.createGroup(accountId, title, participants);
  }
}

export async function downloadMessageMedia(
  accountId: string,
  conversationId: string,
  messageId: string
): Promise<void> {
  if (isTauri()) {
    await invoke('download_message_media', { accountId, conversationId, messageId });
  }
}

export async function downloadStatusMedia(accountId: string, messageId: string): Promise<void> {
  if (isTauri()) {
    await invoke('download_status_media', { accountId, messageId });
  }
}

export async function readMessageMedia(path: string): Promise<string> {
  return invoke('read_message_media', { path });
}

export async function shuttleFilesRoot(accountId: string): Promise<string> {
  return invoke('shuttle_files_root', { accountId });
}

export async function fetchConversationAvatar(
  accountId: string,
  conversationId: string
): Promise<void> {
  if (isTauri()) {
    await invoke('fetch_conversation_avatar', { accountId, conversationId });
  }
}

export async function syncConversation(accountId: string, conversationId: string): Promise<void> {
  if (isTauri()) {
    await invoke('sync_conversation', { accountId, conversationId });
  }
}

export async function updateConversation(
  conversationId: string,
  patch: ConversationPatch
): Promise<Conversation> {
  return isTauri()
    ? invoke('update_conversation', { conversationId, patch })
    : mockApi.updateConversation(conversationId, patch);
}

export async function getMessages(conversationId: string, limit?: number): Promise<Message[]> {
  return isTauri()
    ? invoke('get_messages', { conversationId, limit })
    : mockApi.getMessages(conversationId);
}

export async function sendMessage(
  accountId: string,
  conversationId: string,
  text: string
): Promise<Message> {
  return isTauri()
    ? invoke('send_message', { accountId, conversationId, text })
    : mockApi.sendMessage(accountId, conversationId, text);
}

export async function sendAttachment(
  accountId: string,
  conversationId: string,
  attachment: AttachmentPayload
): Promise<Message> {
  return isTauri()
    ? invoke('send_attachment', {
        accountId,
        conversationId,
        kind: attachment.kind,
        caption: attachment.caption ?? null,
        filename: attachment.filename ?? null,
        mime: attachment.mime ?? null,
        dataBase64: attachment.data_base64 ?? null,
        latitude: attachment.latitude ?? null,
        longitude: attachment.longitude ?? null,
        question: attachment.question ?? null,
        options: attachment.options ?? null,
        maxAnswer: attachment.max_answer ?? null,
      })
    : mockApi.sendAttachment(accountId, conversationId, attachment);
}

export async function forwardMessage(
  destAccountId: string,
  destConversationId: string,
  text: string
): Promise<Message> {
  return isTauri()
    ? invoke('forward_message', { destAccountId, destConversationId, text })
    : mockApi.sendMessage(destAccountId, destConversationId, text);
}

export async function markRead(conversationId: string, sendRemote?: boolean): Promise<void> {
  return isTauri()
    ? invoke('mark_read', { conversationId, sendRemote: sendRemote ?? null })
    : mockApi.markRead(conversationId);
}

export async function markUnread(conversationId: string): Promise<void> {
  return isTauri() ? invoke('mark_unread', { conversationId }) : mockApi.markUnread(conversationId);
}

export async function searchConversations(query: string): Promise<Conversation[]> {
  return isTauri()
    ? invoke('search_conversations', { query })
    : mockApi.searchConversations(query);
}

export async function searchMessages(
  query: string,
  scope: SearchScope,
  accountId?: string,
  conversationId?: string
): Promise<SearchResults> {
  return isTauri()
    ? invoke('search_messages', {
        query,
        scope,
        accountId: accountId ?? null,
        conversationId: conversationId ?? null,
      })
    : { conversations: await mockApi.searchConversations(query), messages: [] };
}

export async function starMessage(messageId: string, starred: boolean): Promise<Message> {
  return isTauri() ? invoke('star_message', { messageId, starred }) : ({} as Message);
}

export async function pinMessage(messageId: string, pinned: boolean): Promise<Message> {
  return isTauri() ? invoke('pin_message', { messageId, pinned }) : ({} as Message);
}

export async function fetchContactProfile(
  accountId: string,
  conversationId: string
): Promise<ContactProfileBundle> {
  return isTauri()
    ? invoke('fetch_contact_profile', { accountId, conversationId })
    : {
        profile: {},
        media: [],
        docs: [],
        links: [],
        starred: [],
      };
}

export async function startCall(
  accountId: string,
  conversationId: string,
  mode: 'audio' | 'video',
  shareScreen?: boolean
): Promise<CallState> {
  return isTauri()
    ? invoke('start_call', { accountId, conversationId, mode, shareScreen: shareScreen ?? false })
    : ({
        call_id: 'mock',
        conversation_id: conversationId,
        account_id: accountId,
        direction: 'outbound',
        mode,
        status: 'ringing',
      } as CallState);
}

export async function acceptCall(accountId: string, callId: string): Promise<void> {
  if (isTauri()) await invoke('accept_call', { accountId, callId });
}

export async function rejectCall(accountId: string, callId: string): Promise<void> {
  if (isTauri()) await invoke('reject_call', { accountId, callId });
}

export async function hangupCall(accountId: string, callId: string): Promise<void> {
  if (isTauri()) await invoke('hangup_call', { accountId, callId });
}

export async function totalUnread(): Promise<number> {
  return isTauri() ? invoke('total_unread') : mockApi.totalUnread();
}

export async function updateTrayUnread(count: number): Promise<void> {
  if (!isTauri()) return;
  await invoke('update_tray_unread', { count });
}

export async function getAppConfig(): Promise<AppConfig> {
  return isTauri() ? invoke('get_app_config') : mockApi.getAppConfig();
}

export async function saveAppConfig(config: AppConfig): Promise<AppConfig> {
  return isTauri() ? invoke('save_app_config', { config }) : mockApi.saveAppConfig(config);
}

export async function fetchTweakcnTheme(themeId: string): Promise<import('./tweakcn').TweakcnTheme> {
  if (isTauri()) {
    const json = await invoke<string>('fetch_tweakcn_theme', { themeId });
    return JSON.parse(json);
  }
  const res = await fetch(`https://tweakcn.com/r/themes/${encodeURIComponent(themeId)}`);
  if (!res.ok) {
    throw new Error(`Theme not found (${res.status})`);
  }
  return res.json();
}

export async function listWorkspaces(): Promise<Workspace[]> {
  return isTauri() ? invoke('list_workspaces') : mockApi.listWorkspaces();
}

export async function createWorkspace(name: string): Promise<Workspace> {
  return isTauri() ? invoke('create_workspace', { name }) : mockApi.createWorkspace(name);
}

export async function renameWorkspace(id: string, name: string): Promise<void> {
  return isTauri() ? invoke('rename_workspace', { id, name }) : mockApi.renameWorkspace(id, name);
}

export async function deleteWorkspace(id: string): Promise<void> {
  return isTauri() ? invoke('delete_workspace', { id }) : mockApi.deleteWorkspace(id);
}

export async function listPriorityGroups(): Promise<PriorityGroup[]> {
  return isTauri() ? invoke('list_priority_groups') : mockApi.listPriorityGroups();
}

export async function createPriorityGroup(name: string, color?: string): Promise<PriorityGroup> {
  return isTauri()
    ? invoke('create_priority_group', { name, color: color ?? null })
    : mockApi.createPriorityGroup(name, color);
}

export async function renamePriorityGroup(id: string, name: string): Promise<void> {
  return isTauri() ? invoke('rename_priority_group', { id, name }) : Promise.resolve();
}

export async function deletePriorityGroup(id: string): Promise<void> {
  return isTauri() ? invoke('delete_priority_group', { id }) : Promise.resolve();
}

export async function listTodos(conversationId: string): Promise<ChatTodo[]> {
  return isTauri() ? invoke('list_todos', { conversationId }) : mockApi.listTodos(conversationId);
}

export async function addTodo(
  conversationId: string,
  accountId: string,
  body: string,
  dueAt?: string
): Promise<ChatTodo> {
  return isTauri()
    ? invoke('add_todo', { conversationId, accountId, body, dueAt: dueAt ?? null })
    : mockApi.addTodo(conversationId, accountId, body, dueAt);
}

export async function setTodoDone(id: string, done: boolean): Promise<void> {
  return isTauri() ? invoke('set_todo_done', { id, done }) : mockApi.setTodoDone(id, done);
}

export async function deleteTodo(id: string): Promise<void> {
  return isTauri() ? invoke('delete_todo', { id }) : mockApi.deleteTodo(id);
}

export async function listReminders(conversationId?: string): Promise<Reminder[]> {
  return isTauri()
    ? invoke('list_reminders', { conversationId: conversationId ?? null })
    : mockApi.listReminders(conversationId);
}

export async function createReminder(
  conversationId: string,
  accountId: string,
  fireAt: string,
  kind?: string,
  note?: string
): Promise<Reminder> {
  return isTauri()
    ? invoke('create_reminder', {
        conversationId,
        accountId,
        fireAt,
        kind: kind ?? null,
        note: note ?? null,
      })
    : mockApi.createReminder(conversationId, accountId, fireAt, kind, note);
}

export async function deleteReminder(id: string): Promise<void> {
  return isTauri() ? invoke('delete_reminder', { id }) : mockApi.deleteReminder(id);
}

export async function listForwardRules(): Promise<ForwardRule[]> {
  return isTauri() ? invoke('list_forward_rules') : mockApi.listForwardRules();
}

export async function createForwardRule(draft: ForwardRuleDraft): Promise<ForwardRule> {
  return isTauri() ? invoke('create_forward_rule', { draft }) : mockApi.createForwardRule(draft);
}

export async function updateForwardRule(id: string, patch: ForwardRulePatch): Promise<ForwardRule> {
  return isTauri() ? invoke('update_forward_rule', { id, patch }) : mockApi.updateForwardRule(id, patch);
}

export async function deleteForwardRule(id: string): Promise<void> {
  return isTauri() ? invoke('delete_forward_rule', { id }) : mockApi.deleteForwardRule(id);
}

export async function listScheduledMessages(includeSent = false): Promise<ScheduledMessage[]> {
  return isTauri()
    ? invoke('list_scheduled_messages', { includeSent })
    : mockApi.listScheduledMessages(includeSent);
}

export async function scheduleMessage(draft: ScheduleMessageDraft): Promise<ScheduledMessage> {
  return isTauri() ? invoke('schedule_message', { draft }) : mockApi.scheduleMessage(draft);
}

export async function deleteScheduledMessage(id: string): Promise<void> {
  return isTauri() ? invoke('delete_scheduled_message', { id }) : mockApi.deleteScheduledMessage(id);
}

export async function updateScheduledMessage(
  id: string,
  patch: { body?: string; send_at?: string }
): Promise<ScheduledMessage> {
  return isTauri()
    ? invoke('update_scheduled_message', {
        id,
        body: patch.body ?? null,
        sendAt: patch.send_at ?? null,
      })
    : mockApi.updateScheduledMessage(id, patch);
}

export async function exportBackup(
  path: string,
  password: string,
  includeMessages = true,
  includeMedia = false
): Promise<BackupManifest> {
  return isTauri()
    ? invoke('export_backup', { path, password, includeMessages, includeMedia })
    : mockApi.exportBackup(path, password, includeMessages, includeMedia);
}

export async function restoreBackup(path: string, password: string): Promise<void> {
  return isTauri() ? invoke('restore_backup', { path, password }) : mockApi.restoreBackup(path, password);
}

export async function restartApp(): Promise<void> {
  if (isTauri()) await invoke('restart_app');
}

export async function openExternal(url: string): Promise<void> {
  if (isTauri()) await invoke('open_external', { url });
  else window.open(url, '_blank');
}

export async function openDevtools(): Promise<void> {
  if (isTauri()) await invoke('open_devtools');
}

export function onShuttleEvent(handler: (event: ShuttleEvent) => void): Promise<UnlistenFn> {
  if (!isTauri()) return mockApi.onShuttleEvent();
  return listen<ShuttleEvent>('shuttle-event', (e) => handler(e.payload));
}

export async function telemetryTrack(
  event: string,
  props: TelemetryProps = {}
): Promise<void> {
  if (!isTauri()) return;
  await invoke('telemetry_track', { event, props });
}

export async function telemetryError(
  message: string,
  context: TelemetryErrorContext = {}
): Promise<void> {
  if (!isTauri()) return;
  await invoke('telemetry_error', { message, context });
}

export async function telemetryPerformance(
  operation: string,
  props: TelemetryProps = {}
): Promise<void> {
  if (!isTauri()) return;
  await invoke('telemetry_performance', { operation, props });
}

export async function telemetrySetForeground(foreground: boolean): Promise<void> {
  if (!isTauri()) return;
  await invoke('telemetry_set_foreground', { foreground });
}

export const DATETIME_FORMATS: { id: string; label: string; example: string }[] = [
  { id: '12h_full', label: '12-hour with date', example: '07:02 AM 19 Aug' },
  { id: '24h_full', label: '24-hour with date', example: '19:02 19 Aug' },
  { id: '12h_long', label: '12-hour, full date', example: '07:02 AM Aug 19, 2026' },
  { id: '24h_long', label: '24-hour, full date', example: '19:02 19 Aug 2026' },
  { id: 'relative', label: 'Relative (today / weekday)', example: '7:02 AM' },
  { id: 'eu', label: 'Day/month/year 24-hour', example: '19/08/2026 19:02' },
  { id: 'us', label: 'Month/day/year 12-hour', example: '08/19/2026 07:02 AM' },
  { id: 'iso', label: 'ISO-style', example: '2026-08-19 19:02' },
];

const MONTHS_SHORT = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'];

function pad2(n: number): string {
  return String(n).padStart(2, '0');
}

function clock12(d: Date): { h: string; mm: string; ampm: string } {
  const hour = d.getHours();
  return {
    h: pad2(hour % 12 || 12),
    mm: pad2(d.getMinutes()),
    ampm: hour >= 12 ? 'PM' : 'AM',
  };
}

export function formatTime(iso: string | null, format = '12h_full'): string {
  if (!iso) return '';
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return '';
  const dd = pad2(d.getDate());
  const mmm = MONTHS_SHORT[d.getMonth()];
  const yyyy = d.getFullYear();
  const h24 = pad2(d.getHours());
  const mm = pad2(d.getMinutes());
  const c12 = clock12(d);

  switch (format) {
    case '24h_full':
      return `${h24}:${mm} ${dd} ${mmm}`;
    case '12h_long':
      return `${c12.h}:${c12.mm} ${c12.ampm} ${mmm} ${d.getDate()}, ${yyyy}`;
    case '24h_long':
      return `${h24}:${mm} ${dd} ${mmm} ${yyyy}`;
    case 'relative': {
      const now = new Date();
      const diff = now.getTime() - d.getTime();
      if (diff < 86400000 && d.getDate() === now.getDate()) {
        return `${c12.h}:${c12.mm} ${c12.ampm}`;
      }
      if (diff < 604800000) {
        return d.toLocaleDateString([], { weekday: 'short' });
      }
      return `${dd} ${mmm}`;
    }
    case 'eu':
      return `${dd}/${pad2(d.getMonth() + 1)}/${yyyy} ${h24}:${mm}`;
    case 'us':
      return `${pad2(d.getMonth() + 1)}/${dd}/${yyyy} ${c12.h}:${c12.mm} ${c12.ampm}`;
    case 'iso':
      return `${yyyy}-${pad2(d.getMonth() + 1)}-${dd} ${h24}:${mm}`;
    case '12h_full':
    default:
      return `${c12.h}:${c12.mm} ${c12.ampm} ${dd} ${mmm}`;
  }
}

export function getInitials(name: string): string {
  return name
    .split(' ')
    .map((w) => w[0])
    .join('')
    .slice(0, 2)
    .toUpperCase();
}

export function avatarColor(name: string): string {
  const colors = ['#6C5CE7', '#00B894', '#0984E3', '#E17055', '#FDCB6E', '#E84393', '#636E72'];
  let hash = 0;
  for (let i = 0; i < name.length; i++) hash = name.charCodeAt(i) + ((hash << 5) - hash);
  return colors[Math.abs(hash) % colors.length];
}

export function conversationAvatar(conv: { metadata?: Record<string, unknown> | null }): string | null {
  const data = conv.metadata?.avatar_data ?? conv.metadata?.avatar_url;
  return typeof data === 'string' && data.length > 8 ? data : null;
}

export function accountAvatar(account: {
  metadata?: Record<string, unknown> | null;
  name?: string;
}): string | null {
  const data = account.metadata?.avatar_data ?? account.metadata?.avatar_url;
  return typeof data === 'string' && data.length > 8 ? data : null;
}
