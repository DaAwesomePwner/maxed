import { render } from '@testing-library/svelte';
import { tick } from 'svelte';
import { vi } from 'vitest';
import Shell from '../../src/lib/shell/Shell.svelte';
import { productionFeatures } from '../../src/lib/navigation/routes';
import { createClient, type Client } from '../../src/lib/bridge/client';
import { createFixtureTransport } from '../../src/lib/bridge/fixture';
import { fakeHistory } from '../navigation/fake-history';
import { homeSnapshot, homeSummary, saved } from './fixtures';

/** Let snapshot-driven effects, promise replies and their DOM updates settle. */
export async function flushHomeRender() {
  await tick();
  if (vi.isFakeTimers()) await vi.advanceTimersByTimeAsync(0);
  else await new Promise<void>(resolve => setTimeout(resolve, 0));
  await tick();
}

export function mountHome(snapshot = homeSnapshot({ area_kind: 'nexus', run: null }), summary = homeSummary(), hash = '#/', { preview = false, demo = false, homeRedesign = productionFeatures.homeRedesign, queryHome, setupRead }: { preview?: boolean; demo?: boolean; homeRedesign?: boolean; queryHome?: Client['queryHome']; setupRead?: Client['querySetup'] } = {}) {
  const transport = createFixtureTransport(snapshot, { client_fingerprint: 'synthetic-home', objects: [] }, summary);
  const client = createClient(transport);
  const querySetup = client.querySetup;
  client.querySetup = vi.fn(async (...args: Parameters<typeof querySetup>) => ({ ...await querySetup(...args), demo }));
  if (setupRead) client.querySetup = setupRead;
  client.queryStore = vi.fn(async () => saved(snapshot.sequence, snapshot.stream_id));
  if (queryHome) client.queryHome = queryHome;
  const query = vi.spyOn(client, 'queryHome');
  const listeners = [vi.spyOn(client, 'onSnapshot'), vi.spyOn(client, 'onCapture'), vi.spyOn(client, 'onEngineEvent')];
  const browser = fakeHistory(hash);
  const page = render(Shell, { client, historyAdapter: browser.adapter, features: { ...productionFeatures, homeRedesign }, preview });
  return { ...page, transport, client, query, listeners, browser };
}
