import type {
  Account,
  AccountPatch,
  AppConfig,
  AttachmentPayload,
  ChatTodo,
  ConnectorInfo,
  Conversation,
  Contact,
  ConversationPatch,
  ForwardRule,
  ForwardRuleDraft,
  ForwardRulePatch,
  ForwardResult,
  ForwardTarget,
  Message,
  MessageActions,
  MessageAnchor,
  MessageOp,
  MessageOpResult,
  MessagePage,
  PriorityGroup,
  BackupManifest,
  ChatNote,
  ComponentsOverview,
  NoteSchedule,
  ScheduledMessage,
  ScheduleMessageDraft,
  SenderContact,
  ShuttleEvent,
  StatusItem,
  Translation,
  Workspace,
} from './types';
import { offeredActions, withMyReaction } from './messageActions';
import { captionText } from './messageMedia';

const DEMO_PHOTO =
  'data:image/svg+xml;utf8,' +
  encodeURIComponent(
    '<svg xmlns="http://www.w3.org/2000/svg" width="320" height="200"><defs><linearGradient id="g" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#f6a35b"/><stop offset="1" stop-color="#6b4fa3"/></linearGradient></defs><rect width="320" height="200" fill="url(#g)"/><circle cx="160" cy="130" r="34" fill="#ffd27a"/><rect y="150" width="320" height="50" fill="#2d3c66"/></svg>'
  );

const DEMO_STICKER =
  'data:image/svg+xml;utf8,' +
  encodeURIComponent(
    '<svg xmlns="http://www.w3.org/2000/svg" width="160" height="160"><g fill="none" stroke="#e11d48" stroke-width="9" stroke-linecap="round"><path d="M46 118c22-4 40-22 44-44"/><path d="M56 58l-6 14M88 46l-2 16M108 62l-12 10"/></g><circle cx="44" cy="120" r="19" fill="#f59e0b"/><circle cx="116" cy="118" r="9" fill="#22d3ee"/><circle cx="98" cy="96" r="6" fill="#a855f7"/></svg>'
  );

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
    last_message_from_me: true,
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
    last_message_preview: 'Updated the mockups in Figma',
    last_message_from_me: false,
    last_message_sender_id: 'alex@s.whatsapp.net',
    last_message_sender_name: 'Alex Rivera',
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
    last_message_from_me: false,
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
    last_message_from_me: false,
    last_message_sender_id: '+14155550123',
    last_message_sender_name: null,
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
    last_message_from_me: true,
    pinned: false,
    archived: false,
    muted: false,
    metadata: {},
  },
  {
    id: 'c6',
    account_id: 'wa-1',
    remote_id: 'r6@g.us',
    contact_id: null,
    title: 'Rent Appartment Riyadh',
    conversation_type: 'group',
    unread_count: 3,
    last_message_at: new Date(Date.now() - 3600000).toISOString(),
    last_message_preview: 'Maintenance visit on Thursday at 10:00',
    last_message_from_me: false,
    last_message_sender_id: '15550123456@s.whatsapp.net',
    last_message_sender_name: 'Building Manager',
    pinned: false,
    archived: false,
    muted: false,
    // Only admins may post here and this account is not one: the composer is locked.
    metadata: { announce_only: true, self_is_admin: false },
  },
  // Places where WhatsApp only gave a label, to check the UI never prints one when a real
  // name is known, and prints a neutral label when it is not.
  {
    id: 'g1',
    account_id: 'wa-1',
    remote_id: '120363040000000001@g.us',
    contact_id: null,
    title: 'Group',
    conversation_type: 'group',
    unread_count: 0,
    last_message_at: new Date(Date.now() - 9000000).toISOString(),
    last_message_preview: 'Handover notes are in the shared folder',
    last_message_from_me: false,
    pinned: false,
    archived: false,
    muted: false,
    metadata: { names: { subject: 'Villa Owners' } },
  },
  {
    id: 'g2',
    account_id: 'wa-1',
    remote_id: '120363040000000002@g.us',
    contact_id: null,
    title: 'Group',
    conversation_type: 'group',
    unread_count: 0,
    last_message_at: new Date(Date.now() - 30000000).toISOString(),
    last_message_preview: 'Anyone still in the building?',
    last_message_from_me: false,
    pinned: false,
    archived: false,
    muted: false,
    metadata: {},
  },
  {
    id: 'ch1',
    account_id: 'wa-1',
    remote_id: '120363040000000003@newsletter',
    contact_id: null,
    title: 'Channel',
    conversation_type: 'channel',
    unread_count: 3,
    last_message_at: new Date(Date.now() - 1800000).toISOString(),
    last_message_preview: 'Shuttle 0.5 is out with a faster inbox',
    last_message_from_me: false,
    pinned: false,
    archived: false,
    muted: false,
    metadata: { names: { subject: 'Shuttle News' } },
  },
  {
    id: 'ch2',
    account_id: 'wa-1',
    remote_id: '120363040000000004@newsletter',
    contact_id: null,
    title: 'Newsletter 120363040000000004',
    conversation_type: 'channel',
    unread_count: 0,
    last_message_at: new Date(Date.now() - 7200000).toISOString(),
    last_message_preview: 'This week: type scales that hold up',
    last_message_from_me: false,
    pinned: false,
    archived: false,
    muted: false,
    metadata: { names: { subject: 'Design Digest' } },
  },
  {
    id: 'ch3',
    account_id: 'wa-1',
    remote_id: '120363040000000005@newsletter',
    contact_id: null,
    title: 'Newsletter 120363040000000005',
    conversation_type: 'channel',
    unread_count: 0,
    last_message_at: new Date(Date.now() - 90000000).toISOString(),
    last_message_preview: 'Restock day is Friday',
    last_message_from_me: false,
    pinned: false,
    archived: false,
    muted: false,
    metadata: {},
  },
];

