<script lang="ts">
  import type { InstanceEventView } from '../bindings/InstanceEventView';
  import { eventParts, eventLabels } from './events-screen';
  import { clockTime, dayLabel } from './logs-format';
  import Badge from '../components/Badge.svelte';
  let { view, className = () => null }: { view: InstanceEventView | null; className?: (id: number) => string | null } = $props();
  let parts = $derived(view ? eventParts(view, className) : null);
</script>
<section class="card" aria-label="Event details">
  <header><h2>{parts ? eventLabels[parts.kind] : 'Event'}</h2>{#if view}<small>{dayLabel(view.at, new Date())} · {clockTime(view.at)}</small>{/if}</header>
  {#if view && parts}
    <div class="who"><strong class="name" title={parts.who ?? '—'}>{parts.who ?? '—'}</strong>{#if view.provisional}<Badge label="unconfirmed" tone="warning" />{/if}</div>
    <dl>
      {#if view.kind.type === 'player_death'}
        <dt>Level</dt><dd>{view.kind.level ?? '—'}</dd><dt>Class</dt><dd>{view.kind.class_id == null ? '—' : className(view.kind.class_id) ?? '—'}</dd>
        <dt>Killed by</dt><dd>{view.kind.killed_by ?? '—'}</dd><dt>Where</dt><dd>{parts.where}</dd>
        <dt>Grave</dt><dd>{view.kind.grave_type == null ? 'Not seen' : 'Seen'}</dd><dt>You</dt><dd>{view.kind.is_local ? 'Yes' : 'No'}</dd>
      {:else if view.kind.type === 'player_left'}
        <dt>Player</dt><dd>{view.kind.player_name ?? '—'}</dd><dt>Class</dt><dd>{view.kind.class_id == null ? '—' : className(view.kind.class_id) ?? '—'}</dd><dt>Where</dt><dd>{parts.where}</dd>
      {:else if view.kind.type === 'portal_opened'}
        <dt>Opened by</dt><dd>{view.kind.opener ?? '—'}</dd><dt>Dungeon</dt><dd>{view.kind.dungeon ?? '—'}</dd><dt>Where</dt><dd>{parts.where}</dd><dt>You</dt><dd>{view.kind.opener_is_local == null ? '—' : view.kind.opener_is_local ? 'Yes' : 'No'}</dd>
      {:else}<dt>Boss</dt><dd>{view.kind.boss_key || '—'}</dd><dt>Killed by</dt><dd>{view.kind.killer ?? '—'}</dd><dt>Where</dt><dd>{parts.where}</dd>{/if}
    </dl>
  {:else}<p>Select an event to see its details.</p>{/if}
</section>
<style>
  .card { min-width: 0; padding: var(--space-4); border: 1px solid var(--hairline); border-radius: var(--radius-card); background: var(--surface-panel); font-size: var(--text-caption); line-height: var(--leading-caption); }
  .card { max-height: 480px; overflow: auto; }
  header { display: flex; align-items: baseline; justify-content: space-between; gap: var(--space-2); margin-bottom: var(--space-3); } h2 { margin: 0; font-size: var(--text-label); line-height: var(--leading-label); font-weight: var(--weight-medium); }
  small, dt, p { color: var(--text-muted); } small { font-size: var(--text-meta); } p { margin: 0; }
  .who { display: flex; align-items: center; gap: var(--space-2); min-width: 0; }
  .name { min-width: 0; font-weight: var(--weight-medium); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  dl { display: grid; grid-template-columns: 76px minmax(0, 1fr); gap: var(--space-2); margin: var(--space-3) 0 0; } dd { margin: 0; overflow-wrap: anywhere; }
</style>
