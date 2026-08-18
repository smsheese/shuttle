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
  Workspace,
} from './types';

const DEMO_ACCOUNTS: Account[] = [
  {
    id: 'wa-1',
    connector_id: 'whatsapp',
    name: 'WhatsApp Work',
    identity: '+1 555-0100',
    status: 'connected',
    metadata: {},
    created_at: new Date().toISOString(),
    updated_at: new Date().toISOString(),
  },
  {
    id: 'tg-1',
    connector_id: 'telegram',
    name: 'Telegram Personal',
    identity: '@alexdev',
    status: 'connected',
    metadata: {},
    created_at: new Date().toISOString(),
    updated_at: new Date().toISOString(),
  },
];

const DEMO_CONVERSATIONS: Conversation[] = [
  {
    id: 'c1',
    account_id: 'wa-1',
    remote_id: 'r1',
    contact_id: null,
    title: 'Sarah Chen',
    conversation_type: 'direct',
    unread_count: 2,
    last_message_at: new Date(Date.now() - 600000).toISOString(),
    last_message_preview: 'See you tomorrow! 🎉',
    pinned: true,
    archived: false,
    muted: false,
    metadata: {},
  },
  {
    id: 'c2',
    account_id: 'wa-1',
    remote_id: 'r2',
    contact_id: null,
    title: 'Design Team',
    conversation_type: 'group',
    unread_count: 5,
    last_message_at: new Date(Date.now() - 7200000).toISOString(),
    last_message_preview: 'Alex: Updated the mockups in Figma',
    pinned: false,
    archived: false,
    muted: false,
    metadata: {},
  },
  {
    id: 'c3',
    account_id: 'tg-1',
    remote_id: 'r3',
    contact_id: null,
    title: 'Mom',
    conversation_type: 'direct',
    unread_count: 1,
    last_message_at: new Date(Date.now() - 18000000).toISOString(),
    last_message_preview: "Don't forget to call grandma this weekend ❤️",
    pinned: false,
    archived: false,
    muted: false,
    metadata: {},
  },
  {
    id: 'c4',
    account_id: 'tg-1',
    remote_id: 'r4',
    contact_id: null,
    title: 'Rust Developers',
    conversation_type: 'group',
    unread_count: 12,
    last_message_at: new Date(Date.now() - 86400000).toISOString(),
    last_message_preview: 'New async patterns discussion thread',
    pinned: false,
    archived: false,
    muted: false,
    metadata: {},
  },
  {
    id: 'c5',
    account_id: 'wa-1',
    remote_id: 'r5',
    contact_id: null,
    title: 'James Park',
    conversation_type: 'direct',
    unread_count: 0,
    last_message_at: new Date(Date.now() - 172800000).toISOString(),
    last_message_preview: 'Thanks for the intro!',
    pinned: false,
    archived: false,
    muted: false,
    metadata: {},
  },
];

