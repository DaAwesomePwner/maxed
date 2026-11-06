<script lang="ts">
  import type { ActivityItem } from './player-view';
  import { formatClock } from '../styles/format';
  import './players.css';
  let { items }: { items: ActivityItem[] } = $props();
</script>
<section class="player-card" aria-label="Activity">
  <h3>Activity</h3>
  <ul class="player-activity">{#each items as item}
    <li data-kind={item.kind}><time>{formatClock(String(Math.round(item.at * 1000))) ?? '—'}</time><i aria-hidden="true"></i><span>{item.label}</span></li>
  {:else}<li class="player-muted">Not seen</li>{/each}</ul>
</section>
