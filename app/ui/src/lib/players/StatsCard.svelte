<script lang="ts">
  import type { PlayerProfileView } from '../bindings/PlayerProfileView';
  import type { HomeVisuals } from '../home/visuals';
  import { objectVisual } from '../bridge/visual-index';
  import { formatStat } from '../styles/format';
  import { statTone } from '../shell/vitals';
  import './players.css';
  let { profile, visuals, missingText = 'Not recorded for this run' }: { missingText?: string; profile: PlayerProfileView | null; visuals: HomeVisuals } = $props();
  const stats = [{ id: 0, boost: 46, label: 'HP' }, { id: 3, boost: 47, label: 'MP' },
    { id: 20, boost: 48, label: 'ATT' }, { id: 21, boost: 49, label: 'DEF' }, { id: 22, boost: 50, label: 'SPD' },
    { id: 28, boost: 53, label: 'DEX' }, { id: 26, boost: 51, label: 'VIT' }, { id: 27, boost: 52, label: 'WIS' }];
  let observed = $derived(stats.flatMap(stat => {
    const value = profile?.stats.find(row => row.stat_id === stat.id)?.value;
    const boost = profile?.stats.find(row => row.stat_id === stat.boost)?.value;
    return value === undefined || !Number.isFinite(value) ? [] : [{ ...stat, value, boost: boost !== undefined && Number.isFinite(boost) ? boost : null }];
  }));
</script>
<section class="player-card" aria-label="Stats">
  <h3>Stats {#if profile}<span class="player-card-sub">{profile.maxed ?? '—'}/8 · boosts shown</span>{/if}</h3>
  {#if profile === null}<p class="player-muted">{missingText}</p>
  {:else}
    <dl class="player-stats">
      {#each observed as stat}
        <div data-stat-id={stat.id} data-tone={statTone(profile.maxed_stats?.includes(stat.id) ?? false, stat.boost)}><dt>{stat.label}</dt><dd>{formatStat(stat.value)}{#if stat.boost === null}<small class="player-muted" title="Boost not seen">—</small>{:else if stat.boost !== 0}<small class:player-danger={stat.boost < 0}>{stat.boost > 0 ? '+' : '−'}{formatStat(Math.abs(stat.boost))}</small>{/if}</dd></div>
      {/each}
    </dl>
    {#if observed.length < stats.length}<p class="player-muted">Stats not observed</p>{/if}
    <dl class="player-meta"><dt>Character fame</dt><dd>{formatStat(profile.fame ?? null)}</dd>
      <dt>Stars</dt><dd>{profile.stars == null ? '—' : `★ ${profile.stars}`}</dd>
      <dt>Skin</dt><dd>{profile.skin_id == null ? 'Not seen' : objectVisual(visuals.objects, profile.skin_id)?.name ?? `Skin ${profile.skin_id}`}</dd>
    </dl>
  {/if}
</section>
