import { cleanup, fireEvent, render, screen } from '@testing-library/svelte';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { createClient } from '../../src/lib/bridge/client';
import { createFixtureTransport } from '../../src/lib/bridge/fixture';
import type { Route } from '../../src/lib/navigation/routes';
import { homeSnapshot } from '../home/fixtures';
import { flush } from '../runs/fixtures';
import StoreTicks from './StoreTicks.svelte';
import RouteFrame from '../../src/routes/RouteFrame.svelte';
import type { StoreStatus } from '../../src/lib/bindings/StoreStatus';

beforeEach(() => vi.useFakeTimers());
afterEach(() => { cleanup(); vi.clearAllTimers(); vi.useRealTimers(); });
const views: Array<[string, Route, 'listDrops' | 'getItemHistory' | 'getLootStats' | 'getCollection']> = [
  ['Feed', { kind: 'loot', view: 'feed' }, 'listDrops'],
  ['Item', { kind: 'item', type: 101 }, 'getItemHistory'],
  ['Stats', { kind: 'loot', view: 'stats' }, 'getLootStats'],
  ['Collection', { kind: 'loot', view: 'collection' }, 'getCollection'],
];
it.each(views)('%s reads once through RouteFrame when only numeric store progress changes', async (_name, route, method) => {
  const client = createClient(createFixtureTransport(homeSnapshot()));
  const read = vi.spyOn(client, method);
  render(StoreTicks, { client, route });
  await flush();
  expect(read).toHaveBeenCalledTimes(1);
  for (let i = 0; i < 3; i++) {
    await fireEvent.click(screen.getByRole('button', { name: 'Storage tick' }));
    await flush();
  }
  expect(read).toHaveBeenCalledTimes(1);
});

it.each(views)('%s retains its controller when RouteFrame receives a new store prop', async (_name, route, method) => {
  const snapshot = homeSnapshot();
  const client = createClient(createFixtureTransport(snapshot));
  const read = vi.spyOn(client, method);
  let store: StoreStatus = { state: 'ready', queued: 10, durable: '10', stream_id: snapshot.stream_id, message: '' };
  const page = render(RouteFrame, { route, client, snapshot, store, required: '10' });
  await flush();
  expect(read).toHaveBeenCalledTimes(1);
  for (let i = 0; i < 3; i++) {
    store = { ...store, queued: store.queued + 1, durable: String(Number(store.durable) + 1) };
    await page.rerender({ store });
    await flush();
  }
  expect(read).toHaveBeenCalledTimes(1);
});
