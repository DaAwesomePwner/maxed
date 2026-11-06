import { render, screen } from '@testing-library/svelte';
import { vi } from 'vitest';
import Shell from '../../src/lib/shell/Shell.svelte';
import { productionFeatures } from '../../src/lib/navigation/routes';
import type { EngineEvent } from '../../src/lib/bindings/EngineEvent';
import type { LiveSnapshot } from '../../src/lib/bindings/LiveSnapshot';
import type { FightView } from '../../src/lib/bindings/FightView';
import type { HomeSummary } from '../../src/lib/bindings/HomeSummary';
import type { ObjectVisual } from '../../src/lib/bindings/ObjectVisual';
import { createClient, type Client } from '../../src/lib/bridge/client';
import { createFixtureTransport } from '../../src/lib/bridge/fixture';
import { homeSnapshot, homeSummary, saved } from '../home/fixtures';
import { fakeHistory } from '../navigation/fake-history';

export { homeSnapshot, homeSummary, deferred, flush } from '../home/fixtures';
export { flushHomeRender } from '../home/shell-fixture';

// Invented observations only. Names distinguish evidence from another scope.
export function fight(id: string, overrides: Partial<FightView> = {}): FightView {
  return { ...homeSnapshot().fights[0], id, target_type: 300, target_kind: 'event_boss',
    target_hp: 240, max_hp: 1000, issues: [], ...overrides };
}

export function realmSnapshot(): LiveSnapshot {
  return homeSnapshot({ area_kind: 'realm', map: 'Synthetic Realm', run: null, fights: [], drops: [],
    instance: { context: { realm_name: 'Synthetic Realm', instance_cap: 85, entered_from_realm: false, modifiers: [] },
      population: 12, population_peak: 15, population_provisional: true, realm_score: { current: 150, max: 100 }, events_dropped: 0 } });
}

export function catalog(object_type: number, name: string): ObjectVisual {
  return { object_type, name, sprite: null, bag: null, item_tier: null };
}

export function mountV2(snapshot = homeSnapshot(), summary: HomeSummary = homeSummary(), options: {
  hash?: string; features?: { lootBrowsing: boolean; logs: boolean; stats: boolean; party: boolean; homeRedesign: boolean; character: boolean }; preview?: boolean; demo?: boolean; objects?: ObjectVisual[];
  configure?: (client: Client) => void;
} = {}) {
  const transport = createFixtureTransport(snapshot, { client_fingerprint: 'a'.repeat(64), objects: options.objects ?? [] }, summary);
  const client = createClient(transport);
  const setup = client.querySetup;
  client.querySetup = vi.fn(async (...args: Parameters<typeof setup>) => ({ ...await setup(...args), demo: options.demo ?? false }));
  client.queryStore = vi.fn(async () => saved(snapshot.sequence, snapshot.stream_id));
  options.configure?.(client);
  // Install before render, including every history method exposed by Client.
  const history = [vi.spyOn(client, 'queryHome'), vi.spyOn(client, 'listRuns'), vi.spyOn(client, 'getRun'),
    vi.spyOn(client, 'getFight'), vi.spyOn(client, 'getPlayerBreakdown'), vi.spyOn(client, 'listAlerts')];
  const listeners = [vi.spyOn(client, 'onSnapshot'), vi.spyOn(client, 'onCapture'), vi.spyOn(client, 'onEngineEvent')];
  const invoke = vi.spyOn(transport, 'invoke');
  const browser = fakeHistory(options.hash);
  // Legacy Home suites exercise the explicit v3 rollback path.
  const page = render(Shell, { client, historyAdapter: browser.adapter, features: options.features ?? { ...productionFeatures, homeRedesign: false }, preview: options.preview ?? false });
  let sequence = Number(snapshot.sequence);
  const send = (event: EngineEvent, at = '6000000') => transport.publishEngineEvent({
    stream_id: snapshot.stream_id, sequence: String(++sequence), at, event,
  });
  const publish = (next: LiveSnapshot) => transport.publishSnapshot({ ...next, sequence: String(++sequence) });
  return { ...page, client, transport, history, listeners, invoke, browser, send, publish };
}

export function section(name: string | RegExp): HTMLElement {
  const heading = screen.getByRole('heading', { name });
  const region = heading.closest('section');
  if (!(region instanceof HTMLElement)) throw new Error(`No section for ${String(name)}`);
  return region;
}
