# Shuttle UI Glossary

Reference for naming the parts of the frontend when giving directions.

---

## Overall Layout (Desktop)

Shuttle is a 3-column messenger layout:

```
┌──────────┬─────────────────┬──────────────────────────────┐
│ Sidebar  │  List pane      │  Thread pane                 │
│ (rail)   │  (conversations)│  (active chat)               │
│          │                 │  [+ optional Chat panel]     │
└──────────┴─────────────────┴──────────────────────────────┘
```

On **mobile**, it stacks: list or thread on top, with a bottom **Inbox | Settings** tab bar instead of the left rail.

---

## 1. Sidebar (left rail)

**Component:** `Sidebar.svelte`  
**Also called:** sidebar, rail, network rail, left nav

| What you see | What to call it |
|---|---|
| Shuttle logo at top | brand / logo button |
| Speech bubble icon (all chats) | All Chats button |
| Red badge on All Chats | unread badge / total unread count |
| Colored network icons (WhatsApp, Telegram, etc.) | account buttons / network buttons |
| Yellow/orange dot on an account | status dot (connecting/disconnected) |
| `+` button at bottom | Add account button |
| Gear icon at bottom | Settings button (desktop) |
| Horizontal line between All Chats and accounts | divider |

**Mobile only:** the rail is hidden; you get **Inbox** and **Settings** tabs at the bottom instead.

---

## 2. List Pane (middle column)

**Container class:** `list-pane`  
**Main component:** `ConversationList.svelte`

### Org filters
**Class:** `org-filters`  
Workspace and Priority dropdowns above the list. Only shown when custom workspaces or priority groups exist.

### Conversation list header

| What you see | What to call it |
|---|---|
| "All Chats" / account name title | list header / header title |
| Unread count next to title | header badge |
| Search icon / search bar | search / search bar |
| "New" button | New / compose button |
| "Archived" toggle | archived toggle |
| WhatsApp / Telegram / All chips | filter chips / network filters |

### Conversation rows
Each chat is a **conv-item** (conversation row):

| Part | Name |
|---|---|
| Avatar circle | avatar |
| Small network icon on avatar | network badge |
| Blue dot on avatar | unread dot |
| Contact/group name | title |
| Last message preview | preview |
| Time on the right | time |
| Number badge | unread badge |
| Pin icon | pinned conversation |

Pinned chats appear in a **pinned section** above the rest.

### Compose modals (from "New")
- **New contact chat** — start a DM
- **New group** — create a group chat

---

## 3. Thread Pane (right column — active chat)

**Container class:** `thread-pane`  
**Main component:** `ThreadView.svelte`

### Thread header
**Class:** `thread-header`

| Part | Name |
|---|---|
| Back arrow (mobile) | back button |
| Avatar + network badge | header avatar |
| Name + account subtitle | header info (click opens contact details) |
| Info `ⓘ` button | panel button / chat details button |

### Messages area
**Class:** `messages`

| Part | Name |
|---|---|
| "Today", "Yesterday", etc. | date separator |
| Each message row | message row / msg-row |
| The bubble itself | bubble |
| Your messages (right) | outbound messages |
| Their messages (left) | inbound messages |
| Sender name in groups | sender label |
| Images/videos/audio | message media |
| Timestamp + checkmarks | msg-time / read receipts (✓ sent, ✓✓ delivered/read) |

Right-click a message → **context menu** (Copy, Reply, Forward, etc.)

### Composer (bottom input area)
**Class:** `composer`

| Part | Name |
|---|---|
| Bold / Italic / Strike / Code buttons | format bar |
| Emoji / Sticker / GIF buttons | picker buttons |
| Popups from those buttons | picker popover |
| Paperclip | attach button → attach menu (Photo, Video, Audio, Document, Location, Poll) |
| Clock icon | send later picker |
| Mic button | voice record |
| Text input box | composer input / draft / message box |
| Send button | send button |

When no chat is selected, you see the **empty thread** state.

---

## 4. Chat Panel (side extras)

**Component:** `ChatPanel.svelte`  
**Also called:** chat panel, extras panel, side panel

Opens from the `ⓘ` button in the thread header. Sections:

| Section | Name |
|---|---|
| Local notes textarea | Notes |
| Checkbox todo list | Todos |
| "Remind me" datetime + note | Remind me |
| Queued future messages | Send later (scheduled messages for this chat) |

On narrow screens it moves to the **top** of the thread instead of the side.

---

## 5. Settings Pane

**Container class:** `settings-pane`  
**Component:** `SettingsPanel.svelte`

Replaces the list pane when Settings is open. Tabs:

| Tab | Covers |
|---|---|
| Appearance | Theme, color scheme, font scale, datetime format |
| Notifications | Enable/disable, quiet hours |
| Privacy | Crash reports, usage diagnostics |
| Channels | Per-network color/tag styling |
| Workspaces & priorities | Organize chats into buckets |
| Accounts | Manage connected accounts |
| Routing | Forward rules (auto-forward messages) |
| Backup | Export/restore encrypted backup |
| About | License, attributions |

---

## 6. Overlays & Modals

| UI | Component | When it appears |
|---|---|---|
| Account setup | `AccountSetup.svelte` | Adding a new account, QR login, connector install |
| Contact details | `ContactDetails.svelte` | Click thread header → workspace/priority for that chat |
| Remind modal | `RemindModal.svelte` | "Remind me about this chat" from message menu |
| Forward modal | inline in `+page.svelte` | Forward a message to another chat |
| Context menu | `ContextMenu.svelte` | Right-click on conversation, message, or account |
| Compose modal | inside `ConversationList.svelte` | New DM or new group |

**Account setup** has two views: connector **list** (pick network) and **setup** (credentials, QR code, verification).

---

## 7. Quick Reference

| You say… | I'll look at… |
|---|---|
| "the rail" / "left sidebar" | `Sidebar.svelte` |
| "All Chats button" | sidebar, `all-chats` nav item |
| "the conversation list" / "inbox list" | `ConversationList.svelte` |
| "a chat row" / "conversation item" | `conv-item` in the list |
| "filter chips" | network filter buttons under the list header |
| "the chat view" / "thread" | `ThreadView.svelte` |
| "message bubbles" | `.bubble` inside `.messages` |
| "the composer" / "message input" | `.composer` footer in ThreadView |
| "attach menu" / "paperclip menu" | attach picker popover |
| "format bar" | bold/italic/strike/code buttons |
| "chat panel" / "notes panel" | `ChatPanel.svelte` |
| "contact details modal" | `ContactDetails.svelte` |
| "add account flow" / "setup wizard" | `AccountSetup.svelte` |
| "settings" + tab name | `SettingsPanel.svelte` → that tab |
| "right-click menu" / "context menu" | `ContextMenu.svelte` |
| "forward modal" | forward dialog in `+page.svelte` |
| "workspace/priority dropdowns" | `org-filters` above the list |

---

## 8. Domain Terms

| Term | Meaning |
|---|---|
| Account | One logged-in network identity (e.g. your WhatsApp) |
| Connector | The network type (whatsapp, telegram, signal, etc.) |
| Conversation | A chat thread (DM, group, or channel) |
| Workspace | A folder/bucket to group conversations |
| Priority group | A label tier for important chats |
| Forward rule | Auto-forward messages between chats/accounts |
