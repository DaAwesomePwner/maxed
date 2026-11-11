<script lang="ts">
  import type { ChatRow } from '../bindings/ChatRow';
  import type { ChatSenderSummary } from '../bindings/ChatSenderSummary';
  import { channelTag, channelTagLabel, clockTime } from './logs-format';
  import { runDate } from '../runs/runs-list';
  import { routeHref } from '../navigation/routes';
  import '../styles/controls.css';
  let { row, summary, onOnly, onHide }: { row: ChatRow | null; summary: ChatSenderSummary | null; onOnly: (sender: string) => void; onHide: (sender: string) => void } = $props();
  let matchedSummary = $derived(summary && row && summary.name.toLowerCase() === row.message.sender.toLowerCase() ? summary : null);
</script>
<section class="card" aria-label="Sender details">
  <header><h3>Sender</h3>{#if row}<small>{clockTime(row.message.at)}</small>{/if}</header>
  {#if row}
    <div class="who"><span class="avatar" aria-hidden="true">{row.message.sender.slice(0, 1)}</span><div><strong title={row.message.sender}>{row.message.sender}</strong>
    <p>★ {matchedSummary?.stars ?? row.message.stars ?? '—'}{matchedSummary?.channels.includes('guild') ? ' · guildmate' : ''}{matchedSummary?.supporter ? ' · supporter' : ''}</p></div></div>
    <blockquote>{row.message.text}{row.message.truncated ? '…' : ''}<small>{channelTagLabel[channelTag(row.message)]} · {row.message.map ?? '—'}</small></blockquote>
    <dl><dt>Lines stored</dt><dd>{matchedSummary?.lines.toLocaleString('en-US') ?? '—'}</dd><dt>First seen</dt><dd>{matchedSummary ? runDate(matchedSummary.first_at).toLocaleDateString('en-US', { month: 'short', day: 'numeric' }) : '—'}</dd><dt>Channels</dt><dd>{matchedSummary?.channels.map(ch => channelTagLabel[ch]).join(' · ') ?? '—'}</dd></dl>
    <div class="buttons"><button class="ctl" title={`Only ${row.message.sender}`} onclick={() => onOnly(row.message.sender)}>Only {row.message.sender}</button>{#if row.message.sender_kind === 'player' && !row.message.is_local}<a class="ctl" title={`Whispers with ${row.message.sender}`} href={routeHref({ kind: 'logs', view: 'chat', channel: 'whisper', partner: row.message.sender })}>Whispers with {row.message.sender}</a>{/if}{#if row.message.sender_kind === 'player' && !row.message.is_local}<button class="ctl danger" onclick={() => onHide(row.message.sender)}>Hide sender</button>{/if}</div>
  {:else}<p>Select a line to see its sender.</p>{/if}
</section>
<style>
  .card { min-width: 0; padding: var(--space-4); border: 1px solid var(--hairline); border-radius: var(--radius-card); background: var(--surface-panel); font-size: var(--text-caption); line-height: var(--leading-caption); }
  header { display: flex; align-items: baseline; justify-content: space-between; gap: var(--space-2); margin-bottom: var(--space-3); } h3 { margin: 0; font-size: var(--text-label); line-height: var(--leading-label); font-weight: var(--weight-medium); }
  strong { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: var(--text-body); line-height: var(--leading-body); font-weight: var(--weight-medium); }
  p, small { color: var(--text-muted); } small { font-size: var(--text-meta); } p { margin: 0; }
  .who { display: flex; gap: var(--space-3); align-items: center; padding: var(--space-2); border: 1px solid var(--hairline); border-radius: var(--radius-control); background: var(--surface-raised); } .who > div { min-width: 0; }
  .avatar { display: grid; place-items: center; width: 32px; height: 32px; flex: none; border-radius: var(--radius-sprite); background: var(--surface-panel); color: var(--text-secondary); font-weight: var(--weight-medium); font-size: var(--text-label); }
  blockquote { margin: var(--space-3) 0; padding: var(--space-2) var(--space-3); border-left: 2px solid var(--hairline-strong); background: var(--surface-inset); overflow-wrap: anywhere; max-height: 200px; overflow-y: auto; } blockquote small { display: block; margin-top: var(--space-1); }
  dl { display: grid; grid-template-columns: auto minmax(0, 1fr); gap: var(--space-2); margin: 0; } dt { color: var(--text-muted); } dd { margin: 0; text-align: right; overflow-wrap: anywhere; }
  .buttons { display: flex; flex-wrap: wrap; gap: var(--space-2); margin-top: var(--space-3); } .buttons .ctl { max-width: 100%; overflow: hidden; text-overflow: ellipsis; } .danger { color: var(--danger); }
</style>
