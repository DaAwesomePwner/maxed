<script lang="ts">
  import type { SpriteRef } from '../../bindings/SpriteRef';
  import { objectVisual } from '../../bridge/visual-index';
  import SpriteImage from '../../components/SpriteImage.svelte';
  import { routeHref } from '../../navigation/routes';
  import { formatDps, formatShare } from '../../styles/format';
  import { clockTime } from '../nexus';
  import type { HomeVisuals } from '../visuals';
  import PartyComparison from './PartyComparison.svelte';
  import PartySparkline from './PartySparkline.svelte';
  import { fightRank, type VisitEventRow } from './recap-view';

  let { rows, partial = false, visuals, resolveSpriteUrl }: {
    rows: VisitEventRow[]; partial?: boolean; visuals: HomeVisuals; resolveSpriteUrl: (s: SpriteRef) => string;
  } = $props();
  const headingId = $props.id();
</script>

<section class="v4-card visit-events-card" aria-labelledby={headingId}>
  <header class="v4-card-heading"><h3 id={headingId}>Events this visit</h3>
    {#if rows.some(row => row.buckets !== null)}
      <div class="recap-legend"><span><i class="you" aria-hidden="true"></i>You</span><span><i class="party" aria-hidden="true"></i>Party average</span></div>
    {/if}
  </header>
  {#if partial}<p class="recap-note">Partial event history</p>{/if}
  {#if rows.length === 0}<p class="recap-note">No events observed</p>
  {:else}
    <div class="recap-table-scroll">
      <table class="recap-table">
        <thead><tr><th scope="col">Event</th><th scope="col">Your DPS</th><th scope="col">vs party</th><th scope="col">Rank</th><th scope="col">DPS over fight</th><th scope="col">Bags</th></tr></thead>
        <tbody>{#each rows as row (row.key)}
          {@const visual = objectVisual(visuals.objects, row.targetType)}
          <tr class:announced={row.state === 'announced'} data-event-id={row.key}>
            <th scope="row"><div class="recap-target">
              <span aria-hidden="true"><SpriteImage sprite={visual?.sprite ?? null}
                sheetUrl={visual?.sprite ? resolveSpriteUrl(visual.sprite) : null} fit={32} alt="" /></span>
              <div>{visual?.name ?? row.name}
                {#if row.state !== 'announced'}<span class="recap-row-detail">{row.kill ?? '—'}</span>{/if}
                <time class="recap-row-detail">{clockTime(row.at) ?? '—'}</time>
              </div>
            </div></th>
            {#if row.state === 'announced'}
              <td colspan="4" class="recap-note">Announced · you weren't there</td>
            {:else}
              <td>{formatDps(row.dps)}{#if row.best}<span class="recap-tag">BEST DPS</span>{/if}</td>
              <PartyComparison value={row.vsParty} />
              <td>{fightRank(row.rank, row.players)}{#if row.share !== null}<span class="recap-row-detail">{formatShare(row.share)} of damage</span>{/if}</td>
              <td>{#if row.buckets}<PartySparkline buckets={row.buckets} width={96} height={28} />{:else}<span class="sr-only">Party average line not recorded for this fight</span><span class="recap-note" aria-hidden="true">Not recorded</span>{/if}</td>
            {/if}
            <td>{#if row.bags.length}<div class="event-bags">
              {#each row.bags as bag (bag.dropId)}
                {@const bagVisual = objectVisual(visuals.objects, bag.bagType)}
                <a href={routeHref({ kind: 'loot', view: 'feed' })} data-drop-id={bag.dropId}
                  title={bag.estimated ? 'Linked by estimated source' : undefined}>
                  <span class="sr-only">{bagVisual?.name ?? 'Bag'}</span>
                  <span aria-hidden="true"><SpriteImage sprite={bagVisual?.sprite ?? null}
                    sheetUrl={bagVisual?.sprite ? resolveSpriteUrl(bagVisual.sprite) : null} fit={20} alt="" /></span>
                </a>
              {/each}
            </div>{:else}—{/if}</td>
          </tr>
        {/each}</tbody>
      </table>
    </div>
  {/if}
</section>

<style>
  .announced { opacity: .55; }
  .event-bags { display: flex; flex-wrap: wrap; gap: 4px; }
  .event-bags a { display: flex; align-items: center; justify-content: center; min-width: 24px; min-height: 24px; border-radius: var(--radius-control); }
</style>
