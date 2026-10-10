<script lang="ts">
  import type { FightView } from '../bindings/FightView';
  import { emptyHomeVisuals, type HomeVisuals } from '../home/visuals';
  import { objectVisual } from '../bridge/visual-index';
  import { spriteUrl } from '../bridge/visuals';
  import SpriteImage from '../components/SpriteImage.svelte';
  import { debugMode } from '../debug/debug';
  import DebugNote from '../debug/DebugNote.svelte';
  import { parseObjectId, routeHref } from '../navigation/routes';
  import { formatDps, formatStat, formatShare } from '../styles/format';
  import { damageRank, nonnegative } from './damage';
  import { limitRows, renderCount, RENDER_STEP, type RenderLimit } from './render-limit';
  import ShowMore from './ShowMore.svelte';
  let { fight, visuals = emptyHomeVisuals(), runId }: { fight: FightView; visuals?: HomeVisuals; runId?: string } = $props();
  let counts = $derived.by(() => {
    const result = new Map<number, number>();
    for (const player of fight.players) result.set(player.object_id, (result.get(player.object_id) ?? 0) + 1);
    return result;
  });
  let limit = $state<RenderLimit>({ key: '', count: RENDER_STEP });
  let count = $derived(renderCount(limit, fight.id));
  let shown = $derived(limitRows(fight.players, count, row => row.is_local));
  let top = $derived(Math.max(0, ...fight.players.map(row => nonnegative(row.damage) ?? 0)));
  const width = (value: number) => top > 0 ? Math.min(100, (nonnegative(value) ?? 0) / top * 100) : 0;
  function followRow(event: MouseEvent) {
    if ((event.target as HTMLElement).closest('a')) return;
    event.currentTarget instanceof HTMLElement && event.currentTarget.querySelector<HTMLAnchorElement>('a')?.click();
  }
</script>
<section class="area-card players" aria-label="Players">
  <div class="heading"><h3>Players</h3><span>Click a player for their damage sources</span></div>
  <div class="damage-table">
    <table aria-label="Fight Players">
      <colgroup><col style="width: 28px" /><col style="width: 20%" /><col /><col style="width: 64px" /><col style="width: 60px" /><col style="width: 56px" /><col style="width: 64px" /><col style="width: 52px" /></colgroup>
      <thead><tr><th scope="col">#</th><th scope="col">Player</th><th scope="col">Damage</th><th scope="col">DPS</th><th scope="col">Share</th><th scope="col">Hits</th><th scope="col">Best hit</th><th scope="col">Deaths</th></tr></thead>
      <tbody>
        {#each shown as player (player.player_id)}
          {@const visual = objectVisual(visuals.objects, player.class_id)}
          {@const linked = counts.get(player.object_id) === 1 && parseObjectId(String(player.object_id)) !== null}
          <tr class="damage-row" class:self={player.is_local} class:linked data-row-id={player.player_id}
            onclick={linked ? followRow : undefined}>
            <td>{damageRank(player.rank)}</td>
            <th scope="row"><div class="identity">
              <SpriteImage sprite={visual?.sprite ?? null} sheetUrl={visual?.sprite ? spriteUrl(visuals.client_fingerprint, visual.sprite) : null} fit={24} alt={visual?.name ?? 'Class'} />
              {#if linked}<a class="name" aria-label={`${damageRank(player.rank)} ${player.name ?? 'Unknown player'}`} title={player.name ?? 'Unknown player'} href={routeHref({ kind: 'player', fightId: fight.id, objectId: player.object_id, runId })}>{player.name ?? 'Unknown player'}</a>
              {:else}<span class="name" title={player.name ?? 'Unknown player'}>{player.name ?? 'Unknown player'}</span>{/if}
              {#if player.is_local}<span class="you">You</span>{/if}
            </div></th>
            <td aria-label="Damage"><span class="amount"><span class="damage-fill" style:width={`${width(player.damage)}%`}></span><b>{formatStat(nonnegative(player.damage))}</b></span></td>
            <td aria-label="DPS">{formatDps(player.dps)}</td><td aria-label="Share">{formatShare(player.share)}</td>
            <td aria-label="Hits">{formatStat(nonnegative(player.hits))}</td><td aria-label="Biggest Hit">{formatStat(nonnegative(player.biggest_hit))}</td><td aria-label="Deaths" class:deaths={player.deaths > 0}>{formatStat(nonnegative(player.deaths))}</td>
          </tr>
          {#if !linked && $debugMode}<tr><td colspan="8"><DebugNote text="Player navigation is ambiguous because the object ID was reused or is unavailable." /></td></tr>{/if}
        {:else}<tr><td colspan="8">No players</td></tr>{/each}
        <tr class="other"><td></td><th scope="row">Other damage</th><td><span class="amount"><span class="damage-fill" style:width={`${width(fight.unattributed_damage)}%`}></span><b>{formatStat(nonnegative(fight.unattributed_damage))}</b></span></td><td>—</td><td>—</td><td>—</td><td>—</td><td>—</td></tr>
      </tbody>
    </table>
  </div>
  <ShowMore shown={shown.length} total={fight.players.length} noun="players"
    onmore={() => { limit = { key: fight.id, count: count + RENDER_STEP }; }} />
</section>
<style>
  .players { grid-area: table; align-self: start; }
  .heading { display: flex; align-items: baseline; gap: 12px; flex-wrap: wrap; }
  .heading > span { color: var(--text-muted); font-size: 11px; }
  .damage-table { overflow: auto; max-height: 390px; min-width: 0; font-size: 12px; font-variant-numeric: tabular-nums; }
  table { width: 100%; table-layout: fixed; border-collapse: collapse; }
  th, td { height: 30px; box-sizing: border-box; padding: 0 4px; text-align: right; white-space: nowrap; }
  thead { position: sticky; top: 0; z-index: 1; background: var(--surface-panel); }
  thead th { font-size: 10px; color: var(--text-muted); font-weight: 500; }
  th:nth-child(2), tbody th { text-align: left; }
  tbody tr { border-top: 1px solid var(--border); }
  .identity { display: flex; gap: 5px; align-items: center; min-width: 0; }
  .name { overflow: hidden; text-overflow: ellipsis; font-weight: 500; color: inherit; text-decoration: none; }
  a.name { line-height: 30px; }
  .you { color: var(--gold); border: 1px solid #f2c14e55; border-radius: 4px; padding: 0 4px; font-size: 10px; }
  .amount { position: relative; display: block; height: 18px; background: var(--surface-inset, #0c0b11); border-radius: 3px; }
  .damage-fill { position: absolute; inset: 0 auto 0 0; background: var(--violet); opacity: .7; border-radius: inherit; }
  .amount b { position: absolute; inset: 0 4px 0 0; line-height: 18px; font-size: 11px; color: white; text-shadow: 1px 0 #000, -1px 0 #000, 0 1px #000, 0 -1px #000; font-weight: 600; }
  .self { background: color-mix(in srgb, var(--gold) 7%, transparent); }
  .self th { color: var(--gold); } .self .damage-fill { background: var(--gold); }
  .self td:first-child { box-shadow: inset 2px 0 var(--gold); }
  .other { color: var(--text-muted); } .other .damage-fill { background: var(--border-strong); }
  .linked { cursor: pointer; } .linked:hover { background: #ffffff07; } .linked:focus-within { outline: 1px solid var(--gold); outline-offset: -1px; }
  .deaths { color: var(--danger); }
  @media (max-width: 1150px) { .damage-table { font-size: 11px; } th, td { padding-inline: 2px; } .identity { gap: 3px; } }
</style>
