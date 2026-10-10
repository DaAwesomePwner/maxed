<script lang="ts">
  import { debugMode } from '../debug/debug';
  import DebugNote from '../debug/DebugNote.svelte';
  import type { PlayerDamage } from '../bindings/PlayerDamage';
  import { emptyHomeVisuals, type HomeVisuals } from '../home/visuals';
  import ItemTile from '../components/ItemTile.svelte';
  import { objectVisual } from '../bridge/visual-index';
  import { spriteUrl } from '../bridge/visuals';
  import { enchantCount, rarityName } from '../styles/enchant';
  import { formatShare, formatStat } from '../styles/format';
  import { nonnegative, sourceExplanation, sourceLabels } from './damage';
  import { sourceRows } from './player-screen';
  let { player, label = 'Damage sources', visuals = emptyHomeVisuals(), rowPrefix = 'source' }: {
    player: PlayerDamage; label?: string; visuals?: HomeVisuals; rowPrefix?: string;
  } = $props();
  let rows = $derived(sourceRows(player, visuals));
</script>

<section aria-label={label} class="area-card sources">
  <div class="heading"><h3>{label}</h3><span>{rows.length} {rows.length === 1 ? 'source' : 'sources'} · {formatStat(nonnegative(player.damage))} total</span></div>
  <!-- Scrollable tables need a keyboard focus target for arrow-key scrolling. -->
  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <div class="source-table" tabindex="0" role="region" aria-label={`${label} table scroll area`}>
    <table aria-label={label}>
      <colgroup><col style="width: 30%" /><col style="width: 9%" /><col /><col style="width: 58px" /><col style="width: 52px" /><col style="width: 62px" /><col style="width: 64px" /></colgroup>
      <thead><tr><th scope="col">Source</th><th scope="col">Type</th><th scope="col">Damage</th><th scope="col">Share</th><th scope="col">Hits</th><th scope="col">Avg hit</th><th scope="col">Best hit</th></tr></thead>
      <tbody>
        {#each rows as row (row.key)}
          {@const visual = objectVisual(visuals.objects, row.itemType)}
          {@const count = enchantCount(row.enchant)}
          <tr data-row-id={`${rowPrefix}:${row.key}`} tabindex="-1">
            <th scope="row"><div class="identity">
              <ItemTile sprite={visual?.sprite ?? null} resolveSpriteUrl={sprite => spriteUrl(visuals.client_fingerprint, sprite)} alt={row.name} fit={32} enchant={row.enchant} />
              <div class="item-text"><span class="name" class:ut={row.tier === 'UT'} class:st={row.tier === 'ST'} title={row.name}>{row.name}</span>
                <div class="meta"><span class:ut={row.tier === 'UT'} class:st={row.tier === 'ST'}>{row.tier ?? '—'}</span>
                  {#if count !== null}
                    {#if count > 0}<span class="enchant-pips" aria-hidden="true">{'•'.repeat(count)}</span>{/if}
                    {@const summary = count === 0 ? 'no enchants' : `${rarityName(count)} · ${count} ${count === 1 ? 'enchant' : 'enchants'}`}
                    <span title={summary}>{summary}</span>
                  {/if}
                </div>
              </div>
            </div></th>
            <td class="kind">{sourceLabels[row.kind]}{#if row.estimated && $debugMode}<span class="estimated">estimated</span>{/if}</td>
            <td><span class="amount" class:local={player.is_local}><span class="damage-fill" style:width={`${(row.share ?? 0) * 100}%`}></span><b>{formatStat(row.damage)}</b></span></td>
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
  .heading { display: flex; align-items: baseline; justify-content: space-between; gap: 8px; flex-wrap: wrap; }
  .heading > span { color: var(--text-muted); font-size: 11px; font-variant-numeric: tabular-nums; }
  .source-table { max-height: 420px; overflow: auto; min-width: 0; }
  table { width: 100%; min-width: 580px; table-layout: fixed; border-collapse: collapse; font-size: 12px; font-variant-numeric: tabular-nums; }
  th, td { padding: 0 4px; text-align: right; box-sizing: border-box; font-weight: 400; white-space: nowrap; }
  thead { position: sticky; top: 0; z-index: 1; background: var(--surface-panel); }
  thead th { height: 26px; color: var(--text-muted); font-size: 10px; }
  th:first-child, th:nth-child(2), .kind { text-align: left; }
  tbody tr { border-top: 1px solid var(--border); height: 40px; }
  .identity { display: flex; gap: 7px; align-items: center; min-width: 0; }
  .item-text { min-width: 0; }
  .name, .meta span:last-child { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .ut { color: var(--violet-text); } .st { color: var(--bag-orange, #ffc076); }
  .meta { display: flex; align-items: center; gap: 6px; color: var(--text-muted); font-size: 10px; }
  .meta > span:first-child { flex: none; }
  .meta .enchant-pips { flex: none; color: var(--gold); letter-spacing: -1px; }
  .kind { color: var(--text-muted); font-size: 10px; white-space: normal; overflow-wrap: anywhere; }
  .estimated { display: block; width: fit-content; font-size: 9px; border: 1px solid var(--border); border-radius: 4px; padding: 0 3px; }
  .amount { position: relative; display: block; height: 18px; background: var(--surface-inset, #0c0b11); border-radius: 3px; }
  .damage-fill { position: absolute; inset: 0 auto 0 0; background: var(--violet); opacity: .7; border-radius: inherit; }
  .local .damage-fill { background: var(--gold); }
  .amount b { position: absolute; inset: 0 4px 0 0; line-height: 18px; font-size: 11px; color: white; text-shadow: 1px 0 #000, -1px 0 #000, 0 1px #000, 0 -1px #000; font-weight: 600; }
  .hint { color: var(--text-muted); font-size: 11px; overflow-wrap: anywhere; }
  summary { cursor: pointer; min-height: 26px; align-content: center; }
</style>
