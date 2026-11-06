<script lang="ts">
  import { untrack } from 'svelte';
  import { validHiddenTerm } from './validation';
  import type { ChatSettings } from '../bindings/ChatSettings';
  let { settings, update, clear, editable }: {
    settings: ChatSettings; update: (next: ChatSettings) => Promise<void>; clear: () => Promise<void>; editable: boolean;
  } = $props();
  let draft = $state<ChatSettings>(untrack(() => structuredClone($state.snapshot(settings))));
  let message = $state(''), busy = $state(false), keyword = $state('');
  const channels = [['public', 'All', 'Public and unclassified messages.'], ['guild', 'Guild', 'Messages from your guild.'], ['party', 'Party', 'Messages from your party.'], ['whisper', 'Whisper', 'Private conversations.']] as const;
  const id = $props.id();
  function addKeyword() {
    const word = keyword.trim();
    if (!validHiddenTerm(word, 64) || /\p{Cc}/u.test(keyword)) {
      message = 'Use 1–64 UTF-8 bytes with no control characters.'; return;
    }
    if (draft.hidden_keywords.includes(word)) { message = 'Keyword already hidden.'; return; }
    if (draft.hidden_keywords.length >= 128) { message = 'You can hide up to 128 keywords.'; return; }
    draft.hidden_keywords = [...draft.hidden_keywords, word]; keyword = ''; message = '';
  }
  async function save() {
    if (!editable || busy) return;
    if (!Number.isInteger(draft.retention_days) || draft.retention_days < 1 || draft.retention_days > 3650) { message = 'Choose 1 to 3650 days.'; return; }
    busy = true; message = '';
    try { await update($state.snapshot(draft)); message = 'Chat settings saved'; } catch { message = "Couldn't save chat settings. Retry."; } finally { busy = false; }
  }
  async function clearHistory() {
    if (!editable || busy) return;
    if (!window.confirm('Delete all stored chat and chat alerts? This cannot be undone.')) return;
    busy = true; message = '';
    try { await clear(); message = 'Chat history cleared'; } catch { message = "Couldn't clear chat history. Retry."; } finally { busy = false; }
  }
</script>

<fieldset disabled={!editable || busy}>
  <legend>Store chat</legend>
  {#each channels as [key, label, hint]}
    <div class="channel-choice">
      <label class="check"><input type="checkbox" aria-label={`Store ${label} chat`} aria-describedby={`${id}-${key}`} bind:checked={draft.store[key]} /> {label}</label>
      <p id={`${id}-${key}`}>{hint}</p>
    </div>
  {/each}
  <label>Keep chat for (days) <input class="ctl" type="number" min="1" max="3650" bind:value={draft.retention_days} /></label>
  <div class="channel-choice">
    <label class="check"><input type="checkbox" bind:checked={draft.filter_spam} aria-describedby={`${id}-spam`} /> Filter spam ads and repeated public messages</label>
    <p id={`${id}-spam`}>Hide matching messages in chat views.</p>
  </div>
  <h3>Hidden senders</h3>
  {#if !draft.hidden_senders.length}<p>No hidden senders.</p>{/if}
  <ul>{#each draft.hidden_senders as sender}<li>{sender} <button class="ctl" type="button" aria-label={`Unhide ${sender}`} onclick={() => { draft.hidden_senders = draft.hidden_senders.filter(s => s !== sender); }}>Unhide</button></li>{/each}</ul>
  <h3>Hidden keywords</h3>
  {#if !draft.hidden_keywords.length}<p>No hidden keywords.</p>{/if}
  <ul>{#each draft.hidden_keywords as word}<li>{word} <button class="ctl" type="button" aria-label={`Remove ${word}`} onclick={() => { draft.hidden_keywords = draft.hidden_keywords.filter(k => k !== word); }}>Remove</button></li>{/each}</ul>
  <form onsubmit={(e) => { e.preventDefault(); addKeyword(); }}>
    <label>Add keyword <input class="ctl" bind:value={keyword} /></label><button class="ctl" type="submit">Add</button>
  </form>
  <div class="actions"><button class="ctl" type="button" onclick={save}>Save chat settings</button>
  <button class="ctl danger" type="button" onclick={clearHistory}>Clear chat history</button></div>
  <p role="status" aria-live="polite">{message}</p>
</fieldset>

<style>
  fieldset { border: 0; padding: 0; margin: 0; min-width: 0; display: grid; gap: 8px; }
  legend { padding: 0; margin-bottom: 8px; font-weight: 600; }
  .channel-choice { padding-bottom: 8px; border-bottom: 1px solid var(--border); }
  .check { display: flex; align-items: start; gap: 8px; line-height: 20px; }
  input[type='checkbox'] { flex: none; width: 16px; height: 16px; margin: 2px 0 0; accent-color: var(--violet); }
  .channel-choice p { margin: 2px 0 0 24px; }
  p { margin: 0; color: var(--text-secondary); font-size: 12px; }
  h3 { margin: 8px 0 0; font-size: 12px; font-weight: 600; }
  label:not(.check) { display: grid; gap: 4px; }
  input[type='number'] { width: 100px; }
  input:not([type='checkbox']) { min-width: 0; max-width: 100%; box-sizing: border-box; }
  ul { list-style: none; padding: 0; margin: 0; display: grid; gap: 6px; }
  li { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; }
  form, .actions { display: flex; flex-wrap: wrap; align-items: end; gap: 8px; }
  form label { flex: 1; min-width: 0; }
  .danger { color: var(--danger); }
  [role='status']:empty { display: none; }
</style>
