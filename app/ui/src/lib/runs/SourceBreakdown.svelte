<script lang="ts">
  import { itemHover } from '../components/item-hover';
  import { debugMode } from '../debug/debug';
  import DebugNote from '../debug/DebugNote.svelte';
  import type { PlayerDamage } from '../bindings/PlayerDamage';
  import { emptyHomeVisuals, type HomeVisuals } from '../home/visuals';
  import ItemTile from '../components/ItemTile.svelte';
  import Badge from '../components/Badge.svelte';
  import { objectVisual } from '../bridge/visual-index';
  import { spriteUrl } from '../bridge/visuals';
  import { enchantCount, rarityName } from '../styles/enchant';
  import { formatShare, formatStat } from '../styles/format';
  import { nonnegative, sourceExplanation, sourceLabels } from './damage';
  import { sourceRows } from './player-screen';
  let { player, label = 'Damage sources', visuals = emptyHomeVisuals(), rowPrefix = 'source', showEstimates = false }: {
    player: PlayerDamage; label?: string; visuals?: HomeVisuals; rowPrefix?: string; showEstimates?: boolean;
  } = $props();
  let rows = $derived(sourceRows(player, visuals));
</script>

<section aria-label={label} class="area-card sources">
  <div class="heading"><h3>{label}</h3><span>{rows.length} {rows.length === 1 ? 'source' : 'sources'} · {formatStat(nonnegative(player.damage))} total</span></div>
  <!-- Scrollable tables need a keyboard focus target for arrow-key scrolling. -->
  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <div class="source-table" tabindex="0" role="region" aria-label={`${label} table scroll area`}>
    <table aria-label={label}>
      <colgroup><col style="width: 30%" /><col style="width: 9%" /><col /><col style="width: 56px" /><col style="width: 52px" /><col style="width: 64px" /><col style="width: 64px" /></colgroup>
      <thead><tr><th scope="col">Source</th><th scope="col">Type</th><th scope="col">Damage</th><th scope="col">Share</th><th scope="col">Hits</th><th scope="col">Avg hit</th><th scope="col">Best hit</th></tr></thead>
      <tbody>
        {#each rows as row, index (row.key)}
          {@const visual = objectVisual(visuals.objects, row.itemType)}
          {@const count = enchantCount(row.enchant)}
          <tr data-row-id={`${rowPrefix}:${row.key}`} tabindex="-1">
            <th scope="row">
              {#snippet itemIdentity()}
                <ItemTile sprite={visual?.sprite ?? null} resolveSpriteUrl={sprite => spriteUrl(visuals.client_fingerprint, sprite)} alt={row.name} fit={32} enchant={row.enchant} />
                <div class="item-text"><span class="name" class:ut={row.tier === 'UT'} class:st={row.tier === 'ST'}>{row.name}</span>
                  <div class="meta"><span class:ut={row.tier === 'UT'} class:st={row.tier === 'ST'}>{row.tier ?? '—'}</span>
                    {#if count !== null}
                      {#if count > 0}<span class="enchant-pips" aria-hidden="true" style:color={`var(--rarity-${rarityName(count)?.toLowerCase()})`}>{'•'.repeat(count)}</span>{/if}
                      {@const summary = count === 0 ? 'no enchants' : `${rarityName(count)} · ${count} ${count === 1 ? 'enchant' : 'enchants'}`}
                      <span>{summary}</span>
                    {/if}
                  </div>
                  {#if showEstimates && player.sources[index].reasons.includes('inferred-equipped-weapon')}
                    <span class="inferred-note">Inferred from the equipped weapon</span>
                  {/if}
                </div>
              {/snippet}
              {#if row.itemType !== null}<div class="identity" use:itemHover={{ type: row.itemType, enchant: row.enchant, name: row.name, tier: row.tier }}>{@render itemIdentity()}</div>
              {:else}<div class="identity">{@render itemIdentity()}</div>{/if}
            </th>
            <td class="kind">{sourceLabels[row.kind]}{#if row.estimated && (showEstimates || $debugMode)}<span class="estimated"><Badge label={showEstimates ? 'est.' : 'estimated'} /></span>{/if}</td>
            <td><span class="amount"><span class="track"><span class="damage-fill" style:width={`${(row.share ?? 0) * 100}%`}></span></span><b>{formatStat(row.damage)}</b></span></td>
            <td>{formatShare(row.share)}</td><td>{formatStat(row.hits)}</td><td>{formatStat(row.avgHit)}</td><td>{formatStat(row.bestHit)}</td>
          </tr>
        {:else}<tr><td colspan="7">No sources</td></tr>{/each}
      </tbody>
    </table>
  </div>
  {#if $debugMode}<details class="hint"><summary>Source coverage</summary>
    <p>Saved source entries remain separate, including different enchant evidence for the same item. Unknown Damage has no supported source attribution. Omitted reconstruction has no saved damage amount and is not counted as zero.</p>
    {#each rows as row, index (row.key)}
      {@const source = player.sources[index]}
      <DebugNote text={`${row.name}: Enchant evidence: ${source.enchant.state}. Slots: ${formatStat(source.enchant.slots)} · Applied: ${formatStat(source.enchant.applied)} · IDs: ${source.enchant.ids.join(', ') || '—'}`} />
      {#if sourceExplanation(source)}<DebugNote text={`${row.name}: ${sourceExplanation(source)}`} />{/if}
    {/each}
  </details>{/if}
</section>
<style>
  .sources { min-width: 0; align-self: start; }
  .heading { display: flex; align-items: baseline; justify-content: space-between; gap: var(--space-2); flex-wrap: wrap; }
  .heading > span { color: var(--text-muted); font-size: var(--text-meta); line-height: var(--leading-meta); font-variant-numeric: tabular-nums; }
  .source-table { max-height: 420px; overflow: auto; min-width: 0; }
  table { width: 100%; min-width: 580px; table-layout: fixed; border-collapse: collapse; font-size: var(--text-caption); line-height: var(--leading-caption); font-variant-numeric: tabular-nums; }
  th, td { padding: 0 var(--space-1); text-align: right; box-sizing: border-box; font-weight: var(--weight-normal); white-space: nowrap; }
  thead { position: sticky; top: 0; z-index: 1; background: var(--surface-panel); }
  thead th { height: var(--space-6); color: var(--text-muted); font-size: var(--text-meta); font-weight: var(--weight-medium); border-bottom: 1px solid var(--hairline-strong); }
  th:first-child, th:nth-child(2), .kind { text-align: left; }
  tbody tr { height: var(--row-min); }
  tbody tr + tr { border-top: 1px solid var(--hairline); }
  .identity { display: flex; gap: var(--space-2); align-items: center; min-width: 0; }
  .item-text { min-width: 0; }
  .name, .meta span:last-child { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .ut { color: var(--tier-ut); } .st { color: var(--tier-st); }
  .meta { display: flex; align-items: center; gap: var(--space-2); color: var(--text-muted); font-size: var(--text-meta); line-height: var(--leading-meta); }
  .meta > span:first-child { flex: none; }
  .meta .enchant-pips { flex: none; }
  .inferred-note { display: block; color: var(--text-muted); font-size: var(--text-meta); line-height: var(--leading-meta); white-space: normal; }
  .kind { color: var(--text-muted); font-size: var(--text-meta); white-space: normal; overflow-wrap: anywhere; }
  .estimated { display: flex; width: fit-content; }
  /* Damage: a thin track, with the figure in its own right-aligned slot (matches DamageTable).
     Every row belongs to one player, so the fill stays neutral. */
  .amount { display: flex; align-items: center; gap: var(--space-2); }
  .track { position: relative; flex: 1; min-width: 0; height: 6px; border-radius: 2px; background: var(--bar-track); overflow: hidden; }
  .damage-fill { position: absolute; inset: 0 auto 0 0; background: var(--bar-party); border-radius: inherit; }
  .amount b { flex: none; min-width: 9ch; text-align: right; font-weight: var(--weight-normal); color: var(--text-primary); }
  .hint { color: var(--text-muted); font-size: var(--text-meta); overflow-wrap: anywhere; }
  summary { cursor: pointer; min-height: var(--target-min); align-content: center; }
</style>
