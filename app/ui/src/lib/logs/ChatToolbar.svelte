<script lang="ts">
  import type { ChatFilter } from '../bindings/ChatFilter';
  import type { ChatRow } from '../bindings/ChatRow';
  import type { WhisperPartner } from '../bindings/WhisperPartner';
  import { routeHref, type ChatTab } from '../navigation/routes';
  import { clockTime } from './logs-format';
  import SenderPicker from './SenderPicker.svelte';
  import '../styles/controls.css';
  let { whisper = false, channel, runId, filter, rows, partners, hiddenCount, latestAt, onApply, onOpen }: { whisper?: boolean; channel: ChatTab; runId: string | null; filter: ChatFilter; rows: ChatRow[]; partners: WhisperPartner[]; hiddenCount: number; latestAt: string | null; onApply: (filter: ChatFilter) => void; onOpen: () => void } = $props();
  let query = $state(''), search: HTMLInputElement;
  let appliedQuery: string | undefined;
  $effect(() => { if (filter.query !== appliedQuery) { appliedQuery = filter.query; query = filter.query; } });
  $effect(() => {
    const value = query;
    if (value === filter.query) return;
    const timer = setTimeout(() => onApply({ ...filter, query: value, cursor: null }), 250);
    return () => clearTimeout(timer);
  });
  function shortcut(event: KeyboardEvent) {
    if (event.key !== '/' || event.ctrlKey || event.metaKey || event.altKey || event.target instanceof Element && event.target.closest('input, textarea, select, [contenteditable="true"]')) return;
    event.preventDefault(); search?.focus();
  }
</script>
<svelte:window onkeydown={shortcut} />
<div class="toolbar">
  <nav class="seg" aria-label="Chat channels">
    {#each ['all', 'guild', 'party', 'whisper'] as ch}<a class:on={channel === ch} aria-current={channel === ch ? 'page' : undefined} href={routeHref({ kind: 'logs', view: 'chat', channel: ch as ChatTab, runId: runId ?? undefined })}>{#if ch !== 'all'}<i class="dot" style={`background: var(--ch-${ch})`} aria-hidden="true"></i>{/if}{ch === 'all' ? 'All' : ch[0].toUpperCase() + ch.slice(1)}</a>{/each}
  </nav>
  <div class="search"><input class="ctl" aria-label="Search" type="search" placeholder={whisper ? "Search whispers" : "Search messages"} bind:this={search} bind:value={query} /><span aria-hidden="true">/</span></div>
  {#if !whisper}<SenderPicker value={filter.sender} {rows} {partners} {onOpen} onChange={sender => onApply({ ...filter, sender, cursor: null })} />
  <button class="ctl" aria-pressed={filter.show_npc} onclick={() => onApply({ ...filter, show_npc: !filter.show_npc, cursor: null })}><span class="check" aria-hidden="true"></span>NPC lines</button>{/if}
  <button class="ctl" aria-pressed={filter.show_hidden} onclick={() => onApply({ ...filter, show_hidden: !filter.show_hidden, cursor: null })}><span class="check" aria-hidden="true"></span>Hidden{filter.show_hidden ? ` ${hiddenCount}` : ''}</button>
  {#if latestAt}<small>Latest line {clockTime(latestAt)}</small>{/if}
</div>
<style>
  .toolbar { display: flex; align-items: center; flex-wrap: wrap; gap: 6px; min-width: 0; font-size: 12px; }
  .search { position: relative; flex: 1 1 180px; max-width: 280px; } input { width: 100%; padding-right: 24px; } .search span { position: absolute; right: 9px; top: 6px; color: var(--text-muted); }
  small { margin-left: auto; color: var(--text-muted); } [aria-pressed="true"] { color: var(--gold); border-color: var(--gold); }
  .seg a { display: inline-flex; align-items: center; padding: 0 9px; border-radius: 4px; color: var(--text-secondary); text-decoration: none; }
  .seg a.on { background: var(--surface-hover); color: var(--text-primary); box-shadow: inset 0 0 0 1px #c4a7ff55; }
  .seg a:focus-visible { outline: 2px solid var(--focus-ring); outline-offset: 3px; }
  .dot { width: 6px; height: 6px; border-radius: 2px; margin-right: 5px; } .check { width: 11px; height: 11px; border: 1px solid var(--text-muted); border-radius: 3px; } [aria-pressed="true"] .check { background: var(--gold); border-color: var(--gold); }
</style>
