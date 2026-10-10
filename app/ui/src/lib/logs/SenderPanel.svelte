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
  .card { min-width: 0; padding: 10px 12px; background: var(--surface-panel); border: 1px solid var(--border); border-radius: 8px; font-size: 12px; }
  header { display: flex; justify-content: space-between; margin-bottom: 12px; } h3 { margin: 0; font-size: 11px; text-transform: uppercase; letter-spacing: .07em; color: var(--text-muted); }
  strong { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 16px; } p, small { color: var(--text-muted); } p { margin: 4px 0; }
  .who { display: flex; gap: 10px; align-items: center; padding: 8px; border: 1px solid var(--border); border-radius: 6px; background: var(--surface-raised); } .who > div { min-width: 0; } .avatar { display: grid; place-items: center; width: 30px; height: 30px; flex: none; color: var(--ch-guild); font-weight: 700; font-size: 22px; }
  blockquote { margin: 12px 0; padding: 8px 10px; border-left: 2px solid var(--violet); background: #ffffff04; overflow-wrap: anywhere; max-height: 200px; overflow-y: auto; } blockquote small { display: block; margin-top: 5px; }
  dl { display: grid; grid-template-columns: auto minmax(0, 1fr); gap: 7px; } dt { color: var(--text-muted); } dd { margin: 0; text-align: right; overflow-wrap: anywhere; }
  .buttons { display: flex; flex-wrap: wrap; gap: 6px; margin-top: 12px; } .buttons .ctl { max-width: 100%; overflow: hidden; text-overflow: ellipsis; } .danger { color: var(--danger); }
</style>
