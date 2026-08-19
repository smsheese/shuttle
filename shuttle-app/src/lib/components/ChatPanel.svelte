<script lang="ts">
  import {
    addTodo,
    createReminder,
    deleteReminder,
    deleteScheduledMessage,
    deleteTodo,
    listReminders,
    listTodos,
    setTodoDone,
    updateConversation,
    updateScheduledMessage,
  } from '$lib/api';
  import type { ChatTodo, Conversation, Reminder, ScheduledMessage } from '$lib/types';

  interface Props {
    conversation: Conversation;
    scheduledMessages: ScheduledMessage[];
    placement?: 'side' | 'top';
    onupdated: () => void;
  }

  let { conversation, scheduledMessages, placement = 'side', onupdated }: Props = $props();
  let notes = $state('');
  let todos = $state<ChatTodo[]>([]);
  let reminders = $state<Reminder[]>([]);
  let todoDraft = $state('');
  let remindAt = $state('');
  let remindNote = $state('');
  let editingId = $state<string | null>(null);
  let editBody = $state('');
  let editAt = $state('');

  const queued = $derived(
    scheduledMessages.filter((m) => m.dest_conversation_id === conversation.id && !m.sent)
  );

  $effect(() => {
    notes = conversation.notes ?? '';
    listTodos(conversation.id).then((t) => (todos = t));
    listReminders(conversation.id).then((r) => (reminders = r.filter((x) => !x.fired)));
    editingId = null;
  });

  function toLocalInput(iso: string): string {
    const d = new Date(iso);
    if (Number.isNaN(d.getTime())) return '';
    const pad = (n: number) => String(n).padStart(2, '0');
    return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}T${pad(d.getHours())}:${pad(d.getMinutes())}`;
  }

  async function saveNotes() {
    await updateConversation(conversation.id, { notes });
    onupdated();
  }

  async function add() {
    if (!todoDraft.trim()) return;
    await addTodo(conversation.id, conversation.account_id, todoDraft.trim());
    todoDraft = '';
    todos = await listTodos(conversation.id);
  }

  async function addReminder() {
    if (!remindAt) return;
    const iso = new Date(remindAt).toISOString();
    await createReminder(conversation.id, conversation.account_id, iso, 'nudge', remindNote || undefined);
    remindAt = '';
    remindNote = '';
    reminders = (await listReminders(conversation.id)).filter((x) => !x.fired);
  }

  function startEdit(msg: ScheduledMessage) {
    editingId = msg.id;
    editBody = msg.body;
    editAt = toLocalInput(msg.send_at);
  }

  async function saveEdit() {
    if (!editingId || !editAt) return;
    await updateScheduledMessage(editingId, {
      body: editBody.trim() || undefined,
      send_at: new Date(editAt).toISOString(),
    });
    editingId = null;
    onupdated();
  }

  async function cancelScheduled(id: string) {
    await deleteScheduledMessage(id);
    if (editingId === id) editingId = null;
    onupdated();
  }
</script>

<aside class="panel" class:top={placement === 'top'}>
  <h3>Notes</h3>
  <textarea bind:value={notes} rows="4" placeholder="Local notes — never sent to the network" onblur={saveNotes}></textarea>

  <h3>Todos</h3>
  <ul>
    {#each todos as todo (todo.id)}
      <li>
        <input
          type="checkbox"
          checked={todo.done}
          onchange={async (e) => {
            await setTodoDone(todo.id, e.currentTarget.checked);
            todos = await listTodos(conversation.id);
          }}
        />
        <span class:done={todo.done}>{todo.body}</span>
        <button
          type="button"
          class="tiny"
          onclick={async () => {
            await deleteTodo(todo.id);
            todos = await listTodos(conversation.id);
          }}>×</button
        >
      </li>
    {/each}
  </ul>
  <div class="row">
    <input bind:value={todoDraft} placeholder="Add a todo" onkeydown={(e) => e.key === 'Enter' && add()} />
    <button type="button" onclick={add}>Add</button>
  </div>

  <h3>Remind me</h3>
  <input type="datetime-local" bind:value={remindAt} />
  <input bind:value={remindNote} placeholder="Optional note" />
  <button type="button" onclick={addReminder}>Set reminder</button>
  <ul>
    {#each reminders as r (r.id)}
      <li>
        <span>{new Date(r.fire_at).toLocaleString()}</span>
        <button
          type="button"
          class="tiny"
          onclick={async () => {
            await deleteReminder(r.id);
            reminders = (await listReminders(conversation.id)).filter((x) => !x.fired);
          }}>×</button
        >
      </li>
    {/each}
  </ul>

  <h3>Send later</h3>
  {#if queued.length === 0}
    <p class="empty-hint">No scheduled messages for this chat</p>
  {:else}
    <ul class="sched-list">
      {#each queued as msg (msg.id)}
        <li class="sched-item">
          {#if editingId === msg.id}
            <textarea bind:value={editBody} rows="3"></textarea>
            <input type="datetime-local" bind:value={editAt} />
            <div class="row">
              <button type="button" onclick={saveEdit}>Save</button>
              <button type="button" class="tiny" onclick={() => (editingId = null)}>Cancel</button>
            </div>
          {:else}
            <div class="sched-body">{msg.body}</div>
            <div class="sched-when">{new Date(msg.send_at).toLocaleString()}</div>
            <div class="row">
              <button type="button" class="tiny" onclick={() => startEdit(msg)}>Edit</button>
              <button type="button" class="tiny" onclick={() => cancelScheduled(msg.id)}>Cancel send</button>
            </div>
          {/if}
        </li>
      {/each}
    </ul>
  {/if}
</aside>

<style>
  .panel {
    width: 260px;
    min-width: 220px;
    border-left: 1px solid var(--border-subtle);
    background: var(--bg-panel);
    padding: 16px;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .panel.top {
    width: 100%;
    min-width: 0;
    max-height: 38%;
    border-left: none;
    border-bottom: 1px solid var(--border-subtle);
  }

  h3 {
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--text-muted);
    margin-top: 8px;
  }
  textarea, input, button {
    font: inherit;
    color: inherit;
  }
  textarea, input, button {
    width: 100%;
    padding: 8px;
  }
  textarea, input:not([type='checkbox']) {
    background: var(--bg-input);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    color: var(--text);
  }
  button {
    cursor: pointer;
    background: var(--accent-muted);
    border-color: transparent;
    color: var(--accent);
    font-weight: 600;
  }
  .row {
    display: flex;
    gap: 6px;
  }
  ul {
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  li {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 13px;
  }
  .done {
    text-decoration: line-through;
    color: var(--text-muted);
  }
  .tiny {
    width: auto;
    padding: 2px 8px;
    background: transparent;
    color: var(--text-muted);
  }
  input[type='checkbox'] {
    width: auto;
    accent-color: var(--accent);
  }
  .empty-hint {
    font-size: 12px;
    color: var(--text-muted);
  }
  .sched-list {
    gap: 10px;
  }
  .sched-item {
    flex-direction: column;
    align-items: stretch;
    padding: 8px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    background: var(--bg-hover);
  }
  .sched-body {
    white-space: pre-wrap;
    word-break: break-word;
    font-size: 13px;
  }
  .sched-when {
    font-size: 11px;
    color: var(--text-muted);
  }
</style>
