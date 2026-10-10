<script lang="ts">
  import type { RunStory } from '../bindings/RunStory';
  import type { ChatOverview } from '../bindings/ChatOverview';
  import { runCounts } from './events-screen';
  import { clockTime, dayLabel } from './logs-format';
  import { routeHref } from '../navigation/routes';
  import { formatElapsed, titleCase } from '../styles/format';
  let { runId, story, chat }: { runId: string; story: RunStory | null; chat: ChatOverview | null } = $props();
  let counts = $derived(story ? runCounts(story) : null);
  let run = $derived(story?.run);
  let duration = $derived(run?.ended ? formatElapsed(run.started, run.ended, true) : '—');
</script>
<section class="card" aria-label="Run">
  <header><h2>Run</h2><a href={routeHref({ kind: 'run', id: runId })}>Open run ›</a></header>
  <strong title={run?.dungeon}>{run?.dungeon ?? '—'}</strong>
  <p>{run ? `${dayLabel(run.started, new Date())} · ${clockTime(run.started)}` : '—'} · {duration} · {run?.outcome ? titleCase(run.outcome) : '—'}</p>
  <dl><dt>Deaths</dt><dd>{story?.events == null ? '—' : `${counts?.deaths}${story.events_limited ? '+' : ''}`}</dd><dt>Departures</dt><dd>{story?.events == null ? '—' : `${counts?.departures}${story.events_limited ? '+' : ''}`}</dd><dt>Chat lines</dt><dd>{chat?.lines.toLocaleString('en-US') ?? '—'}</dd></dl>
  <nav aria-label="Run links"><a class="ctl ghost" href={routeHref({ kind: 'logs', view: 'events', channel: 'all', runId })}>Events in this run</a><a class="ctl ghost" href={routeHref({ kind: 'logs', view: 'chat', channel: 'all', runId })}>Chat in this run</a></nav>
</section>
<style>
  .card { padding: 12px; border: 1px solid var(--border); border-radius: 8px; background: var(--surface-panel); font-size: 12px; max-height: 480px; overflow: auto; }
  header { display: flex; justify-content: space-between; gap: 6px; margin-bottom: 12px; } h2 { margin: 0; font-size: 13px; } a { color: var(--violet-text); text-decoration: none; } p, dt { color: var(--text-muted); }
  strong { display: block; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; } dl { display: grid; grid-template-columns: 1fr auto; gap: 7px; } dd { margin: 0; font-variant-numeric: tabular-nums; } nav { display: flex; flex-wrap: wrap; gap: 4px; }
</style>
