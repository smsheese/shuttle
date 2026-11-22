import { invoke, convertFileSrc } from '@tauri-apps/api/core';
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
  MediaCacheInfo,
  Conversation,
  Contact,
  ContactProfileBundle,
  CallState,
  ConversationPatch,
  ForwardRule,
  ForwardRuleDraft,
  ForwardRulePatch,
  ForwardResult,
  ForwardTarget,
  Message,
  MessageOp,
  MessageOpResult,
  PriorityGroup,
  BackupManifest,
  ChatNote,
  NoteSchedule,
  ScheduledMessage,
  ScheduleMessageDraft,
  SearchResults,
  SearchScope,
  SenderContact,
  ShuttleEvent,
  Translation,
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

/**
 * Ask the connector whether a number is on WhatsApp. Resolves true/false, or null when the
 * check could not be made (offline, sleeping account, timeout): the caller carries on then.
 */
export async function checkWhatsappNumber(accountId: string, phone: string): Promise<boolean | null> {
  if (!isTauri()) return mockApi.checkWhatsappNumber(phone);
  const requestId = crypto.randomUUID();
  let finish!: (value: boolean | null) => void;
  const answered = new Promise<boolean | null>((resolve) => { finish = resolve; });
  const unlisten = await onShuttleEvent((event) => {
    if (event.kind !== 'number.checked' || event.payload.account_id !== accountId) return;
    const data = event.payload.data as { request_id?: string; on_whatsapp?: boolean | null } | undefined;
    if (data?.request_id !== requestId) return;
    finish(typeof data.on_whatsapp === 'boolean' ? data.on_whatsapp : null);
  });
  const timeout = setTimeout(() => finish(null), 6000);
  try {
    // Subscribed first, so a fast reply cannot be missed.
    await invoke('check_whatsapp_number', { accountId, phone, requestId });
    return await answered;
  } catch {
    return null;
  } finally {
    clearTimeout(timeout);
    unlisten();
  }
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

/**
 * Download a message's media into the app cache. `auto` marks a download nobody asked for (a
 * thread rendering): the core only goes ahead when the chat/account auto-download setting
 * allows that kind. Returns whether a download was requested.
 */
export async function downloadMessageMedia(
  accountId: string,
  conversationId: string,
  messageId: string,
  auto = false
): Promise<boolean> {
  if (!isTauri()) return false;
  return invoke<boolean>('download_message_media', { accountId, conversationId, messageId, auto });
}

export async function downloadStatusMedia(accountId: string, messageId: string): Promise<void> {
  if (isTauri()) {
    await invoke('download_status_media', { accountId, messageId });
  }
}

/** Subscribe before requesting: the command acknowledges queuing, not completion. */
export async function downloadMessageMediaAndWait(
  accountId: string, conversationId: string, messageId: string
): Promise<Message> {
  let finish!: (message: Message) => void;
  let fail!: (error: Error) => void;
  const completed = new Promise<Message>((resolve, reject) => { finish = resolve; fail = reject; });
  const unlisten = await onShuttleEvent((event) => {
    if (event.kind !== 'media.downloaded' || event.payload.account_id !== accountId) return;
    const message = event.payload.message as Message | undefined;
    if (message?.id !== messageId || message.conversation_id !== conversationId) return;
    if (message.metadata?.media_error) fail(new Error(String(message.metadata.media_error)));
    else finish(message);
  });
  const timeout = setTimeout(() => fail(new Error('Media download timed out')), 120000);
  try {
    // Observe completion while the invoke is in flight so failures never become
    // unhandled rejections and fast connector replies cannot be missed.
    const [message] = await Promise.all([completed, downloadMessageMedia(accountId, conversationId, messageId)]);
    return message;
  } finally {
    clearTimeout(timeout);
    unlisten();
  }
}

export async function readMessageMedia(path: string): Promise<string> {
  const localPath = await invoke<string>('read_message_media', { path });
  return convertFileSrc(localPath);
}

/** The two names that head a generated file name. */
export interface MediaNameOwner {
  accountName?: string | null;
  chatName?: string | null;
}

/** What the UI knows about a media message; the core builds a readable file name from it. */
export interface MediaNameInfo {
  /** `image`, `video`, `audio`, `sticker`, `document`, ... */
  kind?: string | null;
  /** The file name WhatsApp reported (kept for documents). */
  filename?: string | null;
  /** Message time (RFC 3339). */
  timestamp?: string | null;
  /** The account's name, the first half of a generated name. */
  accountName?: string | null;
  /** The chat's title, the second half of a generated name. */
  chatName?: string | null;
}

/**
 * Copy a downloaded file to the user's save folder (native dialog when "ask each time" is on).
 * Resolves the path written, or null when the user cancelled. Never overwrites. The file is
 * named from `name` (documents keep their name) or "Work-Mum-261112-1325.jpg" style, built
 * from the account, the chat and the message time.
 */
export async function saveMediaFile(sourcePath: string, name: MediaNameInfo = {}): Promise<string | null> {
  return invoke<string | null>('save_media_file', {
    sourcePath,
    suggestedName: name.filename ?? null,
    kind: name.kind ?? null,
    timestamp: name.timestamp ?? null,
    accountName: name.accountName ?? null,
    chatName: name.chatName ?? null,
  });
}

/** What `copyMediaToClipboard` put on the clipboard (see src-tauri/src/clipboard.rs). */
export type ClipboardCopyKind = 'image' | 'file' | 'path_text';

/**
 * Copy a downloaded media file to the system clipboard so it pastes into other apps: image
 * data plus a file reference for images, a file reference otherwise. The pasted file has a
 * readable name built from `name`. `path_text` means the OS could not take a file and only
 * the path was copied.
 */
export async function copyMediaToClipboard(
  path: string,
  name: MediaNameInfo = {}
): Promise<ClipboardCopyKind> {
  return invoke<ClipboardCopyKind>('copy_media_to_clipboard', {
    path,
    kind: name.kind ?? null,
    filename: name.filename ?? null,
    timestamp: name.timestamp ?? null,
    accountName: name.accountName ?? null,
    chatName: name.chatName ?? null,
  });
}

/** Show a saved file selected in the OS file manager. */
export async function revealInFileManager(path: string): Promise<void> {
  await invoke('reveal_in_file_manager', { path });
}

export async function mediaCacheInfo(): Promise<MediaCacheInfo> {
  return isTauri()
    ? invoke('media_cache_info')
    : { bytes: 0, files: 0, save_dir: '~/Documents/shuttle' };
}

export async function clearMediaCache(): Promise<{ files: number; bytes: number }> {
  return isTauri() ? invoke('clear_media_cache') : { files: 0, bytes: 0 };
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
  text: string,
  replyToMessageId?: string | null
): Promise<Message> {
  return isTauri()
    ? invoke('send_message', {
        accountId,
        conversationId,
        text,
        replyToMessageId: replyToMessageId ?? null,
      })
    : mockApi.sendMessage(accountId, conversationId, text, replyToMessageId ?? null);
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
        replyToMessageId: attachment.reply_to_message_id ?? null,
      })
    : mockApi.sendAttachment(accountId, conversationId, attachment);
}

