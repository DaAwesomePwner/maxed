<script lang="ts">
  import type { Client } from '../bridge/client';
  import type { InstanceEventView } from '../bindings/InstanceEventView';
  import { eventParts, eventLabels, eventGlyphs } from './events-screen';
  import { clockTime, dayLabel } from './logs-format';
  import EventPortal from './EventPortal.svelte';
  import Badge from '../components/Badge.svelte';
  import './glyphs.css';
  import type { EnteredRun } from '../bindings/EnteredRun';
  import { routeHref } from '../navigation/routes';
  import { titleCase } from '../styles/format';
  let { rows, selectedId, onSelect, client, className = () => null, mode = 'events', enteredRuns = [] }: {
    rows: InstanceEventView[]; selectedId: string | null; onSelect: (id: string) => void;
    mode?: 'events' | 'keys'; enteredRuns?: EnteredRun[];
    client?: Client; className?: (id: number) => string | null;
  } = $props();
  function select(event: MouseEvent | KeyboardEvent, id: string) {
    if (event instanceof KeyboardEvent && event.key !== 'Enter' && event.key !== ' ') return;
    if (event.target instanceof Element && event.target.closest('a')) {
      if (mode === 'keys') onSelect(id);
      return;
    }
    event.preventDefault(); onSelect(id);
    if (mode === 'keys' && event.currentTarget instanceof HTMLElement) event.currentTarget.querySelector<HTMLAnchorElement>('a')?.click();
  }
</script>
<table aria-label={mode === 'keys' ? 'Key pops' : 'Events'}>
  {#if mode === 'keys'}<colgroup><col class="time" /><col /><col class="opener" /><col class="where" /><col class="run" /></colgroup>
  <thead><tr><th>Time</th><th>Dungeon</th><th>Opened by</th><th>Where</th><th>Your run</th></tr></thead>{:else}
  <colgroup><col class="time" /><col class="kind" /><col /><col class="where" /></colgroup>
  <thead><tr><th>Time</th><th>Kind</th><th>What happened</th><th>Where</th></tr></thead>
  {/if}
  <tbody>
    {#each rows as row, index (row.id)}
      {@const parts = eventParts(row, className)}
      {#if index === 0 || dayLabel(row.at, new Date()) !== dayLabel(rows[index - 1].at, new Date())}<tr class="group"><td colspan={mode === 'keys' ? 5 : 4}>{dayLabel(row.at, new Date())}</td></tr>{/if}
      <tr data-row-id={row.id} title={row.provisional ? 'Unconfirmed: seen without a confirming observation.' : 'Confirmed observation'} tabindex="0" aria-selected={selectedId === row.id} class:selected={selectedId === row.id} onclick={event => select(event, row.id)} onkeydown={event => select(event, row.id)}>
        <td>{clockTime(row.at)}</td>
        {#if mode === 'keys'}
          {@const portal = row.kind.type === 'portal_opened' ? row.kind : null}
          {@const run = enteredRuns.find(run => run.event_id === row.id)}
          <td title={portal?.dungeon ?? '—'}><div class="description"><span class="k portal-name"><EventPortal {client} dungeon={portal?.dungeon ?? null} /><strong>{portal?.dungeon ?? '—'}</strong></span>{#if row.provisional}<span class="sr-only">Unconfirmed: seen without a confirming observation.</span>{:else}<span class="confirmed">confirmed</span>{/if}</div></td>
          <td title={portal?.opener ?? '—'} class:local={portal?.opener_is_local === true}><div class="description"><strong class="summary">{portal?.opener ?? '—'}</strong>{#if portal?.opener_is_local === true}<Badge label="you" tone="gold" />{/if}</div></td>
          <td title={row.map ?? '—'}>{row.map ?? '—'}</td>
          <td>{#if run}<a href={routeHref({ kind: 'run', id: run.run_id })} title={`A ${run.dungeon} run started ${Math.round(Number(BigInt(run.started_at) - BigInt(row.at)) / 1_000_000).toLocaleString('en-US')} s after this portal opened`}>{run.dungeon} · {run.outcome ? titleCase(run.outcome) : '—'} ›</a>{/if}</td>
        {:else}
        <td><span class="k {parts.kind}">{#if row.kind.type === 'portal_opened'}<EventPortal {client} dungeon={row.kind.dungeon ?? null} />{:else}<i class="g {eventGlyphs[parts.kind]}" aria-hidden="true"></i>{/if}<span>{eventLabels[parts.kind]}</span></span></td>
        <td title={[parts.who, parts.detail].filter(Boolean).join(' · ')}><div class="description"><span class="summary"><strong title={parts.who ?? '—'}>{parts.who ?? '—'}</strong> <span class="detail">{parts.detail}</span></span>{#if row.provisional}<span class="sr-only">Unconfirmed: seen without a confirming observation.</span>{:else}<span class="confirmed">confirmed</span>{/if}</div></td>
        <td title={parts.where}>{parts.where}</td>
        {/if}
      </tr>
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
  .time { width: 72px; } .kind { width: 120px; } .where { width: 22%; } .opener { width: 20%; } .run { width: 22%; }
  td { height: var(--row-default); box-sizing: border-box; border-top: 1px solid var(--hairline); padding: 0 var(--space-2); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  td:first-child, td:last-child, .detail { color: var(--text-muted); }
  strong { font-weight: var(--weight-medium); }
  .description { display: flex; align-items: center; gap: var(--space-2); min-width: 0; } .summary { overflow: hidden; text-overflow: ellipsis; }
  .confirmed { flex: none; font-size: var(--text-meta); color: var(--success); }
  .group td { color: var(--text-muted); font-size: var(--text-meta); font-weight: var(--weight-medium); }
  .local { color: var(--gold); }
  td a { color: var(--text-secondary); text-decoration: none; } td a:hover, td a:focus-visible { color: var(--text-primary); text-decoration: underline; text-underline-offset: 3px; }
  .k strong { overflow: hidden; text-overflow: ellipsis; } .portal-name { min-width: 0; overflow: hidden; }
</style>
