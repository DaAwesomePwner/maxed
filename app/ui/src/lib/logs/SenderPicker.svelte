<script lang="ts">
  import { tick } from 'svelte';
  import type { ChatRow } from '../bindings/ChatRow';
  import type { WhisperPartner } from '../bindings/WhisperPartner';
  import { senderSuggestions } from './chat-screen';
  import { channelTagLabel } from './logs-format';
  import '../styles/controls.css';
  let { value, rows, partners, onChange, onOpen }: { value: string | null; rows: ChatRow[]; partners: WhisperPartner[]; onChange: (value: string | null) => void; onOpen: () => void } = $props();
  let open = $state(false), text = $state(''), active = $state(-1);
  let input = $state<HTMLInputElement>(), trigger: HTMLButtonElement;
  const id = $props.id();
  let suggestions = $derived(senderSuggestions(rows, partners, text));
  $effect(() => { if (open) void tick().then(() => input?.focus()); });
  function close() { open = false; trigger?.focus(); }
  function choose(name: string | null) { onChange(name); close(); }
</script>
<div class="picker">
  <button class="ctl" bind:this={trigger} title={value ?? 'Any sender'} aria-expanded={open} onclick={() => { open = !open; text = ''; active = -1; if (open) onOpen(); }}>{value ?? 'Any sender'} <span aria-hidden="true">▾</span></button>
  {#if open}
    <div class="menu">
      <input class="ctl" bind:this={input} aria-label="Sender" role="combobox" aria-expanded="true" aria-controls={`${id}-options`} aria-autocomplete="list" aria-activedescendant={active >= 0 ? `${id}-${active}` : undefined} bind:value={text} oninput={() => active = -1}
        onkeydown={event => {
          if (event.key === 'Escape') close();
          if (event.key === 'ArrowDown') { event.preventDefault(); active = Math.min(active + 1, suggestions.length - 1); }
          if (event.key === 'ArrowUp') { event.preventDefault(); active = Math.max(0, active - 1); }
          if (event.key === 'Enter') { event.preventDefault(); choose(active >= 0 ? suggestions[active].name : text || null); }
        }} />
      <button class="ctl ghost" onclick={() => choose(null)}>Any sender</button>
      <div id={`${id}-options`} role="listbox" aria-label="Senders">
        {#each suggestions as item, i}<button id={`${id}-${i}`} class="ctl ghost option" role="option" aria-selected={active === i} title={item.name} onclick={() => choose(item.name)}><span>{item.name}</span><small>{channelTagLabel[item.channel]}</small></button>{/each}
      </div>
      <p>Type an exact name and press Enter.</p>
    </div>
  {/if}
</div>
<style>
  .picker { position: relative; min-width: 0; } .picker > button { max-width: 180px; overflow: hidden; text-overflow: ellipsis; }
  .menu { position: absolute; top: calc(var(--control-dense) + var(--space-1)); left: 0; width: 260px; max-width: 70vw; padding: var(--space-2); z-index: 5; display: grid; gap: var(--space-1); background: var(--surface-raised); border: 1px solid var(--hairline-strong); border-radius: var(--radius-control); box-shadow: var(--elevation-popover); }
  input { width: 100%; } .option { display: flex; width: 100%; justify-content: space-between; } .option span { overflow: hidden; text-overflow: ellipsis; } .option[aria-selected="true"] { background: var(--surface-hover); color: var(--text-primary); }
  small, p { color: var(--text-muted); } small { font-size: var(--text-meta); } p { font-size: var(--text-meta); line-height: var(--leading-meta); margin: var(--space-1) 0 0; }
</style>
