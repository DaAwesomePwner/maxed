<script lang="ts">
  import { getContext } from 'svelte';
  import { SHELL_CONNECTION, type ShellConnection } from '../shell/context';
  import type { Client } from '../bridge/client';
  import type { RunView } from '../bindings/RunView';
  import { createRunHistory, type HistoryState } from '../runs/history';
  import { dayLabel, clockTime } from './logs-format';
  import '../styles/controls.css';
  const historyCache = getContext<ShellConnection | undefined>(SHELL_CONNECTION)?.historyCache;
  let { runId, client, removeHref, links, suppliedRun }: {
    suppliedRun?: RunView | null;
    runId: string; client: Client; removeHref: string; links: Array<{ label: string; href: string }>;
  } = $props();
  let history = $state.raw<HistoryState>();
  let run = $derived(suppliedRun !== undefined ? suppliedRun : history?.phase === 'ready' && history.value?.kind === 'run' ? history.value.story?.run : null);
  const label = $derived(run ? `Run: ${run.dungeon} · ${dayLabel(run.started, new Date())}, ${clockTime(run.started)}` : 'Run');
  $effect(() => {
    if (suppliedRun !== undefined) return;
    const controller = createRunHistory(client, next => { history = next; }, { cache: historyCache });
    controller.update({ query: { kind: 'run', id: runId }, scopeKey: '', streamId: null, required: null, store: null });
    return () => controller.dispose();
  });
</script>
<div class="scope">
  <a class="ctl chip" href={removeHref} aria-label={`Remove run filter: ${label}`} title={run?.dungeon ?? 'Run'}>
    <span>{label}</span><span aria-hidden="true">×</span>
  </a>
  {#each links as link}<a class="ctl ghost" href={link.href}>{link.label}</a>{/each}
</div>
<style>
  .scope { display: flex; align-items: center; flex-wrap: wrap; gap: var(--space-2); min-width: 0; }
  .chip { max-width: 100%; border-color: var(--hairline-strong); background: var(--surface-raised); color: var(--text-primary); }
  .chip span:first-child { overflow: hidden; text-overflow: ellipsis; } .chip span:last-child { color: var(--text-muted); }
</style>
