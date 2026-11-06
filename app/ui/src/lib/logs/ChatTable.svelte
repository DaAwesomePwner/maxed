<script lang="ts">
  import { tick } from 'svelte';
  import type { ChatRow } from '../bindings/ChatRow';
  import { placeGroups, copyText } from './chat-screen';
  import { channelTag, channelTagLabel, clockTime } from './logs-format';
  import { routeHref } from '../navigation/routes';
  import { chatLineLabel } from './presentation';
  import '../styles/controls.css';
  let { rows, counts = {}, selectedId, onSelect, onOnly, onHide }: { rows: ChatRow[]; counts?: Record<string, number>; selectedId: string | null; onSelect: (id: string) => void; onOnly: (sender: string) => void; onHide: (sender: string) => void } = $props();
  let copied = $state('');
  let items = $derived(placeGroups(rows, new Date()));
  function select(event: MouseEvent | KeyboardEvent, row: ChatRow) {
    if (event.target instanceof Element && event.target.closest('button, a')) return;
    if (event instanceof KeyboardEvent && event.key !== 'Enter' && event.key !== ' ') return;
    event.preventDefault(); onSelect(row.message.id);
  }
  async function copy(row: ChatRow) {
    if (!navigator.clipboard?.writeText) return;
    try { await navigator.clipboard.writeText(copyText(row)); copied = ''; await tick(); copied = 'Copied'; } catch { /* Clipboard may be denied by the host. */ }
  }
</script>
<p class="sr-only" role="status">{copied}</p>
<table aria-label="Chat lines">
  <colgroup><col class="time" /><col class="channel" /><col class="sender" /><col /><col class="actions-col" /></colgroup>
  <thead><tr><th>Time</th><th>Channel</th><th>Sender</th><th>Message</th><th><span class="sr-only">Actions</span></th></tr></thead>
  <tbody>
    {#each items as item (item.type === 'place' ? `place:${item.key}` : item.row.message.id)}
      {#if item.type === 'place'}
        <tr class="group"><td colspan="5"><div>{#if item.runId}<i class="g portal" aria-hidden="true"></i>{/if}<span class="label" title={item.label}>{item.label} · {item.count} {item.count === 1 ? 'line' : 'lines'}</span><span class="rule" aria-hidden="true"></span>{#if item.runId}<a href={routeHref({ kind: 'run', id: item.runId })}>Open run ›</a>{/if}</div></td></tr>
      {:else}
        {@const row = item.row}{@const message = row.message}{@const tag = channelTag(message)}
        <tr data-row-id={message.id} tabindex="0" aria-selected={selectedId === message.id} class:selected={selectedId === message.id} class:hidden={row.hidden} onclick={event => select(event, row)} onkeydown={event => select(event, row)}>
          <td>{clockTime(message.at)}</td><td><span class="ch {tag}">{channelTagLabel[tag]}</span></td>
          <td class:local={message.is_local}><strong title={chatLineLabel(message)}>{chatLineLabel(message)}</strong>{#if !message.is_local && message.stars != null}<small>★ {message.stars}</small>{/if}{#if message.is_local}<span class="you">you</span>{/if}</td>
          <td class="message">{message.text}{message.truncated ? '…' : ''}{#if (counts[message.id] ?? 1) > 1} <small class="repeat" title="Consecutive identical messages from this sender">×{counts[message.id]}</small>{/if}</td>
          <td><div class="actions"><button class="ctl ghost" onclick={() => onOnly(message.sender)}>Only</button>{#if message.sender_kind === 'player' && !message.is_local}<button class="ctl ghost" aria-label={`Hide ${message.sender}`} onclick={() => onHide(message.sender)}>Hide</button>{/if}<button class="ctl ghost" onclick={() => copy(row)}>Copy</button></div></td>
        </tr>
      {/if}
    {/each}
  </tbody>
</table>
<style>
  .repeat { white-space: nowrap; color: var(--text-muted); }
  table { width: 100%; table-layout: fixed; border-collapse: collapse; font-size: 12px; font-variant-numeric: tabular-nums; }
  .time { width: 68px; } .channel { width: 70px; } .sender { width: 150px; } .actions-col { width: 136px; }
  th { text-align: left; color: var(--text-muted); font-size: 11px; font-weight: 500; padding: 0 6px 6px; }
  td { height: 26px; border-top: 1px solid var(--border); padding: 2px 6px; overflow: hidden; text-overflow: ellipsis; }
  tr[data-row-id] { cursor: pointer; } tr[data-row-id]:hover { background: #ffffff05; } tr.selected { background: #f2c14e14; box-shadow: inset 2px 0 var(--gold); }
  tr:focus-visible { outline: 2px solid var(--focus-ring); outline-offset: -2px; } .hidden { opacity: .5; font-style: italic; }
  strong { display: inline-block; max-width: 100%; overflow: hidden; text-overflow: ellipsis; vertical-align: middle; white-space: nowrap; } small { color: var(--text-muted); font-size: 10px; margin-left: 4px; }
  .local, .you { color: var(--gold); } .you { margin-left: 4px; font-size: 10px; } .message { overflow-wrap: anywhere; }
  .ch { display: inline-flex; align-items: center; font-size: 10px; gap: 4px; color: var(--text-muted); }
  .ch::before { content: ''; width: 6px; height: 6px; border-radius: 2px; background: currentColor; flex: none; }
  .ch.public { color: var(--ch-public); } .ch.guild { color: var(--ch-guild); } .ch.party { color: var(--ch-party); } .ch.whisper { color: var(--ch-whisper); } .ch.npc { color: var(--ch-npc); }
  .actions { display: flex; gap: 0; visibility: hidden; } .actions .ctl { padding: 0 5px; height: 22px; font-size: 11px; }
  tr:hover .actions, tr:focus-within .actions, .selected .actions { visibility: visible; }
  .group td { height: 30px; color: var(--text-muted); font-size: 11px; background: #ffffff03; } .group div { display: flex; gap: 8px; align-items: center; } .group .label { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; min-width: 0; } .group .rule { height: 1px; background: var(--border); flex: 1; min-width: 12px; } .group a { margin-left: auto; flex-shrink: 0; color: var(--violet-text); text-decoration: none; }
  /* The same placeholder portal as glyphs.css, scoped to this table. */
  .group .g { display: inline-block; flex: none; width: 12px; height: 12px; background: #c4a7ff; image-rendering: pixelated; mask: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 8 8' shape-rendering='crispEdges'%3E%3Cpath d='M2 0h4v1h1v1h1v4H7v1H6v1H2V7H1V6H0V2h1V1h1zM3 2v1H2v2h1v1h2V5h1V3H5V2z'/%3E%3C/svg%3E") center / contain no-repeat; }
  @media (max-width: 1100px) { .sender { width: 115px; } .actions-col { width: 126px; } }
</style>
