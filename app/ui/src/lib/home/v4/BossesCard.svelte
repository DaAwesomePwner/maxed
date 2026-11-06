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
  .bosses-card { min-width: 0; padding: 16px; border: 1px solid var(--border); border-radius: var(--radius-card);
    background: var(--surface-panel); color: var(--text-primary); font-family: var(--font-home); }
  h2 { margin: 0 0 12px; font-size: 13px; font-weight: 600; }
  ol { list-style: none; margin: 0; padding: 0; }
  li { display: flex; align-items: flex-start; gap: 10px; padding: 12px 0; }
  li + li { border-top: 1px solid var(--border); }
  li:last-child { padding-bottom: 0; }
  .details { min-width: 0; overflow-wrap: anywhere; }
  .name { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; font-size: 13px; }
  .tag { padding: 1px 5px; border: 1px solid var(--border); border-radius: var(--radius-control); font-size: 11px; }
  p { margin: 4px 0 0; font-size: 12px; color: var(--text-secondary); }
  .faster { color: var(--success); }
  .upcoming { opacity: .65; }
  .empty { margin: 0; }
</style>
