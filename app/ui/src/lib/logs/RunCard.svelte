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
  .card { min-width: 0; padding: var(--space-4); border: 1px solid var(--hairline); border-radius: var(--radius-card); background: var(--surface-panel); font-size: var(--text-caption); line-height: var(--leading-caption); }
  .card { max-height: 480px; overflow: auto; }
  header { display: flex; align-items: baseline; justify-content: space-between; gap: var(--space-2); margin-bottom: var(--space-3); } h2 { margin: 0; font-size: var(--text-label); line-height: var(--leading-label); font-weight: var(--weight-medium); }
  header a { color: var(--text-secondary); text-decoration: none; } header a:hover, header a:focus-visible { color: var(--text-primary); text-decoration: underline; text-underline-offset: 3px; }
  p, dt { color: var(--text-muted); } p { margin: var(--space-1) 0 var(--space-3); }
  strong { display: block; font-weight: var(--weight-medium); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  dl { display: grid; grid-template-columns: 1fr auto; gap: var(--space-2); margin: 0 0 var(--space-3); } dd { margin: 0; font-variant-numeric: tabular-nums; }
  nav { display: flex; flex-wrap: wrap; gap: var(--space-1); }
</style>
