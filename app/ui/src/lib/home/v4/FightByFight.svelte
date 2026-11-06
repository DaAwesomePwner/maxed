<script lang="ts">
  import type { SpriteRef } from '../../bindings/SpriteRef';
  import { objectVisual } from '../../bridge/visual-index';
  import SpriteImage from '../../components/SpriteImage.svelte';
  import { formatDps } from '../../styles/format';
  import type { HomeVisuals } from '../visuals';
  import PartyComparison from './PartyComparison.svelte';
  import PartySparkline from './PartySparkline.svelte';
  import { fightRank, type FightRow } from './recap-view';

  let { rows, partial, visuals, resolveSpriteUrl }: {
    rows: FightRow[]; partial: boolean; visuals: HomeVisuals; resolveSpriteUrl: (s: SpriteRef) => string;
  } = $props();
  const headingId = $props.id();
</script>

<section class="v4-card recap-fights-card" aria-labelledby={headingId}>
  <header class="v4-card-heading"><h3 id={headingId}>Fight by fight</h3>
    {#if rows.some(row => row.buckets !== null)}
      <div class="recap-legend"><span><i class="you" aria-hidden="true"></i>You</span><span><i class="party" aria-hidden="true"></i>Party average</span></div>
    {/if}
  </header>
  {#if partial}<p class="recap-note">Partial fight history</p>{/if}
  {#if rows.length === 0}<p class="recap-note">No fights observed</p>
  {:else}
    <div class="recap-table-scroll">
      <table class="recap-table">
        <thead><tr><th scope="col">Boss</th><th scope="col">Your DPS</th><th scope="col">vs party</th><th scope="col">Rank</th><th scope="col">DPS over fight</th></tr></thead>
        <tbody>{#each rows as row (row.fightId)}
          {@const visual = objectVisual(visuals.objects, row.targetType)}
          <tr>
            <th scope="row"><div class="recap-target">
              <SpriteImage sprite={visual?.sprite ?? null} sheetUrl={visual?.sprite ? resolveSpriteUrl(visual.sprite) : null} fit={32} alt="" />
              <div>{visual?.name ?? row.name ?? '—'}<span class="recap-row-detail">{row.kill}</span></div>
            </div></th>
            <td>{formatDps(row.dps)}{#if row.best}<span class="recap-tag">BEST DPS</span>{/if}</td>
            <PartyComparison value={row.vsParty} /><td>{fightRank(row.rank, row.players)}</td>
            <td>{#if row.buckets}<PartySparkline buckets={row.buckets} />{:else}<span class="recap-note">Party average line not recorded for this fight</span>{/if}</td>
          </tr>
        {/each}</tbody>
      </table>
    </div>
  {/if}
</section>
