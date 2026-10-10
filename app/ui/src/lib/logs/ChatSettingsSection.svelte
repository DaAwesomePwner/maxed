<script lang="ts">
  import { untrack } from 'svelte';
  import { validHiddenTerm } from './validation';
  import type { ChatSettings } from '../bindings/ChatSettings';
  let { settings, update, clear, editable }: {
    settings: ChatSettings; update: (next: ChatSettings) => Promise<void>; clear: () => Promise<void>; editable: boolean;
  } = $props();
  let draft = $state<ChatSettings>(untrack(() => structuredClone($state.snapshot(settings))));
  let message = $state(''), busy = $state(false), keyword = $state('');
  const channels = [['public', 'All'], ['guild', 'Guild'], ['party', 'Party'], ['whisper', 'Whisper']] as const;
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
    try { await update($state.snapshot(draft)); message = 'Saved'; } catch { message = "Couldn't save chat settings. Retry."; } finally { busy = false; }
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
  {#each channels as [key, label]}
    <label class="check"><input type="checkbox" aria-label={`Store ${label} chat`} bind:checked={draft.store[key]} /> {label}</label>
  {/each}
  <label>Keep chat for (days) <input type="number" min="1" max="3650" bind:value={draft.retention_days} /></label>
  <h3>Hidden senders</h3>
  <ul>{#each draft.hidden_senders as sender}<li>{sender} <button type="button" aria-label={`Unhide ${sender}`} onclick={() => { draft.hidden_senders = draft.hidden_senders.filter(s => s !== sender); }}>Unhide</button></li>{/each}</ul>
  <h3>Hidden keywords</h3>
  <ul>{#each draft.hidden_keywords as word}<li>{word} <button type="button" aria-label={`Remove ${word}`} onclick={() => { draft.hidden_keywords = draft.hidden_keywords.filter(k => k !== word); }}>Remove</button></li>{/each}</ul>
  <form onsubmit={(e) => { e.preventDefault(); addKeyword(); }}>
    <label>Add keyword <input bind:value={keyword} /></label><button type="submit">Add</button>
  </form>
  <button type="button" onclick={save}>Save chat settings</button>
  <button type="button" class="danger" onclick={clearHistory}>Clear chat history</button>
  <p role="status" aria-live="polite">{message}</p>
</fieldset>
