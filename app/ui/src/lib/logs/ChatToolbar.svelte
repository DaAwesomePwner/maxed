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
  .toolbar { display: flex; align-items: center; flex-wrap: wrap; gap: var(--space-2); min-width: 0; font-size: var(--text-caption); }
  .search { position: relative; flex: 1 1 180px; max-width: 280px; } input { width: 100%; padding-right: var(--space-6); }
  .search span { position: absolute; right: var(--space-3); top: 50%; transform: translateY(-50%); color: var(--text-muted); font-size: var(--text-meta); }
  small { margin-left: auto; color: var(--text-muted); font-size: var(--text-meta); font-variant-numeric: tabular-nums; }
  .ctl[aria-pressed="true"] { border-color: var(--text-secondary); color: var(--text-primary); }
  .seg a { display: inline-flex; align-items: center; gap: var(--space-1); padding: 0 var(--space-3); border-radius: calc(var(--radius-control) - 2px); color: var(--text-secondary); font-size: var(--text-caption); text-decoration: none;
    transition: background-color var(--motion-fast) var(--motion-ease), color var(--motion-fast) var(--motion-ease); }
  .seg a:hover:not(.on) { background: var(--surface-hover); color: var(--text-primary); }
  .seg a.on { background: var(--surface-raised); color: var(--text-primary); box-shadow: var(--highlight-top); }
  .seg a:focus-visible { outline: 2px solid var(--focus-ring); outline-offset: 2px; }
  .dot { width: 6px; height: 6px; border-radius: 2px; }
  /* Mirrors a native 16px checkbox: neutral when on, never gold. */
  .check { position: relative; box-sizing: border-box; width: 16px; height: 16px; flex: none; border: 1px solid var(--border-strong); border-radius: var(--radius-tag); background: var(--surface-inset); }
  [aria-pressed="true"] .check { border-color: var(--text-primary); background: var(--text-primary); }
  [aria-pressed="true"] .check::after { content: ''; position: absolute; left: 4px; top: 1px; width: 4px; height: 8px; border: solid var(--surface-canvas); border-width: 0 2px 2px 0; transform: rotate(45deg); }
</style>
