<script lang="ts">
  import type { PartyView } from '../bindings/PartyView';
  import type { HomeVisuals } from '../home/visuals';
  import { objectVisual } from '../bridge/visual-index';
  let { view, saved, visuals }: { view: PartyView; saved: boolean; visuals: HomeVisuals } = $props();
  let classes = $derived([...view.classes].sort((a, b) => b.count - a.count || a.class_id - b.class_id));
</script>
<section aria-label="Party summary">
  <dl>
    <div><dt>{saved ? 'Players' : 'In view'}</dt><dd>{saved ? view.members.length : view.counts.in_view}</dd></div>
    {#if view.preset && view.assets_available}
      <div><dt>Pass</dt><dd class="pass">{view.counts.pass}</dd></div>
      <div><dt>Fail</dt><dd class="fail">{view.counts.fail}</dd></div>
      <div><dt>Check</dt><dd class="check">{view.counts.check}</dd></div>
      <div><dt>Not seen</dt><dd>{view.counts.not_seen}</dd></div>
    {/if}
    {#if view.area_kind === 'dungeon'}
      <div><dt>No damage seen</dt><dd class="check">{view.counts.no_damage_seen}</dd></div>
      {#if saved}<div><dt>Left early</dt><dd class="check">{view.counts.left_early}</dd></div>{/if}
    {/if}
    <div class="classes"><dt>Classes</dt><dd>
      {#each classes.slice(0, 5) as row}<span class="chip">{objectVisual(visuals.objects, row.class_id)?.name ?? 'Unknown class'} {row.count}</span>{/each}
      {#if classes.length > 5}<span class="chip">+{classes.length - 5} more</span>{/if}
      {#if !classes.length}<span>No classes observed</span>{/if}
    </dd></div>
  </dl>
</section>
<style>
  dl { display: flex; flex-wrap: wrap; margin: 0 0 var(--space-3); background: var(--surface-panel); border: 1px solid var(--border); border-radius: var(--radius-card); }
  dl > div { padding: var(--space-2) var(--space-3); flex: 1; } dl > div + div { border-left: 1px solid var(--border); }
  dt { font-size: 11px; color: var(--text-secondary); white-space: nowrap; } dd { margin: 0; font-size: 20px; font-weight: 700; font-variant-numeric: tabular-nums; }
  .pass { color: var(--success); } .fail { color: var(--danger); } .check { color: var(--estimated); }
  .classes { flex: 2; } .classes dd { display: flex; flex-wrap: wrap; gap: 4px; font-size: 11px; font-weight: 400; }
  .chip { border: 1px solid var(--border); border-radius: 10px; padding: 0 7px; background: var(--surface-raised); }
</style>
