<script lang="ts">
  import DateTimeField from '$lib/components/DateTimeField.svelte';
  import MessageQuote from '$lib/components/MessageQuote.svelte';
  import NoteScheduleField from '$lib/components/NoteScheduleField.svelte';
  import { datePrefs } from '$lib/dateFormatPref.svelte';
  import {
    formatCountdown,
    isCompleteLocalDateTime,
    isoToLocalDateTime,
    nextMinute,
    toLocalDateTime,
  } from '$lib/localDateTime';
  import { buildSchedule, type NoteScheduleMode } from '$lib/noteSchedule';
  import {
    addNote,
    addTodo,
    deleteNote,
    deleteScheduledMessage,
    deleteTodo,
    listNotes,
    listTodos,
    onShuttleEvent,
    setTodoDone,
    updateNote,
    updateScheduledMessage,
    type MessageRef,
  } from '$lib/api';
  import type { ChatNote, ChatTodo, Conversation, ScheduledMessage } from '$lib/types';

  interface Props {
    conversation: Conversation;
    scheduledMessages: ScheduledMessage[];
    placement?: 'side' | 'top';
    /** A message the next note should quote ("Add note" in the message menu). */
    noteDraftRef?: MessageRef | null;
    /** The pending note draft was added or dismissed. */
    onnotedraftdone?: () => void;
    /** Scroll the thread to a message (loading older history when needed). */
    onjumptomessage?: (messageId: string) => void;
    onupdated: () => void;
  }

  let {
    conversation,
    scheduledMessages,
    placement = 'side',
    noteDraftRef = null,
    onnotedraftdone,
    onjumptomessage,
    onupdated,
  }: Props = $props();

  let notes = $state<ChatNote[]>([]);
  let todos = $state<ChatTodo[]>([]);
  let todoDraft = $state('');
  let editingId = $state<string | null>(null);
  let editBody = $state('');
  let editAt = $state('');

  // New note composer
  let noteDraft = $state('');
  let noteRef = $state<MessageRef | null>(null);
  let noteMode = $state<NoteScheduleMode>('none');
  let noteTimerSecs = $state(600);
  let noteAlarmAt = $state('');
  let noteBusy = $state(false);
  let noteEl = $state<HTMLTextAreaElement | null>(null);

  // Editing an existing note's text, or giving it a schedule
  let editingNoteId = $state<string | null>(null);
  let editingNoteBody = $state('');
  let remindingId = $state<string | null>(null);
  let remindMode = $state<NoteScheduleMode>('timer');
  let remindTimerSecs = $state(600);
  let remindAlarmAt = $state('');

  let now = $state(Date.now());

  const queued = $derived(
    scheduledMessages.filter((m) => m.dest_conversation_id === conversation.id && !m.sent)
  );
  const newSchedule = $derived(buildSchedule(noteMode, noteTimerSecs, noteAlarmAt));
  const canAddNote = $derived(
    !noteBusy && newSchedule !== undefined && (noteDraft.trim() !== '' || noteRef !== null)
  );

  async function reloadNotes() {
    notes = await listNotes(conversation.id);
  }

  async function reloadTodos() {
    todos = await listTodos(conversation.id);
  }

  $effect(() => {
    const id = conversation.id;
    listNotes(id).then((n) => {
      if (conversation.id === id) notes = n;
    });
    listTodos(id).then((t) => {
      if (conversation.id === id) todos = t;
    });
    editingId = null;
    editingNoteId = null;
    remindingId = null;
  });

  // A timer or alarm fired in the backend: show it as fired right away.
  $effect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    onShuttleEvent((event) => {
      if (event.kind === 'reminder.fired' && event.payload.conversation_id === conversation.id) {
        void reloadNotes();
      }
    }).then((fn) => {
      if (cancelled) fn();
      else unlisten = fn;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  });

  // Tick the countdowns: every second while a timer is about to fire, rarely otherwise.
  $effect(() => {
    const pending = notes.filter((n) => n.fire_at && !n.fired);
    if (pending.length === 0) return;
    const soon = pending.some((n) => n.kind === 'timer' && Date.parse(n.fire_at!) - Date.now() < 3600_000);
    const id = setInterval(() => {
      now = Date.now();
      // Past its time but the backend has not fired it yet: pick up the result soon.
      if (pending.some((n) => Date.parse(n.fire_at!) <= now - 1000)) void reloadNotes();
    }, soon ? 1000 : 15000);
    now = Date.now();
    return () => clearInterval(id);
  });

  // "Add note" on a message: start a composer that quotes it.
  $effect(() => {
    if (noteDraftRef) {
      noteRef = noteDraftRef;
      queueMicrotask(() => noteEl?.focus());
    }
  });

  function resetComposer() {
    noteDraft = '';
    noteRef = null;
    noteMode = 'none';
    noteTimerSecs = 600;
    noteAlarmAt = '';
  }

  async function submitNote() {
    if (!canAddNote) return;
    noteBusy = true;
    try {
      await addNote(conversation.id, conversation.account_id, noteDraft.trim(), {
        messageRef: noteRef ?? undefined,
        schedule: newSchedule ?? undefined,
      });
      resetComposer();
      onnotedraftdone?.();
      await reloadNotes();
    } finally {
      noteBusy = false;
    }
  }

  function dismissComposerRef() {
    noteRef = null;
    onnotedraftdone?.();
  }

  async function saveNoteBody(note: ChatNote) {
    const body = editingNoteBody.trim();
    editingNoteId = null;
    if (body === note.body || (body === '' && !note.message_id)) return;
    await updateNote(note.id, { body });
    await reloadNotes();
  }

  function startRemind(note: ChatNote) {
    remindingId = note.id;
    remindMode = 'timer';
    remindTimerSecs = 600;
    remindAlarmAt = '';
  }

  const remindSchedule = $derived(buildSchedule(remindMode, remindTimerSecs, remindAlarmAt));

  async function saveRemind(note: ChatNote) {
    if (!remindSchedule) return;
    await updateNote(note.id, { schedule: remindSchedule });
    remindingId = null;
    await reloadNotes();
  }

  async function cancelSchedule(note: ChatNote) {
    await updateNote(note.id, { clearSchedule: true });
    await reloadNotes();
  }

  async function removeNote(note: ChatNote) {
    await deleteNote(note.id);
    await reloadNotes();
  }

  /** "⏱ 4m 09s left", "⏰ Fri 09:00", or the fired state of either. */
  function scheduleLabel(note: ChatNote, at: number): string {
    if (!note.fire_at) return '';
    const fire = Date.parse(note.fire_at);
    const icon = note.kind === 'timer' ? '⏱' : '⏰';
    if (note.fired) return `${icon} ${note.kind === 'timer' ? 'Timer done' : 'Alarm rang'}`;
    if (note.kind === 'timer') {
      return fire - at <= 0 ? `${icon} Due now` : `${icon} ${formatCountdown(fire - at)} left`;
    }
    return `${icon} ${new Date(fire).toLocaleString(undefined, {
      weekday: 'short',
      day: 'numeric',
      month: 'short',
      hour: 'numeric',
      minute: '2-digit',
      hour12: datePrefs.hour12,
    })}`;
  }

  async function add() {
    if (!todoDraft.trim()) return;
    await addTodo(conversation.id, conversation.account_id, todoDraft.trim());
    todoDraft = '';
    await reloadTodos();
  }

  function startEdit(msg: ScheduledMessage) {
    editingId = msg.id;
    editBody = msg.body;
    editAt = isoToLocalDateTime(msg.send_at);
  }

  async function saveEdit() {
    if (!editingId || !isCompleteLocalDateTime(editAt)) return;
    await updateScheduledMessage(editingId, {
      body: scheduledMessages.find((m) => m.id === editingId)?.forward ? undefined : editBody.trim() || undefined,
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
  <ul class="note-list">
    {#each notes as note (note.id)}
      <li class="note" class:fired={note.fired}>
        {#if note.message_snippet}
          <MessageQuote
            snippet={note.message_snippet}
            onclick={note.message_id && onjumptomessage ? () => onjumptomessage(note.message_id!) : undefined}
          />
        {/if}
        {#if editingNoteId === note.id}
          <textarea bind:value={editingNoteBody} rows="3"></textarea>
          <div class="row">
            <button type="button" onclick={() => saveNoteBody(note)}>Save</button>
            <button type="button" class="tiny" onclick={() => (editingNoteId = null)}>Cancel</button>
          </div>
        {:else if note.body}
          <div class="note-body">{note.body}</div>
        {/if}
        {#if note.fire_at}
          <div class="note-when" class:done={note.fired}>
            <span>{scheduleLabel(note, now)}</span>
            <button type="button" class="tiny" onclick={() => cancelSchedule(note)}>
              {note.fired ? 'Clear' : note.kind === 'timer' ? 'Cancel timer' : 'Cancel alarm'}
            </button>
          </div>
        {/if}
        {#if remindingId === note.id}
          <NoteScheduleField bind:mode={remindMode} bind:timerSecs={remindTimerSecs} bind:alarmAt={remindAlarmAt} />
          <div class="row">
            <button type="button" onclick={() => saveRemind(note)} disabled={!remindSchedule}>Set</button>
            <button type="button" class="tiny" onclick={() => (remindingId = null)}>Cancel</button>
          </div>
        {:else if editingNoteId !== note.id}
          <div class="row actions">
            <button
              type="button"
              class="tiny"
              onclick={() => {
                editingNoteId = note.id;
                editingNoteBody = note.body;
              }}>Edit</button
            >
            {#if !note.fire_at}
              <button type="button" class="tiny" onclick={() => startRemind(note)}>Remind me</button>
            {/if}
            <button type="button" class="tiny" onclick={() => removeNote(note)}>Delete</button>
          </div>
        {/if}
      </li>
    {/each}
  </ul>

  <div class="composer">
    {#if noteRef}
      <div class="quote-row">
        <MessageQuote snippet={noteRef.snippet} />
        <button type="button" class="tiny" aria-label="Don't link a message" onclick={dismissComposerRef}>×</button>
      </div>
    {/if}
    <textarea
      bind:this={noteEl}
      bind:value={noteDraft}
      rows="3"
      placeholder="Add a note — stays on this device"
      onkeydown={(e) => {
        if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) {
          e.preventDefault();
          void submitNote();
        }
      }}
    ></textarea>
    <NoteScheduleField bind:mode={noteMode} bind:timerSecs={noteTimerSecs} bind:alarmAt={noteAlarmAt} disabled={noteBusy} />
    <button type="button" onclick={submitNote} disabled={!canAddNote}>Add note</button>
  </div>

  <h3>Todos</h3>
  <ul>
    {#each todos as todo (todo.id)}
      <li class="todo">
        <div class="todo-main">
          <input
            type="checkbox"
            checked={todo.done}
            onchange={async (e) => {
              await setTodoDone(todo.id, e.currentTarget.checked);
              await reloadTodos();
            }}
          />
          <span class="todo-text" class:done={todo.done}>{todo.body}</span>
          <button
            type="button"
            class="tiny"
            aria-label="Delete todo"
            onclick={async () => {
              await deleteTodo(todo.id);
              await reloadTodos();
            }}>×</button
          >
        </div>
        {#if todo.message_snippet}
          <MessageQuote
            snippet={todo.message_snippet}
            onclick={todo.message_id && onjumptomessage ? () => onjumptomessage(todo.message_id!) : undefined}
          />
        {/if}
      </li>
    {/each}
  </ul>
  <div class="row">
    <input bind:value={todoDraft} placeholder="Add a todo" onkeydown={(e) => e.key === 'Enter' && add()} />
    <button type="button" onclick={add}>Add</button>
  </div>

  <h3>Send later</h3>
  {#if queued.length === 0}
    <p class="empty-hint">No scheduled messages for this chat</p>
  {:else}
    <ul class="sched-list">
      {#each queued as msg (msg.id)}
        <li class="sched-item">
          {#if editingId === msg.id}
            {#if !msg.forward}
              <textarea bind:value={editBody} rows="3"></textarea>
            {/if}
            <DateTimeField bind:value={editAt} min={toLocalDateTime(nextMinute())} title="Send at" />
            <div class="row">
              <button type="button" onclick={saveEdit} disabled={!isCompleteLocalDateTime(editAt)}>Save</button>
              <button type="button" class="tiny" onclick={() => (editingId = null)}>Cancel</button>
            </div>
          {:else}
            <div class="sched-body">{#if msg.forward}<span class="sched-forward">Forward:</span> {/if}{msg.body}</div>
            <div class="sched-when">{new Date(msg.send_at).toLocaleString(undefined, { dateStyle: 'medium', timeStyle: 'short', hour12: datePrefs.hour12 })}</div>
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
  .sched-forward {
    margin-right: 4px;
    color: var(--text-muted);
    font-style: italic;
  }
    .sched-when {
    font-size: 11px;
    color: var(--text-muted);
  }
  .note-list {
    gap: 8px;
  }
  .note {
    flex-direction: column;
    align-items: stretch;
    gap: 6px;
    padding: 8px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    background: var(--bg-hover);
  }
  .note.fired {
    opacity: 0.8;
  }
  .note-body {
    white-space: pre-wrap;
    word-break: break-word;
  }
  .note-when {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 6px;
    font-size: 12px;
    color: var(--accent);
    font-weight: 600;
  }
  .note-when.done {
    color: var(--text-muted);
    font-weight: 500;
  }
  .actions {
    justify-content: flex-start;
  }
  .composer {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .quote-row {
    display: flex;
    align-items: flex-start;
    gap: 4px;
  }
  .todo {
    flex-direction: column;
    align-items: stretch;
    gap: 4px;
  }
  .todo-main {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .todo-text {
    flex: 1;
    min-width: 0;
    word-break: break-word;
  }
</style>
