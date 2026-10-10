<script lang="ts">
  import type { AlertView } from '../bindings/AlertView';
  import { alertHeading, alertAction, kindLabel, shownAs, whyItFired } from './alerts-screen';
  import { alertTime } from './filters';
  import { clockTime, dayLabel } from '../logs/logs-format';
  import { runDate } from '../runs/runs-list';
  import DebugNote from '../debug/DebugNote.svelte';
  import ObjectLabel from '../home/ObjectLabel.svelte';
  import { emptyHomeVisuals, type HomeVisuals } from '../home/visuals';
  import { formatDps, formatElapsed } from '../styles/format';
  let { alert, visuals = emptyHomeVisuals() }: { alert: AlertView | null; visuals?: HomeVisuals } = $props();
  const weekDate = (at: string) => runDate(at).toLocaleDateString('en-US', { month: 'short', day: 'numeric' });
</script>
{#if alert}
  {@const detail = alert.details}
  {@const action = alertAction(alert)}
  <section aria-label="Alert details"><h2>{kindLabel[alert.kind]}</h2><p class="time" title={alertTime(alert.at)}>{dayLabel(alert.at, new Date())} · {clockTime(alert.at)}</p>{#if alertHeading(alert) !== kindLabel[alert.kind]}<strong>{alertHeading(alert)}</strong>{/if}
    <dl><dt>Shown as</dt><dd>{shownAs(alert.presentation)}</dd>
      {#if detail?.type === 'white_bag'}
        <dt>Bag</dt><dd><ObjectLabel type={detail.bag_type} kind="bag" {visuals} /></dd>
        <dt>Source</dt><dd>{#if detail.source_enemy !== null}<ObjectLabel type={detail.source_enemy} kind="enemy" {visuals} />{:else}—{/if} {#if detail.source_estimated}<span class="est">est.</span>{/if}</dd>
        <dt>Weekly count</dt><dd>{detail.ordinal ?? '—'}</dd><dt>Week</dt><dd title={detail.window ? `${alertTime(detail.window.start)} – ${alertTime(detail.window.end)}` : undefined}>{detail.window ? `${weekDate(detail.window.start)} – ${weekDate(String(BigInt(detail.window.end) - 1n))}` : '—'}</dd>
        <dt>Contents</dt><dd>{detail.contents_known ? 'Seen' : 'Unseen'}</dd>
      {:else if detail?.type === 'personal_best'}
        <dt>Boss</dt><dd><ObjectLabel type={detail.boss_type} kind="enemy" {visuals} /></dd><dt>DPS</dt><dd>{formatDps(detail.dps)}</dd><dt>Previous DPS</dt><dd>{formatDps(detail.previous_dps)}</dd>
      {:else if detail?.type === 'run_time_personal_best'}
        <dt>Dungeon</dt><dd>{detail.dungeon}</dd><dt>Clear time</dt><dd>{formatElapsed('0', String(detail.duration_micros))}</dd><dt>Previous time</dt><dd>{formatElapsed('0', String(detail.previous_micros))}</dd>
      {:else if detail?.type === 'key_popped'}<dt>Dungeon</dt><dd>{detail.dungeon}</dd>
      {:else if detail?.type === 'chat'}<dt>Sender</dt><dd title={detail.sender}>{detail.sender}</dd><dt>Message</dt><dd>{detail.excerpt}</dd>
      {:else if detail?.type === 'death'}<dt>Character</dt><dd>{detail.character_id ?? '—'}</dd>
      {:else}<dt>Details</dt><dd>—</dd>{/if}
    </dl>
    <div class="actions">{#if action?.href}<a class="ctl ghost" href={action.href}>{action.label} ›</a>{/if}</div>
  </section>
  <section aria-label="Why it fired"><h2>Why it fired</h2><p>{whyItFired(alert)}</p><DebugNote text={alert.explanation} /></section>
{:else}<section><h2>Alert details</h2><p>Select an alert to see its details.</p></section>{/if}
<style>
  section { padding: 12px; border: 1px solid var(--border); border-radius: 8px; background: var(--surface-panel); min-width: 0; overflow-wrap: anywhere; max-height: 550px; overflow: auto; } h2 { font-size: 13px; margin: 0 0 8px; } p, dl { font-size: 12px; color: var(--text-secondary); } .time { font-size: 11px; color: var(--text-muted); } strong { font-size: 14px; } dl { display: grid; grid-template-columns: 85px minmax(0, 1fr); gap: 7px; } dd { margin: 0; } dt { color: var(--text-muted); } .est { color: var(--estimated); font-size: 10px; } .actions { display: flex; flex-wrap: wrap; gap: 6px; }
</style>