/** Inline pictures for the mock statuses, so a preview shows without a real media file. */
function demoShot(from: string, to: string, label: string): string {
  const svg =
    `<svg xmlns="http://www.w3.org/2000/svg" width="240" height="360">` +
    `<defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1">` +
    `<stop offset="0" stop-color="${from}"/><stop offset="1" stop-color="${to}"/>` +
    `</linearGradient></defs>` +
    `<rect width="240" height="360" fill="url(#g)"/>` +
    `<circle cx="186" cy="74" r="48" fill="rgba(255,255,255,0.26)"/>` +
    `<rect x="26" y="256" width="132" height="14" rx="7" fill="rgba(255,255,255,0.8)"/>` +
    `<rect x="26" y="284" width="82" height="10" rx="5" fill="rgba(255,255,255,0.5)"/>` +
    `<text x="26" y="58" font-family="sans-serif" font-size="19" fill="rgba(255,255,255,0.92)">${label}</text>` +
    `</svg>`;
  return `data:image/svg+xml;utf8,${encodeURIComponent(svg)}`;
}

const DEMO_STATUSES: StatusItem[] = [
  {
    account_id: 'wa-1',
    sender_id: 'r1',
    sender_name: 'Sarah Chen',
    timestamp: new Date(Date.now() - 10800000).toISOString(),
    media_type: 'image',
    count: 2,
    posts: [
      {
        id: 'sp-sarah-1',
        media_type: 'image',
        text: 'Sunrise over the bay',
        timestamp: new Date(Date.now() - 10800000).toISOString(),
        thumbnail_path: demoShot('#f59e0b', '#be123c', 'Sunrise'),
      },
      {
        id: 'sp-sarah-2',
        media_type: 'text',
        text: 'Same place, ten minutes later',
        timestamp: new Date(Date.now() - 10400000).toISOString(),
      },
    ],
  },
  {
    account_id: 'wa-1',
    sender_id: 'r9',
    sender_name: 'Alex Rivera',
    timestamp: new Date(Date.now() - 7200000).toISOString(),
    media_type: 'image',
    count: 1,
    posts: [
      {
        id: 'sp-alex-1',
        media_type: 'image',
        text: 'Studio wall',
        timestamp: new Date(Date.now() - 7200000).toISOString(),
        thumbnail_path: demoShot('#0ea5e9', '#1e3a8a', 'Studio'),
      },
    ],
  },
  {
    account_id: 'wa-1',
    sender_id: 'r10',
    sender_name: 'Priya Shah',
    timestamp: new Date(Date.now() - 5400000).toISOString(),
    media_type: 'text',
    count: 1,
    posts: [
      {
        id: 'sp-priya-1',
        media_type: 'text',
        text: 'Shipping the new build tonight',
        timestamp: new Date(Date.now() - 5400000).toISOString(),
      },
    ],
  },
  {
    account_id: 'wa-1',
    sender_id: 'r11',
    sender_name: 'Maya Okafor',
    timestamp: new Date(Date.now() - 3600000).toISOString(),
    media_type: 'text',
    count: 1,
    posts: [
      {
        id: 'sp-maya-1',
        media_type: 'text',
        text: 'https://www.instagram.com/reel/CxYzAbCdEf/',
        timestamp: new Date(Date.now() - 3600000).toISOString(),
        link_preview: {
          url: 'https://www.instagram.com/reel/CxYzAbCdEf/',
          title: 'Behind the scenes at the studio',
          thumbnail_path: demoShot('#7c3aed', '#db2777', 'Reel'),
        },
      },
    ],
  },
  {
    account_id: 'wa-1',
    sender_id: 'r12',
    sender_name: 'Omar Haddad',
    timestamp: new Date(Date.now() - 2400000).toISOString(),
    media_type: 'text',
    count: 1,
    posts: [
      {
        id: 'sp-omar-1',
        media_type: 'text',
        text: 'https://www.facebook.com/watch?v=1098765432',
        timestamp: new Date(Date.now() - 2400000).toISOString(),
        link_preview: {
          url: 'https://www.facebook.com/watch?v=1098765432',
          title: null,
          thumbnail_path: null,
        },
      },
    ],
  },
  {
    account_id: 'tg-1',
    sender_id: 'r3',
    sender_name: 'Mom',
    timestamp: new Date(Date.now() - 1800000).toISOString(),
    media_type: 'text',
    count: 1,
    posts: [
      {
        id: 'sp-mom-1',
        media_type: 'text',
        text: '❤️',
        timestamp: new Date(Date.now() - 1800000).toISOString(),
      },
    ],
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
      metadata: { reply_to: { remote_id: 'rm1b' } },
      reply: {
        remote_id: 'rm1b',
        message_id: 'm1b',
        sender_name: 'Sarah Chen',
        from_me: false,
        snippet: 'I was thinking that new Thai place on 5th — heard great things',
        available: true,
      },
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
      id: 'm6c',
      conversation_id: 'c1',
      remote_id: 'rm6c',
      sender_id: null,
      sender_name: 'Sarah Chen',
      direction: 'inbound',
      body: 'Contact: Zunaira Sameen (+966 55 610 9476)',
      timestamp: new Date(Date.now() - 1750000).toISOString(),
      status: 'delivered',
      metadata: {},
    },
    {
      id: 'm6l',
      conversation_id: 'c1',
      remote_id: 'rm6l',
      sender_id: null,
      sender_name: 'Sarah Chen',
      direction: 'inbound',
      body: 'https://maps.google.com/?q=24.6803947,46.7077217',
      timestamp: new Date(Date.now() - 1720000).toISOString(),
      status: 'delivered',
      metadata: {},
    },
    {
      id: 'm6k',
      conversation_id: 'c1',
      remote_id: 'rm6k',
      sender_id: null,
      sender_name: 'Sarah Chen',
      direction: 'inbound',
      body: 'Menu is here https://example.com/thai-basil, see you at 12:30',
      timestamp: new Date(Date.now() - 1700000).toISOString(),
      status: 'delivered',
      metadata: {},
    },
    {
      id: 'm6call',
      conversation_id: 'c1',
      remote_id: 'rm6call',
      sender_id: null,
      sender_name: 'Sarah Chen',
      direction: 'inbound',
      body: 'Incoming call',
      timestamp: new Date(Date.now() - 1690000).toISOString(),
      status: 'delivered',
      metadata: { media_type: 'call' },
    },
    {
      id: 'm6u',
      conversation_id: 'c1',
      remote_id: 'rm6u',
      sender_id: null,
      sender_name: 'Sarah Chen',
      direction: 'inbound',
      body: '[order]',
      timestamp: new Date(Date.now() - 1680000).toISOString(),
      status: 'delivered',
      metadata: { media_type: 'order' },
    },
    {
      id: 'm6p',
      conversation_id: 'c1',
      remote_id: 'rm6p',
      sender_id: null,
      sender_name: 'Sarah Chen',
      direction: 'inbound',
      body: 'Sunset from the pier last night',
      timestamp: new Date(Date.now() - 1500000).toISOString(),
      status: 'delivered',
      metadata: { media_type: 'image', media_data: DEMO_PHOTO, mime: 'image/svg+xml', filename: 'pier.svg' },
    },
    {
      id: 'm6q',
      conversation_id: 'c1',
      remote_id: 'rm6q',
      sender_id: null,
      sender_name: 'Sarah Chen',
      direction: 'inbound',
      body: '[Image]',
      timestamp: new Date(Date.now() - 1400000).toISOString(),
      status: 'delivered',
      metadata: { media_type: 'image', media_data: DEMO_PHOTO, mime: 'image/svg+xml' },
    },
    {
      id: 'm6r',
      conversation_id: 'c1',
      remote_id: 'rm6r',
      sender_id: null,
      sender_name: 'Sarah Chen',
      direction: 'inbound',
      body: '[Location]',
      timestamp: new Date(Date.now() - 1300000).toISOString(),
      status: 'delivered',
      metadata: { media_type: 'location', latitude: 40.74, longitude: -73.99 },
    },
    {
      id: 'm6s',
      conversation_id: 'c1',
      remote_id: 'rm6s',
      sender_id: null,
      sender_name: 'Sarah Chen',
      direction: 'inbound',
      body: '[Video]',
      timestamp: new Date(Date.now() - 1200000).toISOString(),
      status: 'delivered',
      metadata: { media_type: 'video', media_error: 'The file is no longer on WhatsApp' },
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
    {
      id: 'm7a',
      conversation_id: 'c1',
      remote_id: 'rm7a',
      sender_id: null,
      sender_name: 'Sarah Chen',
      direction: 'inbound',
      body: [
        'The plan:',
        '*Book the table* for _eight_ people',
        '~Six~ _ten_ of us now',
        'Bring `cash` just in case',
        '> They close at 11',
        '1. Table at 12:30',
        '2. Cake at 13:00',
        '- candles',
        '- a lighter',
        '```',
        'cash total = 8 * 46.50',
        '```',
      ].join(String.fromCharCode(10)),
      timestamp: new Date(Date.now() - 540000).toISOString(),
      status: 'delivered',
      metadata: {},
    },
    {
      id: 'm7b',
      conversation_id: 'c1',
      remote_id: 'rm7b',
      sender_id: null,
      sender_name: 'You',
      direction: 'outbound',
      body: 'Forwarded from the group chat',
      timestamp: new Date(Date.now() - 480000).toISOString(),
      status: 'read',
      metadata: { forwarded: true },
    },
    {
      id: 'm7c',
      conversation_id: 'c1',
      remote_id: 'rm7c',
      sender_id: null,
      sender_name: 'Sarah Chen',
      direction: 'inbound',
      body: '[Sticker]',
      timestamp: new Date(Date.now() - 420000).toISOString(),
      status: 'delivered',
      metadata: { media_type: 'sticker', media_data: DEMO_STICKER, mime: 'image/svg+xml', filename: 'party.svg' },
    },
    {
      id: 'm7p1',
      conversation_id: 'c1',
      remote_id: 'rm7p1',
      sender_id: null,
      sender_name: 'Sarah Chen',
      direction: 'inbound',
      body: 'Where shall we eat on Friday?',
      timestamp: new Date(Date.now() - 360000).toISOString(),
      status: 'delivered',
      metadata: {
        media_type: 'poll',
        poll: {
          question: 'Where shall we eat on Friday?',
          multiple: false,
          options: [
            { id: 'p1a', text: 'The ramen place', votes: 3, voters: [
              { id: '15550001111@s.whatsapp.net', name: 'Sarah Chen' },
              { id: '15550002222@s.whatsapp.net', name: 'Alex Rivera' },
              { id: '15550003333@s.whatsapp.net', name: 'Mina Patel' },
            ], mine: false },
            { id: 'p1b', text: 'Pizza by the pier', votes: 1, voters: [
              { id: '15550004444@s.whatsapp.net', name: 'Tom Okafor' },
            ], mine: false },
            { id: 'p1c', text: 'Somewhere with a table for eight', votes: 0, voters: [], mine: false },
          ],
        },
      },
    },
    {
      id: 'm7p2',
      conversation_id: 'c1',
      remote_id: 'rm7p2',
      sender_id: null,
      sender_name: 'You',
      direction: 'outbound',
      body: 'Which night works for the party?',
      timestamp: new Date(Date.now() - 300000).toISOString(),
      status: 'read',
      metadata: {
        media_type: 'poll',
        poll: {
          question: 'Which night works for the party?',
          multiple: false,
          options: [
            { id: 'p2a', text: 'Friday', votes: 2, voters: [
              { id: '15550001111@s.whatsapp.net', name: 'Sarah Chen' },
              { id: 'self@s.whatsapp.net', name: 'You' },
            ], mine: true },
            { id: 'p2b', text: 'Saturday', votes: 0, voters: [], mine: false },
          ],
        },
      },
    },
    {
      id: 'm7p3',
      conversation_id: 'c1',
      remote_id: 'rm7p3',
      sender_id: null,
      sender_name: 'Sarah Chen',
      direction: 'inbound',
      body: 'Pick the cakes you want',
      timestamp: new Date(Date.now() - 240000).toISOString(),
      status: 'delivered',
      metadata: {
        media_type: 'poll',
        poll: {
          question: 'Pick the cakes you want',
          multiple: true,
          options: [
            { id: 'p3a', text: 'Chocolate', votes: 2, voters: [
              { id: '15550001111@s.whatsapp.net', name: 'Sarah Chen' },
              { id: 'self@s.whatsapp.net', name: 'You' },
            ], mine: true },
            { id: 'p3b', text: 'Lemon drizzle', votes: 1, voters: [
              { id: 'self@s.whatsapp.net', name: 'You' },
            ], mine: true },
            { id: 'p3c', text: 'Cheesecake', votes: 1, voters: [
              { id: '15550002222@s.whatsapp.net', name: 'Alex Rivera' },
            ], mine: false },
          ],
        },
      },
    },
    {
      id: 'm7p4',
      conversation_id: 'c1',
      remote_id: 'rm7p4',
      sender_id: null,
      sender_name: 'You',
      direction: 'outbound',
      body: 'Shall we split the bill?',
      timestamp: new Date(Date.now() - 180000).toISOString(),
      status: 'sent',
      metadata: {
        media_type: 'poll',
        poll: {
          question: 'Shall we split the bill?',
          multiple: false,
          options: [
            { id: 'p4a', text: 'Split it evenly', votes: 0, voters: [], mine: false },
            { id: 'p4b', text: 'Everyone pays for their own', votes: 0, voters: [], mine: false },
          ],
        },
      },
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
    {
      id: 'g1',
      conversation_id: 'c2',
      remote_id: 'rg1',
      sender_id: 'r1',
      sender_name: 'Sarah Chen',
      direction: 'inbound',
      body: 'Love the new header. Can we try it in dark mode too?',
      timestamp: new Date(Date.now() - 7100000).toISOString(),
      status: 'delivered',
      metadata: {},
    },
    {
      id: 'g2',
      conversation_id: 'c2',
      remote_id: 'rg2',
      sender_id: '15550123456@s.whatsapp.net',
      sender_name: 'Priya Shah',
      direction: 'inbound',
      body: 'I can pair on that this afternoon',
      timestamp: new Date(Date.now() - 7000000).toISOString(),
      status: 'delivered',
      metadata: {},
    },
    {
      id: 'g3',
      conversation_id: 'c2',
      remote_id: 'rg3',
      sender_id: null,
      sender_name: 'You',
      direction: 'outbound',
      body: 'Great, thanks both',
      timestamp: new Date(Date.now() - 6900000).toISOString(),
      status: 'read',
      metadata: {},
    },
  ],
  c6: [
    {
      id: 'h1',
      conversation_id: 'c6',
      remote_id: 'rh1',
      sender_id: '15550123456@s.whatsapp.net',
      sender_name: 'Building Manager',
      direction: 'inbound',
      body: 'Maintenance visit on Thursday at 10:00',
      timestamp: new Date(Date.now() - 3600000).toISOString(),
      status: 'delivered',
      metadata: {},
    },
  ],
  c3: [
    {
      id: 'tg1',
      conversation_id: 'c3',
      remote_id: 'rt1',
      sender_id: null,
      sender_name: 'Mom',
      direction: 'inbound',
      body: 'Markdown here is *stars* and _underscores_, not WhatsApp markup',
      timestamp: new Date(Date.now() - 2600000).toISOString(),
      status: 'delivered',
      metadata: {},
    },
  ],
};

/** The mock's forward: the same rules the core applies, delivered at once. */
async function mockForwardOne(target: ForwardTarget, msg: Message, sendAt: string | null): Promise<ForwardResult> {
  const base = { ...target, message_id: msg.id };
  const kind = typeof msg.metadata?.media_type === 'string' ? msg.metadata.media_type : null;
  if (msg.metadata?.revoked_at) return { ...base, status: 'not_forwardable', reason: 'The message was deleted' };
  if (kind === 'location') return { ...base, status: 'not_forwardable', reason: 'Locations cannot be forwarded' };
  if (kind === 'poll') return { ...base, status: 'not_forwardable', reason: 'Polls cannot be forwarded' };
  if (kind && msg.metadata?.media_error) {
    return { ...base, status: 'failed', error: `The media could not be downloaded: ${msg.metadata.media_error}` };
  }
  if (sendAt) {
    await mockApi.scheduleMessage({
      dest_account_id: target.account_id,
      dest_conversation_id: target.conversation_id,
      source_conversation_id: msg.conversation_id,
      source_message_id: msg.id,
      body: kind ? captionText(msg) || `[${kind}]` : msg.body,
      send_at: sendAt,
    });
    scheduledMessages = scheduledMessages.map((s, i) => (i === scheduledMessages.length - 1 ? { ...s, forward: true } : s));
    return { ...base, status: 'scheduled' };
  }
  const sent = await mockApi.sendMessage(target.account_id, target.conversation_id, kind ? captionText(msg) || `[${kind}]` : msg.body);
  const media = kind
    ? { media_type: kind, media_data: msg.metadata.media_data, mime: msg.metadata.mime, filename: msg.metadata.filename }
    : {};
  messages[target.conversation_id] = (messages[target.conversation_id] ?? []).map((m) =>
    m.id === sent.id ? { ...m, metadata: { ...media, forwarded: true } } : m
  );
  return { ...base, status: 'sent' };
}

const NO_ACTIONS: MessageActions = {
  react: false,
  edit: false,
  delete: false,
  delete_everyone: false,
  edit_window_secs: null,
  delete_everyone_window_secs: null,
};

const CONNECTORS: ConnectorInfo[] = [
  {
    id: 'whatsapp',
    name: 'WhatsApp',
    description: 'Scan QR code with WhatsApp on your phone',
    auth_type: 'qr',
    capabilities: ['text', 'media', 'read_receipts', 'groups'],
    actions: {
      react: true,
      edit: true,
      delete: true,
      delete_everyone: true,
      edit_window_secs: 15 * 60,
      delete_everyone_window_secs: 2 * 24 * 60 * 60,
    },
  },
  {
    id: 'telegram',
    name: 'Telegram',
    description: 'Log in with your phone number',
    auth_type: 'phone',
    capabilities: ['text', 'media', 'read_receipts', 'groups', 'channels'],
    actions: NO_ACTIONS,
  },
  {
    id: 'signal',
    name: 'Signal',
    description: 'Register with your phone number',
    auth_type: 'phone',
    capabilities: ['text', 'media', 'read_receipts', 'groups'],
    actions: NO_ACTIONS,
  },
  {
    id: 'messenger',
    name: 'Messenger',
    description: 'Log in with Facebook email and password',
    auth_type: 'password',
    capabilities: ['text', 'media', 'groups'],
    actions: NO_ACTIONS,
  },
  {
    id: 'instagram',
    name: 'Instagram',
    description: 'Log in to Instagram DMs',
    auth_type: 'password',
    capabilities: ['text', 'media'],
    actions: NO_ACTIONS,
  },
  {
    id: 'email',
    name: 'Email',
    description: 'Connect IMAP and SMTP',
    auth_type: 'email',
    capabilities: ['text'],
    actions: NO_ACTIONS,
  },
  {
    id: 'matrix',
    name: 'Matrix',
    description: 'Log in to a Matrix homeserver',
    auth_type: 'password',
    capabilities: ['text', 'groups', 'channels'],
    actions: NO_ACTIONS,
  },
];

let accounts = [...DEMO_ACCOUNTS];
let conversations = [...DEMO_CONVERSATIONS];
const messages = { ...DEMO_MESSAGES };
let workspaces: Workspace[] = [{ id: 'default', name: 'Default', builtin: true, sort_order: 0 }];
let priorityGroups: PriorityGroup[] = [];
function mockSchedule(schedule?: NoteSchedule): Pick<ChatNote, 'fire_at' | 'kind'> {
  if (schedule?.timer_secs) {
    return { fire_at: new Date(Date.now() + schedule.timer_secs * 1000).toISOString(), kind: 'timer' };
  }
  if (schedule?.alarm_at) return { fire_at: new Date(schedule.alarm_at).toISOString(), kind: 'alarm' };
  return { fire_at: null, kind: null };
}

let todos: ChatTodo[] = [];
let notes: ChatNote[] = [];
let forwardRules: ForwardRule[] = [];
let scheduledMessages: ScheduledMessage[] = [];
let appConfig: AppConfig = {
  appearance: { color_scheme: 'light', theme_id: 'cmlhfpjhw000004l4f4ax3m7z', datetime_format: '12h_full', font_scale: 1, tweakcn_css: null },
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
  sleep: {
    enabled: false,
    after_minutes: 5,
    check_minutes: 15,
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
  media: { save_dir: null, ask_each_time: false, keep_days: 60 },
  ai: { openrouter_api_key: '', model: '' },
  integrations: { giphy_api_key: '' },
};

const listeners = new Set<(event: ShuttleEvent) => void>();

/** Deliver an event the way the core would, a moment after the action that caused it. */
function emitLater(event: ShuttleEvent) {
  setTimeout(() => listeners.forEach((fn) => fn(event)), 120);
}

function applyMockOp(conversationId: string, msg: Message, op: MessageOp): string | undefined {
  const conv = conversations.find((c) => c.id === conversationId);
  const account = accounts.find((a) => a.id === conv?.account_id);
  const actions = CONNECTORS.find((c) => c.id === account?.connector_id)?.actions;
  const offered = offeredActions(actions, msg);
  const allowed =
    op.kind === 'react' ? offered.react
    : op.kind === 'edit' ? offered.edit && op.text.trim() !== ''
    : op.kind === 'delete_for_everyone' ? offered.deleteForEveryone
    : offered.deleteForMe;
  if (!allowed) return 'Not allowed for this message';
  const list = messages[conversationId] ?? [];
  const base = { account_id: conv?.account_id, conversation_id: conversationId };
  if (op.kind === 'react') {
    const reactions = withMyReaction(msg.reactions, op.emoji);
    messages[conversationId] = list.map((m) => (m.id === msg.id ? { ...m, reactions } : m));
    emitLater({ kind: 'message.reactions', payload: { ...base, updates: [{ remote_id: msg.remote_id, reactions }] } });
  } else if (op.kind === 'delete_for_me') {
    messages[conversationId] = list.filter((m) => m.id !== msg.id);
    emitLater({ kind: 'message.removed', payload: { ...base, message_id: msg.id } });
  } else {
    const now = new Date().toISOString();
    const next: Message =
      op.kind === 'edit'
        ? { ...msg, body: op.text, metadata: { ...msg.metadata, edited_at: now } }
        : { ...msg, body: '', metadata: { revoked_at: now }, reactions: [] };
    messages[conversationId] = list.map((m) => (m.id === msg.id ? next : m));
    emitLater({ kind: 'message.updated', payload: { ...base, message: next } });
  }
  return undefined;
}

/** Browser preview of Settings, About: the same shape the Rust command returns. */
const MOCK_COMPONENTS: ComponentsOverview = {
  platform: 'linux-x86_64',
  expected_version: '0.0.0-dev',
  manifest_version: null,
  manifest_available: false,
  connectors: [
    {
      connector_id: 'whatsapp',
      connector_label: 'WhatsApp',
      components: [
        { id: 'script:whatsapp', label: 'Whatsapp connector', installed: true, optional: false, version: '0.0.0-dev', size: 18_400, source: 'managed' },
        { id: 'native:gowa', label: 'WhatsApp helper (GOWA)', installed: false, optional: false, version: null, size: 24_800_000, source: null },
      ],
    },
    {
      connector_id: 'telegram',
      connector_label: 'Telegram',
      components: [
        { id: 'script:telegram', label: 'Telegram connector', installed: false, optional: false, version: null, size: 21_000, source: null },
        { id: 'native:tdlib', label: 'Telegram library (TDLib)', installed: false, optional: false, version: null, size: 9_600_000, source: null },
      ],
    },
    {
      connector_id: 'signal',
      connector_label: 'Signal',
      components: [
        { id: 'script:signal', label: 'Signal connector', installed: false, optional: false, version: null, size: 19_500, source: null },
        { id: 'native:signal-cli', label: 'Signal CLI', installed: false, optional: false, version: null, size: 61_000_000, source: null },
      ],
    },
    {
      connector_id: 'messenger',
      connector_label: 'Messenger',
      components: [
        { id: 'script:messenger', label: 'Messenger connector', installed: false, optional: false, version: null, size: 33_000, source: null },
        { id: 'script:shuttle_ipc', label: 'Shuttle Ipc connector', installed: false, optional: false, version: null, size: 12_000, source: null },
        { id: 'python-runtime', label: 'Python runtime', installed: true, optional: false, version: '3.13.2', size: 15_000_000, source: 'system' },
        { id: 'python:deps:messenger', label: 'Messenger libraries', installed: false, optional: false, version: null, size: 4_200_000, source: null },
      ],
    },
    {
      connector_id: 'instagram',
      connector_label: 'Instagram',
      components: [
        { id: 'script:instagram', label: 'Instagram connector', installed: false, optional: false, version: null, size: 29_000, source: null },
        { id: 'script:shuttle_ipc', label: 'Shuttle Ipc connector', installed: false, optional: false, version: null, size: 12_000, source: null },
        { id: 'python-runtime', label: 'Python runtime', installed: true, optional: false, version: '3.13.2', size: 15_000_000, source: 'system' },
        { id: 'python:deps:instagram', label: 'Instagram libraries', installed: false, optional: false, version: null, size: 4_600_000, source: null },
      ],
    },
    {
      connector_id: 'matrix',
      connector_label: 'Matrix',
      components: [
        { id: 'script:matrix', label: 'Matrix connector', installed: false, optional: false, version: null, size: 26_000, source: null },
        { id: 'script:shuttle_ipc', label: 'Shuttle Ipc connector', installed: false, optional: false, version: null, size: 12_000, source: null },
        { id: 'python-runtime', label: 'Python runtime', installed: false, optional: true, version: null, size: 15_000_000, source: null },
      ],
    },
    {
      connector_id: 'email',
      connector_label: 'Email',
      components: [
        { id: 'script:email', label: 'Email connector', installed: false, optional: false, version: null, size: 31_000, source: null },
        { id: 'script:shuttle_ipc', label: 'Shuttle Ipc connector', installed: false, optional: false, version: null, size: 12_000, source: null },
        { id: 'python-runtime', label: 'Python runtime', installed: false, optional: true, version: null, size: 15_000_000, source: null },
      ],
    },
  ],
};

export const mockApi = {
  listAccounts: async () => accounts,
  listConnectors: async () => CONNECTORS,
  componentsOverview: async () => MOCK_COMPONENTS,
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
    accounts = accounts.map((a) => (a.id === id ? { ...a, ...patch, metadata: { ...a.metadata, ...(patch.accent_color ? { accent_color: patch.accent_color } : {}) } } : a));
    return accounts.find((a) => a.id === id)!;
  },
  wakeAccount: async (_accountId: string) => 'already running',
  setActiveAccount: async (_accountId: string | null) => {},
  connectAccount: async (_accountId?: string) => 'ok',
  listConversations: async (
    accountId?: string,
    workspaceId?: string,
    priorityGroup?: string,
    archivedOnly?: boolean,
    offset = 0,
    limit = 30
  ) => {
    let list = accountId ? conversations.filter((c) => c.account_id === accountId) : conversations;
    list = list.filter((c) => (archivedOnly ? c.archived : !c.archived));
    if (workspaceId) list = list.filter((c) => (c.workspace_id ?? 'default') === workspaceId);
    if (priorityGroup) list = list.filter((c) => c.priority_group === priorityGroup);
    return list.slice(offset, offset + limit);
  },
  countConversations: async (
    accountId?: string,
    workspaceId?: string,
    priorityGroup?: string,
    archivedOnly?: boolean
  ) => {
    let list = accountId ? conversations.filter((c) => c.account_id === accountId) : conversations;
    list = list.filter((c) => (archivedOnly ? c.archived : !c.archived));
    if (workspaceId) list = list.filter((c) => (c.workspace_id ?? 'default') === workspaceId);
    if (priorityGroup) list = list.filter((c) => c.priority_group === priorityGroup);
    return list.length;
  },
  listContacts: async (accountId: string): Promise<Contact[]> =>
    conversations
      .filter((c) => c.account_id === accountId && c.conversation_type === 'direct')
      .map((c) => ({
        id: c.id,
        account_id: c.account_id,
        remote_id: c.remote_id,
        display_name: c.title,
        avatar_url: null,
        metadata: { saved: true, has_chat: true },
      })),
  startConversation: async (accountId: string, remoteId: string, title: string) => {
    const existing = conversations.find((c) => c.account_id === accountId && c.remote_id === remoteId);
    if (existing) return existing;
    const conv: Conversation = {
      id: crypto.randomUUID(),
      account_id: accountId,
      remote_id: remoteId,
      contact_id: null,
      title: title || remoteId,
      conversation_type: 'direct',
      unread_count: 0,
      last_message_at: new Date().toISOString(),
      last_message_preview: '',
      pinned: false,
      archived: false,
      muted: false,
      metadata: {},
    };
    conversations = [conv, ...conversations];
    return conv;
  },
  checkWhatsappNumber: async (_phone: string): Promise<boolean | null> => true,
  createGroup: async (accountId: string, title: string, _participants: string[]) => {
    const conv: Conversation = {
      id: crypto.randomUUID(),
      account_id: accountId,
      remote_id: `g-${crypto.randomUUID()}`,
      contact_id: null,
      title,
      conversation_type: 'group',
      unread_count: 0,
      last_message_at: new Date().toISOString(),
      last_message_preview: '',
      pinned: false,
      archived: false,
      muted: false,
      metadata: {},
    };
    conversations = [conv, ...conversations];
  },
  updateConversation: async (id: string, patch: ConversationPatch) => {
    conversations = conversations.map((c) => (c.id === id ? { ...c, ...patch } : c));
    return conversations.find((c) => c.id === id)!;
  },
  getMessagePage: async (
    conversationId: string,
    anchor: MessageAnchor = { kind: 'newest' },
    limit = 50
  ): Promise<MessagePage> => {
    const all = messages[conversationId] ?? [];
    const at = (id: string) => all.findIndex((m) => m.id === id);
    if (anchor.kind === 'newest') {
      return {
        messages: all.slice(-limit),
        has_older: all.length > limit,
        has_newer: false,
      };
    }
    const index = at(anchor.message_id);
    if (index < 0) return { messages: [], has_older: false, has_newer: false };
    if (anchor.kind === 'before') {
      const end = index;
      const start = Math.max(0, end - limit);
      return {
        messages: all.slice(start, end),
        has_older: start > 0,
        has_newer: true,
      };
    }
    if (anchor.kind === 'after') {
      const start = index + 1;
      const end = Math.min(all.length, start + limit);
      return {
        messages: all.slice(start, end),
        has_older: true,
        has_newer: end < all.length,
      };
    }
    if (anchor.kind === 'from') {
      const start = Math.max(0, index - limit + 1);
      return {
        messages: all.slice(start, index + 1),
        has_older: start > 0,
        has_newer: index < all.length - 1,
      };
    }
    const half = Math.max(1, Math.floor(limit / 2));
    const start = Math.max(0, index - half + 1);
    const end = Math.min(all.length, index + (limit - half) + 1);
    return {
      messages: all.slice(start, end),
      has_older: start > 0,
      has_newer: end < all.length,
    };
  },
  sendMessage: async (
    _accountId: string,
    conversationId: string,
    text: string,
    replyToMessageId: string | null = null
  ) => {
    const original = replyToMessageId
      ? (messages[conversationId] ?? []).find((m) => m.id === replyToMessageId)
      : undefined;
    const id = crypto.randomUUID();
    const msg: Message = {
      id,
      conversation_id: conversationId,
      // The mock "sends" at once, so the message has a network id like an echoed send.
      remote_id: `mock-${id}`,
      sender_id: null,
      sender_name: 'You',
      direction: 'outbound',
      body: text,
      timestamp: new Date().toISOString(),
      status: 'sent',
      metadata: original?.remote_id ? { reply_to: { remote_id: original.remote_id } } : {},
      reply: original?.remote_id
        ? {
            remote_id: original.remote_id,
            message_id: original.id,
            sender_name: original.direction === 'outbound' ? 'You' : (original.sender_name ?? undefined),
            from_me: original.direction === 'outbound',
            snippet: original.body.slice(0, 160),
            available: true,
          }
        : undefined,
    };
    messages[conversationId] = [...(messages[conversationId] ?? []), msg];
    conversations = conversations.map((c) =>
      c.id === conversationId
        ? {
            ...c,
            last_message_preview: text,
            last_message_at: msg.timestamp,
            last_message_from_me: true,
            last_message_sender_id: null,
            last_message_sender_name: null,
          }
        : c
    );
    return msg;
  },
  sendAttachment: async (accountId: string, conversationId: string, attachment: AttachmentPayload) => {
    const body =
      attachment.caption ||
      attachment.question ||
      attachment.filename ||
      `[${attachment.kind}]`;
    return mockApi.sendMessage(accountId, conversationId, body, attachment.reply_to_message_id ?? null);
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
  totalUnread: async () =>
    conversations.reduce((s, c) => {
      if (c.archived || c.muted) return s;
      const account = accounts.find((a) => a.id === c.account_id);
      if (account?.muted) return s;
      return s + c.unread_count;
    }, 0),
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
  addTodo: async (
    conversationId: string,
    accountId: string,
    body: string,
    dueAt?: string,
    messageRef?: { id: string; snippet: string }
  ) => {
    const t: ChatTodo = {
      id: crypto.randomUUID(),
      conversation_id: conversationId,
      account_id: accountId,
      body,
      due_at: dueAt ?? null,
      done: false,
      created_at: new Date().toISOString(),
      message_id: messageRef?.id ?? null,
      message_snippet: messageRef?.snippet ?? null,
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
  listNotes: async (conversationId: string) => notes.filter((n) => n.conversation_id === conversationId),
  addNote: async (
    conversationId: string,
    accountId: string,
    body: string,
    opts: { messageRef?: { id: string; snippet: string }; schedule?: NoteSchedule } = {}
  ) => {
    const n: ChatNote = {
      id: crypto.randomUUID(),
      conversation_id: conversationId,
      account_id: accountId,
      body,
      message_id: opts.messageRef?.id ?? null,
      message_snippet: opts.messageRef?.snippet ?? null,
      ...mockSchedule(opts.schedule),
      fired: false,
      created_at: new Date().toISOString(),
    };
    notes = [...notes, n];
    return n;
  },
  updateNote: async (
    id: string,
    patch: { body?: string; schedule?: NoteSchedule; clearSchedule?: boolean }
  ) => {
    let updated: ChatNote | undefined;
    notes = notes.map((n) => {
      if (n.id !== id) return n;
      updated = {
        ...n,
        body: patch.body ?? n.body,
        ...(patch.schedule
          ? { ...mockSchedule(patch.schedule), fired: false }
          : patch.clearSchedule
            ? { fire_at: null, kind: null, fired: false }
            : {}),
      };
      return updated;
    });
    if (!updated) throw new Error('note not found');
    return updated;
  },
  deleteNote: async (id: string) => {
    notes = notes.filter((n) => n.id !== id);
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
      attempts: 0,
      last_error: null,
      failed: false,
    };
    scheduledMessages = [...scheduledMessages, msg];
    return msg;
  },
  deleteScheduledMessage: async (id: string) => {
    scheduledMessages = scheduledMessages.filter((msg) => msg.id !== id);
  },
  updateScheduledMessage: async (id: string, patch: { body?: string; send_at?: string }) => {
    scheduledMessages = scheduledMessages.map((msg) =>
      msg.id === id && !msg.sent
        ? {
            ...msg,
            body: patch.body ?? msg.body,
            send_at: patch.send_at ?? msg.send_at,
          }
        : msg
    );
    return scheduledMessages.find((msg) => msg.id === id)!;
  },
  exportBackup: async (_path: string, _password: string, includeMessages = true, includeMedia = false) =>
    ({
      exported_at: new Date().toISOString(),
      includes_messages: includeMessages,
      includes_media: includeMedia,
    }) satisfies BackupManifest,
  restoreBackup: async (_path?: string, _password?: string) => {},
  applyMessageOp: async (conversationId: string, messageIds: string[], op: MessageOp): Promise<MessageOpResult[]> =>
    messageIds.map((id) => {
      const msg = (messages[conversationId] ?? []).find((m) => m.id === id);
      const error = msg ? applyMockOp(conversationId, msg, op) : 'Message not found';
      return error ? { message_id: id, error } : { message_id: id };
    }),
  forwardMessages: async (
    sourceConversationId: string,
    messageIds: string[],
    targets: ForwardTarget[],
    sendAt: string | null
  ): Promise<ForwardResult[]> => {
    await new Promise((r) => setTimeout(r, 400));
    const stored = messages[sourceConversationId] ?? [];
    const sources = messageIds
      .map((id) => stored.find((m) => m.id === id))
      .filter((m): m is Message => !!m)
      .sort((a, b) => a.timestamp.localeCompare(b.timestamp));
    const results: ForwardResult[] = [];
    for (const target of targets) {
      for (const msg of sources) results.push(await mockForwardOne(target, msg, sendAt));
    }
    return results;
  },
  starMessage: async (messageId: string, starred: boolean): Promise<Message> => {
    for (const [conversationId, list] of Object.entries(messages)) {
      const found = list.find((m) => m.id === messageId);
      if (!found) continue;
      const next = { ...found, starred };
      messages[conversationId] = list.map((m) => (m === found ? next : m));
      return next;
    }
    throw new Error('Message not found');
  },
  senderContact: async (accountId: string, senderId: string): Promise<SenderContact> => ({
    conversation:
      conversations.find((c) => c.account_id === accountId && c.conversation_type === 'direct' && c.remote_id === senderId) ?? null,
    contact: null,
  }),
  translateText: async (conversationId: string, messageIds: string[], target: string): Promise<Translation[]> => {
    if (!appConfig.ai?.openrouter_api_key.trim()) throw { kind: 'not_configured' };
    await new Promise((r) => setTimeout(r, 300));
    return messageIds.map((id) => {
      const body = (messages[conversationId] ?? []).find((m) => m.id === id)?.body ?? '';
      return body.trim()
        ? { message_id: id, text: `(${target}) ${body}` }
        : { message_id: id, error: 'Nothing to translate' };
    });
  },
  onShuttleEvent: async (handler: (event: ShuttleEvent) => void) => {
    listeners.add(handler);
    emitLater({
      kind: 'status.feed',
      payload: { account_id: 'wa-1', items: DEMO_STATUSES.filter((item) => item.account_id === 'wa-1') },
    });
    emitLater({
      kind: 'status.feed',
      payload: { account_id: 'tg-1', items: DEMO_STATUSES.filter((item) => item.account_id === 'tg-1') },
    });
    return () => {
      listeners.delete(handler);
    };
  },
};

export function isTauri(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}