const DEMO_MESSAGES: Record<string, Message[]> = {
  c1: [
    {
      id: 'm1',
      conversation_id: 'c1',
      remote_id: 'rm1',
      sender_id: null,
      sender_name: 'Sarah Chen',
      direction: 'inbound',
      body: 'Hey! Are we still on for lunch tomorrow?',
      timestamp: new Date(Date.now() - 14400000).toISOString(),
      status: 'delivered',
      metadata: {},
    },
    {
      id: 'm1b',
      conversation_id: 'c1',
      remote_id: 'rm1b',
      sender_id: null,
      sender_name: 'Sarah Chen',
      direction: 'inbound',
      body: 'I was thinking that new Thai place on 5th — heard great things',
      timestamp: new Date(Date.now() - 14340000).toISOString(),
      status: 'delivered',
      metadata: {},
    },
    {
      id: 'm2',
      conversation_id: 'c1',
      remote_id: 'rm2',
      sender_id: null,
      sender_name: 'You',
      direction: 'outbound',
      body: 'Yes! How about 12:30?',
      timestamp: new Date(Date.now() - 10800000).toISOString(),
      status: 'read',
      metadata: {},
    },
    {
      id: 'm2b',
      conversation_id: 'c1',
      remote_id: 'rm2b',
      sender_id: null,
      sender_name: 'You',
      direction: 'outbound',
      body: 'The usual spot works too if you prefer something closer',
      timestamp: new Date(Date.now() - 10740000).toISOString(),
      status: 'read',
      metadata: {},
    },
    {
      id: 'm3',
      conversation_id: 'c1',
      remote_id: 'rm3',
      sender_id: null,
      sender_name: 'Sarah Chen',
      direction: 'inbound',
      body: 'Perfect, 12:30 at Thai Basil sounds great',
      timestamp: new Date(Date.now() - 7200000).toISOString(),
      status: 'delivered',
      metadata: {},
    },
    {
      id: 'm3b',
      conversation_id: 'c1',
      remote_id: 'rm3b',
      sender_id: null,
      sender_name: 'Sarah Chen',
      direction: 'inbound',
      body: 'Should I book a table?',
      timestamp: new Date(Date.now() - 7140000).toISOString(),
      status: 'delivered',
      metadata: {},
    },
    {
      id: 'm4',
      conversation_id: 'c1',
      remote_id: 'rm4',
      sender_id: null,
      sender_name: 'You',
      direction: 'outbound',
      body: 'Yes please! Party of 2',
      timestamp: new Date(Date.now() - 5400000).toISOString(),
      status: 'read',
      metadata: {},
    },
    {
      id: 'm5',
      conversation_id: 'c1',
      remote_id: 'rm5',
      sender_id: null,
      sender_name: 'Sarah Chen',
      direction: 'inbound',
      body: 'Done ✓ Table for 2 at 12:30',
      timestamp: new Date(Date.now() - 3600000).toISOString(),
      status: 'delivered',
      metadata: {},
    },
    {
      id: 'm6',
      conversation_id: 'c1',
      remote_id: 'rm6',
      sender_id: null,
      sender_name: 'You',
      direction: 'outbound',
      body: "You're the best 🙌",
      timestamp: new Date(Date.now() - 1800000).toISOString(),
      status: 'read',
      metadata: {},
    },
    {
      id: 'm7',
      conversation_id: 'c1',
      remote_id: 'rm7',
      sender_id: null,
      sender_name: 'Sarah Chen',
      direction: 'inbound',
      body: 'See you tomorrow! 🎉',
      timestamp: new Date(Date.now() - 600000).toISOString(),
      status: 'delivered',
      metadata: {},
    },
  ],
  c2: [
    {
      id: 'm4',
      conversation_id: 'c2',
      remote_id: 'rm4',
      sender_id: null,
      sender_name: 'Alex',
      direction: 'inbound',
      body: 'Updated the mockups in Figma — take a look when you get a chance',
      timestamp: new Date(Date.now() - 7200000).toISOString(),
      status: 'delivered',
      metadata: {},
    },
  ],
};

const CONNECTORS: ConnectorInfo[] = [
  {
    id: 'whatsapp',
    name: 'WhatsApp',
    description: 'Scan QR code with WhatsApp on your phone',
    auth_type: 'qr',
    capabilities: ['text', 'media', 'read_receipts', 'groups'],
  },
  {
    id: 'telegram',
    name: 'Telegram',
    description: 'Log in with your phone number',
    auth_type: 'phone',
    capabilities: ['text', 'media', 'read_receipts', 'groups', 'channels'],
  },
  {
    id: 'signal',
    name: 'Signal',
    description: 'Register with your phone number',
    auth_type: 'phone',
    capabilities: ['text', 'media', 'read_receipts', 'groups'],
  },
  {
    id: 'messenger',
    name: 'Messenger',
    description: 'Log in with Facebook email and password',
    auth_type: 'password',
    capabilities: ['text', 'media', 'groups'],
  },
  {
    id: 'instagram',
    name: 'Instagram',
    description: 'Log in to Instagram DMs',
    auth_type: 'password',
    capabilities: ['text', 'media'],
  },
  {
    id: 'email',
    name: 'Email',
    description: 'Connect IMAP and SMTP',
    auth_type: 'email',
    capabilities: ['text'],
  },
  {
    id: 'matrix',
    name: 'Matrix',
    description: 'Log in to a Matrix homeserver',
    auth_type: 'password',
    capabilities: ['text', 'groups', 'channels'],
  },
];

let accounts = [...DEMO_ACCOUNTS];
let conversations = [...DEMO_CONVERSATIONS];
const messages = { ...DEMO_MESSAGES };
let workspaces: Workspace[] = [
  { id: 'personal', name: 'Personal', builtin: true, sort_order: 0 },
  { id: 'work', name: 'Work', builtin: true, sort_order: 1 },
  { id: 'others', name: 'Others', builtin: true, sort_order: 2 },
];
let priorityGroups: PriorityGroup[] = [
  { id: 'urgent', name: 'Urgent', color: '#ef4444', builtin: true, sort_order: 0 },
  { id: 'waiting', name: 'Waiting', color: '#f59e0b', builtin: true, sort_order: 1 },
  { id: 'later', name: 'Later', color: '#64748b', builtin: true, sort_order: 2 },
];
let todos: ChatTodo[] = [];
let reminders: Reminder[] = [];
let forwardRules: ForwardRule[] = [];
let scheduledMessages: ScheduledMessage[] = [];
let appConfig: AppConfig = {
  appearance: { color_scheme: 'system', theme_id: 'shuttle', tweakcn_css: null },
  notifications: {
    enabled: true,
    quiet_hours_enabled: false,
    quiet_hours_start: '22:00',
    quiet_hours_end: '08:00',
  },
  privacy: {
    crash_reports: false,
    usage_diagnostics: false,
  },
  channel_styles: {
    whatsapp: { tag: '#25D366' },
    telegram: { tag: '#2AABEE' },
    signal: { tag: '#3A76F0' },
    messenger: { tag: '#0084FF' },
    instagram: { tag: '#E1306C' },
    email: { tag: '#EA4335' },
    matrix: { tag: '#0DBD8B' },
  },
};

