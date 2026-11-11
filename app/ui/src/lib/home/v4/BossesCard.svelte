<script lang="ts">
  import type { SpriteRef } from '../../bindings/SpriteRef';
  import SpriteImage from '../../components/SpriteImage.svelte';
  import { formatDps, formatElapsed } from '../../styles/format';
  import type { HomeVisuals } from '../visuals';
  import { killDelta, type BossRow } from './bosses';

  let { rows, visuals, resolveSpriteUrl }: {
    rows: BossRow[]; visuals: HomeVisuals; resolveSpriteUrl: (sprite: SpriteRef) => string;
  } = $props();
  const time = (micros: number | null) => micros === null ? '—' : formatElapsed('0', String(micros));
</script>

<section class="bosses-card" aria-label="Bosses">
  <h2>Bosses</h2>
  {#if rows.length === 0}<p class="empty">No bosses fought yet</p>
  {:else}
    <ol>
      {#each rows as row}
        {@const sprite = row.sprite ?? visuals.objects.find(object => object.object_type === row.targetType)?.sprite ?? null}
        {@const comparison = killDelta(row)}
        <li class:upcoming={row.state === 'upcoming'}>
          <SpriteImage {sprite} sheetUrl={sprite ? resolveSpriteUrl(sprite) : null} alt={row.name} fit={32} />
          <div class="details">
            <div class="name"><strong>{row.name}</strong>
              {#if row.state === 'live'}<span class="tag">Live</span>
              {:else if row.state === 'ended'}<span class="tag">Ended</span>
              {:else if row.state === 'upcoming'}<span class="tag">Upcoming</span>{/if}
            </div>
            {#if row.state === 'upcoming'}
              <p>{comparison || 'usually —'}</p>
              <p>best DPS {formatDps(row.bestDps)}</p>
            {:else}
              <p>{row.state === 'live' ? `Elapsed ${time(row.elapsedMicros)}` : `Kill time ${time(row.killMicros)}`}</p>
              {#if comparison}<p class:faster={comparison.startsWith('▲')}>{comparison}</p>{/if}
              <p>Your DPS {formatDps(row.dps)} · {row.rank === null ? '—' : `#${row.rank}`}</p>
            {/if}
          </div>
        </li>
      {/each}
    </ol>
  {/if}
</section>

<style>
  .bosses-card { min-width: 0; padding: var(--space-4); border: 1px solid var(--hairline); border-radius: var(--radius-card);
    background: var(--surface-panel); color: var(--text-primary); font-family: var(--font-home); }
  h2 { margin: 0 0 var(--space-1); font-size: var(--text-label); line-height: var(--leading-label); font-weight: var(--weight-medium); }
  ol { list-style: none; margin: 0; padding: 0; }
  li { display: flex; align-items: flex-start; gap: var(--space-3); padding: var(--space-3) 0; }
  li + li { border-top: 1px solid var(--hairline); }
  li:last-child { padding-bottom: 0; }
  .details { min-width: 0; overflow-wrap: anywhere; }
  .name { display: flex; flex-wrap: wrap; align-items: center; gap: var(--space-2); font-size: var(--text-caption); line-height: var(--leading-caption); }
  .name strong { font-weight: var(--weight-medium); }
  .tag { display: inline-flex; align-items: center; box-sizing: border-box; min-height: 18px; padding: 0 var(--space-1);
    border: 1px solid var(--hairline-strong); border-radius: var(--radius-tag); color: var(--text-secondary);
    font-size: var(--text-meta); line-height: var(--leading-meta); font-weight: var(--weight-medium); }
  p { margin: var(--space-1) 0 0; color: var(--text-secondary); font-size: var(--text-meta); line-height: var(--leading-meta); font-variant-numeric: tabular-nums; }
  .faster { color: var(--success); }
  .upcoming { opacity: .6; }
  .empty { margin: var(--space-2) 0 0; }
</style>
