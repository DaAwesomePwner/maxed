<script lang="ts">
  import { tick } from 'svelte';
  import type { ChatRow } from '../bindings/ChatRow';
  import { placeGroups, copyText } from './chat-screen';
  import { channelTag, channelTagLabel, clockTime } from './logs-format';
  import { routeHref } from '../navigation/routes';
  import { chatLineLabel } from './presentation';
  import Badge from '../components/Badge.svelte';
  import '../styles/controls.css';
  import './glyphs.css';
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
          <td class:local={message.is_local}><strong title={chatLineLabel(message)}>{chatLineLabel(message)}</strong>{#if !message.is_local && message.stars != null}<small>★ {message.stars}</small>{/if}{#if message.is_local} <Badge label="you" tone="gold" />{/if}</td>
          <td class="message">{message.text}{message.truncated ? '…' : ''}{#if (counts[message.id] ?? 1) > 1} <small class="repeat" title="Consecutive identical messages from this sender">×{counts[message.id]}</small>{/if}</td>
          <td><div class="actions"><button class="ctl ghost" onclick={() => onOnly(message.sender)}>Only</button>{#if message.sender_kind === 'player' && !message.is_local}<button class="ctl ghost" aria-label={`Hide ${message.sender}`} onclick={() => onHide(message.sender)}>Hide</button>{/if}<button class="ctl ghost" onclick={() => copy(row)}>Copy</button></div></td>
        </tr>
      {/if}
    {/each}
  </tbody>
</table>
<style>
  table { width: 100%; table-layout: fixed; border-collapse: collapse; font-size: var(--text-caption); line-height: var(--leading-caption); font-variant-numeric: tabular-nums; }
  th { text-align: left; color: var(--text-muted); font-size: var(--text-meta); line-height: var(--leading-meta); font-weight: var(--weight-medium); padding: 0 var(--space-2) var(--space-2); border-bottom: 1px solid var(--hairline-strong); }
  tbody tr:first-child td { border-top: 0; }
  tr[data-row-id] { cursor: pointer; transition: background-color var(--motion-fast) var(--motion-ease); } tr[data-row-id]:hover { background: var(--surface-hover); }
  tr.selected, tr.selected:hover { background: var(--surface-selected); box-shadow: var(--selected-edge); }
  tr:focus-visible { outline: 2px solid var(--focus-ring); outline-offset: -2px; }
  .repeat { white-space: nowrap; color: var(--text-muted); }
  .time { width: 72px; } .channel { width: 88px; } .sender { width: 152px; } .actions-col { width: 136px; }
  td { height: var(--row-compact); box-sizing: border-box; border-top: 1px solid var(--hairline); padding: 0 var(--space-2); overflow: hidden; text-overflow: ellipsis; }
  td:first-child { color: var(--text-muted); }
  .hidden { opacity: .5; font-style: italic; }
  strong { display: inline-block; max-width: 100%; overflow: hidden; text-overflow: ellipsis; vertical-align: middle; white-space: nowrap; font-weight: var(--weight-medium); }
  small { color: var(--text-muted); font-size: var(--text-meta); margin-left: var(--space-1); }
  .local { color: var(--gold); } .message { overflow-wrap: anywhere; }
  td :global(.badge) { margin-left: var(--space-1); vertical-align: middle; } td :global(.ch) { white-space: nowrap; }
  .actions { display: flex; gap: 0; visibility: hidden; } .actions .ctl { padding: 0 var(--space-2); height: 24px; font-size: var(--text-meta); }
  tr:hover .actions, tr:focus-within .actions, .selected .actions { visibility: visible; }
  .group td { height: var(--row-compact); color: var(--text-muted); font-size: var(--text-meta); } .group div { display: flex; gap: var(--space-2); align-items: center; }
  .group .label { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; min-width: 0; } .group .rule { height: 1px; background: var(--hairline); flex: 1; min-width: 12px; }
  .group a { margin-left: auto; flex-shrink: 0; } .group a { color: var(--text-secondary); text-decoration: none; } .group a:hover, .group a:focus-visible { color: var(--text-primary); text-decoration: underline; text-underline-offset: 3px; }
  @media (max-width: 1100px) { .sender { width: 116px; } .actions-col { width: 128px; } }
</style>
