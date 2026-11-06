<script lang="ts">
  import type { SpriteRef } from '../../bindings/SpriteRef';
  import { objectVisual } from '../../bridge/visual-index';
  import { itemHover } from '../../components/item-hover';
  import SpriteImage from '../../components/SpriteImage.svelte';
  import { routeHref } from '../../navigation/routes';
  import { formatDps, formatElapsed } from '../../styles/format';
  import type { HomeVisuals } from '../visuals';
  import { relative } from './hero';
  import PartyComparison from './PartyComparison.svelte';
  import type { ChainView } from './recap-view';

  let { chain, visuals, resolveSpriteUrl }: {
    chain: ChainView; visuals: HomeVisuals; resolveSpriteUrl: (s: SpriteRef) => string;
  } = $props();
  const headingId = $props.id();
</script>

<section class="v4-card recap-chain" aria-labelledby={headingId}>
  <header class="v4-card-heading"><h3 id={headingId}>Chain</h3><span class="recap-note">inferred</span></header>
  <div class="recap-table-scroll"><table class="recap-table">
    <thead><tr><th scope="col">Step</th><th scope="col">Time</th><th scope="col">Your DPS</th><th scope="col">vs party</th><th scope="col">Loot</th></tr></thead>
    <tbody>{#each chain.steps as step}
      <tr><th scope="row">{step.name}</th><td>{step.ended === null ? '—' : formatElapsed(step.started, step.ended)}</td>
        <td>{formatDps(step.local_dps)}</td><PartyComparison value={relative(step.local_dps, step.party_dps)} />
        <td><div class="recap-chain-loot"><span>{step.bags} {step.bags === 1 ? 'bag' : 'bags'}</span>
          {#each step.item_types as type}
            {@const visual = objectVisual(visuals.objects, type)}
            <a href={routeHref({ kind: 'item', type })} use:itemHover={{ type, name: visual?.name ?? '—' }}>
              <SpriteImage sprite={visual?.sprite ?? null} sheetUrl={visual?.sprite ? resolveSpriteUrl(visual.sprite) : null} fit={24} alt={visual?.name ?? '—'} />
            </a>
          {/each}
        </div></td>
      </tr>
    {/each}</tbody>
  </table></div>
  <p class="recap-chain-total">Chain total · {chain.totals.time} · {chain.totals.bags} {chain.totals.bags === 1 ? 'bag' : 'bags'} · {chain.totals.whites} {chain.totals.whites === 1 ? 'white' : 'whites'}</p>
</section>