/**
 * Forward messages of one chat to chats on any account. Each message is sent again as itself
 * (text, or media with its caption); with `sendAt` (ISO) the forward is scheduled instead.
 * One result per (target, message).
 */
export async function forwardMessages(
  sourceConversationId: string,
  messageIds: string[],
  targets: ForwardTarget[],
  sendAt?: string | null
): Promise<ForwardResult[]> {
  return isTauri()
    ? invoke('forward_messages', { sourceConversationId, messageIds, targets, sendAt: sendAt ?? null })
    : mockApi.forwardMessages(sourceConversationId, messageIds, targets, sendAt ?? null);
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

/** React to, edit or delete messages of one chat. The stored result arrives as events. */
export async function applyMessageOp(
  conversationId: string,
  messageIds: string[],
  op: MessageOp
): Promise<MessageOpResult[]> {
  return isTauri()
    ? invoke('apply_message_op', { conversationId, messageIds, op })
    : mockApi.applyMessageOp(conversationId, messageIds, op);
}

/**
 * Translate messages of one chat into `target` with the user's OpenRouter model. Rejects with a
 * `TranslateError` (`not_configured` when Settings has no OpenRouter key).
 */
export async function translateText(
  conversationId: string,
  messageIds: string[],
  target: string
): Promise<Translation[]> {
  return isTauri()
    ? invoke('translate_text', { conversationId, messageIds, target })
    : mockApi.translateText(conversationId, messageIds, target);
}

export async function starMessage(messageId: string, starred: boolean): Promise<Message> {
  return isTauri() ? invoke('star_message', { messageId, starred }) : mockApi.starMessage(messageId, starred);
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

/** A group member's direct chat on the same account, or their contact entry. Creates nothing. */
export async function senderContact(accountId: string, senderId: string): Promise<SenderContact> {
  return isTauri()
    ? invoke('sender_contact', { accountId, senderId })
    : mockApi.senderContact(accountId, senderId);
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

export async function unreadByAccount(): Promise<Record<string, number>> {
  if (isTauri()) return invoke('unread_by_account');
  const conversations = await mockApi.listConversations(undefined, undefined, undefined, false, 0, 10000);
  return conversations.reduce((out: Record<string, number>, c: Conversation) => { out[c.account_id] = (out[c.account_id] ?? 0) + c.unread_count; return out; }, {});
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
  dueAt?: string,
  messageRef?: MessageRef
): Promise<ChatTodo> {
  return isTauri()
    ? invoke('add_todo', {
        conversationId,
        accountId,
        body,
        dueAt: dueAt ?? null,
        messageId: messageRef?.id ?? null,
        messageSnippet: messageRef?.snippet ?? null,
      })
    : mockApi.addTodo(conversationId, accountId, body, dueAt, messageRef);
}

export async function setTodoDone(id: string, done: boolean): Promise<void> {
  return isTauri() ? invoke('set_todo_done', { id, done }) : mockApi.setTodoDone(id, done);
}

export async function deleteTodo(id: string): Promise<void> {
  return isTauri() ? invoke('delete_todo', { id }) : mockApi.deleteTodo(id);
}

/** A message a todo or note was created from. */
export interface MessageRef {
  id: string;
  snippet: string;
}

export async function listNotes(conversationId: string): Promise<ChatNote[]> {
  return isTauri() ? invoke('list_notes', { conversationId }) : mockApi.listNotes(conversationId);
}

export async function addNote(
  conversationId: string,
  accountId: string,
  body: string,
  opts: { messageRef?: MessageRef; schedule?: NoteSchedule } = {}
): Promise<ChatNote> {
  return isTauri()
    ? invoke('add_note', {
        conversationId,
        accountId,
        body,
        messageId: opts.messageRef?.id ?? null,
        messageSnippet: opts.messageRef?.snippet ?? null,
        schedule: opts.schedule ?? null,
      })
    : mockApi.addNote(conversationId, accountId, body, opts);
}

/** Change a note's text and/or schedule; `clearSchedule` cancels its timer or alarm. */
export async function updateNote(
  id: string,
  patch: { body?: string; schedule?: NoteSchedule; clearSchedule?: boolean }
): Promise<ChatNote> {
  return isTauri()
    ? invoke('update_note', {
        id,
        body: patch.body ?? null,
        schedule: patch.schedule ?? null,
        clearSchedule: patch.clearSchedule ?? false,
      })
    : mockApi.updateNote(id, patch);
}

export async function deleteNote(id: string): Promise<void> {
  return isTauri() ? invoke('delete_note', { id }) : mockApi.deleteNote(id);
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
  if (!isTauri()) return mockApi.onShuttleEvent(handler);
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

export async function telemetryInstallationId(): Promise<string> {
  if (!isTauri()) return '';
  return invoke<string>('telemetry_installation_id');
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
