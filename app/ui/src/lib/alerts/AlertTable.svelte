<script lang="ts">
  import type { AlertView } from '../bindings/AlertView';
  import { alertTime } from './filters';
  import { alertHeading, alertAction, kindLabel, kindGlyph, shownAs } from './alerts-screen';
  import { formatDps } from '../styles/format';
  import { clockTime, dayLabel } from '../logs/logs-format';
  let { rows, selectedId, onSelect, muted, onMute, filtered = () => false, canMute = false }: {
    rows: AlertView[]; selectedId: string | null; onSelect: (id: string) => void;
    muted: (dungeon: string) => boolean; onMute: (dungeon: string) => void; canMute?: boolean; filtered?: (dungeon: string) => boolean;
  } = $props();
</script>
<div class="scroll"><table aria-label="Saved alerts">
  <colgroup><col class="time" /><col class="kind" /><col /><col class="shown" /><col class="action" /></colgroup>
  <thead><tr><th>Time</th><th>Kind</th><th>Alert</th><th>Shown as</th><th><span class="sr-only">Action</span></th></tr></thead>
  <tbody>{#each rows as row, index (row.id)}
    {@const day = dayLabel(row.at, new Date())}
    {@const action = alertAction(row)}
    {#if index === 0 || dayLabel(rows[index - 1].at, new Date()) !== day}<tr class="day"><td colspan="5">{day}</td></tr>{/if}
    <tr data-row-id={row.id} class:selected={selectedId === row.id} tabindex="0" aria-selected={selectedId === row.id}
      onclick={() => onSelect(row.id)} onkeydown={event => { if (event.target === event.currentTarget && (event.key === 'Enter' || event.key === ' ')) { event.preventDefault(); onSelect(row.id); } }}>
      <td title={alertTime(row.at)}>{clockTime(row.at)}</td>
      <td><span class="k" class:white={row.kind === 'white_bag'} class:key={row.kind === 'key_popped'} class:death={row.kind === 'death'} class:pbk={row.kind === 'personal_best'} class:chat={row.details?.type === 'chat'} class:flaw={row.kind === 'flawless_run'}><i class={`g ${kindGlyph[row.kind]}`} aria-hidden="true"></i><span>{kindLabel[row.kind]}</span></span></td>
      <td class="title" title={alertHeading(row)}><strong>{alertHeading(row)}</strong>{#if row.details?.type === 'chat'} <span title={row.details.excerpt}>{row.details.excerpt}</span>{/if}{#if row.details?.type === 'white_bag' && row.details.source_estimated} <span class="est">est.</span>{/if}{#if row.details?.type === 'white_bag' && row.details.ordinal !== null}<span>{row.details.ordinal} this week</span>{:else if row.details?.type === 'personal_best'}<span>previous: {formatDps(row.details.previous_dps)}</span>{/if}</td>
      <td><span class="delivery" class:banner={row.presentation === 'banner'}>{shownAs(row.presentation)}</span></td>
      <td>{#if action?.href}<a href={action.href}>{action.label} ›</a>{:else if action?.mute}{#if filtered(action.mute)}<span>Filtered by rules</span>{:else}<button class="ctl ghost" aria-label={`${muted(action.mute) ? 'Unmute' : 'Mute'} ${action.mute}`} disabled={!canMute} onclick={() => onMute(action.mute!)}>{muted(action.mute) ? 'Unmute' : 'Mute'}</button>{/if}{/if}</td>
    </tr>
  {/each}</tbody>
</table></div>
<style>
  .scroll { max-height: max(220px, calc(100vh - 340px)); overflow: auto; } table { width: 100%; table-layout: fixed; border-collapse: collapse; font-size: 12px; } .time { width: 76px; } .kind { width: 132px; } .shown { width: 80px; } .action { width: 92px; }
  th { text-align: left; font-weight: 500; color: var(--text-muted); font-size: 11px; } th, td { padding: 5px 7px; border-bottom: 1px solid var(--border); height: 30px; box-sizing: border-box; } td:last-child { padding-block: 1px; } td { font-variant-numeric: tabular-nums; color: var(--text-secondary); } tr { cursor: pointer; } .selected { background: #f2c14e14; box-shadow: inset 2px 0 var(--gold); } tr:focus-visible { outline: 2px solid var(--gold); outline-offset: -2px; } .day td { font-size: 11px; font-weight: 600; background: var(--surface-raised); height: 26px; }
  .title > span { margin-left: 5px; } .title { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; } strong { color: var(--text-primary); font-weight: 500; } a { color: var(--violet-text); text-decoration: none; font-size: 11px; } .delivery { font-size: 10px; border: 1px solid var(--border); padding: 1px 4px; border-radius: 4px; white-space: nowrap; } .banner { color: var(--gold); } .est { color: var(--estimated); font-size: 10px; } .ghost { font-size: 11px; }
  @media (max-width: 600px) { .kind { width: 104px; } .time { width: 65px; } .action { width: 70px; } }
</style>
