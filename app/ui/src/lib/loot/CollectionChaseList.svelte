<script lang="ts">
  import { itemHover } from '../components/item-hover';
  import type { Client } from '../bridge/client';
  import type { AppSettings } from '../bindings/AppSettings';
  import type { ChaseItem } from '../bindings/ChaseItem';
  import type { ChaseRow } from '../bindings/ChaseRow';
  import type { HomeVisuals } from '../home/visuals';
  import { spriteUrl } from '../bridge/visuals';
  import { shortWhen, clockWhen, tierClass } from './loot-format';
  import { chaseOrder } from './collection-screen';
  import SpriteImage from '../components/SpriteImage.svelte';
  import EmptyState from '../components/EmptyState.svelte';
  import PinButton from './PinButton.svelte';
  import { plural } from './presentation';
  let { client, settings, rows, visuals, onSaved, saving = false, onSaving, portals = new Map() }: {
    client: Client; settings: AppSettings; rows: ChaseRow[]; visuals: HomeVisuals; onSaved: (chase: ChaseItem[]) => void;
    saving?: boolean; onSaving?: (saving: boolean) => void; portals?: Map<string, number>;
  } = $props();
  let sorted = $derived(chaseOrder(rows.filter(row => settings.loot.chase.some(item =>
    item.item_type === row.item.item_type && item.dungeon === row.item.dungeon))));

  let longest = $derived(Math.max(1, ...sorted.map(row => row.dry_runs)));
</script>

{#if sorted.length}
  <div class="table-scroll" role="region" aria-label="Chase list rows">
    <table aria-label="Chase list">
      <colgroup><col style="width:28%" /><col style="width:17%" /><col style="width:6%" /><col style="width:13%" /><col /><col style="width:32px" /></colgroup>
      <thead><tr><th>Item</th><th>Dungeon</th><th class="number">Seen</th><th class="number">Last seen</th><th>Dry streak</th><th><span class="sr-only">Chase</span></th></tr></thead>
      <tbody>
        {#each sorted as row (JSON.stringify([row.item.item_type, row.item.dungeon]))}
          {@const visual = visuals.objects.find(object => object.object_type === row.item.item_type)}
          {@const name = visual?.name ?? `#${row.item.item_type}`}
          {@const streak = `${row.dry_runs} ${plural(row.dry_runs, 'run')} ${row.last_at === null ? 'since you started chasing' : 'since last seen'}`}
          <tr>
            <td><a class={tierClass(visual?.item_tier)} use:itemHover={{ type: row.item.item_type, name, tier: visual?.item_tier }} href={`#/loot/item/${row.item.item_type}`} data-row-id={JSON.stringify(['chase', row.item.item_type, row.item.dungeon])}>
              <span class="tile"><SpriteImage sprite={visual?.sprite ?? null} sheetUrl={visual?.sprite ? spriteUrl(visuals.client_fingerprint, visual.sprite) : null} fit={24} alt="" /></span>
              <span class="ellipsis">{name}</span>
            </a></td>
            <td title={row.item.dungeon ?? 'Any dungeon'}><div class="place">
              {#if row.item.dungeon !== null}
                {@const portal = visuals.objects.find(object => object.object_type === portals.get(row.item.dungeon!))}
                <SpriteImage sprite={portal?.sprite ?? null} sheetUrl={portal?.sprite ? spriteUrl(visuals.client_fingerprint, portal.sprite) : null} fit={24} alt="" />
              {/if}
              <span class="ellipsis">{row.item.dungeon ?? 'Any dungeon'}</span>
            </div></td>
            <td class="number">{row.seen}</td>
            <td class="number" title={row.last_at === null ? undefined : clockWhen(row.last_at, new Date())}>{row.last_at === null ? 'Not seen yet' : shortWhen(row.last_at, new Date())}</td>
            <td><div class="streak" class:amber={row.dry_runs >= 20} title={streak}><span class="bar"><i style:width={`${row.dry_runs / longest * 100}%`}></i></span><span class="ellipsis">{streak}</span></div></td>
            <td><PinButton {client} itemType={row.item.item_type} dungeon={row.item.dungeon} {name} variant="icon" {settings} {onSaved} disabled={saving} {onSaving} /></td>
          </tr>
        {/each}
      </tbody>
    </table>
  </div>
{:else}<EmptyState heading="Chase an item to track it">
  {#snippet action()}<p>Search above, pick a dungeon and click the star.</p>{/snippet}
</EmptyState>{/if}

<style>
  .table-scroll { max-height: 272px; overflow: auto; }
  table { width: 100%; table-layout: fixed; border-collapse: collapse; font-size: var(--text-caption); line-height: var(--leading-caption); }
  th { text-align: left; color: var(--text-muted); font-size: var(--text-meta); font-weight: var(--weight-medium); position: sticky; top: 0; background: var(--surface-panel); z-index: 1; }
  th, td { height: var(--row-default); box-sizing: border-box; padding: 0 var(--space-2); border-bottom: 1px solid var(--hairline); }
  thead th { border-bottom-color: var(--hairline-strong); }
  a { display: flex; align-items: center; gap: var(--space-2); color: var(--text-primary); text-decoration: none; min-width: 0; font-weight: var(--weight-medium); }
  .tile { border: 1px solid currentColor; border-radius: var(--radius-sprite); display: inline-flex; flex: none; }
  .ellipsis { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .place { display: flex; align-items: center; gap: var(--space-2); min-width: 0; }
  .number { text-align: right; font-variant-numeric: tabular-nums; }
  .ut { color: var(--tier-ut); } .st { color: var(--tier-st); }
  .streak { display: flex; align-items: center; gap: var(--space-2); min-width: 0; font-size: var(--text-meta); color: var(--text-secondary); }
  .bar { flex: 1; min-width: 24px; max-width: 180px; height: 4px; background: var(--bar-track); }
  .bar i { display: block; height: 100%; background: var(--text-muted); }
  .amber { color: var(--estimated); } .amber i { background: var(--estimated); }
  a:focus-visible, .table-scroll:focus-visible { outline: 2px solid var(--focus-ring); outline-offset: -2px; }
</style>
