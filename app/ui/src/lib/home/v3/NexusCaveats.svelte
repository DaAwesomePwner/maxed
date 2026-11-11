<script lang="ts">
  import type { LiveSnapshot } from '../../bindings/LiveSnapshot';
  import type { HomeHistoryState } from '../summary';
  import { debugMode } from '../../debug/debug';
  import DebugNote from '../../debug/DebugNote.svelte';
  import Badge from '../../components/Badge.svelte';
  import { coverageLabels, reasonLabel, reasonNotes, statText } from '../evidence';
  let { history, snapshot }: { history: HomeHistoryState; snapshot: LiveSnapshot | null } = $props();
  let summary = $derived(history.summary);
  let top = $derived(summary?.top_fight ?? null);
  let locals = $derived(top?.players.filter(player => player.is_local) ?? []);
  let local = $derived(locals.length === 1 ? locals[0] : null);
  let estimated = $derived(local?.sources.filter(source => source.estimated) ?? []);
  let reasons = $derived(reasonNotes(estimated.flatMap(source => source.reasons)));
  let notes = $derived([...new Set([...[...(top?.issues ?? []), ...(summary?.issues ?? [])].map(issue => coverageLabels[issue]), ...reasons])]);
  let topPartial = $derived(top !== null && (!top.complete_capture || top.issues.length > 0 || estimated.length > 0));
</script>
{#if $debugMode}
  <aside class="caveats">
    <DebugNote text="Session scope ends after 30 minutes without game traffic." />
    {#if snapshot?.session?.issues.length}<DebugNote text="Partial session observations." />{/if}
    {#if !/^(Refreshing|Loading)\b/.test(history.message)}<DebugNote text={history.message} />{/if}
    {#if summary}
      <DebugNote>{summary.fame_missing_baselines > 0 ? `Partial known-character change; ${summary.fame_missing_baselines} missing baselines` : summary.fame_delta === null ? 'Comparable baseline unavailable' : 'Known baselines'}{summary.fame_sampled ? '; sampled fame' : ''}</DebugNote>
      <DebugNote>{topPartial ? 'Partial or estimated fight observations.' : statText(local?.dps) === null ? 'No unique local DPS candidate' : 'Highest single fight'}</DebugNote>
      {#if estimated.length}<Badge label="Top fight source" tone="estimated" estimated={{ reason: reasons.join('; ') || reasonLabel(null) }} />{/if}
      {#if topPartial}<DebugNote text="Top fight has partial observations." />{/if}
      {#if notes.length || estimated.length}<details><summary>Coverage notes ({notes.length || 1})</summary><ul>{#each notes as note}<li>{note}</li>{/each}{#if estimated.length && !reasons.length}<li>{reasonLabel(null)}</li>{/if}</ul></details>{/if}
      {#if summary.unranked_drop_count > 0}<DebugNote>{summary.unranked_drop_count} unranked observed bags excluded from selection.</DebugNote>{/if}
    {/if}
  </aside>
{/if}
<style>
  .caveats { grid-column: 1 / -1; min-width: 0; font-size: var(--text-meta); overflow-wrap: anywhere; }
</style>
