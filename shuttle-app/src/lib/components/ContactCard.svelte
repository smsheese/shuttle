<script lang="ts">
  import type { ContactPayload } from '$lib/messageKinds';
  import { phoneDigits, vcardFor } from '$lib/messageKinds';

  let {
    contact,
    onmessage,
    onAccent = false,
  }: {
    contact: ContactPayload;
    /** Open or start the chat with this number; absent when the account cannot. */
    onmessage?: (digits: string, name: string) => void;
    onAccent?: boolean;
  } = $props();

  const phone = $derived(contact.phones[0] ?? '');
  const digits = $derived(phone ? phoneDigits(phone) : null);
  const initial = $derived((contact.name.trim()[0] ?? '?').toUpperCase());

  let copied = $state(false);

  async function copy() {
    try {
      await navigator.clipboard.writeText(phone);
      copied = true;
      setTimeout(() => (copied = false), 1500);
    } catch {
      copied = false;
    }
  }

  function save() {
    const blob = new Blob([vcardFor(contact)], { type: 'text/vcard' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = `${contact.name.replace(/[^\w.-]+/g, '_') || 'contact'}.vcf`;
    a.click();
    setTimeout(() => URL.revokeObjectURL(url), 1000);
  }
</script>

<div class="contact-card" class:on-accent={onAccent}>
  <div class="head">
    <span class="avatar" aria-hidden="true">{initial}</span>
    <span class="who">
      <span class="name">{contact.name}</span>
      {#each contact.phones as p (p)}<span class="phone">{p}</span>{/each}
    </span>
  </div>
  <div class="actions">
    {#if digits && onmessage}
      <button type="button" onclick={() => onmessage?.(digits, contact.name)}>Message</button>
    {/if}
    {#if phone}
      <button type="button" onclick={copy}>{copied ? 'Copied' : 'Copy number'}</button>
    {/if}
    <button type="button" onclick={save}>Save .vcf</button>
  </div>
</div>

<style>
  .contact-card {
    display: flex;
    flex-direction: column;
    gap: 8px;
    min-width: 220px;
    max-width: 300px;
    padding: 8px 4px 2px;
  }
  .head {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .avatar {
    flex: none;
    width: 40px;
    height: 40px;
    border-radius: 50%;
    display: grid;
    place-items: center;
    font-weight: 600;
    font-size: 1.05rem;
    background: color-mix(in srgb, currentColor 16%, transparent);
  }
  .who {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .name {
    font-weight: 600;
    overflow-wrap: anywhere;
  }
  .phone {
    font-size: 0.85em;
    opacity: 0.75;
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    padding-top: 6px;
    border-top: 1px solid color-mix(in srgb, currentColor 18%, transparent);
  }
  .actions button {
    flex: 1 1 auto;
    padding: 5px 8px;
    border: 0;
    border-radius: 6px;
    background: color-mix(in srgb, currentColor 12%, transparent);
    color: inherit;
    font: inherit;
    font-size: 0.85em;
    cursor: pointer;
  }
  .actions button:hover {
    background: color-mix(in srgb, currentColor 22%, transparent);
  }
</style>
