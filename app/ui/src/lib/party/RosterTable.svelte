<script lang="ts">
  import type { PartyMember } from '../bindings/PartyMember';
  import type { PartyView } from '../bindings/PartyView';
  import type { SpriteRef } from '../bindings/SpriteRef';
  import type { HomeVisuals } from '../home/visuals';
  import ItemTile from '../components/ItemTile.svelte';
  import SpriteImage from '../components/SpriteImage.svelte';
  import { itemHover } from '../components/item-hover';
  import { objectVisual } from '../bridge/visual-index';
  import { effectiveMaxed, maxedClaimsSupported } from '../players/maxed-evidence';
  import { formatShare } from '../styles/format';
  import { equipSlots, flagLabel, presenceTime, reasonText } from './party-screen';
  let { view, members, saved, selected, visuals, resolveSpriteUrl, onSelect, onRun }: {
    view: PartyView; members: PartyMember[]; saved: boolean; selected: string | null; visuals: HomeVisuals;
    resolveSpriteUrl: (sprite: SpriteRef) => string | null;
    onSelect: (member: PartyMember, keyboard?: boolean) => void; onRun: (member: PartyMember) => void;
  } = $props();
  const labels = { pass: 'Pass', fail: 'Fail', check: 'Check', not_seen: 'Not seen' };
  let parseVisible = $derived(!!view.preset && view.assets_available);
  let dungeon = $derived(view.area_kind === 'dungeon');
</script>
<div class="table-scroll">
  <table aria-label="Party roster">
    <thead><tr>{#if parseVisible}<th>Parse</th>{/if}<th>Player</th><th class="number">Maxed</th>
      <th>Weapon</th><th>Ability</th><th>Armor</th><th>Ring</th>
      {#if dungeon}<th class="number">Dmg share</th><th class="number">Boss share</th>{/if}<th>Notes</th>
    </tr></thead>
    <tbody>
      {#each members as member (member.name_key ?? member.player_id)}
        {@const key = member.name_key ?? member.player_id}
        {@const named = !!member.name && !!member.name_key}
        {@const maxed = effectiveMaxed(member)}
        {@const reasons = parseVisible ? (member.parse?.reasons ?? []).filter(reason => reason.kind !== 'below_maxed' || maxedClaimsSupported(member)) : []}
        {@const classVisual = objectVisual(visuals.objects, member.class_id)}
        <tr data-row-id={key} class:selected={selected === key}
          class:out-of-view={!saved && member.present === false} tabindex={named ? 0 : undefined}
          aria-selected={named ? selected === key : undefined}
          onclick={() => { if (named) onSelect(member); }} ondblclick={() => { if (named) onRun(member); }}
          onkeydown={event => { if (event.target === event.currentTarget && named && (event.key === 'Enter' || event.key === ' ')) { event.preventDefault(); onSelect(member, true); } }}>
          {#if parseVisible}<td><span class="pill {member.parse?.result ?? 'not_seen'}">{labels[member.parse?.result ?? 'not_seen']}</span></td>{/if}
          <td class="player">
            {#if classVisual?.sprite}<SpriteImage sprite={classVisual.sprite} sheetUrl={resolveSpriteUrl(classVisual.sprite)} alt={classVisual.name ?? 'Class'} fit={18} />{/if}
            <strong>{member.name ?? 'No name yet'}</strong>
            {#if member.is_local}<span class="you">You</span>{/if}
            {#if named && member.mark}<span class="mark {member.mark}">{member.mark === 'watched' ? 'Watched' : 'Banned'}</span>{/if}
            {#if named && member.runs_together >= 1}<span class="seen" title="Earlier saved runs together">{member.runs_together}×</span>{/if}
            {#if member.departed_at != null}<small>Left</small>
            {:else if !saved && member.present === false}<small>Out of view{member.out_of_view_since != null ? ` since ${presenceTime(member.out_of_view_since)}` : ''}</small>{/if}
          </td>
          <td class="number" class:failing={reasons.some(reason => reason.kind === 'below_maxed')}>{maxed == null ? '—' : `${maxed}/8`}</td>
          {#each equipSlots as slot, index}
            {@const item = member.equipment.find(item => item.slot === index)}
            {@const visual = objectVisual(visuals.objects, item?.item_type)}
            {@const name = visual?.name ?? 'Unknown item'}
            {@const tier = visual?.item_tier == null ? '—' : /^\d+$/.test(visual.item_tier) ? `T${visual.item_tier}` : visual.item_tier}
            {@const failing = reasons.some(reason => reason.slot === slot && ['slot_empty', 'below_tier', 'ut_st_not_accepted'].includes(reason.kind))}
            <td>
              {#if view.gear_recorded && item?.item_type != null}
                <span class="gear" use:itemHover={{ type: item.item_type, enchant: item.enchant, name, tier: visual?.item_tier }}>
                  <ItemTile sprite={visual?.sprite ?? null} {resolveSpriteUrl} alt={name} fit={20} enchant={item.enchant} />
                  <span class:failing>{tier}</span>
                </span>
              {:else}<span class:failing>—</span>{/if}
            </td>
          {/each}
          {#if dungeon}<td class="number">{formatShare(member.share ?? null)}</td><td class="number">{formatShare(member.boss_share ?? null)}</td>{/if}
          <td class="notes">
            {#if reasons.length}<span class:warning={member.parse?.result === 'check'} class:failing={member.parse?.result === 'fail'}>{reasonText(reasons)}</span>{/if}
            {#if dungeon && member.flags.length}<span class="warning">{member.flags.map(flagLabel).join(' · ')}</span>{/if}
            {#if !reasons.length && !(dungeon && member.flags.length) && view.gear_recorded && maxed == null && member.level == null}<span class="muted">Gear not observed yet</span>{/if}
          </td>
        </tr>
      {/each}
    </tbody>
  </table>
</div>
{#if !members.length}<p class="muted">No players match this filter.</p>{/if}
<style>
  .table-scroll { overflow-x: auto; } table { width: 100%; border-collapse: collapse; font-size: 12px; font-variant-numeric: tabular-nums; }
  th { text-align: left; color: var(--text-muted); font-size: 11px; font-weight: 500; }
  th, td { padding: 5px 6px; white-space: nowrap; } td { border-top: 1px solid var(--border); } .number { text-align: right; }
  tr[tabindex] { cursor: pointer; } tr[tabindex]:hover { background: var(--surface-hover); }
  tr.selected { background: #f2c14e14; box-shadow: inset 2px 0 var(--gold); } tr:focus-visible { outline: 2px solid var(--focus-ring); outline-offset: -2px; }
  .out-of-view { opacity: .55; } .player small { display: block; color: var(--text-secondary); }
  .pill { font-size: 11px; border: 1px solid currentColor; padding: 1px 5px; border-radius: var(--radius-control); }
  .pass { color: var(--success); } .fail, .failing { color: var(--danger); } .check, .warning, .watched { color: var(--estimated); }
  .not_seen, .muted { color: var(--text-muted); } .you { color: var(--gold); } .seen { color: var(--violet-text); }
  .mark, .you, .seen { margin-left: 5px; font-size: 10px; } .mark { border: 1px solid currentColor; padding: 0 4px; border-radius: 4px; } .banned { color: var(--danger); }
  .gear { display: inline-flex; align-items: center; gap: 4px; font-size: 11px; } .notes { white-space: normal; min-width: 130px; }
  .notes > span { display: block; }
</style>
