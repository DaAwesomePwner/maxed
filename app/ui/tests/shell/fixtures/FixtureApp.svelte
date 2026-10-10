<script lang="ts">
  import Shell from '../../../src/lib/shell/Shell.svelte';
  import RouteFrame from '../../../src/routes/RouteFrame.svelte';
  import Card from '../../../src/lib/components/Card.svelte';
  import EmptyState from '../../../src/lib/components/EmptyState.svelte';
  import { createClient, type Client } from '../../../src/lib/bridge/client';
  import { createFixtureTransport } from '../../../src/lib/bridge/fixture';
  import { routeHref } from '../../../src/lib/navigation/routes';
  import type { HistoryAdapter } from '../../../src/lib/navigation/history';
  import type { RunView } from '../../../src/lib/bindings/RunView';
  import fixture from '../../fixtures/t0-live';
  import FixtureRuns from './FixtureRuns.svelte';
  import { statsQueries } from './stats';
  import { logsQueries } from './logs';
  import { lootQueries, lootSettings, lootVisuals, lootDungeons } from './loot';

  let { client = createClient(createFixtureTransport(fixture, lootVisuals, undefined, undefined, undefined, undefined, undefined,
    { lootSettings, catalogDungeons: lootDungeons }, lootQueries, logsQueries, statsQueries)), historyAdapter, runs = [fixture.run], rowsReady = () => Promise.resolve() }: {
    client?: Client; historyAdapter?: HistoryAdapter; runs?: RunView[]; rowsReady?: (key: string) => Promise<unknown>;
  } = $props();
</script>

<Shell {client} preview features={{ lootBrowsing: true, logs: true, stats: true }} {historyAdapter} {rowsReady}>
  {#snippet content(navigation, router, context)}
    {@const route = navigation.entry.route}
    {#if route.kind === 'runs'}
      <FixtureRuns {navigation} {router} {runs} />
    {:else if route.kind === 'run'}
      {@const run = runs.find((row) => row.id === route.id)}
      {#if run}
        <div class="story">
          <Card heading={run.dungeon}><p>Synthetic run story · {run.outcome ?? 'Outcome unavailable'}. Comparison data unavailable.</p></Card>
          <Card heading="Timeline">
            <p>Only supplied fixture observations are shown.</p>
            <button onclick={() => router.navigate(route, { anchor: 'fight' })}>Jump to observed fight</button>
          </Card>
          <section data-anchor="fight" tabindex="-1" aria-label="Observed fight">
            <Card heading="Fights"><a href={routeHref({ kind: 'fight', runId: run.id, id: fixture.fights[0].id })}>Open synthetic fight</a><p>Target name and complete capture unavailable.</p></Card>
          </section>
          <Card heading="Loot"><p>Observed fixture item type {fixture.drops[0].items[0]}; item name unavailable.</p></Card>
        </div>
      {:else}<EmptyState heading="Synthetic run unavailable" description="This fixture row has been removed. Back returns to the list." />{/if}
    {:else if route.kind === 'fight'}
      <Card heading="Synthetic fight"><a href={routeHref({ kind: 'player', runId: route.runId, fightId: route.id, objectId: fixture.fights[0].players[0].object_id })}>Open observed player</a></Card>
    {:else}<RouteFrame {route} snapshot={fixture} {client} preview store={context.store}
      savedState={navigation.entry} onStateChange={(patch) => router.save(patch)}
      onRowsReady={context.onRowsReady} entryKey={navigation.entry.key} scopeKey={navigation.entry.scope} />{/if}
  {/snippet}
</Shell>

<style>.story { display: grid; gap: var(--space-6); }</style>
