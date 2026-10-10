<script lang="ts">
  import RouteFrame from '../../src/routes/RouteFrame.svelte';
  import type { Client } from '../../src/lib/bridge/client';
  import type { Route } from '../../src/lib/navigation/routes';
  import type { StoreStatus } from '../../src/lib/bindings/StoreStatus';
  import { homeSnapshot } from '../home/fixtures';
  let { client, route }: { client: Client; route: Route } = $props();
  const snapshot = homeSnapshot();
  let store = $state<StoreStatus>({ state: 'ready', queued: 10, durable: '10', stream_id: snapshot.stream_id, message: '' });
</script>
<button onclick={() => { store = { ...store, queued: store.queued + 1, durable: String(Number(store.durable) + 1) }; }}>Storage tick</button>
<RouteFrame {route} {client} {snapshot} {store} required="10" scopeKey="fixture" entryKey="loot" />
