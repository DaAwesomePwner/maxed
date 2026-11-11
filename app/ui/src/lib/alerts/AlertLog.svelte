<script lang="ts">
  import MuteDungeon from './MuteDungeon.svelte';
  import { formatDps } from '../styles/format';
  import { alertTitle } from './presentation';
  import DebugNote from '../debug/DebugNote.svelte';
  import type { AlertView } from '../bindings/AlertView';
  import { alertDestination } from './destination';
  import { alertKindLabels, alertTime } from './filters';
  import { loudnessLabels } from './preferences';
  import { emptyHomeVisuals, type HomeVisuals } from '../home/visuals';
  import ObjectLabel from '../home/ObjectLabel.svelte';
  let { rows, visuals = emptyHomeVisuals() }: { rows: AlertView[]; visuals?: HomeVisuals } = $props();
  function shortTime(at: string) {
    try {
      const date = new Date(Number(BigInt(at) / 1000n));
      return Number.isFinite(date.getTime()) ? date.toLocaleString(undefined, { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' }) : '—';
    } catch { return '—'; }
  }
</script>

<ul aria-label="Saved Alerts">
  {#each rows as row (row.id)}
    {@const destination = alertDestination(row)}
    {@const detail = row.details}
    <li>
      <article data-row-id={row.id} tabindex="-1" aria-label={alertKindLabels[row.kind]}>
        <div class="row-actions">
        <details>
          <summary aria-label={`${alertKindLabels[row.kind]} details`}><strong>{alertKindLabels[row.kind]}</strong> <span class="number" title={alertTime(row.at)}>{shortTime(row.at)}</span></summary>
          <p>Time: <span class="number">{alertTime(row.at)}</span></p>
          <p>Presentation: {row.presentation ? loudnessLabels[row.presentation] : '—'}</p>
          {#if !detail}<DebugNote text="Legacy details unavailable. The saved explanation is retained." />
          {:else if detail.type === 'run_time_personal_best'}
            <p>{alertTitle(row)}</p>
          {:else if detail.type === 'personal_best'}
            <p><ObjectLabel type={detail.boss_type} kind="enemy" {visuals} /></p>
            <p>DPS: <span class="number">{formatDps(detail.dps)}</span> · Previous DPS: <span class="number">{formatDps(detail.previous_dps)}</span></p>
            {#if !detail.character_id}<DebugNote text="Character identity unavailable." />{/if}
          {:else if detail.type === 'white_bag'}
            <p><ObjectLabel type={detail.bag_type} kind="bag" {visuals} /></p>
            {#if detail.ordinal !== null}<p>Weekly count: {detail.ordinal}</p>{:else}<DebugNote text="Ordinal unavailable" />{/if}
            {#if detail.window}<p>Week: {alertTime(detail.window.start)} – {alertTime(detail.window.end)}</p>
            {:else}<DebugNote text="Calendar window unavailable." />{/if}
            {#if detail.source_enemy !== null}<p>Source: <ObjectLabel type={detail.source_enemy} kind="enemy" {visuals} />.</p>
            {:else}<DebugNote text="Source enemy unavailable." />{/if}
            <DebugNote>{detail.source_estimated ? 'Source estimated.' : 'Source not marked estimated.'} {detail.contents_known ? 'Contents observed; item details are not included in this alert.' : 'Contents unseen.'} Observation does not establish pickup or ownership.</DebugNote>
          {:else if detail.type === 'key_popped'}
            <p>{detail.dungeon} key popped</p>
          {:else if detail.type === 'death'}
            {#if !detail.character_id}<DebugNote text="Character identity unavailable." />{/if}
            <DebugNote text="Killer and fame loss are unavailable." />
          {/if}
          {#if !destination && row.kind !== 'key_popped'}<DebugNote>Saved {row.kind === 'personal_best' && detail?.type !== 'run_time_personal_best' ? 'fight' : 'run'} relationship unavailable. No destination can be opened.</DebugNote>{/if}
        </details>
          {#if row.kind === 'key_popped' && detail?.type === 'key_popped'}<MuteDungeon dungeon={detail.dungeon} />{/if}
          {#if destination}<a href={destination}>{row.kind === 'personal_best' && detail?.type !== 'run_time_personal_best' ? 'View fight' : row.kind === 'death' ? 'View death' : 'View run'}</a>{/if}
        </div>
        <DebugNote text={row.explanation} />
      </article>
    </li>
  {/each}
</ul>

<style>
  ul { list-style: none; padding: 0; margin: 0; }
  article { border-bottom: 1px solid var(--hairline); padding: var(--space-2) 0; min-height: var(--row-min); overflow-wrap: anywhere; }
  .row-actions { display: flex; flex-wrap: wrap; align-items: start; gap: var(--space-2); }
  .row-actions details { flex: 1; min-width: 0; }
  .row-actions details[open] { flex-basis: 100%; }
  a { color: var(--text-primary); min-height: var(--target-min); display: inline-flex; align-items: center; font-size: var(--text-label); text-decoration: none; text-underline-offset: 3px; }
  a:hover, a:focus-visible { text-decoration: underline; }
  p { margin: var(--space-1) 0; }
  details { color: var(--text-secondary); font-size: var(--text-label); }
  summary { min-height: var(--target-min); display: list-item; align-content: center; cursor: pointer; color: var(--text-secondary); }
  summary strong { color: var(--text-primary); font-weight: var(--weight-medium); }
  summary .number { color: var(--text-muted); }
  .number { font-variant-numeric: tabular-nums; }
</style>
