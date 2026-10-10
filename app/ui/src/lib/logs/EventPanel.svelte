<script lang="ts">
  import type { InstanceEventView } from '../bindings/InstanceEventView';
  import { eventParts, eventLabels } from './events-screen';
  import { clockTime, dayLabel } from './logs-format';
  let { view, className = () => null }: { view: InstanceEventView | null; className?: (id: number) => string | null } = $props();
  let parts = $derived(view ? eventParts(view, className) : null);
</script>
<section class="card" aria-label="Event details">
  <header><h2>{parts ? eventLabels[parts.kind] : 'Event'}</h2>{#if view}<small>{dayLabel(view.at, new Date())} · {clockTime(view.at)}</small>{/if}</header>
  {#if view && parts}
    <strong class="name" title={parts.who ?? '—'}>{parts.who ?? '—'}</strong>{#if view.provisional}<span class="unconf">unconfirmed</span>{/if}
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
  .card { padding: 12px; border: 1px solid var(--border); border-radius: 8px; background: var(--surface-panel); font-size: 12px; max-height: 480px; overflow: auto; }
  header { display: flex; align-items: center; justify-content: space-between; gap: 6px; margin-bottom: 12px; } h2 { margin: 0; font-size: 13px; } small, dt, p { color: var(--text-muted); }
  .name { display: inline-block; max-width: 100%; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; vertical-align: middle; }
  dl { display: grid; grid-template-columns: 76px minmax(0, 1fr); gap: 7px; margin: 12px 0 0; } dd { margin: 0; overflow-wrap: anywhere; }
</style>
