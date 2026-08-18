<script lang="ts">
  import {
    addTodo,
    createReminder,
    deleteReminder,
    deleteTodo,
    listReminders,
    listTodos,
    setTodoDone,
    updateConversation,
  } from '$lib/api';
  import type { ChatTodo, Conversation, PriorityGroup, Reminder, Workspace } from '$lib/types';

  interface Props {
    conversation: Conversation;
    workspaces: Workspace[];
    priorityGroups: PriorityGroup[];
    onupdated: () => void;
  }

  let { conversation, workspaces, priorityGroups, onupdated }: Props = $props();
  let notes = $state(conversation.notes ?? '');
  let todos = $state<ChatTodo[]>([]);
  let reminders = $state<Reminder[]>([]);
  let todoDraft = $state('');
  let remindAt = $state('');
  let remindNote = $state('');

  $effect(() => {
    notes = conversation.notes ?? '';
    listTodos(conversation.id).then((t) => (todos = t));
    listReminders(conversation.id).then((r) => (reminders = r.filter((x) => !x.fired)));
  });

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

  async function setWorkspace(id: string) {
    if (id === '') await updateConversation(conversation.id, { clear_workspace: true });
    else await updateConversation(conversation.id, { workspace_id: id });
    onupdated();
  }

  async function setPriority(id: string) {
    if (id === '') await updateConversation(conversation.id, { clear_priority: true });
    else await updateConversation(conversation.id, { priority_group: id });
    onupdated();
  }

  async function addReminder() {
    if (!remindAt) return;
    const iso = new Date(remindAt).toISOString();
    await createReminder(conversation.id, conversation.account_id, iso, 'nudge', remindNote || undefined);
    remindAt = '';
    remindNote = '';
    reminders = (await listReminders(conversation.id)).filter((x) => !x.fired);
  }
</script>

<aside class="panel">
  <h3>Organize</h3>
  <label>
    Workspace
    <select value={conversation.workspace_id ?? ''} onchange={(e) => setWorkspace(e.currentTarget.value)}>
      <option value="">Account default</option>
      {#each workspaces as ws (ws.id)}
        <option value={ws.id}>{ws.name}</option>
      {/each}
    </select>
  </label>
  <label>
    Priority
    <select value={conversation.priority_group ?? ''} onchange={(e) => setPriority(e.currentTarget.value)}>
      <option value="">None</option>
      {#each priorityGroups as g (g.id)}
        <option value={g.id}>{g.name}</option>
      {/each}
    </select>
  </label>

  <h3>Notes</h3>
  <textarea bind:value={notes} rows="5" placeholder="Local notes — never sent to the network" onblur={saveNotes}></textarea>

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
  h3 {
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--text-muted);
    margin-top: 8px;
  }
  label, select, textarea, input, button {
    font: inherit;
    color: inherit;
  }
  select, textarea, input, button {
    width: 100%;
    background: var(--bg-input);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    padding: 8px;
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
  @media (max-width: 768px) {
    .panel {
      width: 100%;
      min-width: 0;
      border-left: none;
      border-top: 1px solid var(--border-subtle);
    }
  }
</style>
