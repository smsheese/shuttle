import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { isTauri, mockApi } from './mock';
import type {
  Account,
  AccountPatch,
  AppConfig,
  ChatTodo,
  ConnectorInfo,
  Conversation,
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
  priorityGroup?: string
): Promise<Conversation[]> {
  return isTauri()
    ? invoke('list_conversations', {
        accountId: accountId ?? null,
        workspaceId: workspaceId ?? null,
        priorityGroup: priorityGroup ?? null,
      })
    : mockApi.listConversations(accountId, workspaceId, priorityGroup);
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

export async function totalUnread(): Promise<number> {
  return isTauri() ? invoke('total_unread') : mockApi.totalUnread();
}

export async function getAppConfig(): Promise<AppConfig> {
  return isTauri() ? invoke('get_app_config') : mockApi.getAppConfig();
}

export async function saveAppConfig(config: AppConfig): Promise<AppConfig> {
  return isTauri() ? invoke('save_app_config', { config }) : mockApi.saveAppConfig(config);
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

export async function exportBackup(
  path: string,
  password: string,
  includeMessages = true
): Promise<BackupManifest> {
  return isTauri()
    ? invoke('export_backup', { path, password, includeMessages })
    : mockApi.exportBackup(path, password, includeMessages);
}

export async function restoreBackup(path: string, password: string): Promise<void> {
  return isTauri() ? invoke('restore_backup', { path, password }) : mockApi.restoreBackup(path, password);
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

export function formatTime(iso: string | null): string {
  if (!iso) return '';
  const d = new Date(iso);
  const now = new Date();
  const diff = now.getTime() - d.getTime();
  if (diff < 86400000 && d.getDate() === now.getDate()) {
    return d.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
  }
  if (diff < 604800000) {
    return d.toLocaleDateString([], { weekday: 'short' });
  }
  return d.toLocaleDateString([], { month: 'short', day: 'numeric' });
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