export const mockApi = {
  listAccounts: async () => accounts,
  listConnectors: async () => CONNECTORS,
  createAccount: async (connectorId: string, name: string) => {
    const a: Account = {
      id: crypto.randomUUID(),
      connector_id: connectorId,
      name,
      identity: null,
      status: 'awaiting_auth',
      metadata: {},
      created_at: new Date().toISOString(),
      updated_at: new Date().toISOString(),
      disabled: false,
      muted: false,
      send_receipts: false,
    };
    accounts = [...accounts, a];
    return a;
  },
  deleteAccount: async (id: string) => {
    accounts = accounts.filter((a) => a.id !== id);
    conversations = conversations.filter((c) => c.account_id !== id);
  },
  updateAccount: async (id: string, patch: AccountPatch) => {
    accounts = accounts.map((a) => (a.id === id ? { ...a, ...patch } : a));
    return accounts.find((a) => a.id === id)!;
  },
  connectAccount: async (_accountId?: string) => 'ok',
  listConversations: async (accountId?: string, workspaceId?: string, priorityGroup?: string) => {
    let list = accountId ? conversations.filter((c) => c.account_id === accountId) : conversations;
    if (workspaceId) list = list.filter((c) => (c.workspace_id ?? 'others') === workspaceId);
    if (priorityGroup) list = list.filter((c) => c.priority_group === priorityGroup);
    return list;
  },
  updateConversation: async (id: string, patch: ConversationPatch) => {
    conversations = conversations.map((c) => (c.id === id ? { ...c, ...patch } : c));
    return conversations.find((c) => c.id === id)!;
  },
  getMessages: async (conversationId: string) => messages[conversationId] ?? [],
  sendMessage: async (accountId: string, conversationId: string, text: string) => {
    const msg: Message = {
      id: crypto.randomUUID(),
      conversation_id: conversationId,
      remote_id: null,
      sender_id: null,
      sender_name: 'You',
      direction: 'outbound',
      body: text,
      timestamp: new Date().toISOString(),
      status: 'sent',
      metadata: {},
    };
    messages[conversationId] = [...(messages[conversationId] ?? []), msg];
    conversations = conversations.map((c) =>
      c.id === conversationId
        ? { ...c, last_message_preview: text, last_message_at: msg.timestamp }
        : c
    );
    return msg;
  },
  markRead: async (conversationId: string) => {
    conversations = conversations.map((c) =>
      c.id === conversationId ? { ...c, unread_count: 0 } : c
    );
  },
  markUnread: async (conversationId: string) => {
    conversations = conversations.map((c) =>
      c.id === conversationId ? { ...c, unread_count: Math.max(1, c.unread_count) } : c
    );
  },
  searchConversations: async (query: string) => {
    const q = query.toLowerCase();
    return conversations.filter(
      (c) =>
        c.title.toLowerCase().includes(q) ||
        (c.last_message_preview?.toLowerCase().includes(q) ?? false)
    );
  },
  totalUnread: async () => conversations.reduce((s, c) => s + c.unread_count, 0),
  getAppConfig: async () => appConfig,
  saveAppConfig: async (cfg: AppConfig) => {
    appConfig = cfg;
    return cfg;
  },
  listWorkspaces: async () => workspaces,
  createWorkspace: async (name: string) => {
    const w: Workspace = { id: crypto.randomUUID(), name, builtin: false, sort_order: workspaces.length };
    workspaces = [...workspaces, w];
    return w;
  },
  renameWorkspace: async (id: string, name: string) => {
    workspaces = workspaces.map((w) => (w.id === id ? { ...w, name } : w));
  },
  deleteWorkspace: async (id: string) => {
    workspaces = workspaces.filter((w) => w.id !== id);
  },
  listPriorityGroups: async () => priorityGroups,
  createPriorityGroup: async (name: string, color?: string) => {
    const g: PriorityGroup = {
      id: crypto.randomUUID(),
      name,
      color: color ?? '#888',
      builtin: false,
      sort_order: priorityGroups.length,
    };
    priorityGroups = [...priorityGroups, g];
    return g;
  },
  listTodos: async (conversationId: string) => todos.filter((t) => t.conversation_id === conversationId),
  addTodo: async (conversationId: string, accountId: string, body: string, dueAt?: string) => {
    const t: ChatTodo = {
      id: crypto.randomUUID(),
      conversation_id: conversationId,
      account_id: accountId,
      body,
      due_at: dueAt ?? null,
      done: false,
      created_at: new Date().toISOString(),
    };
    todos = [...todos, t];
    return t;
  },
  setTodoDone: async (id: string, done: boolean) => {
    todos = todos.map((t) => (t.id === id ? { ...t, done } : t));
  },
  deleteTodo: async (id: string) => {
    todos = todos.filter((t) => t.id !== id);
  },
  listReminders: async (conversationId?: string) =>
    conversationId ? reminders.filter((r) => r.conversation_id === conversationId) : reminders,
  createReminder: async (
    conversationId: string,
    accountId: string,
    fireAt: string,
    kind?: string,
    note?: string
  ) => {
    const r: Reminder = {
      id: crypto.randomUUID(),
      conversation_id: conversationId,
      account_id: accountId,
      fire_at: fireAt,
      kind: kind ?? 'nudge',
      note: note ?? null,
      fired: false,
      created_at: new Date().toISOString(),
    };
    reminders = [...reminders, r];
    return r;
  },
  deleteReminder: async (id: string) => {
    reminders = reminders.filter((r) => r.id !== id);
  },
  listForwardRules: async () => forwardRules,
  createForwardRule: async (draft: ForwardRuleDraft) => {
    const rule: ForwardRule = {
      id: crypto.randomUUID(),
      enabled: true,
      source_account_id: draft.source_account_id ?? null,
      source_conversation_id: draft.source_conversation_id ?? null,
      source_workspace_id: draft.source_workspace_id ?? null,
      dest_account_id: draft.dest_account_id,
      dest_conversation_id: draft.dest_conversation_id,
      inbound_only: draft.inbound_only ?? true,
      include_self: draft.include_self ?? false,
      keyword: draft.keyword ?? null,
      prefix: draft.prefix ?? null,
      suffix: draft.suffix ?? null,
      strip_sender: draft.strip_sender ?? false,
      skip_if_forwarded: draft.skip_if_forwarded ?? true,
      delay_seconds: draft.delay_seconds ?? 0,
      created_at: new Date().toISOString(),
    };
    forwardRules = [rule, ...forwardRules];
    return rule;
  },
  updateForwardRule: async (id: string, patch: ForwardRulePatch) => {
    forwardRules = forwardRules.map((rule) =>
      rule.id === id
        ? {
            ...rule,
            ...patch,
            source_account_id: patch.clear_source_account ? null : patch.source_account_id ?? rule.source_account_id,
            source_conversation_id: patch.clear_source_conversation ? null : patch.source_conversation_id ?? rule.source_conversation_id,
            source_workspace_id: patch.clear_source_workspace ? null : patch.source_workspace_id ?? rule.source_workspace_id,
            keyword: patch.clear_keyword ? null : patch.keyword ?? rule.keyword,
            prefix: patch.clear_prefix ? null : patch.prefix ?? rule.prefix,
            suffix: patch.clear_suffix ? null : patch.suffix ?? rule.suffix,
          }
        : rule
    );
    return forwardRules.find((rule) => rule.id === id)!;
  },
  deleteForwardRule: async (id: string) => {
    forwardRules = forwardRules.filter((rule) => rule.id !== id);
  },
  listScheduledMessages: async (includeSent = false) =>
    includeSent ? scheduledMessages : scheduledMessages.filter((msg) => !msg.sent),
  scheduleMessage: async (draft: ScheduleMessageDraft) => {
    const msg: ScheduledMessage = {
      id: crypto.randomUUID(),
      source_account_id: draft.source_account_id ?? null,
      source_conversation_id: draft.source_conversation_id ?? null,
      source_message_id: draft.source_message_id ?? null,
      dest_account_id: draft.dest_account_id,
      dest_conversation_id: draft.dest_conversation_id,
      body: draft.body,
      send_at: draft.send_at,
      sent: false,
      created_at: new Date().toISOString(),
    };
    scheduledMessages = [...scheduledMessages, msg];
    return msg;
  },
  deleteScheduledMessage: async (id: string) => {
    scheduledMessages = scheduledMessages.filter((msg) => msg.id !== id);
  },
  exportBackup: async (_path: string, _password: string, includeMessages = true) =>
    ({
      exported_at: new Date().toISOString(),
      includes_messages: includeMessages,
    }) satisfies BackupManifest,
  restoreBackup: async (_path?: string, _password?: string) => {},
  onShuttleEvent: async () => () => {},
};

export function isTauri(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}
